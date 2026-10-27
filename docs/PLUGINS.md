# Plugin & extension design

This port keeps its core a faithful mirror of upstream `rich`. Everything we add
lives on the *outside* of that core, through a small set of **extension points**.
This document describes how that works today (internal-facing) and the intended
path to opening it up to third-party plugins.

## Why

If our own features were woven into `crates/rich`, every upstream sync would be a
manual three-way merge. Instead:

- `crates/rich` defines **extension-point traits** and calls them, but ships only
  upstream's built-in implementations. It has **no knowledge** of `rich-ext`.
- `crates/rich-ext` provides extra implementations and **registers** them onto a
  `Console`. Syncing upstream never touches `rich-ext`.

## Extension points (today)

Defined in [`crates/rich/src/protocol.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich/src/protocol.rs):

| trait          | upstream analogue          | purpose |
|----------------|----------------------------|---------|
| `Renderable`   | `__rich_console__` protocol | make a custom type printable by `Console` |
| `LineRenderable` | incremental consumption of rendering generators | stream styled lines without collecting the full rendered output; implemented by `Table` |
| `Highlighter`  | `Highlighter` ABC          | add style spans to `Text` (numbers, URLs, syntax, …) |
| `FenceRenderer` | none (upstream always highlights a fence) | draw ```` ```lang ```` fences in `Markdown` instead of highlighting them, via `Markdown::fence_renderer`; with none added, Markdown is unchanged (0.0.12) |
| `CodeHighlighter` | Pygments behind `Syntax` | the syntax-highlighting engine for `Syntax` and Markdown code; `SyntectHighlighter` is the default, and `Syntax::highlighter` / `Markdown::highlighter` take any other (0.0.12) |

More seams (custom `Box` sets, spinners, themes) are added here as the
corresponding modules are ported — always as a trait the core calls, never as an
`if cfg!(feature = "ours")` branch inside core logic.

## Registration (explicit by default)

Registration is **explicit** by default: a host names each plugin it adds, which
is easy to debug, reason about and test, and keeps the install order
deterministic. Linked and runtime plugins (below) are opt-in on top of that.

```rust
use rich::{Console, ColorSystem};
use rich_ext::{ExtensionRegistry, ConsoleExt};

// Option A: the convenience trait
let mut console = Console::new();
console.install_extensions();

// Option B: a registry you compose yourself
let mut console = Console::new();
let mut registry = ExtensionRegistry::new();
registry.register_highlighter(|| Box::new(rich_ext::NumberHighlighter::new()));
registry.install(&mut console);
```

The registry ([`crates/rich-ext/src/registry.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/src/registry.rs))
holds *factories* so one registry can be installed onto many consoles.

## Writing a plugin (0.0.12)

The plugin contract is its own crate, `rs-rich-plugin-api` (imported as
`rich_plugin_api`, and re-exported as `rich_ext::plugin`). It depends only on
core `rich`, so a plugin crate never depends on `rich-ext`, and every
first-party plugin goes through the same contract.

A plugin implements `Plugin`: `metadata()` says who it is, and `register()`
adds capabilities through a `PluginRegistrar`:

| registrar method | adds |
|---|---|
| `highlighter(factory)` | a regex `Highlighter` for printed text |
| `code_highlighter(name, engine)` | a `CodeHighlighter`, selectable by name |
| `theme(name, theme)` | a named `Theme` |
| `box_style(name, box)` | a named table/panel box style |
| `renderer(name, renderer)` | a `SourceRenderer` that turns source text into a renderable |
| `fence_renderer(language, renderer)` | a `FenceRenderer` for Markdown fences in that language |
| `transform(name, transform)` | a `TextTransform`, chained by name into a text pipeline (0.0.12; see [Transforms](guide/ext/transforms.md)) |
| `action(name, action)` | a `CustomAction` on interactive views: list items, table rows, tree nodes and file entries (0.0.13, #491; see [Actions](guide/interact/index.md#actions)). It has a default body, so a host without interactive views ignores it |
| `component(name, factory)` | an interactive component (a `PluginComponent`, from `rich_plugin_api::component`) that an app mounts by name beside the built-ins with `rich_interact::plugin::PluginView` (0.0.14; see [Components from plugins](guide/interact/custom-components.md#components-from-plugins)). The factory makes a fresh one per mount. It has a default body, so a host without interactive views ignores it |

```rust
use rich_ext::plugin::{Plugin, PluginError, PluginMetadata, PluginRegistrar};
use rich_ext::ExtensionRegistry;

struct Solarized;

impl Plugin for Solarized {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("solarized", "Solarized themes", env!("CARGO_PKG_VERSION"))
    }
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        let theme = rich::Theme::from_styles([("repr.number", "#268bd2")], true)
            .map_err(|e| PluginError::Other(e.to_string()))?;
        registrar.theme("solarized", theme);
        Ok(())
    }
}

let mut registry = ExtensionRegistry::with_defaults();
registry.add_plugin(&Solarized)?;
```

`ExtensionRegistry::add_plugin` is the host. It is all-or-nothing: a plugin
whose `register` fails, or that breaks a rule below, leaves the registry as it
was.

- **API version.** `PluginMetadata::new` records `PLUGIN_API_VERSION`. A host
  refuses a plugin built for another version (`PluginError::IncompatibleApi`);
  `is_supported_api_version` is that check, for a tool that chooses a plugin
  before adding it. At 0.0.x the contract still changes, and every breaking
  change bumps it.
- **Names.** The plugin id and every capability name use lowercase letters,
  digits, `-`, `_` and `.` (at most 64 bytes), and start with a letter or
  digit, so they are safe to print and to use as CLI values.
- **No silent overrides.** Two plugins may not register the same capability
  under the same name, and a plugin id may be added once (`Conflict`,
  `DuplicatePlugin`).

`ExtensionRegistry::with_defaults()` adds the built-in `rich-ext` plugin, which
provides the number highlighter and the `syntect` code highlighter. Query what
is registered with `plugins()`, `code_highlighter(name)`, `theme(name)`,
`box_style(name)`, `renderer(name)`, `fence_renderer(language)`,
`transform(name)`, `transform_names()`, `actions()`, `component(name)`,
`component_names()`, `create_component(name)` (a fresh instance, or `None`
when the factory panics) and `provided_by(capability)`.
`text_pipeline(names)` chains registered text transforms, in the order given,
into a `rich_ext::transform::Pipeline`. `fences()` combines every registered fence renderer
into one for `Markdown::fence_renderer`, routed by language.

**A default code highlighter.** `ExtensionRegistry::set_default_code_highlighter(name,
theme)` chooses a registered code highlighter (and optionally one of its
themes) for every console the registry is installed onto, through core's
`ConsoleCodeHighlighting`. `Syntax`, Markdown code, `source_view` and the diff
views without a highlighter of their own use it. An unknown name or theme is a
`HighlighterChoiceError` listing the choices. The CLI exposes this as
`--highlighter` and `--code-theme`.

`rs-rich-lumis` (`LumisPlugin`) registers the code highlighter `"lumis"`:
tree-sitter grammars with lumis's Neovim themes, and `ansi_dark`/`ansi_light`
mapped from tree-sitter capture names to upstream's Pygments token styles.

The first plugin built this way is `rs-rich-mermaid` (`MermaidPlugin`): a
`mermaid` fence renderer and source renderer, with flowcharts drawn as text and
every diagram type through `mmdc` behind its `mmdc` feature.
`rs-rich-diagram` (`rich_diagram::plugin::DotPlugin`, behind its `plugin`
feature) registers DOT the same way: `dot` and `graphviz` fence renderers and a
`dot` source renderer, drawn natively through the diagram layout.
`rich doctor` lists the registered plugins and the API version (and includes
them in `--report json`).

## Document formats (0.0.19)

`rich_ext::document` keeps documentation formats in a `DocumentRegistry` of
its own, beside the extension registry: Markdown, reStructuredText, Org,
AsciiDoc and man pages are built in, and a host adds a format with
`DocumentRegistry::register(Arc<dyn DocumentFormat>)`. A format names its id,
aliases and extensions, sniffs content (a confidence from 0 to 1), and
renders source to a renderable with diagnostics; see
[Documents](guide/ext/documents.md).

A plugin cannot declare a document format through `PluginRegistrar` in this
API version: that needs a registrar method (and the extensions and sniffer it
would carry) in `rich-plugin-api`, which is left for a later version rather
than forced into `renderer`. Today a plugin registers a `SourceRenderer` with
`renderer(name, …)` as before, and a host that wants it as a document format
wraps it:

```rust
use rich_ext::document::{DocumentRegistry, SourceRendererFormat};

let renderer = extensions.renderer("textile").expect("the plugin's renderer");
let mut documents = DocumentRegistry::with_builtins();
documents.register(std::sync::Arc::new(
    SourceRendererFormat::new("textile", renderer).extensions(&["textile"]),
))?;
```

The `rich` binary uses the built-in formats only.

## Third-party plugins (0.0.13)

Beyond `add_plugin`, a plugin can reach a registry in three more ways, each
more opt-in than the last. Why they work this way is in the design note,
[Plugin loading](design/plugin-loading.md).

### Linked: add the dependency

A plugin crate registers itself once:

```rust
rich_plugin_api::export_plugin!(MyPlugin);
```

A host that wants every linked plugin asks for them:

```rust
let registry = rich_ext::ExtensionRegistry::with_linked_plugins()?;
```

They are collected at link time (with `inventory`) and added **sorted by
plugin id**, never in link order; two with the same id are an error
(`PluginError::DuplicatePlugin`) and none is added. `with_defaults()` does not
add them. The `rich` binary does, so a custom build that depends on a plugin
crate gets it. If the binary never names the crate, the linker may drop it:
write `use my_plugin as _;`.

### Runtime: native libraries and WASM modules

Runtime plugins are loaded from a path, behind `rs-rich-ext` features that are
off by default: `dylib-plugins` for native libraries (`.so`, `.dylib`,
`.dll`) and `wasm-plugins` for WASM modules (`.wasm`).

```rust
use rich_ext::plugin_loading::{load, LoadOptions};

let plugin = load(path, &LoadOptions::default())?; // a LoadError names the path
registry.add_plugin(&plugin)?;
```

Rust types cannot cross a library boundary, so a runtime plugin exchanges UTF-8
text only, through the ABI in `rich_plugin_api::abi`. It may contribute:

| kind | input | output |
|---|---|---|
| `transform` | plain text | plain text, as a named `TextTransform` |
| `highlighter` | plain text | `START END STYLE` lines (byte offsets), as a `Highlighter` |
| `fence-markup` | a fence's code and the width | `rich` markup, as a `FenceRenderer` |
| `fence-ansi` | a fence's code and the width | ANSI SGR text, as a `FenceRenderer` |

Themes, box styles, renderables, code highlighters and interactive
components stay compile-time only. A component keeps state between events,
and the runtime ABI is stateless text in and out: each WASM call runs in a
fresh instance. So a component comes from a plugin added with `add_plugin`
or linked with `export_plugin!`, and `rich plugins list` shows it as
`component "name"`.

- **A native plugin** is a `cdylib` that depends on `rs-rich-plugin-api` and
  calls `export_dylib_plugin!` with an `abi::Exports` description; it needs no
  unsafe code. See
  [`examples/dylib-plugin`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/rich-plugin-api/examples/dylib-plugin).
- **A WASM plugin** exports `memory`, `rich_plugin_alloc`,
  `rich_plugin_manifest` and `rich_plugin_call` (see `abi::wasm`) and imports
  nothing. See the hand-written
  [`examples/wasm/shout.wat`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-plugin-api/examples/wasm/shout.wat).

The ABI is versioned separately from `PLUGIN_API_VERSION`: it is
`abi::ABI_MAJOR.abi::ABI_MINOR`, 1.0. A host refuses a plugin built for
another major (`abi::is_supported` is the check both loaders make), and one
that declares a capability kind the host does not know.

### In the `rich` CLI

`rich plugins list` shows every plugin with its source (`built-in`, `linked`,
`native`, `wasm`), version, ABI and capabilities, and `rich plugins info NAME`
shows one; both take `--report json`. A default build has three built in:
`rich-ext` (the syntect code highlighter), `dot` and `mermaid`, plus `lumis` in
a build with that feature. A default build loads WASM plugins (its
`wasm-plugins` feature, on by default since 0.0.22), and a build with
`dylib-plugins` (`cargo install rs-rich-cli --features dylib-plugins`)
native ones too. `--plugin PATH` loads a runtime plugin for one run, and a `plugins = [...]`
list in `~/.config/rich/config.toml` or a file given with `--config` loads
them every time. A WASM plugin installed with `rich plugin install` (below)
loads every time too, until it is disabled. Loaded plugins draw Markdown
fences and highlight printed text, and `--transform NAME` (repeatable, in
the order given) applies their transforms to text, `--print` and
`--syntax`, after `--filter` and before `--highlight`.

### The marketplace client (0.0.22)

`rs-rich-marketplace` finds, checks and installs packages from the
[marketplace](design/marketplace.md); `rich plugin` is built on it. It has
no terminal code, and it is the only place the marketplace's trust rules
live:

- **The key chain.** `KeySet` is one generation of the main key (it signs
  indexes) and the backup key (it signs only the next generation).
  `KeySet::walk` accepts later generations one at a time, each signed by
  both keys of the one before, or by its backup key alone when it revokes a
  lost main key.
- **The index.** `verify_index` refuses a signature by any key but the
  current main key before checking it, refuses minisign's legacy unhashed
  algorithm, and checks the trusted comment's serial, the
  `schema_version`, a rollback against the last serial accepted
  (`AcceptedIndex`) and the expiry.
- **Listings.** `VerifiedIndex::read_listing` checks a listing's SHA-256
  against the index, then the rules `scripts/validate_marketplace.py`
  checks.
- **Resolution.** `resolve` picks the newest version that is not yanked and
  that this host loads, with `abi::is_supported` and
  `is_supported_api_version`, the loaders' own checks. A WASM version with
  a capability kind this host does not know is skipped.
- **Installing.** `Installer` downloads over HTTPS only, through a
  `Transport` (`HttpsTransport`, ureq, behind the default `http` feature;
  it trusts the system's certificate store first, so a TLS-inspecting
  proxy's root CA works, and Mozilla's bundled roots as the fallback: see
  `rich_marketplace::tls`),
  reads at most the declared size, checks the SHA-256, then checks the
  artifact by kind: a WASM module's header (a host with the sandbox passes
  an `ArtifactChecker` that runs `load_wasm` and `compare_wasm_manifest`),
  a micro pack's `pack.json`, a theme's parse. Only then is the file moved
  into place and recorded in the lockfile.
- **Loading.** `Installer::load` and `Lockfile::verify` read an installed
  file only when it still hashes to what the lockfile records. No network
  and no index are needed, so an expired index never stops what is
  installed.

The official public keys (generation 1, the same file as the marketplace
repository's `keys/keys-1.json`) are compiled in since 0.0.23, as
`crates/rich-marketplace/keys/keys-1.json`. The crate's tests sign a
fixture marketplace with test keys checked into
`crates/rich-marketplace/tests/fixtures/keys`, trusted only by those tests.

### `rich plugin`: installing from the marketplace (0.0.22)

`rich plugin` is the marketplace in the CLI, on `rs-rich-marketplace`:

```bash
rich plugin search diagram        # id, kind, tier, the version this host loads
rich plugin search --sort downloads  # most downloaded first
rich plugin info textcase         # tier, source, versions, what loads here, downloads, 👍/👎
rich plugin install textcase      # shows tier, permissions, capabilities; asks
rich plugin install textcase@0.1.0 --yes
rich plugin list                  # what is installed; no network
rich plugin disable textcase      # keep it, do not load it
rich plugin update                # newest for this host, asking first
rich plugin verify                # every file, and every version against the index
rich plugin remove textcase
rich plugin browse                # the same, in a selector with provenance
```

On the web, the marketplace's own site,
[buchochelliq-labs.github.io/rs-rich-marketplace](https://buchochelliq-labs.github.io/rs-rich-marketplace/),
lists the same packages in a browser, with search, filters, download
counts and reviews, and the command that installs each one; the docs'
[Marketplace page](marketplace.md) does too. `rich plugin search` matches capabilities
and authors too, and filters with `--kind`, `--tier` and `--recommended`
([the CLI guide](cli.md#install-plugins-from-the-marketplace-0022)).

**Which packages are rs-rich's own.** The `official` tier, shown as
`✓ official` in the CLI and an "Official · by rs-rich" badge on the page,
is only for packages built and published by rs-rich from its own
repositories: the marketplace refuses `official` for any other repository,
and only a maintainer can set a tier. `rich plugin info` and the install
prompt say who publishes every package and who authored it, and a
non-official package says "not by rs-rich". `reviewed` packages are shown
as **Approved**: a maintainer read the source and rebuilt the same file.
`community` packages were only checked by the marketplace's CI. A
maintainer can also mark any package `★ recommended`, with a short reason
([the format](design/marketplace.md#recommended)).

- **What is listed (0.0.23).** The official marketplace,
  [`buchochelliq-labs/rs-rich-marketplace`](https://github.com/buchochelliq-labs/rs-rich-marketplace),
  goes live with 0.0.23. It lists:
  - the Mermaid, DOT and lumis crates;
  - `textcase`, a WASM plugin. 0.1.0 is
    `crates/rich-plugin-api/examples/wasm/textcase.wat`: the `upcase` and
    `downcase` transforms and a `loud` fence. 0.2.0
    (`crates/rich-cli/official/wasm/textcase-0.2.0.wat`) adds the
    `titlecase`, `snakecase`, `kebabcase` and `camelcase` transforms;
  - `wordplay` 0.1.0, a WASM plugin
    (`crates/rich-cli/official/wasm/wordplay-0.1.0.wat`): the `rot13`,
    `reverse` and `slug` transforms;
  - the built-in micro packs `status`, `dev` and `fun`, as `.richmicro`
    archives;
  - five themes, from `crates/rich-cli/official/themes/`: `dusk` and
    `ocean` (dark), `paper` (light), `high-contrast` (every text colour at
    least 7:1 against black, WCAG AAA) and `mono` (no colour, attributes
    only). Each is an upstream `[styles]` file that sets every style of
    rich's default theme that has a colour (but the colour names, such as
    `red`); the coloured ones meet WCAG AA
    (4.5:1) against the background each file names.

  The case transforms, `rot13` and `slug` change ASCII letters only, and
  pass every byte of a non-ASCII character through as part of a word, so
  their output is UTF-8. The identifier cases and `slug` work line by line.
  `reverse` reverses each line by character cluster: a character with its
  combining marks, variation selectors, skin tones and ZWJ sequences, and a
  flag's two regional indicators, move as one. It needs no Unicode tables,
  so it is not full UAX #29 (conjoining Hangul jamo and Indic spacing marks
  are split). Each transform takes at least 256 KiB in one call within the
  default fuel. A listed version never changes, so each WASM version has
  its own source file. The plugins, packs and themes are built
  reproducibly, and `crates/rich-cli/tests/official_artifacts.rs` checks
  each rebuild against its pinned hash (`official_packages.rs` checks what
  the plugins and themes do). The marketplace serves them from its own
  `artifacts/`. Until the maintainer signs the first index,
  `search` says the marketplace has no `index.json` and exits 3.
- **Downloads, ratings and reviews (0.0.23).** `info` shows a package's
  downloads (per version), its 👍 and 👎 and its reviews link, and its
  JSON report has a `stats` object; `search --sort downloads` sorts by
  downloads. They come from the marketplace's `stats.json`, written
  nightly to its `stats` branch from GitHub's release download counts,
  crates.io's counts and each package's discussion in the "Reviews"
  category. It is not signed and is shown only: nothing installs by it,
  a missing or slow file is left out silently (3-second timeout), and
  `rich` sends nothing to be counted. To rate a package, react 👍 or 👎
  to its Reviews discussion; to review it, comment there. See
  [Stats](design/marketplace.md#stats).
- **Hosting (0.0.23).** The marketplace hosts the official WASM plugins,
  packs and themes as release assets of its repository, one release per
  version (`releases/download/ID-vVERSION/ID-VERSION.EXT`); the first
  listings keep their raw `main` URLs.
- **What installs.** A WASM plugin, a micro pack or a theme. A crate is
  listed only: `install` refuses it and prints its `cargo add` line, and
  `info` shows the same. An installed, enabled micro pack is loaded into
  micro's user layer on every run (after the user's own directory, so the
  user's own asset keeps a name both claim), once its file is checked
  against the lockfile; `disable` stops it. An installed, enabled theme is
  what `--theme ID` selects when the user's config has no theme of that
  name (the config's own theme wins a clash), once its file is checked
  against the lockfile; a changed file is not used, with a warning.
- **The prompt.** `install` and `update` print the package's tier, source,
  revision, artifact URL, SHA-256, size, requirement, capabilities and
  permissions, then ask. Nothing is downloaded before the answer. `--yes`
  answers for a script; without a terminal and without `--yes`, nothing is
  asked or written and the command exits 3. Answering no exits 1.
- **The checks.** The index's signature, key, serial (a rollback is
  refused) and expiry; each listing's hash; the version against this host
  (a yanked version, or one for another plugin ABI, is refused); then the
  download's size and SHA-256. A WASM module is loaded in the sandbox with
  the same loader `--plugin` uses, and its manifest's name, version, ABI and
  capabilities must equal the listing's. Only then is the file written,
  with the lockfile.
- **Loading.** An installed, enabled WASM plugin loads on every run without
  `--plugin` (a build without the default `wasm-plugins` feature warns and
  skips them). Each file is read once,
  checked against the lockfile, and loaded from the bytes checked. A file
  that changed on disk is not loaded: the run goes on without it and warns.
  A plugin named with `--plugin` wins over an installed copy.
- **Verifying.** `rich plugin verify` checks every installed file against
  the lockfile and every installed version against the current index: a
  version no longer listed or listed with another hash is a problem (exit
  4); a version yanked since is reported with its reason (exit 0), shown by
  `list` from then on, and replaced by `update`.
- **Where.** Packages go to `--plugin-dir DIR`, else `RICH_PLUGIN_DIR`, else
  `$XDG_DATA_HOME/rich/plugins` (`~/.local/share/rich/plugins`;
  `%LOCALAPPDATA%\rich\plugins` on Windows), as `ID/ID-VERSION.EXT` beside
  `marketplace.lock.json`. `marketplace.state.json` there keeps the disabled
  packages, the last index each marketplace served, the newest key
  generation verified and the yank reasons last seen.
- **Which marketplace.** `--marketplace LOCATION`, else `RICH_MARKETPLACE`,
  else the official one. A location is an `https://` base URL, or a
  directory (a path or a `file://` URL) laid out as the marketplace
  repository is. A local marketplace serves each artifact
  `https://HOST/PATH` from its own `artifacts/HOST/PATH`, never from the
  network: that is how the tests and CI's compatibility job run offline.
- **Which keys.** `--marketplace-keys FILE` (a `keys-N.json`), else
  `RICH_MARKETPLACE_KEYS`, else the official keys compiled into the crate
  (since 0.0.23). Later key generations are fetched from the
  marketplace's `keys/` and accepted one at a time.
- **Errors** use the CLI's exit classes: 2 usage (an unknown id, a crate, a
  version not listed, no trust root), 3 input (a file or download that
  cannot be read, no terminal for the prompt), 4 data (a signature, hash,
  manifest, yanked version or incompatible version). `--report json` writes
  the result or error envelope on stderr and nothing on stdout.

