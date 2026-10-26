"""A small GitHub API client for the marketplace's workflows: REST, GraphQL
and release asset uploads, with the job's `GITHUB_TOKEN`.

Only the standard library is needed. Requests carry a generic User-Agent
and the token, nothing else. The transport is one function, `send(method,
url, headers, body) -> (status, headers, body)`, so the tests replay
recorded responses instead of calling GitHub.
"""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.parse
import urllib.request

import marketplace_common as common

API = "https://api.github.com/"
UPLOADS = "https://uploads.github.com/"
GRAPHQL = API + "graphql"
USER_AGENT = "rs-rich-marketplace-workflows"


class ApiError(Exception):
    def __init__(self, status: int, url: str, message: str):
        super().__init__(f"{url}: HTTP {status}: {message}")
        self.status = status


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):  # noqa: D102 - stdlib hook
        return None


def urllib_send(method: str, url: str, headers: dict, body: bytes | None):
    """The real transport: no redirects followed (a caller that wants one
    reads `Location`), at most 64 MiB read."""
    if not url.startswith("https://"):
        raise ValueError(f"{url}: only https:// is used")
    request = urllib.request.Request(url, data=body, method=method, headers=headers)
    opener = urllib.request.build_opener(_NoRedirect)
    try:
        with opener.open(request, timeout=60) as response:
            return response.status, dict(response.headers), response.read(common.MAX_DOWNLOAD + 1)
    except urllib.error.HTTPError as error:
        return error.code, dict(error.headers or {}), error.read(1 << 20)


class GitHub:
    def __init__(self, token: str | None = None, repository: str = common.REPOSITORY,
                 send=urllib_send):
        self.token = token if token is not None else os.environ.get("GITHUB_TOKEN", "")
        self.repository = repository
        self.owner, self.name = repository.split("/")
        self.send = send

    def headers(self, extra: dict | None = None, auth: bool = True) -> dict:
        headers = {"User-Agent": USER_AGENT, "Accept": "application/vnd.github+json",
                   "X-GitHub-Api-Version": "2022-11-28"}
        if auth and self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        headers.update(extra or {})
        return headers

    def call(self, method: str, url: str, body=None, headers: dict | None = None,
             allow: tuple = ()):
        """A JSON request; `None` for a status in `allow` (such as 404)."""
        data = None
        extra = dict(headers or {})
        if body is not None and not isinstance(body, bytes):
            data = json.dumps(body).encode()
            extra.setdefault("Content-Type", "application/json")
        elif isinstance(body, bytes):
            data = body
        status, _, payload = self.send(method, url, self.headers(extra), data)
        if status in allow:
            return None
        if not 200 <= status < 300:
            raise ApiError(status, url, payload[:500].decode("utf-8", "replace"))
        return json.loads(payload) if payload else None

    def rest(self, method: str, path: str, body=None, allow: tuple = ()):
        return self.call(method, API + path, body, allow=allow)

    def repo(self, path: str = "") -> str:
        return f"repos/{self.repository}{path}"

    def paged(self, path: str):
        """Every item of a paged REST list."""
        page = 1
        while True:
            separator = "&" if "?" in path else "?"
            items = self.rest("GET", f"{path}{separator}per_page=100&page={page}")
            if not items:
                return
            yield from items
            if len(items) < 100:
                return
            page += 1

    def graphql(self, query: str, variables: dict) -> dict:
        result = self.call("POST", GRAPHQL, {"query": query, "variables": variables})
        if result.get("errors"):
            messages = "; ".join(e.get("message", "?") for e in result["errors"])
            raise ApiError(200, GRAPHQL, messages)
        return result["data"]

    def releases(self) -> list:
        return list(self.paged(self.repo("/releases")))

    def release_by_tag(self, tag: str):
        return self.rest("GET", self.repo(f"/releases/tags/{urllib.parse.quote(tag)}"),
                         allow=(404,))

    def create_release(self, tag: str, target: str, name: str, body: str) -> dict:
        return self.rest("POST", self.repo("/releases"), {
            "tag_name": tag, "target_commitish": target, "name": name, "body": body,
            "draft": False, "prerelease": False, "make_latest": "false",
        })

    def upload_asset(self, release: dict, name: str, data: bytes) -> dict:
        path = self.repo(f"/releases/{release['id']}/assets")
        url = f"{UPLOADS}{path}?name={urllib.parse.quote(name)}"
        return self.call("POST", url, data, {"Content-Type": "application/octet-stream"})

    def download_asset(self, asset: dict) -> bytes:
        """An asset's bytes through the API (so a just-uploaded asset is read
        from the release itself), following the one redirect to storage
        without the token."""
        url = API + self.repo(f"/releases/assets/{asset['id']}")
        status, headers, payload = self.send(
            "GET", url, self.headers({"Accept": "application/octet-stream"}), None)
        if status in (301, 302, 303, 307, 308):
            location = headers.get("Location") or headers.get("location")
            if not location or not location.startswith("https://"):
                raise ApiError(status, url, "a redirect without an https:// Location")
            status, _, payload = self.send("GET", location, {"User-Agent": USER_AGENT}, None)
        if status != 200:
            raise ApiError(status, url, payload[:200].decode("utf-8", "replace"))
        if len(payload) > common.MAX_DOWNLOAD:
            raise ApiError(status, url, "larger than the largest artifact")
        return payload
