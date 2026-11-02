# Threat model

**Status:** written for 0.0.24 ([workstream 9](../plans/0.0.24.md#9-a-threat-model-docs)),
read against the code on the 0.0.24 branch. It is the starting point of
the structured security review in
[stage 4 of the plan to 1.0.0](../plans/1.0.0.md#stage-4-hardening-01x),
which works through the [checklist](#the-review-checklist) at the end.
Reporting a vulnerability is covered by
[`SECURITY.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/SECURITY.md).

Most of rs-rich is a rendering library: it turns text into terminal output
and has no more authority than the program that calls it. This page covers
the parts that do more. They run code someone else wrote, take files and
signatures from the network, serve a network, drive a browser or start
other programs:

1. [Plugins](#1-plugins-dylib-and-wasm): native libraries and WASM modules.
2. [The marketplace](#2-the-marketplace-and-its-signer): the signed index,
   the key chain, installs, the lockfile and the signing workflow.
3. [`rich serve` and rs-rich-web](#3-rich-serve-and-rs-rich-web).
4. [Noctiluca and Chrome](#4-noctiluca-and-chrome).
5. [rs-rich-embed's PTY host](#5-rs-rich-embeds-pty-host).
6. [TLS, certificates and redirects](#6-tls-certificates-and-redirects).
7. [ACP agents for Cornacai](#7-acp-agents-for-cornacai) (designed, not
   built yet).

## How to read this page

Each surface has the same parts:

- **Assets:** what an attacker wants.
- **Trust boundaries:** where data or control passes from a less trusted
  party to a more trusted one.
- **Entry points:** where the less trusted side's input arrives.
- **Threats,** in a table. The letter is the
  [STRIDE](https://en.wikipedia.org/wiki/STRIDE_model) category:
  **S**poofing, **T**ampering, **R**epudiation, **I**nformation disclosure,
  **D**enial of service, **E**levation of privilege. Each threat names its
  mitigation, linked to where it is implemented or documented, and its
  state:
  - **verified:** read in the code for this page (not yet checked against
    tests: that is stage 4's work);
  - **documented:** the docs say so, and the code was not read closely
    enough to confirm it;
  - **unverified:** neither could be confirmed;
  - **gap:** no mitigation, or only a partial one;
  - **mitigated:** a gap the 0.0.24 hardening round closed, linked to
    the fix and its test;
  - **planned:** the feature is designed but not built.
- **Gaps:** what the review should decide on: fix, accept or record.

Code links point at `main` and name a file, not a line, so they do not go
stale; the review reads the code at the release candidate.

## Who the attackers are

| Attacker | Can | Cannot (assumed) |
|---|---|---|
| **A plugin author** | Publish a WASM plugin, micro pack or theme to the marketplace; hand someone a native library | Change the signed index without the main key |
| **A marketplace contributor** | Open listing pull requests | Merge without a maintainer, or reach the signing environment's secrets |
| **A network attacker** | Read and change traffic it carries; run a TLS-inspecting proxy with a root the system trusts | Forge a certificate for a host without such a root |
| **A web page** | Run script in Chrome; send any text, title, URL or markup to Noctiluca; open WebSockets to `127.0.0.1` from a victim's browser | Escape Chrome's sandbox (out of scope: Chrome's own bugs) |
| **A program's output** | Write any bytes to a PTY: escape sequences, OSC strings, queries | Write to the user's terminal except through the emulator |
| **A coding agent** | Send any ACP message; run with its own process's permissions | Reach the client's file system or terminals except through what the client offers |
| **Another local user** | Connect to `127.0.0.1` ports; read world-readable files | Read another user's private (mode 700) files, or their process memory |
| **A cloned repository** | Ship a `./rich.toml`, Markdown, DOT, Mermaid or other files the user renders | Choose what the user's own config or command line says |

Out of scope: an attacker who already runs code as the user, owns the
user's account or the maintainer's GitHub account, or holds both
marketplace keys. Bugs in Chrome, the terminal emulator, the operating
system's PTY or the Rust toolchain belong to those projects, though the
review notes where rs-rich makes one easier to reach.

## Cross-cutting mitigations

These apply to more than one surface, and the sections below refer to them.

- **Terminal output from untrusted input is sanitized.** Plugin output,
  page text in Noctiluca, `rich view`, `rich diff`, `rich log` and the
  schema view make every control character visible
  ([`rich_ext::sanitize`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/src/sanitize.rs),
  [`noctiluca::clean`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/noctiluca/src/clean.rs)).
  Upstream's rendering commands (`rich FILE`, `rich URL`, `--markdown` and
  the rest) keep upstream's output, so they pass terminal controls through
  unless `--sanitize` is given (see [section 6](#6-tls-certificates-and-redirects)).
- **A project's config is untrusted.** A `./rich.toml` in the working
  directory may not load plugins, name a theme file, choose files to write,
  turn sanitizing off, start a browser (`mermaid_backend = "mmdc"`), run
  Graphviz or trust its own micro assets; each such key is dropped with a
  note
  ([`config.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/src/config.rs),
  `UNTRUSTED_*`; [PLUGINS](../PLUGINS.md#security-threat-model)).
  **verified**.
- **Parsers of outside input are fuzzed** nightly: markup, ANSI, Markdown,
  DOT, Mermaid, CSV, JSON, JSONL, stylesheets, the VT emulator, tapes,
  WebSocket frames and the page's input, the plugin manifest and WASM
  loading, and the marketplace's documents, signatures and artifacts
  ([fuzzing](fuzzing.md)).
- **Network features are opt-in at build time.** `rich serve` is behind
  the CLI's off-by-default `serve` feature, native plugins behind
  `dylib-plugins`, and URL fetching behind `fetch`.

## 1. Plugins: dylib and WASM

Documented in [PLUGINS: Security](../PLUGINS.md#security-threat-model) and
[the plugin-loading design](plugin-loading.md). Code:
[`rich-ext/src/plugin_loading/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/rich-ext/src/plugin_loading)
and [`rich-plugin-api/src/abi.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-plugin-api/src/abi.rs).

**Assets:** the user's account (files, credentials, network) for a native
plugin; the integrity of what the terminal shows, and the terminal itself,
for every plugin; the `rich` process's availability.

**Trust boundaries:**

- the `rich` process and a native library loaded into it (none in
  practice: the library runs as the process);
- the host and a WASM instance (the wasmi interpreter);
- a plugin's output and the user's terminal.

**Entry points:** `--plugin PATH`; `plugins = [...]` in the user's config or
a `--config` file; packages installed with `rich plugin install`; the
manifest a module returns; every call's output (text, highlighter spans,
fence output, markup).

| | Threat | Mitigation | State |
|---|---|---|---|
| E | A cloned repository's `./rich.toml` loads its own plugin | `plugins` in a working-directory config is ignored with a note ([`config.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/src/config.rs), `UNTRUSTED_PLUGINS`) | verified |
| E | A native library runs arbitrary code in `rich` | Trust-only by design. `dylib-plugins` is off by default in the CLI; a library loads only from a path the user gave, canonicalized first so a bare name does not search the system's library paths ([`native.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/src/plugin_loading/native.rs)). The marketplace has no native kind ([`listing.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/listing.rs), `Kind`) | verified |
| T | A native plugin built for another ABI corrupts memory | The ABI major is checked before the rest of the descriptor is read; only `repr(C)` data crosses; panics become errors in `export_dylib_plugin!` ([`abi.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-plugin-api/src/abi.rs)) | documented |
| E | A WASM module reaches the file system, network, clock or randomness | A module that imports anything is refused; the linker provides nothing ([`wasm.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/src/plugin_loading/wasm.rs), `load_wasm_bytes`) | verified |
| E | A malformed module reaches an interpreter bug | wasmparser validates the whole module before wasmi reads it; wasmi's strict enforced limits; portable dispatch, not tail calls; fuzzed (`plugin_wasm`) ([plugin loading](plugin-loading.md)) | verified (validation), documented (dispatch) |
| D | A module loops for ever or allocates without bound | Fuel per call, instantiation included (50 million by default); a 64 MiB memory cap and 10,000 table elements, one memory, one table, one instance; growth past a cap traps; a module whose memory starts above the cap is refused; the module itself at most 16 MiB (`WasmLimits`) | verified |
| T | Calls share state, so one input affects another's output | A fresh store and instance for every call | verified |
| T | A module returns a pointer or length outside its memory | Reads are bounds-checked through wasmi's `Memory::data`; at most 4 MiB; UTF-8 required | verified |
| T | Plugin output drives the terminal: cursor moves, titles, OSC 52 clipboard writes, hyperlinks | Every control, ESC included, made visible in transform, highlighter and markup output; `fence-ansi` keeps SGR only and drops OSC ([`mod.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/src/plugin_loading/mod.rs), [output hygiene](plugin-loading.md#output-hygiene)) | verified |
| S | A plugin shows one URL and links to another | Links and click meta are cleared from every style in plugin output and in highlighter spans (`without_links`, `clear_meta_and_links`) | verified |
| S | A runtime plugin replaces a built-in or another plugin's capability | `add_plugin`'s name rules and conflict check: a name already provided is refused | documented |
| D | A highlighter returns millions of spans | At most 10,000 spans per call (`MAX_SPANS`); a span off a character boundary or with a bad style is skipped | verified |

**Gaps:**

- **A WASM plugin still decides what the user reads.** A transform or
  highlighter can change or hide text within its output budget: the sandbox
  keeps it from the system, not from lying about content. The review should
  decide whether output shown from a community plugin is marked as such.
- **Time per render is bounded only per call.** A page with many fences
  calls a plugin once per fence, each with its own 50 million units of fuel;
  there is no budget per render or per process. **gap** (availability).
- **Load-time cost.** Validation and wasmi's translation of a 16 MiB module
  are not fuel-metered. **unverified** whether translation is lazy and
  metered.
- **Native plugins in the Python package and other hosts.** Whether every
  host that enables `dylib-plugins` (the Python bindings included) keeps the
  same "only a path the user chose" rule. **unverified**.

## 2. The marketplace and its signer

Documented in [the listing format: Signing](marketplace.md#signing) and
[Key rotation](marketplace.md#key-rotation), [the Marketplace
page](../marketplace.md#mkt-trust) and [PLUGINS: the marketplace
client](../PLUGINS.md#the-marketplace-client-0022). Code:
[`crates/rich-marketplace`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/rich-marketplace),
the CLI's [`marketplace.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/src/marketplace.rs),
and the marketplace repository's workflows, kept in
[`docs/marketplace-repo/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/docs/marketplace-repo).

**Assets:** the main and backup signing keys; the deploy key (moving to a
GitHub App in [workstream 8](../plans/0.0.24.md)); the integrity of
`index.json`, every listing and every artifact; the user's install
directory (its lockfile, its state file and the installed files); which
packages a user believes are official.

**Trust boundaries:**

- a listing pull request (anyone) and `main` of the marketplace repository
  (a maintainer's merge);
- `main` and the signing job in the protected `marketplace-signing`
  environment (a required reviewer);
- the signing job and third-party code it runs (actions, pip and apt
  packages, rs-rich-cli's validator at `RS_RICH_CLI_REF`);
- the network (index, listings, key generations, artifacts on any HTTPS
  host) and the client;
- the install directory on disk and each later run that loads from it.

**Entry points:** `index.json` and `index.json.minisig`;
`keys/keys-N.json` and their signatures; `packages/ID.json`; artifact URLs;
`--marketplace`, `RICH_MARKETPLACE`, `--marketplace-keys`,
`RICH_MARKETPLACE_KEYS`, `--plugin-dir`, `RICH_PLUGIN_DIR`;
`marketplace.lock.json` and `marketplace.state.json`.

| | Threat | Mitigation | State |
|---|---|---|---|
| T | A mirror, proxy or compromised host serves a changed index | minisign `ED` signature by the main key, the key id checked first and revoked ids refused; the trusted comment's serial must equal the index's ([`index.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/index.rs), `verify_index`) | verified |
| T | An old, validly signed index is replayed (rollback, freeze) | The last accepted serial and hash per marketplace location are kept in `marketplace.state.json`; a lower serial, or the same serial with another hash, is refused; an index expires 30 days after it is signed ([`marketplace.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/src/marketplace.rs), `open`) | verified |
| T | A listing or artifact is swapped after signing | The index pins each listing's SHA-256, each listing pins each artifact's size and SHA-256; the download is read to at most the declared size and must match both ([`install.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/install.rs)) | verified |
| T | An installed file is changed on disk between runs | Each run checks every file against the lockfile before loading, and WASM is loaded from the bytes it checked ([`lockfile.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/lockfile.rs), `verify`; `load_entry`) | verified |
| T | A listing's id or file name writes outside the install directory | Ids are lowercase letters, digits and `-`; the file is `<id>/<id>-<version>.<ext>` with an alphanumeric extension; the lockfile accepts only two plain path components; the write is atomic through a temporary file beside it | verified |
| E | A package installs a native library | Installable kinds are WASM plugins, micro packs and themes; a crate is listed only. A WASM artifact is loaded in the sandbox at install and its manifest compared with the listing | verified |
| S | A community package passes as official | Reserved ids and names need a maintainer's label; tiers need `tier-approved` ([`check-listings.yml`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/marketplace-repo/.github/workflows/check-listings.yml)); the CLI shows the tier and asks before installing unless `--yes` | documented (labels), verified (prompt) |
| T | A listing pull request weakens its own checks or reads secrets | `pull_request`, not `pull_request_target`; read-only token, no secrets; checks run from the base branch's scripts | documented |
| S | A leaked main key signs a malicious index | The backup key, kept offline, signs a new generation that revokes the main key; clients walk the chain one generation at a time, and a revoked key never returns ([`keys.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/keys.rs), `rotate`) | verified |
| S | A rotation replaces the backup key with only the backup's signature | A generation signed by the backup alone must revoke the main key and keep the same backup | verified |
| E | The signing job's secrets are taken by a merged listing or a scheduled run | The `marketplace-signing` environment runs from `main` only, after a required reviewer; the key is written to a mode-600 file in `$RUNNER_TEMP` and removed; its shape, never the key, is printed ([`sign-index.yml`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/marketplace-repo/.github/workflows/sign-index.yml)) | documented (environment rules are GitHub settings) |
| D | A huge index, listing or artifact exhausts memory | Every fetch has a byte bound (index, listing, keys, signature; artifact at most its declared size and its kind's maximum); one 60-second budget for a download and its redirects | verified |
| I | Installs reveal what a user installs | Nothing is sent but the downloads themselves; download counts and ratings live in `stats.json`, outside the signed index, and `rich` never reads them ([Marketplace: Trust](../marketplace.md#mkt-trust)) | documented |

**Gaps:**

- **Supply chain of the signing job.** Its actions were pinned by tag, not
  by commit; `jsonschema` was installed from PyPI unpinned, in the same job
  that later receives the key; the validator was fetched from rs-rich-cli
  at `RS_RICH_CLI_REF`, `main` unless the variable was set. Now every
  action in [`sign-index.yml`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/marketplace-repo/.github/workflows/sign-index.yml)
  is pinned to a full commit SHA, `jsonschema` and its dependencies are
  installed only from
  [`requirements-signer.txt`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/marketplace-repo/.github/requirements-signer.txt)
  with `--require-hashes`, and `RS_RICH_CLI_REF` defaults to the commit of
  rs-rich-cli's `release-0.0.23` tag (`check-listings.yml` too). Applies
  once the marketplace repository takes the change. **mitigated**
  (0.0.24). Left: `minisign` still comes from Ubuntu's signed apt archive,
  and the other marketplace workflows (no signing secrets) still pin
  actions by tag.
- **Revocation needs the client to see the next generation.** Key
  generations are fetched from the same location as the index. A client
  pointed at a location that withholds `keys-N+1.json` (a stale mirror, a
  `RICH_MARKETPLACE` the attacker chose) kept trusting a revoked main key,
  even when another location had already shown it the revocation, because
  the walk's starting point was kept per location. It is now kept per trust
  root, so once any location has served a revoking generation, an index
  signed by the revoked key is refused everywhere, with an error saying the
  location is stale or withholding keys
  ([`marketplace.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/src/marketplace.rs),
  `open`, `stale_keys_hint`; tested in
  [`plugin_marketplace.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/tests/plugin_marketplace.rs)).
  **mitigated** (0.0.24) for a client that has seen the revocation. Left,
  by design: a client that never has depends on the compiled-in generation,
  so a revocation is followed by a release that compiles in the new one. A
  signed "current generation" field in the index was considered and not
  added: the index is signed by the main key, so a holder of a leaked main
  key could write any value there.
- **The state file was a trust anchor.** `marketplace.state.json` cached
  the newest verified key generation as a key set, read back without
  re-verification and checked only against the root's main key id, and the
  install directory was made with the default permissions (`umask`). The
  state now keeps the generations as their `keys-N.json` files and
  signatures, verified again from the trust root (compiled in, or the one
  named) whenever they are read; a chain that does not verify is dropped
  with a warning, so the file can take the walk back to the root but never
  add a key. A 0.0.23 cached key set is not read. Directories the
  marketplace creates are mode 700 on Unix (`create_private_dir_all`). Same
  links as above. **mitigated** (0.0.24). Write access to the install
  directory still lets someone change what is installed, as the lockfile's
  entry below says.
- **`RICH_MARKETPLACE_KEYS` and `--marketplace-keys` replace the trust
  root** with any file. That is their purpose; an environment variable is,
  however, easy to set in a shared shell profile or CI. Consider a warning
  whenever the root is not the compiled-in one. **gap** (low).
- **Micro packs are checked at registry load, then read by path** when
  their images are drawn, so a file swapped after the check is read
  unchecked. Low: it needs write access to the install directory. **gap**.
- **The move to a GitHub App** (workstream 8) changes who can push to the
  marketplace's `main`; the review covers the App's permissions and key
  storage. **planned**.

## 3. `rich serve` and rs-rich-web

Documented in [Serving an app to a browser: Security](../guide/intuituive/web.md#security).
Code: [`crates/rich-web/src`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/rich-web/src)
(`server.rs`, `http.rs`, `isolate.rs`, `pty.rs`, `session.rs`) and the
CLI's [`serve.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/src/serve.rs).

**Assets:** whatever the served app or program can do: for `rich serve --
bash`, a shell as the user. The token. The server's availability. The
browser's clipboard.

**Trust boundaries:**

- the network (or other local users) and the HTTP server;
- any web page in the user's browser and the server's WebSocket on a
  loopback port;
- the server and each session's process (Unix: its process group;
  Windows: a job object);
- a served program's output and the page (xterm.js).

**Entry points:** the HTTP request line and headers (`Host`, `Origin`, the
`token`, `cols`, `rows`, `renderer` parameters); WebSocket frames (keys,
pastes, mouse, resizes, acknowledgements); the loopback port each session's
process connects back to; `RS_RICH_WEB_SESSION`.

| | Threat | Mitigation | State |
|---|---|---|---|
| E | Someone on the network opens a session | Listens on `127.0.0.1` by default; any other address prints a warning ([`serve.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/src/serve.rs)) | verified |
| S | A request without the printed address opens a session | A 128-bit token from the OS's random source, required by the page and the upgrade, compared in constant time ([`server.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-web/src/server.rs), [`http.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-web/src/http.rs), `same_token`) | verified |
| S | Another site's page opens a WebSocket to the local port (cross-site WebSocket hijacking) | The `Origin` must be `http://` plus the `Host` the request was sent to, and that `Host` must be the bound address (or `localhost`) unless bound to all interfaces; or an origin given with `--allow-origin`. No origin, no session (`origin_allowed`) | verified |
| S | DNS rebinding makes the attacker's name resolve to `127.0.0.1` | For a loopback or specific address, `Host` must be that address or `localhost` | verified |
| T | The page is framed, or loads another site's script | CSP `default-src 'none'`, scripts and styles from itself, `connect-src` to itself, `frame-ancestors 'none'`, `X-Frame-Options: DENY`; xterm.js is vendored, nothing from a CDN; the title is HTML-escaped and the `Host` filtered before it enters the CSP | verified |
| D | Slow or many connections, or many tabs | 10 seconds to send a request; 64 pending at most, then `503`; sessions capped (8 by default) | verified |
| D | A huge frame or a flood of input | WebSocket messages and frames at most 1 MiB; terminal size bounded (`MAX_CELLS`); output backpressure (`max_buffered`) and 1 MiB of unread input | verified (limits), documented (backpressure) |
| E | A crashing or runaway app takes down the server or other tabs | One process per session by default, killed with its process group or job object; optional memory and CPU caps on Unix ([`isolate.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-web/src/isolate.rs)) | documented |
| S | Another local process connects to a session's loopback port first | The child proves itself with a secret from its environment and the server answers with a second one; 2 seconds to send it | verified |
| T | A served program's output writes the browser's clipboard | OSC 52 is honoured for apps only, never for programs, whose output can hold anything ([`app.js`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-web/assets/app.js)) | verified |

**Gaps:**

- **No TLS, and the token travels in the URL.** It is in the printed
  address, the browser's history and any proxy's access log, and lasts as
  long as the server. Behind a reverse proxy, the proxy's logs need care.
  Consider moving the token to a cookie or the first WebSocket message after
  the page loads. **gap** (documented as "use a proxy with TLS").
- **Bound to all interfaces,** any `Host` is accepted as long as the origin
  matches it, so DNS rebinding is stopped by the token alone. **gap**
  (low; the bind already warns).
- **Session processes inherit the server's whole environment,** secrets
  included; a served program can read them. Document it, or offer an
  allow-list. **gap** (low).
- **xterm.js's handling of links (OSC 8) and other OSC strings** in a served
  program's output: what the vendored xterm.js 6.0.0 does when one is
  clicked. **unverified**.
- **Resource caps on Windows** (memory and CPU) are Unix-only. **gap**
  (documented).

## 4. Noctiluca and Chrome

Documented in [Privacy and security](../guide/noctiluca/privacy.md) and
[the browser design: Security](browser.md#security). Code:
[`crates/noctiluca/src`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/noctiluca/src)
(`cdp.rs`, `clean.rs`, `url.rs`) and rs-rich-embed's
[`chrome.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-embed/src/chrome.rs).

**Assets:** the browser profile (cookies, storage, history), above all a
kept one with signed-in sessions; local files reachable as `file://`; the
user's terminal; what the user believes about a page (its real host, which
link goes where); the DevTools channel, which controls the whole browser.

**Trust boundaries:**

- a web page (inside Chrome's sandbox) and Noctiluca's process: everything
  the page says arrives as DevTools JSON;
- Noctiluca and the user's terminal;
- other local processes and users and the DevTools endpoint;
- (planned) an agent or script and the control socket of
  [#816](https://github.com/buchochelliq-labs/rs-rich-cli/issues/816).

**Entry points:** DOM snapshots and accessibility trees; titles, URLs, link
targets, alt text, option labels, dialog text, download names; the
bookmarks file; DevTools events (`Runtime.bindingCalled`,
`Browser.downloadWillBegin`); the address bar; Chrome's switches; `--chrome`
and `RICH_EMBED_CHROME`.

| | Threat | Mitigation | State |
|---|---|---|---|
| T | Page text drives the terminal (escape sequences) or styles itself with rich markup | C0, C1 and DEL removed from everything a page says, and square brackets shown as text, never read as markup ([`clean.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/noctiluca/src/clean.rs); [page text is cleaned](../guide/noctiluca/privacy.md#page-text-is-cleaned)) | documented |
| S | Text reads in a false order (`exe.png` for `gnp.exe`), or a host is confusable | Bidirectional overrides, embeddings, isolates and marks removed; the real host shown, punycode for confusable names ([browser design](browser.md#security)) | documented |
| E | Page script reaches Noctiluca's process | No page script runs there; the page is read by two DevTools calls it cannot observe; Noctiluca's one script runs in an isolated world and reports only "something changed" | documented |
| E | Chrome's sandbox is turned off | Every switch that turns any part of it off (`--no-sandbox`, `--disable-*sandbox*`, `--single-process`, `--no-zygote`, any spelling) is dropped, so Noctiluca cannot run as root ([`chrome.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-embed/src/chrome.rs), `turns_sandbox_off`) | verified |
| E | Another process drives the browser through DevTools | Linux and macOS: `--remote-debugging-pipe`, no port; every `--remote-debugging-*` switch from the caller is dropped. Windows: a random `127.0.0.1` port, reachable by local users, documented | verified |
| I | Cookies and storage outlive the run, or other users read them | A new temporary profile per run, mode 700, removed on quit, signals and panics; a kept profile is opt-in (`--profile NAME`), private, under the data directory ([the browser profile](../guide/noctiluca/privacy.md#the-browser-profile)) | verified (mode, creation), documented (removal) |
| T | A page writes files through downloads | Downloads are denied (`Browser.setDownloadBehavior`); Noctiluca sets no quarantine ([`main.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/noctiluca/src/main.rs)) | verified |
| I | Password fields reach the screen, the model or `--dump` | Never read; shown as a fixed mask; the change-watcher leaves them out | documented |
| E | A page navigates to `file://` or `chrome://` | Links are followed by clicking them in the page, so Chrome's own rules apply; only the user's address bar and bookmarks call `Page.navigate` directly | verified (link path), unverified (Chrome's behaviour for every scheme) |
| I | Noctiluca phones home | No telemetry, update check or account; searches go to the configured engine | documented |

**Gaps:**

- **Extra Chrome switches.** Only sandbox and DevTools switches are
  filtered. A caller of `ChromeEngine::args` can still pass switches that
  weaken the browser in other ways (`--disable-web-security`,
  `--allow-file-access-from-files`, `--remote-allow-origins`,
  `--load-extension`). Noctiluca passes none today. Decide whether the
  filter becomes an allow-list. **gap** (low).
- **Downloads by quarantine** are designed (ask first, a cleaned name, never
  run) but not wired into Noctiluca; when they are, the review covers the
  quarantine's location, its permissions and the move. **planned**.
- **The agent-control CLI (#816):** a control socket, `noctiluca ls` and
  `action`, Ctrl+G and `--read-only`, off by default and generic in the
  framework, with the agent's verbs in Noctiluca. To review when it lands:
  - the socket's location and mode (this user only), and that it refuses
    other users' connections (peer credentials on Unix; a named pipe's
    ACL on Windows);
  - `--read-only` enforced by the server, not by the client asking nicely;
  - an agent driving a kept, signed-in profile: page text is attacker
    input to the agent (prompt injection), so what a page says must not
    turn into actions the user did not approve; consider refusing control
    with a kept profile unless the user says so;
  - every action recorded somewhere the user can read (repudiation).
  **planned**.
- **The SGR-Pixels mouse (#819) and terminal hosts' APIs (#820):** new
  input parsers (fuzz them) and, for #820, a client that talks to the
  terminal's own control channel (kitty's or WezTerm's remote control, for
  example), which can run commands in the terminal. It should use only the
  verbs it needs, and only on the user's own terminal. **planned**.
- **`--ssh HOST` (#817):** where Chrome and the profile live, and what
  crosses the connection. **planned**.
- **Bot checks:** the `Runtime` domain is visible to pages; accepted and
  documented, not a security gap.

## 5. rs-rich-embed's PTY host

Code: [`host.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-embed/src/host.rs)
(`PtyHost`, `LocalPty`), [`term.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-embed/src/term.rs)
and [`pane.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-embed/src/pane.rs);
the emulator is rs-rich-record's (vt100).

**Assets:** the user's real terminal; the host app's availability; the
child's process tree.

**Trust boundaries:** the child program's output and the host app's screen;
the host app and the child (the child runs as the user, unsandboxed: it is
the program the app chose to run).

**Entry points:** every byte the program writes; its exit; resizes and
input the app forwards.

| | Threat | Mitigation | State |
|---|---|---|---|
| T | The program's escape sequences reach the real terminal (title, clipboard, hyperlinks, queries) | Its output goes into the VT emulator; the pane draws only each cell's text and its colours and attributes, rebuilt as rich segments, so no sequence passes through ([`term.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-embed/src/term.rs), `lines`) | verified |
| T | A malformed sequence crashes the emulator | The VT emulator is fuzzed (`vt`) | documented |
| D | A program floods output | Output is read on the host's thread into a buffer; rs-rich-web's sessions add backpressure. Scrollback is bounded (`DEFAULT_SCROLLBACK`) | documented |
| D | A program that ignores SIGHUP outlives the app | Hung up on, then its process group killed with SIGKILL a second later | verified |
| I | The program reads the app's environment | It inherits the app's environment, with `TERM=xterm-256color` | verified (by design) |

**Gaps:**

- **Cell text itself:** whether vt100 can store a control or zero-width
  character in a cell that then reaches the terminal. **unverified**.
- **Queries the emulator answers** (device attributes, cursor position):
  a program learns only what any terminal would tell it. **unverified**.
- **Windows:** ConPTY's behaviour, and killing the child's tree there.
  **unverified**.

## 6. TLS, certificates and redirects

Code: rs-rich-marketplace's [`tls.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/tls.rs),
[`redirect.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/redirect.rs)
and [`transport.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/transport.rs),
used by the marketplace, `rich URL` (the CLI's `fetch_url`) and the
marketplace's download statistics. Chrome does its own TLS.

**Assets:** the integrity and confidentiality of fetched documents; the
user's terminal (for `rich URL`); the client's availability.

**Trust boundaries:** the network and the client; the system's certificate
store and the bundled roots.

**Entry points:** server certificates; status lines, headers and
`Location`; bodies, including compressed ones.

| | Threat | Mitigation | State |
|---|---|---|---|
| S | A network attacker serves a forged certificate | rustls through ureq, verification always on; the system's store (the platform verifier), Mozilla's bundled roots only when that store cannot be used ([`tls.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/tls.rs), `may_be_trust`) | verified |
| T | A redirect downgrades a marketplace download, or a `rich URL` fetch, to `http://` | Marketplace downloads and statistics are HTTPS only, redirects included; ureq's own redirects are off and each hop is checked ([`redirect.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/redirect.rs), `get_following`); `rich URL` refuses `https://` to `http://` (0.0.24) | verified |
| D | Endless redirects or a slow server | At most five hops for the marketplace (ten for `rich URL`, three for statistics); one overall deadline for the whole chain and its body | verified |
| D | A decompression bomb | `rich URL` caps the decoded body at 16 MiB; the marketplace reads at most the declared size after decoding | verified |
| T | TLS-inspecting proxies break verification | Accepted: their root is in the system's store, and the marketplace's trust comes from signatures and hashes, not TLS | documented |

**Gaps:**

- **The bundled fallback could override the system's decision.** Any TLS
  or I/O failure with the system's store was retried with the bundled
  roots, so a root removed from the system store on purpose, or a
  revocation the platform verifier enforced, could be overridden. The
  bundled roots are now tried only when the system store cannot be used
  at all (no CA certificates loaded, or no platform verifier); a
  certificate the system's verifier refused stays refused
  ([`tls.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/src/tls.rs),
  `may_be_trust`; tested in
  [`https_trust.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-marketplace/tests/https_trust.rs)).
  **mitigated** (0.0.24).
- **`rich URL` followed a redirect from `https://` to `http://`** (as
  upstream's `requests` does). It now refuses one before requesting it, with
  an error naming both addresses; an `http://` URL given as such is still
  fetched ([`lib.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/src/lib.rs),
  `fetch_url`; tested in
  [`url_fetch.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-cli/tests/url_fetch.rs);
  [DIVERGENCES §50](../DIVERGENCES.md#50-rich-url-no-https-downgrade-and-fetched-content-is-sanitized)).
  **mitigated** (0.0.24).
- **`rich URL` rendered fetched content unsanitized** in upstream's modes,
  so a server could send escape sequences to the user's terminal (titles,
  OSC 52 clipboard writes, OSC 8 links). Content fetched from a URL is now
  sanitized by default in every mode, `rich chart` and `rich profile`
  included; `--no-sanitize` opts out (same links as above). **mitigated**
  (0.0.24).
- **No certificate pinning** for the official marketplace. Not needed while
  signatures carry the trust; recorded so the review confirms it.

## 7. ACP agents for Cornacai

**Planned.** Cornacai is a front end for coding agents over the Agent
Client Protocol (JSON-RPC on stdio), built as two new crates, rs-rich-acp
and Cornacai ([0.0.24 plan, workstream 1](../plans/0.0.24.md#1-cornacai-all-four-phases-794-acp-new-cornacai-new)).
Its design note, `docs/design/cornacai.md`, is being written in that
workstream; this section states the intended model and is reconciled with
the design note when it lands.

**Assets:** the user's workspace and everything outside it; terminals the
client starts for the agent; the user's attention and consent (permission
prompts); the user's terminal.

**Trust boundaries:**

- the agent process and Cornacai. ACP does not sandbox the agent: it runs
  with its own process's permissions, so Cornacai guards only what the agent
  asks the *client* to do;
- the agent's text (and anything it read: files, web pages, tool output,
  any of which may carry prompt injection) and the user's screen;
- the workspace and the rest of the file system.

**Entry points:** `initialize`'s answer, `session/update` notifications
(text, tool calls, plans, diffs), `session/request_permission`,
`fs/read_text_file`, `fs/write_text_file`, `terminal/*`; the agent's launch
command (and, in phase 4, the marketplace's `agent` kind).

| | Threat | Intended mitigation | State |
|---|---|---|---|
| E | The agent reads or writes files it was not given | File-system and terminal capabilities are opt-in, offered in `initialize` from the user's settings; a request for one not offered is refused before the app sees it; paths are confined to the session's workspace, with symbolic links resolved at the time of use | planned |
| E | The agent writes to files that control execution (`.git/hooks`, editor, agent and MCP settings) | Such writes ask the user even when writes are on | planned |
| S | A permission prompt misleads the user | Every request is shown, with commands and paths in full; the agent's own words (title, option names) are marked as such; no auto-approve by default | planned |
| T | Agent text drives the terminal or hides content (escape sequences, bidirectional and zero-width characters) | Agent text, diffs, commands and terminal output are sanitized; Markdown is rendered without raw HTML; links open only on user action | planned |
| I | Images or links in agent output fetch from an agent-chosen URL | Images only from inline data; nothing fetched without the user | planned |
| S | An agent claims to be another | Identified by its launch command, never by what it reports about itself | planned |
| D | A stuck prompt or terminal wait stalls the session | Agent-to-client requests on their own threads; cancelling answers every pending permission request as cancelled | planned |
| E | A marketplace `agent` listing installs a malicious agent | The `agent` kind (phase 4) is designed with the marketplace's signing and tiers; an agent runs as the user, so it is trust-only like a native plugin | planned |

**Gaps** to settle in the design: whether a terminal the agent starts is
confined at all (it runs as the user); an audit log of every approved
action; and the session store's location and permissions once sessions are
saved.

## The review checklist

For [stage 4](../plans/1.0.0.md#stage-4-hardening-01x). For each item:
check the threat against the code at the release candidate and its tests,
then fix it, add a test, or record the decision here (with the release
that decided it). A finding is reported and handled as
[`SECURITY.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/SECURITY.md)
says, not in a public issue.

### Plugins

- [ ] A `./rich.toml` (every table and profile) cannot load a plugin; test
      each of the `UNTRUSTED_*` keys.
- [ ] Native plugins: the descriptor checks in `read_descriptor`, the
      panic guard in `export_dylib_plugin!`, and every host that enables
      `dylib-plugins` (the Python bindings included).
- [ ] WASM: a module with any import is refused; memory, table, fuel and
      module-size limits each have a test that hits them; no host function
      can be reached.
- [ ] WASM: load-time cost of the largest allowed module; whether wasmi's
      translation is metered.
- [ ] Decide on a per-render or per-process budget for plugin calls.
- [ ] Output sanitizing: every capability kind, with ESC, C1, OSC 8,
      OSC 52 and bidirectional controls in the fixture.
- [ ] Links and click meta removed from markup and highlighter spans.
- [ ] Name rules and conflicts in `add_plugin`.

### The marketplace and its signer

- [ ] Signature checks: wrong key id, revoked key, legacy `Ed`
      signatures, a moved signature (serial mismatch), bad trusted
      comments.
- [ ] Rollback, serial reuse and expiry, per marketplace location.
- [ ] Key rotation: planned, main replaced, backup replaced without main,
      a revoked key returning, generation order; withholding a generation
      (the revocation gap).
- [ ] Decide whether `marketplace.state.json`'s cached key chain is
      re-verified from the root on load, and whether the install directory
      is made private.
- [ ] Install: size and hash before any write, the path rules, the atomic
      write, the WASM manifest comparison, every kind's maximum size.
- [ ] Load: every run checks the lockfile; micro packs read after the
      check.
- [ ] The signing workflow: actions pinned by commit, Python packages
      pinned by hash, `RS_RICH_CLI_REF` set to a tag, the environment's
      required reviewer and branch rule, and the GitHub App's permissions.
- [ ] Listing checks: reserved names, tier and recommendation labels,
      `pull_request` only.
- [ ] A warning when the trust root is not the compiled-in one.

### `rich serve` and rs-rich-web

- [ ] Token: entropy, constant-time comparison, required by the page and
      the upgrade; decide whether it moves out of the URL.
- [ ] Origin and `Host`: cross-site WebSocket hijacking and DNS rebinding,
      for loopback, specific and unspecified bind addresses, and with
      `--allow-origin`.
- [ ] CSP and framing headers on every page; no external loads.
- [ ] Limits: pending connections, request time, sessions, message and
      frame size, terminal size, output and input backpressure.
- [ ] Session processes: the hello secret, the kill on close and on server
      death (Unix and Windows), memory and CPU caps; the environment they
      inherit.
- [ ] xterm.js's OSC handling in program mode (OSC 52 off, OSC 8 links).

### Noctiluca and Chrome

- [ ] Every string from a page goes through `clean` before the terminal,
      including dialog text, download names and bookmarks.
- [ ] Sandbox and DevTools switch filters; decide on an allow-list for
      other switches.
- [ ] The DevTools pipe on Unix; the Windows port's exposure.
- [ ] Profiles: mode 700, removal on every exit path, kept profiles opt-in.
- [ ] Downloads denied; the quarantine design before it is wired in.
- [ ] Password fields never read.
- [ ] Navigation to `file://`, `chrome://` and `javascript:` from a page.
- [ ] The control socket (#816), `--read-only`, and prompt injection
      through page text, once it lands.
- [ ] The terminal hosts' API client (#820) and the pixel mouse parser
      (#819), once they land.

### The embed PTY host

- [ ] No escape sequence from the child reaches the real terminal (a
      fixture with OSC 0, 8 and 52, DCS and APC strings).
- [ ] Cell text cannot carry control characters.
- [ ] The child's process group is killed on drop; ConPTY on Windows.

### TLS and redirects

- [ ] Verification is on for every client; no `danger` configuration.
- [ ] Decide when the bundled-roots fallback applies.
- [ ] HTTPS-only redirects for the marketplace; the hop limits and the
      overall deadline.
- [ ] Decoded-size limits for every body.
- [ ] Decide on sanitizing `rich URL`'s content by default, and on a
      downgrade warning.

### ACP agents

- [ ] Capabilities off by default, and refused before the app when not
      offered.
- [ ] Workspace confinement: absolute paths, symbolic links, `..`, races
      between the check and the use.
- [ ] Writes to hooks and settings ask.
- [ ] Permission prompts show the full command and path, and mark the
      agent's words.
- [ ] Agent text sanitized; no fetches without the user.
- [ ] Cancellation answers pending requests.
- [ ] The marketplace's `agent` kind.

### Across surfaces

- [ ] Every parser of outside input has a fuzz target, and the four-week
      clean run of [fuzzing](fuzzing.md#the-four-week-clock) is met.
- [ ] An advisory check (`cargo audit` or `cargo deny`) runs in CI against
      the lockfiles; none does today.
- [ ] The mitigations marked **documented** or **unverified** above are
      read in the code and tested, and their state updated here.