**Publishing.** `rich plugin publish FILE --url URL` checks a package and
writes its listing, for a pull request to the marketplace repository. It
needs no network: the file is read locally, and URL is where its author
will upload it. A WASM plugin is loaded in the sandbox, and its manifest
gives the id, version, ABI and capabilities (`--source-revision COMMIT` is
required); a micro pack is read with micro's limits and its `pack.json`
gives the id, version and assets; a theme must parse, and takes `--id` and
`--version`. The SHA-256 and size are computed, and the listing is read
back with the crate's own parser before it is written to
`packages/ID.json` (or `--output`). A new listing needs `--license` and
`--repository`; `--listing FILE` adds the version to an existing one,
newest first, and refuses a version already listed.

```bash
rich plugin publish shout.wasm \
  --url https://github.com/me/shout/releases/download/v0.2.0/shout.wasm \
  --source-revision "$(git rev-parse HEAD)" \
  --license MIT --repository https://github.com/me/shout
```

**Checking** a listing pull request happens in the marketplace repository
too: its `check-listings` workflow validates every listing, checks each new
version's artifact (size, SHA-256, a crate's crates.io checksum) and
installs each new WASM plugin, micro pack and theme with `rich` from a copy
of the marketplace signed with a throwaway key, with no secrets.

**Signing** happens in the marketplace repository, not here: when a listing
pull request merges, its workflow rebuilds `index.json`, bumps the serial
and signs it with the main key, held in the protected
`marketplace-signing` environment, and pushes it with a deploy key. The workflow and its setup are kept in
[`docs/marketplace-repo/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/docs/marketplace-repo)
to hand over.

## Security: threat model

Plugins are code from someone else. What each kind can do, and what stands in
its way:

- **Nothing loads unless the user opts in.** Compile-time plugins are chosen
  by whoever builds the binary. A runtime plugin needs a path: on the
  command line, or in the user's own config, or an install the user approved
  with `rich plugin install`. The `rich` binary loads sandboxed WASM plugins
  by default (its `wasm-plugins` feature, since 0.0.22); native libraries
  need its `dylib-plugins` feature, which stays off by default. A project's `./rich.toml` may **not** list plugins; `rich` ignores
  the list with a warning, so cloning a repository and running `rich` in it
  never loads that repository's code. (The same rule keeps a project config
  from choosing `mermaid_backend = "mmdc"`, `dot_backend = "graphviz"` or
  files to write.)
- **Native plugins run arbitrary code, so they are trust-only.** Loading a
  library runs its initialisers, and every call runs in the `rich` process
  with the user's permissions. The ABI makes the boundary well-defined (only
  `repr(C)` data crosses it, another ABI major is refused before the rest of
  the descriptor is read, and the functions `export_dylib_plugin!` generates
  turn a panic into an error), but it is not a sandbox. Load only a library you
  would run as a program.
- **WASM plugins are sandboxed.** A module that imports anything is refused,
  so it has no WASI, file system, network, clock or randomness. Each call runs
  in a fresh instance with a fuel limit (a loop that never ends is stopped)
  and a cap on its memory and table (a module that starts or grows past
  either is refused or trapped). A WASM plugin can waste at most one call's fuel and cannot see
  anything but its input.
- **Output is sanitized, whatever the kind.** Output over 4 MiB or not UTF-8
  is an error. Every control character in transform, highlighter and markup
  output, ESC included, is made visible. Only `fence-ansi` may carry ANSI, and
  it goes through the same sanitizer `rich view` uses: SGR styling survives,
  OSC strings (titles, clipboard, hyperlinks) are removed, and every other
  control is made visible. No style in plugin output keeps a link: `[link=…]`
  markup and `link` in a highlighter span are dropped, so a plugin cannot
  show one URL and link to another. A highlighter span that does not fit its
  text is skipped. A failed fence renders as a code block.
- **Names cannot collide or spoof.** Every plugin, compiled in or loaded, goes
  through `add_plugin`: its id and capability names are restricted to
  lowercase letters, digits, `-`, `_` and `.`, and a name another plugin
  already provides is refused, so a runtime plugin cannot replace a built-in.

## Roadmap: from internal to public

1. **Done — internal.** `rich-ext` was the only registrant.
2. **Done — a public contract (0.0.12).** `rs-rich-plugin-api` defines what a
   plugin is; the ext registry hosts it. Not yet a stability promise: see the
   API version above.
3. **Done — third-party loading (0.0.13).** Linked plugins, native and WASM
   runtime plugins, and `rich plugins list/info`, as above.
4. **Live with 0.0.23 — the marketplace (#232).** A Git-backed, signed index of
   packages ([the listing format](design/marketplace.md), 0.0.21), the
   `rs-rich-marketplace` crate (0.0.22, [below](#the-marketplace-client-0022)),
   `rich plugin install` (0.0.22), and the marketplace repository with every
   official plugin and the built-in micro packs listed and its pull request
   checks (0.0.23). It serves its first signed index once the maintainer has
   signed it.

Whatever we add, the invariant holds: **the core never learns about a specific
extension.**

`rich_ext::layout` offers bounded composition independently of core Layout.
`allocate` validates constraints and returns sizes, padding and relaxed request
indices. `fit_segments` shares Unicode-aware overflow across extension renderables.
See `cargo run -p rs-rich-ext --example layout -- 20` for a narrow sidebar layout.

### Explicit rendering environment

The sanctioned `protocol::RenderEnvironment` trait and optional
`protocol::ConsoleEnvironment` let a Console carry an immutable capability snapshot
through nested renderables. This is the only new core seam; a Console without it
keeps upstream-compatible behavior. `rich-ext::target::RenderTarget` implements
destination policy and creates configured consoles. Core has no dependency on ext,
art or CLI. Prefer one observation at the application boundary followed by explicit
capabilities; never redetect stdout inside an explicit render.

`TargetKind` covers Terminal, PlainStream, Capture, Html, Svg and Custom.
Detection provenance is separate from the rendering snapshot. `Support::Inferred`
is a conservative hint; protocol rendering can require `Confirmed`. Unicode and
hyperlink policy are declared by the caller. Legacy renderers may still have their
own sizing choices; wrap them in bounded `LayoutNode` containers for strict cells.
`layout::Overflowing` gives any renderable one explicit `OverflowPolicy`: Syntax,
JSON and Text are rendered at their measured natural width and every line is then
wrapped, folded, cropped or ellipsised to the cell width (wide glyphs never split);
output that already fits is returned byte-identical to core.
`LayoutNode` leaves receive their region's height, like upstream `Layout`, so a
`Panel` leaf fills its region; opt into `.content_height()` for natural height.

Enable `rs-rich-ext` features `testing`, `log` and `tracing` independently. The
snapshot helper has no process environment dependency or assertion-framework
requirement. See the `snapshot`, `diagnostic`, `log_adapter`, `tracing_adapter`,
`layout`, `live_regions` and `expanded_release` examples in the crate.
