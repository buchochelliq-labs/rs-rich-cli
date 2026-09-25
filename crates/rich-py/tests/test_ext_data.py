"""rs_rich.ext.data: parsing, the views, search, selection, diffs and transforms.

Expected output comes from `rich-ext` itself (see test_ext_expected.py).
"""

from __future__ import annotations

import dataclasses

import pytest
from conftest import render
from test_ext_expected import EXPECTED

from rs_rich.ext.data import (
    ConfigFileView,
    DataError,
    DiffView,
    Document,
    Explorer,
    Filter,
    FlatView,
    Highlight,
    Node,
    Redact,
    Redaction,
    SearchQuery,
    SearchResults,
    Select,
    SelectError,
    TableView,
    detect_format,
    diff,
    flatten,
    json,
    parse,
    search,
    select,
    unflatten,
)
from rs_rich.ext.transform import Pipeline

DEPLOY_YAML = """\
defaults: &defaults
  replicas: 2
  image: registry.example/app:1.4
services:
  web:
    port: 8080
    settings: *defaults
  worker: *defaults
"""
API_JSON = '{"name": "api", "ports": [80, 443], "tls": true}'


def check(name, renderable, width):
    assert render(renderable, width=width) == EXPECTED[name]
    assert render(renderable, width=width, color=True) == EXPECTED[f"{name}/color"]


@pytest.fixture
def deploy():
    return parse(DEPLOY_YAML, "yaml")


@pytest.fixture
def api():
    return parse(API_JSON)


def test_parse_and_navigate(deploy, api):
    assert detect_format(API_JSON) == "json"
    assert api.type_name == "map"
    assert api.keys() == ["name", "ports", "tls"]
    assert api["ports"][1].value == 443
    assert api.at("ports[1]").value == 443
    assert api.to_python() == {"name": "api", "ports": [80, 443], "tls": True}
    assert api.to_json() == '{"name":"api","ports":[80,443],"tls":true}'
    assert "name" in api and len(api) == 3
    assert deploy["defaults"].anchor == "defaults"
    assert deploy.at("services.worker").alias == "defaults"
    assert deploy["services"].position == (4, 1)
    assert [path for path, _ in api.walk()] == ["", "name", "ports", "ports[0]", "ports[1]", "tls"]


def test_explorer(deploy):
    check("data/explorer", Explorer(deploy, root_label="deploy.yaml"), 60)
    limited = Explorer(deploy, root_label="deploy.yaml", fold=["defaults"], max_depth=2, show_paths=True)
    check("data/explorer-limits", limited, 60)


def test_explorer_table_view():
    hosts = parse(
        '[{"host": "a.example", "port": 80, "up": true}, {"host": "b.example", "port": 443, "up": false},'
        ' {"host": "c.example", "up": null}]'
    )
    check("data/explorer-table", Explorer(hosts, view="table"), 60)


def test_formats():
    toml = parse('[package]\nname = "demo"\nreleased = 2026-09-23\n', name="Cargo.toml")
    check("data/toml", Explorer(toml, root_label="Cargo.toml"), 60)
    xml = parse('<server id="web"><port>8080</port><port>8443</port></server>', "xml")
    assert xml["server"]["@id"].xml_kind == "attribute"
    check("data/xml", Explorer(xml, root_label="server.xml"), 60)


def test_table_view_from_python_values():
    rows = [
        {"name": "web", "cpu": 0.42, "region": "eu-west-1", "notes": "canary"},
        {"name": "db", "cpu": 0.91, "region": "eu-west-1"},
        {"name": "cache", "cpu": 0.08, "region": "us-east-2"},
    ]
    view = TableView(
        rows, title="Services", columns=["name", "cpu"], headers={"cpu": "CPU"}, justify={"name": "center"}, max_rows=2
    )
    check("data/table-options", view, 60)


def test_parse_error_is_a_diagnostic():
    source = '{\n  "name": "api",\n  "port" 8080\n}\n'
    with pytest.raises(DataError) as caught:
        parse(source, "json", name="api.json")
    error = caught.value
    assert str(error) == EXPECTED["data/error-message"]
    assert (error.format, error.line) == ("json", 3)
    check("data/error", error.diagnostic, 60)


def test_flat_view_and_flatten(api):
    check("data/flat", FlatView(api, show_types=True), 60)
    leaves = flatten(api)
    assert [path for path, _ in leaves] == ["name", "ports[0]", "ports[1]", "tls"]
    assert unflatten(leaves) == api
    assert unflatten([("a.b", 1), ("a.c[0]", "x")]).to_python() == {"a": {"b": 1, "c": ["x"]}}


def test_search(deploy):
    query = SearchQuery(path="services.**", value="8080")
    check("data/search", SearchResults(deploy, query, context=1), 60)
    check("data/search-key", SearchResults(deploy, key="IMAGE", case_insensitive=True), 60)
    hits = search(deploy, key="port")
    assert [(hit.path, hit.matched_on) for hit in hits] == [("services.web.port", "key")]


def test_select(api):
    assert [(path, node.value) for path, node in select(api, "$.ports[?(@ > 100)]")] == [("ports[1]", 443)]
    with pytest.raises(SelectError) as caught:
        select(api, "$.items[1")
    assert str(caught.value) == "expected `]` at column 10"
    assert caught.value.column == 10


def test_diff(api):
    new = parse('{"name": "api", "ports": [80, 8443], "tls": true, "hsts": true}')
    assert [(c.path, c.kind) for c in diff(api, new)] == [("ports[1]", "changed"), ("hsts", "added")]
    check("data/diff", DiffView(api, new), 60)


def test_redaction_and_config_files():
    ini = parse(
        "; production settings\n[database]\nhost = db.internal\n; rotate monthly\npassword = hunter2\n"
        "[api]\ntoken = sk-live-123\ntimeout = 30\n",
        "ini",
    )
    redaction = Redaction(["host"], secrets=True, mask="[hidden]")
    assert redaction.matches_key("API-KEY") and not redaction.matches_key("timeout")
    check("data/config", ConfigFileView(ini, title="app.ini", redaction=redaction), 64)
    check("data/redacted", Explorer(ini.redacted(Redaction.secrets()), max_depth=2), 64)
    env = parse("# the host\nHOST=example.com\nAPI_TOKEN=abc123\n", "dotenv")
    assert env["HOST"].comment == "the host"
    check("data/dotenv", ConfigFileView(env, redaction=Redaction.secrets()), 50)


def test_document_transforms():
    doc = parse('{"servers": [{"host": "a", "token": "t1"}, {"host": "b", "token": "t2"}], "debug": true}')
    pipeline = (
        Pipeline()
        .then("redact", Redact(Redaction.secrets()))
        .then("select", Select("$.servers"))
        .then("highlight", Highlight("$[*].host", "reverse"))
    )
    document = pipeline.apply(Document(doc))
    assert document.label == "servers"
    assert document.highlights == [("[0].host", "reverse"), ("[1].host", "reverse")]
    check("data/transform", document.explorer(), 40)
    check("data/transform", document, 40)
    filtered = Filter("$.servers[1].host").apply(Document(doc, label="doc"))
    check("data/filter", filtered.explorer(), 40)


def test_python_values():
    @dataclasses.dataclass
    class Point:
        x: int
        y: list

    node = Node.from_python(Point(1, [True, None, "s"]))
    assert node.to_python() == {"x": 1, "y": [True, None, "s"]}
    check("data/json", json({"x": 1, "y": [True, None, "s"]}), 40)
    assert render(Explorer({"a": 1}), width=20) == "{…} 1 key\n└── a: 1\n"
