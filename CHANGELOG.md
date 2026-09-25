# Changelog

Notable changes to the port. Crate versions are independent SemVer and do **not**
mirror the upstream release — which upstream version is tracked lives in
[`UPSTREAM.toml`](UPSTREAM.toml). Entries note which upstream release was
absorbed and what our own crates did.

The format loosely follows [Keep a Changelog](https://keepachangelog.com/).

## Core 0.0.5 / ext 0.0.7 / art 0.0.7 / CLI 0.0.9 — published 2026-09-22

- Core 0.0.5: optional immutable rendering-environment extension seam; unchanged
  default parity and public ConsoleOptions/Renderable requirements.
- Ext 0.0.7: explicit targets/capabilities, deterministic optional snapshots,
  constrained layouts/overflow, typed events/diagnostics, optional log/tracing
  adapters and single-writer coordinated Live regions.
- Art 0.0.7: still-image rotation, flips, grayscale and ordered Bayer dithering;
  exact Braille/half-block edge regressions and same-source output examples.
- CLI 0.0.9: directory-preserving/template batch names, config/worker routing,
  typed log presentation, still-image exports, updated guided demo and real media.
- Migration: exhaustive matches must include `Dither::Bayer4x4` and the added
  unsigned/128-bit `Value` variants. New batch naming modes take output directories.
- Fixed before merge: Live `print` no longer drops ordinary writes at interactive
  widths 0/1, and diagnostic snippets strip CRLF carriage returns.
- Release test passed on 2026-09-22 (full validation, golden parity, per-tag plans,
  packaged consumer install, installed-binary screenshots); see
  [expanded notes](docs/releases/0.0.9-expanded.md#release-test-2026-09-22).
- Published from `main` at `c645220` in dependency order by the protected release
  workflow: [`rs-rich-v0.0.5`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35789282880), [`rs-rich-ext-v0.0.7`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35792283558), [`rs-rich-art-v0.0.7`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35792299240), [`rs-rich-cli-v0.0.9`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35792320939). The core
  upload first failed because crates.io required Trusted Publishing; it succeeded
  after the crate setting was adjusted. The workflow still uses a stored token
  (tracked for 0.0.10).

## [0.0.1] — first release

The first published version of all four crates: `rs-rich`, `rs-rich-ext`,
`rs-rich-cli`, `rs-rich-art`.

**Read the version number literally.** `0.0.1` is not modesty — the API takes
breaking changes regularly (three in the week before this release), and the port
is deliberately incomplete. What is implemented is byte-parity tested against
real Python `rich` 15.0.0; what isn't is listed in the README and
`docs/PORTING.md`, and what deliberately differs is in `docs/DIVERGENCES.md`.

Two things worth knowing up front:

- **Package names carry an `rs-` prefix** because `rich` is taken on crates.io.
  The library targets keep the short names, so you write `use rich::…`, and the
  CLI's binary is still `rich`.
- **Crate versions are independent SemVer and do not mirror the upstream release.**
  Which upstream version is tracked lives in `UPSTREAM.toml`. An earlier policy
  mirrored the number; it was dropped before release because publishing a young
  API as `15.0.0` would have been a lie, and the first breaking change would have
  collided with upstream's next major.

Entries below record subsequent releases and development.

## [Unreleased]

Cohort versions for 0.0.12 (not published): core 0.0.8, plugin API 0.0.1
(new), macros 0.0.2, ext 0.0.10, art 0.0.10, Mermaid 0.0.1 (new), lumis 0.0.1
(new), CLI 0.0.12. Core changes below, so
every dependent moves with it. See the [0.0.12 plan](docs/plans/0.0.12.md).

### Pluggable code highlighters, core (0.0.12 workstream 1: #522, #523)

- **`CodeHighlighter` (#522).** A new extension point in `rich::protocol`: an
  engine takes source, a language and a theme, and returns styled spans per
  line (`HighlightedCode`, `HighlightedLine`, `HighlightSpan`, `HighlightError`).
  `Syntax::highlighter` and `Markdown::highlighter` take any engine; Markdown
  passes it to code blocks and to lexer-highlighted inline code.
  - Core validates what an engine returns against the source. It drops
    out-of-range, overlapping or misaligned spans and hyperlinks in span styles,
    renders missing lines unstyled, and falls back to the default theme for an
    unknown theme and to plain text if the engine fails. The rendered characters
    always come from the source, so an engine cannot add text or control
    sequences.
- **`SyntectHighlighter` (#523)** is the default and holds the whole `syntect`
  dependency. Output is byte-identical to 0.0.11: all 558 source, docs and data
  files in the repository render the same through `rich FILE` (default and
  `onig` builds, and with `syntax-cache`), and 150 through `rich view`.
- **ANSI themes (#523).** `ansi_dark` and `ansi_light` use upstream's
  `ANSI_DARK`/`ANSI_LIGHT` styles with the terminal's 16 colours and no
  background. Pygments token types map to TextMate scopes (DIVERGENCES #18).
- **Migration.** `Syntax::theme` names are now the highlighter's own; with the
  default nothing changes, and `ansi_dark`/`ansi_light` are new.

### Plugin API crate and ext host (0.0.12 workstream 2)

- **New crate `rs-rich-plugin-api` 0.0.1** (`rich_plugin_api`, also
  `rich_ext::plugin`). It defines the plugin contract and depends on core only,
  so a plugin never depends on `rs-rich-ext`. A plugin implements `Plugin`
  (`metadata()` plus `register()`) and adds regex highlighters, named code
  highlighters, themes, box styles and source renderers through a
  `PluginRegistrar`. `PLUGIN_API_VERSION` is 1, and every breaking change bumps it.
- **Ext host.** `ExtensionRegistry::add_plugin` checks the plugin's API version
  and names, rejects duplicate ids and conflicting capability names, and adds
  everything or nothing. Look-ups: `plugins()`, `code_highlighter()`,
  `code_highlighter_names()`, `theme()`, `box_style()`, `renderer()` and
  `provided_by()`. `with_defaults()` registers the built-in `rich-ext` plugin
  (the number highlighter and the `syntect` code highlighter). The existing
  `register_highlighter`, `install` and `install_defaults` are unchanged.
- **CLI.** `rich doctor` lists the registered plugins and the plugin API version
  in its text report and as `plugins` in `--json`.
- **Release tooling.** `rs-rich-plugin-api-v*` tags, the CI feature matrix,
  release readiness, and the release scripts and tests know the new crate. Its
  first version is uploaded by hand after core 0.0.8 and before ext 0.0.10
  (BRANCHING, "Registry authentication").

### Render tree spike (0.0.12 workstream 5: #226)

- **Design note, no code change:**
  [docs/design/render-tree.md](docs/design/render-tree.md). It covers what an
  intermediate frame would give export, snapshots, accessibility and live
  repaint, and a migration that leaves core alone.
- **Measurements** come from a standalone prototype
  (`docs/design/render-tree/prototype`, not a workspace member). Rows of
  styled runs with interned styles retain 2–7× less heap than `Vec<Segment>`,
  encode no slower, and reproduce today's bytes exactly. A per-grapheme cell
  grid costs more than segments on text-heavy output.
- **Recommendation:** styled-run frames in `rich-ext`, proposed as milestone
  0.0.13 "Frames".

### Composable transforms (0.0.12 workstream 4: #216)

- **`rich_ext::transform`.** `Transform<T>` rewrites a value; `Pipeline<T>`
  runs named stages in order and reports which one failed (`PipelineError`).
  `from_fn` makes a stage from a closure, and a pipeline is itself a stage.
  - Text: `KeepLines` (lines matching a regular expression, or not) and
    `HighlightMatches`.
  - Data (`rich_ext::data::transform`, `jsonpath` feature): a `Document`
    (tree, label, highlighted paths) with `Redact`, `Select`, `Filter` and
    `Highlight`. `Explorer::highlight(path, style)` draws highlighted lines.
  - Tables (`table::transform`): `Sort` and `Group`. Patches
    (`diff::transform`): `KeepFiles`.
- **Plugins can contribute text transforms.** The plugin API gains
  `TextTransform`, `PluginRegistrar::transform(name, transform)` and
  `Capability::Transform`. `ExtensionRegistry` hosts them: `transform(name)`,
  `transform_names()`, `register_transform` and `text_pipeline(names)`.
  `rich doctor` lists them with each plugin's capabilities. The plugin API is
  unpublished, so `PLUGIN_API_VERSION` stays 1.
- **CLI: `--filter` and `--highlight`.** With `--inspect` they take a JSONPath:
  `--filter` keeps what it selects and the containers above it, and
  `--highlight` shows those tree lines in reverse video. For text, `--print`
  and `--syntax` they take a regular expression, matched per line.
  - One fixed order, whatever order the flags come in: `--redact`, `--select`,
    `--filter`, `--highlight` for `--inspect`; `--filter`, `--highlight` for
    text. It is written down in [docs/cli.md](docs/cli.md#filter-and-highlight)
    and pinned by a test.
  - `--redact` and `--select` now run as stages of that pipeline. Their output
    is unchanged.
  - Other modes refuse both flags, and `--compare`, `--find`, `--flatten` and
    `--table` refuse the combinations they cannot show.
- **Guide: "Transforms"** ([docs/guide/ext/transforms.md](docs/guide/ext/transforms.md)),
  from the compiled `guide_transforms` example, including a plugin transform.

### Highlighter conformance kit, guide and benchmarks (0.0.12 workstream 1: #526)

- **`rich_ext::testing::conformance`** (the `testing` feature).
  `conformance::check(highlighter)` runs one contract against any
  `CodeHighlighter`, and reports every failure with the input that caused it:
  - listed themes highlight, and an unknown theme is `UnknownTheme`;
  - line counts match `split('\n')` for empty input, a missing final newline,
    CRLF, tabs and multi-byte text;
  - spans are sorted, disjoint, in range and on character boundaries;
  - an unknown language renders as no language;
  - rendered output has no control characters beyond styling;
  - 10,000 lines cost at most 40× what 1,000 lines do (a relative budget, not
    a wall-clock one).
  `check_with(…, Options { all_themes, scaling })` tunes it.
  - syntect passes it with every theme, and lumis passes it in its own tests.
  - Eight deliberately broken adapters are each rejected by the check they
    break.
- **`ExtensionRegistry::register_code_highlighter(name, engine)`** registers an
  engine without a plugin, with the same name and conflict rules.
- **Guide: "Code highlighters"**
  ([docs/guide/ext/code-highlighters.md](docs/guide/ext/code-highlighters.md)):
  choosing between syntect and lumis (the trade-offs, and the tree-sitter
  linking caveat), a keyword-only adapter in about 40 lines that passes the
  kit, and the ANSI themes. The example (`guide_highlighters`) is compiled, and
  its conformance test runs with `cargo test`.
- **Benchmarks:** syntect, syntect with `onig`, and lumis on the review files
  ([docs/benchmarks.md](docs/benchmarks.md#0012-code-highlighters)).
  - lumis has a fixed cost per language (0.07–0.12 s), then scales best: 1.87 s
    for 29,160 lines of Python, against 3.66 s with `onig`.
  - The CLI binary with the `lumis` feature is 169 MB, against 15.7 MB without
    it.

### Choosing the code highlighter (0.0.12 workstream 1: #525)

- **Core: a console-wide default.** `protocol::CodeHighlighting` (an engine and
  an optional theme) and `ConsoleCodeHighlighting`, implemented by `Console`.
  - A `Syntax` without a highlighter of its own uses the console's when it
    renders. Markdown code blocks do too. The console's theme applies unless
    the `Syntax` names one.
  - `Syntax::highlight_for(console)` is `highlight()` with the console's
    default.
  - A console without a default highlights exactly as before.
- **Ext.**
  - `ExtensionRegistry::set_default_code_highlighter(name, theme)` chooses a
    registered engine and theme. `install()` puts it on the console, and
    `code_highlighting()` returns it.
  - An unknown name or theme is a `HighlighterChoiceError` that lists the
    choices.
  - `source_view` and the text and patch diff views highlight with the
    console's default.
  - `RenderTarget::with_code_highlighting` passes one to a target's consoles.
- **CLI.**
  - `--highlighter NAME` and `--code-theme NAME`, with the `highlighter` and
    `code_theme` config keys. A working-directory `rich.toml` may set them:
    they are a plain choice among compiled-in engines.
  - `syntect` is always available. `lumis` needs the new off-by-default
    `lumis` feature (and Rust 1.91).
  - An unknown highlighter or theme is a usage error listing the choices, and
    says when lumis needs the feature.
  - They apply to `rich FILE`, `--syntax`, `rich view`, `rich diff`, Markdown
    and batch workers.
  - `rich doctor` reports the compiled-in highlighters with their themes and
    the active one and its theme, in its text and `--json` output.
  - With neither option, nothing changes.
  - Upstream's `--theme` (a syntax theme) is this CLI's config-theme option,
    so the code theme is `--code-theme`.

### The lumis highlighter (0.0.12 workstream 1: #524)

- **New crate `rs-rich-lumis` 0.0.1** (`rich_lumis`): a `CodeHighlighter` over
  [lumis](https://lumis.sh), with tree-sitter grammars and Neovim themes.
  - Use it with `Syntax::highlighter` or `Markdown::highlighter`, or register
    `LumisPlugin` to choose it by the name `"lumis"`.
  - **Themes** are lumis's own names (more than 250). The default is
    `monokai`, upstream rich's default. Themed code gets the theme's background
    and foreground.
  - **`ansi_dark`/`ansi_light`** use upstream rich's ANSI tables, mapped from
    tree-sitter capture names (`keyword.operator`, `function.builtin`, …) to
    Pygments token types, as the syntect adapter does.
  - **Languages:** all of lumis's (116) by default. The `bundle-web`,
    `bundle-web-extra`, `bundle-system` and `bundle-backend` features give a much
    smaller build (the web bundle's binary is 9 MB against 156 MB).
  - An unknown language highlights as plain text. An unknown theme is
    reported, and `Syntax` falls back to the default theme.
- **Its own crate and minimum Rust.** lumis links tree-sitter's C runtime, so
  only builds that ask for it pay for that. It needs Rust 1.91 (lumis's
  minimum). The workspace stays at 1.90, and CI checks this crate on 1.91
  separately.
- **Release tooling and CI.** `rs-rich-lumis-v*` tags, the feature matrix,
  release readiness, and the release scripts and tests. The first upload is
  manual, after the plugin API and before CLI 0.0.12 (BRANCHING).

### Markdown fences and Mermaid diagrams (0.0.12 workstream 3: #222)

- **Core: `FenceRenderer`.** A new extension point in `rich::protocol`.
  `Markdown::fence_renderer` asks each renderer, in the order added, whether it
  draws a ```` ```lang ```` fence; if none does, the fence is highlighted as
  before. Indented code never reaches a renderer. With none added, Markdown is
  byte-identical: goldens are unchanged.
- **Plugin API and ext.** `PluginRegistrar::fence_renderer(language, …)` and
  `Capability::FenceRenderer`. `ExtensionRegistry::fence_renderer(language)`
  looks one up, and `fences()` combines them into one for Markdown, routed by
  language; two plugins cannot claim the same language.
- **New crate `rs-rich-mermaid` 0.0.1** (`rich_mermaid`), built on the plugin
  API alone (`MermaidPlugin`, `MermaidFences`, `Mermaid`).
  - **Flowcharts as text:** `graph`/`flowchart` in `TD`, `TB`, `BT`, `LR` and
    `RL`; the common shapes; solid, thick, dotted and invisible edges with
    arrow, circle and cross heads, labels, longer links, chains and `&`. A
    layered layout gives every edge its own port and track, so lines never
    run together. ASCII where the console is ASCII-only; cropped with a note
    when wider than the terminal. Styling statements are ignored; subgraphs
    are drawn flat with a note.
  - **Every diagram type through `mmdc`** behind the off-by-default `mmdc`
    feature: Mermaid's own CLI renders a PNG, shown as Sixel where the terminal
    supports it, otherwise quadrant blocks with a note. Without real pixels,
    flowcharts still prefer text. The source goes through a private temporary
    file, never a shell; there is a 20 s timeout (Chromium is stopped with its
    process group) and a 64 KiB cap.
  - **Never an error on screen:** whatever cannot be drawn shows its source
    in a code block under a one-line note saying why (unsupported type, parse
    error with its line, `mmdc` missing, timed out or failed). Labels and
    shown source lose control characters, so a diagram cannot write escape
    sequences.
- **CLI.**
  - `rich mermaid FILE` (alias `mmd`), with `.mmd` and `.mermaid` files
    detected.
  - ```` ```mermaid ```` fences draw in `rich --markdown`.
  - `--mermaid-backend text|mmdc|off` and the `mermaid_backend` config key.
    `off` keeps upstream's plain fences.
  - `rich mermaid` tries `mmdc` first in a build with it.
  - A working-directory `rich.toml` cannot choose `mmdc`, since it starts a
    browser: the setting is dropped with a warning and a `config explain` note.
  - The `mermaid` feature is on by default and `mmdc` is off.
  - `rich doctor` lists the Mermaid plugin, and both features.
- **Release tooling and CI.** `rs-rich-mermaid-v*` tags, the feature matrix
  (with and without `mmdc`), release readiness, release scripts, and a
  `Mermaid CLI backend` job that installs `@mermaid-js/mermaid-cli` and renders
  a flowchart, sequence, class and state diagram. The crate's first version is
  a manual upload after art 0.0.10 and before CLI 0.0.12 (BRANCHING).

### Packaging

- The CLI README's "prepared, not yet published" line now names 0.0.12. It
  shipped inside the published `rs-rich-cli` 0.0.11 package, so it could only
  change with a new CLI version.

## Core 0.0.7 / macros 0.0.1 / ext 0.0.9 / art 0.0.9 / CLI 0.0.11 — published 2026-09-24

Cohort versions for 0.0.11: core 0.0.7, macros 0.0.1 (new), ext 0.0.9, art
0.0.9, CLI 0.0.11. Core changes below, so every dependent moves with it.

- Published from `main` at `433c1b1`, one tag at a time in dependency order:
  [`rs-rich-v0.0.7`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/36063309820),
  then `rs-rich-macros` 0.0.1 uploaded by hand with a maintainer token (a new
  crate cannot start with Trusted Publishing), then
  [`rs-rich-ext-v0.0.9`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/36065371117),
  [`rs-rich-art-v0.0.9`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/36065405590)
  and [`rs-rich-cli-v0.0.11`](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/36065429562)
  through Trusted Publishing. See the
  [release notes](docs/releases/0.0.11.md#publication).

### Optional Oniguruma highlighting (`onig`)

- **Core, CLI.** A new `onig` Cargo feature, off by default, runs syntect on
  Oniguruma instead of pure-Rust `fancy-regex` (DIVERGENCES #26). Highlighting
  is 2–4× faster: a full `rich` run on a 1,784-line Rust file went from 0.43 s
  to 0.16 s. Output is unchanged: 558 repository files rendered byte-identically
  under both engines. It needs a C compiler at build time and adds about 330 KB.
  `rich doctor` lists it under build features.

### Release test: security, robustness and parity fixes

Six independent audits of the whole 0.0.11 delta found the issues below. Each was
confirmed by reproduction, and each fix came with a regression test that failed
first. See the [0.0.11 release notes](docs/releases/0.0.11.md#what-the-release-test-found-and-fixed).

- **Dependencies.** `rustls` 0.23.45 for RUSTSEC-2026-0285 (reaches only the
  CLI's `fetch` feature). Core loads only syntect's bundled dumps, which drops
  the unmaintained `yaml-rust` and `plist` from the tree.
- **Core: Markdown links (security and parity).** Link and image destinations
  now go through a port of markdown-it's `normalizeLink`: mdurl
  percent-encoding, plus punycode written by hand, so no new dependency. They are
  also checked with markdown-it's `validateLink`. An escape sequence in a link can
  no longer reach the terminal, `é.com/ü` becomes `xn--9ca.com/%C3%BC`, and
  `javascript:`, `vbscript:`, `file:` and non-image `data:` links print as
  literal text, as upstream does. `~~` inside an autolink is no longer
  strikethrough. A refused reference definition still prints nothing
  (DIVERGENCES #24).
- **Core: parity.**
  - A printed `Text`'s base style no longer colours justify padding.
  - Zero-width table cells get no vertical padding.
  - Byte columns and speeds keep negative values (`-8/100 bytes`).
  - `TextColumn` format specs follow CPython's `format()`: 3,836 golden cases,
    up from 509 that diverged.
- **Core: robustness.**
  - A `LiveProgress::refresh()` inside `with()` no longer deadlocks; a nested
    `with()` panics with a clear message.
  - A render panic on the live thread restores the cursor. `AutoLive::try_stop`
    reports the panic.
  - `ProgressBar` no longer overflows on huge totals.
  - `Tree` renders, measures and drops without recursion, tested at 20,000
    levels.
  - Theme files parse in linear time.
  - JSON deeper than 10,000 levels is an error rather than an unbounded render
    (DIVERGENCES #25).
- **Ext: terminal-control injection.**
  - OSC 8 links could be broken out of in three places, each letting a crafted
    path or URL set the window title:
    - control characters in `LiveCoordinator` link URLs;
    - control characters in `Hyperlinker` file and editor URLs;
    - control characters in diff `TemplateLinks`.
  - File URLs are now percent-encoded outside the RFC 3986 path set, including
    non-ASCII.
  - Stack-trace labels, patch paths decoded from git's octal quoting, and
    JUnit/libtest names and output decoded from `&#x1b;` or `\u001b` now render
    inert.
  - Bidi controls (Trojan Source) are shown as escapes in `rich unicode` and in
    env views.
- **Ext: secrets.** `rich env` / `EnvView` mask secret names by whole segment
  (`*_KEY`, `DB_PASS`, `MYSQL_PWD`, `*_DSN`, `*_COOKIE`, `JWT`, …). They also mask
  credentials inside any value (URL passwords, token prefixes, JWTs, AWS key ids)
  through `redact_value`. Masking is best effort.
- **Ext: crashes and hangs.**
  - Values that panicked `richf!` at run time can no longer escape the template's
    markup.
  - Stack-trace cause chains are capped at 64, and render and drop without
    recursion.
  - Hunk headers with impossible numbers and huge hex line widths are now errors
    or clamped instead of overflowing.
  - A CRLF `Suggestion` span no longer panics.
  - libtest reports and wrapped `SourceView` lines (minified JSON in `rich view`)
    are no longer quadratic.
- **Ext: correctness.**
  - `RichHandler::live` no longer drops lines containing control characters.
  - Non-interactive `LiveCoordinator` output keeps the whole final snapshot.
  - A truncated JUnit file is an error, not a pass.
  - Tracing span fields are replaced, not duplicated.
  - An empty `FORCE_COLOR` is ignored.
  - QA screenshot names can't leave their directory.
- **Ext: structured data (`rich inspect`) and CLI authoring.**
  - YAML aliases are charged by the bytes they copy (64 MiB), and only aliased
    anchors are cloned, which stops alias and nested-anchor memory bombs.
  - JSONPath slices with huge steps no longer overflow; filters nest at most 128
    levels, and a selection past 1,000,000 nodes is an error.
  - YAML anchor names, C1 controls and DEL in keys and values, parse-error
    messages and diagnostic snippets are all escaped.
  - `--redact` masks dash spellings (`api-key`), everything under a secret key,
    and XML element text.
  - Generated completion scripts and man pages can no longer be injected into:
    - zsh/fish command names and fish subcommand words;
    - man page requests from multi-line fields.
  - Man pages escape non-ASCII for groff.
  - Completions offer each subcommand's own and global options.
  - Minified (single-line) XML and TOML parse in linear time: 200,000 elements
    took 35 s and now take 0.3 s.
  - JSON `-0` reads as `0`.
  - YAML duplicate keys, an INI key shadowed by a section, and content after the
    XML root are errors.
- **Core: speed.** `Text::divide` visits only the lines each span touches, as
  upstream does. Rendering 40,000 highlighted lines in `rich view` went from
  132 s to 11 s.
- **CLI: terminal controls.**
  - `rich view` and text `rich diff` neutralise terminal controls by default
    (`--no-sanitize` opts out). Both are new commands with no upstream behaviour
    to mirror, and `view` could otherwise pass clipboard-writing OSC 52 to the
    terminal.
  - `rich diff --sanitize` now covers patches, file names and C1 codes.
  - `rich capture` always cleans its title, and with `--sanitize` also C1 codes
    and the cast.
  - File names in error messages show control characters as symbols.
- **CLI: resources.**
  - `hex` reads only the window it shows.
  - `view`, `hex`, `unicode`, `inspect` and `capture` have documented size
    limits (docs/cli.md "Limits").
  - `capture` stops reading 1 s after the command exits, even while a background
    child still holds the pipe.
- **CLI: correctness.**
  - `capture` gives the command the panel's real width.
  - `config` and `doctor` no longer panic on a closed pipe.
  - `rich <command> --help` shows that command's help.
  - `bench compare` reports success in JSON, and a 0 ns baseline is a change.
  - `hex --bytes-per-line` must be 1–4096.
  - A bad `--select` is a usage error.
- **CLI: config trust.** A working-directory `rich.toml` can no longer set
  `theme_file`, choose export files (`export_html`, `export_svg`; ignored with a
  warning) or turn off the new `view`/`diff` sanitising, the same rule as
  `NO_COLOR`. Theme files must be regular files of at most 1 MiB, so a FIFO no
  longer hangs `rich`.
- **Tests.** Two intermittent failures were traced to the tests:
  - `test_batch_v9.py` stopped a worker before it exec'd, which suspended the
    parent in `posix_spawn`'s vfork. It now stops only exec'd workers.
  - The XML linearity test compared against a wall-clock limit. It now compares
    two sizes, which is how the quadratic XML parse above was found.
- **Art.**
  - GIF decoding is capped at 512 MiB (a 35-byte GIF used to abort on a 17 GB
    allocation).
  - Sixel rasters are capped at 16 megapixels before resizing, and repeat runs are
    split at 65535.
  - `TerminalDefault` keeps the colour of pixels at or above half opacity in every
    palette, and in ASCII and Braille.
  - Reduced-palette Sixel is 4–16× faster, with byte-identical output.
  - Sixel errors now tell "not a terminal" from "terminal not known to support
    Sixel", and `RICH_GRAPHICS=sixel|none` works as documented.
- **Release docs.** A new crate's first version cannot use Trusted Publishing, so
  `rs-rich-macros` 0.0.1 needs one token-authenticated upload before the ext tag
  (`docs/BRANCHING.md`).
- **Demo.** The tour shows `inspect`, `diff`, `view`, the `hex`, `unicode` and
  `ansi explain` inspectors, a theme file, a redacted `capture`, Atkinson with
  OKLab, and both alpha backgrounds.
- **Migration.**
  - `StackTrace` has a new public `omitted_causes` field.
  - New enum variants: `unicode_inspect::Kind::Bidi`,
    `ImageArtError::{SixelTooLarge, SixelNotSupported}`.
  - `richf!` prints a value's `\[` literally; it used to unescape it.
  - New ext items: `ArgSpec::global`, `env_inspect::redact_value`,
    `hex::MAX_BYTES_PER_LINE`, and the `sanitize` helpers
    `sanitize_terminal_and_bidi_controls` and `sanitize_single_line`.
  - `junit::parse` rejects a truncated file.
  - Core additions: `LivePanic`, `AutoLive::try_stop`,
    `filesize::{decimal_signed, pick_unit_and_suffix_signed}`, `json::MAX_DEPTH`.

### Workflow renderables (0.0.11 workstream 10)

All in `rs-rich-ext`, with no new dependencies and no change to core. Every
renderable has a plain fallback that carries its meaning without colour and
follows the accessibility policy (ASCII symbols, reduced motion).

- **Commands, task trees and summaries (`workflow`; #388, #389, #393).**
  - `CommandRecord` holds a process's argv, working directory, interleaved
    stdout/stderr, exit status and duration. Running a process
    (`CommandRunner`, which a `CancelToken` can kill) is separate from showing
    one (`CommandView`: output folded to its tail, stderr marked `!`, a spinner
    while running, a diagnostic after a failure).
  - `TaskTree`: nested tasks whose parents aggregate their children's status,
    with per-task cancellation, progress, notes and time from an injected
    `Clock`. It can collapse finished subtrees.
  - `CompletionSummary`: overall status, counts, duration, the items that need
    attention and next steps, built by hand or from a finished `TaskTree`.
- **Transfers, retries and notifications (#390, #391, #392).**
  - `transfer`: `Transfer`/`Transfers` with a moving-window rate, ETA, retry,
    pause and cancel states; `transfer_columns()` and `Transfer::task_update()`
    drive the core `Progress`; `TransferReader`/`TransferWriter` count bytes and
    stop on a `CancelToken`.
  - `countdown`: `Backoff` (exponential, capped, attempt limit, seeded jitter),
    `RetryStatus`, `RateLimit`, `CountdownBar`, and `CountdownWait`, a
    cancellable wait that redraws in a live region or prints one line per
    attempt when motion is off or the output is not interactive.
  - `notify`: `Notification` toasts (one line or a panel) and a `Notifications`
    stack with expiry, shown below live regions or printed once each when the
    output is not interactive.
- **Tables (`table`; #259, #429).**
  - `StreamingTable`: keyed rows for append and update workloads (upsert keeps a
    row's place), head and tail windows, and capacity eviction. Only changed
    rows are laid out again unless a column's width changes, and the output is
    byte-identical to the same core `Table` built from scratch.
  - `TableData`: stable multi-column sorting (numbers and digit runs compare by
    value, empty cells last, `▲`/`▼` or `^`/`v` in headers), grouping with
    count, sum, min, max, mean or custom aggregates, and totals, rendered as a
    core `Table`.
- **Badges, size bars and formatters (#426, #427, #425).**
  - `Badge`/`Badges`: status, label, link (OSC 8) and metadata chips that read
    `[OK build]` or `[docs <url>]` without styles, and wrap between badges.
  - `SizeBar`: a size against a total or limit, with going over shown by glyph,
    percentage and "over by" text.
  - `format`: sizes (decimal and binary), rates, durations, clock times,
    relative times, UTC timestamps, percentages and numbers.
- **Redaction (`redact`; #224), experimental.** Best effort, not a guarantee:
  check redacted output before sharing it, and report anything that gets
  through as a bug. `Redactor` masks secrets in strings, ANSI text
  and rendered segments, keeping styles and cell widths, so output can be
  redacted between recording and export. Built-in detectors cover secret-named
  keys, bearer tokens, common token prefixes, AWS access keys, JWTs and URL
  passwords; custom patterns can mask a named `secret` group. `SECRET_KEYS`
  moves to `redact`, and `data::SECRET_KEYS` still re-exports it.
- **CLI: `rich capture --redact` and `--redact-pattern REGEX`** (experimental,
  with the same caveat) mask the command
  line and its output before the capture is shown, exported or recorded.
- **Shared:** `cancel::CancelToken` (with child tokens) and
  `theme::STYLE_TABLES`, the one list of style tables `extended_theme()`
  layers, which now includes the new `workflow.*`, `transfer.*`,
  `countdown.*`, `notify.*`, `table.*`, `badge.*` and `size_bar.*` keys.
- **Fixed before merge:** a command that had already exited was reported
  cancelled when its token fired while its pipes drained.

### Art and CLI carry-over: colour modes, alpha backgrounds and theme files (0.0.11 workstream 12)

- **Art: Atkinson dithering and OKLab distance (#498).**
  - `Dither::Atkinson` (`--image-dither atkinson`) spreads six eighths of the
    error over six neighbours.
  - `ColorDistance::Oklab` (`--image-color-distance oklab`, config
    `image_color_distance`) picks the nearest palette colour perceptually. The
    default, `Rgb`, leaves every existing output byte-identical.
- **Art: colour modes for Sixel and GIF (#498).**
  - Sixel accepts `--image-color ansi256|ansi16|grayscale`. It quantizes and
    dithers like the text backends, then encodes exactly the palette's colours.
    Truecolor Sixel is unchanged.
  - GIF frames (`AnimatedArt::color_mode`/`dither`/`color_distance`, and the
    same flags with `--gif`) take a reduced palette.
  - Braille is monochrome and still rejects a reduced palette, now with an
    error that says so.
- **Art: alpha backgrounds (#126).** `ImageBackground` chooses what transparent
  pixels become. `Color` is what `ImageArt::background` did.
  - `TerminalDefault` (`--image-background default`) leaves them unpainted, so
    the terminal's background shows through, including in contain padding.
  - `Checkerboard` (`--image-background checkerboard`) previews them on gray
    squares.
  - Opaque images and every existing option render as before.
- **CLI: `--theme-file PATH` (#499)** (config `theme_file`, relative to the
  config file) loads an upstream `[styles]` theme file.
  - It layers above the built-in theme and below config themes and
    `--theme-style`.
  - A bad file is a usage error naming the file and line.
- **Migration:** exhaustive matches on `Dither` must include `Atkinson`. The
  internal `ImageArt` background field is now an `ImageBackground`, but the
  `background([r, g, b])` builder is unchanged.

### CLI viewers: `view`, `hex`, `unicode`, `env` and `capture` (0.0.11 workstream 11)

- **`rich view FILE` (#401)** shows a file the way it should be read:
  - Markdown, CSV, notebooks, JSON Lines, images, GIFs and patches go to their
    renderers.
  - Source code and structured data (JSON, YAML, TOML, XML, INI) are
    highlighted with line numbers.
  - Binary input becomes a hex dump.
  - It detects the format from the extension, else from the content (stdin
    included, read once), and pages by default.
  - `--search TEXT` highlights case-insensitive matches, marks their line
    numbers and prints a summary to stderr; `--no-line-numbers` drops the
    gutter.
- **`rich hex` (#410):** a `hexdump -C`-style dump with offsets, byte groups, an
  ASCII panel, colour by byte class and `*` for repeated lines. `--offset`,
  `--length`, `--bytes-per-line`, `--group`, and `--search BYTES` (hex or
  quoted text) shape and search it.
- **`rich unicode` (#411):** one row per grapheme (code points, UTF-8 bytes,
  width, a Rust escape and a kind), invalid UTF-8 as rows of its own, and a
  summary; `--limit N`.
- **`rich env` (#412):**
  - Lists environment variables, masking secret-looking names unless
    `--show-secrets` (`AUTH` only as a whole word, so `AUTHOR` shows).
  - Arguments filter names.
  - `rich env PATH` checks each entry: missing, duplicate of #N, empty, not a
    directory.
  - Control characters in names and values are shown inert.
- **`rich capture -- CMD` (#424):** runs a command with colour forced and
  `COLUMNS` set, stdout and stderr merged in order through one pipe, and shows
  the output in a panel with its exit status and duration. `--export-svg`,
  `--export-html` and `--cast FILE` (asciicast v2) record it. It exits with
  the command's status (128 + the signal for a killed command) once the panel
  and exports are written; PNG export is not provided.
- **Ext:** new `source_view::SourceView`, `hex::HexView`,
  `unicode_inspect::UnicodeView` and `env_inspect::{EnvView, PathView}`, all
  width-aware and ASCII-safe, with no new dependencies.

### Ext: tracing spans, span trees, editor links and Live-safe logging (0.0.11 workstream 7)

- **Spans (#286):** `EventLayer` now tracks spans in the subscriber's registry.
  Every event carries the spans it happened in, outermost first, with their
  fields as last recorded (`StructuredEvent::span_context`). With
  `span_open(true)` and `span_close(true)` it also reports spans opening and
  closing, with how long they were open (`span_marker`).
- **`RichHandler` span views:** `SpanView::Inline` (the default) prints
  `outer{a=1}:inner: message`. `SpanView::Tree` indents events under their
  spans with `│ ` guides and draws `┌ name` and `└ name 1.20ms` branches, in
  ASCII on ASCII consoles. `SpanView::Hidden` shows the message alone.
- **Editor links:** `RichHandler::hyperlinker` links the path column through a
  `Hyperlinker`, so its editor template and a base directory for `tracing`'s
  relative paths apply. Core's `LogRender` is unchanged.
- **Live-safe output:** `RichHandler::live` prints through a `LiveCoordinator`,
  above its regions, so logging never tears a live display.
- **Breaking:** `EventLayer` now requires `S: LookupSpan`, as every
  `tracing_subscriber` registry-based subscriber is. Events logged inside spans
  now show the span chain before the message; `span_view(SpanView::Hidden)`
  restores the old output.
- `rich_eprintln!` and `rich_trace!` (#285) shipped with the macros in
  workstream 5; they print through the themed console and are unchanged here.
- The logging guide covers spans, both views, editor links and Live output,
  with two new screenshots. Its example compiles every snippet.

### CLI: a project's `rich.toml` cannot undo `NO_COLOR`

- A `no_color = false` in a `rich.toml` found in the working directory used to
  turn colour back on even with `NO_COLOR` set, so running `rich` inside a
  repository could colour the output of someone who had asked for none. That
  file now can turn colour off but not on while `NO_COLOR` is set, in
  `[defaults]` or a profile. Your own `~/.config/rich/config.toml`, a file named
  with `--config`, and `--color` still override `NO_COLOR`, as the NO_COLOR
  convention allows for user configuration.
- `config show` reports the effective setting, and `config explain` notes when
  the file's value was ignored. The config reference and `docs/cli.md` describe
  the rule.

### Core: table vertical padding follows the row's position

- A table applied its top and bottom padding to every row alike. Upstream
  (`_get_cells`' `get_padding`) picks them per row. With `collapse_padding`,
  every row but the last keeps only `max(0, top - bottom)` below it. With
  `pad_edge` off, the first row (the header, when shown) loses its top padding
  and the last row its bottom. So `Table::grid().padding(0, 2, 1, 0)` no longer
  leaves a blank line after each row, and a grid of tables stacks as it does
  upstream.
- Checked against rich 15.0.0: 240 combinations of padding, `collapse_padding`,
  `pad_edge`, header, row count, boxed and grid match byte for byte (98
  differed before). Goldens: `table_vpad_{grid,boxed,collapse,no_edge,
  header_only,no_header}`.
- The guide's box gallery is regenerated. `box::NONE`'s doc comment no longer
  calls it a port: upstream has no `box.NONE` and uses `box=None`, which draws
  no border lines at all.

### Documentation: the user guide

- **`docs/guide/`** walks through every component: an overview of the crates,
  how they fit together and how one render flows; the core (console, text and
  style, tables, layout, trees, progress and live displays, code and data,
  logging, prompts, export); every rs-rich-ext module (extensions and links,
  diagnostics, logging, live regions and layouts, macros, structured data, CLI
  authoring, diffs and test reports, QA tooling, capabilities, accessibility,
  ANSI explain); rs-rich-art (banners, images, GIFs, image diffs); and the `rich`
  CLI (overview, a hands-on walkthrough of every command, the smoke test).
- **Every code sample is compiled.** Pages include sections of the
  `guide_*` example programs in each crate's `examples/`, and every screenshot
  is that example's real output exported to SVG. `scripts/capture_guide.py`
  regenerates them (`--check` fails on a stale image).
- **`scripts/smoke_cli.py`** runs every CLI command and mode end to end against
  generated fixtures (86 cases, no network or terminal needed), checks exit codes
  and output, and regenerates the CLI screenshots with `--screenshots`.
- **Fixes to existing docs:** the tutorial's `Live` example used an API that does
  not exist (it now includes the compiled one); a markup error exits 4, not 1;
  getting started pins the current release; the VS Code link template in
  `Hyperlinker::editor`'s docs produced a double slash; rich-art's README, crate
  docs and description now cover images, GIFs, Sixel and the current colour and
  dithering rules; docs.rs builds rs-rich-ext and rs-rich-art with all features,
  so feature-gated APIs are documented there.

### Core: no_color, spinner time and prompt output match rich

Three gaps found while writing the user guide, each verified against rich
15.0.0 and pinned by new golden fixtures (`no_color.tsv`, `prompt_ask.tsv`,
three `clock_*` rows in `live_status.tsv`).

- **No-colour mode keeps attributes.** `no_color` (or a non-empty `NO_COLOR`)
  used to drop the colour system, so bold, italic and underline went too.
  Upstream keeps the colour system and removes only colours when output is
  written (`Segment.remove_color` in `Console._render_buffer`).
  - **Behaviour change:** `Console::color_system()` now reports the colour
    system whatever `no_color` says, as upstream's `Console.color_system` does.
    Code asking "will colour reach the terminal?" must also check
    `Console::no_color()`. The CLI's render target, `rich doctor`,
    `rich_ext::qa::explain_console` and rich-art image and GIF rendering now
    do.
  - Capture and printing strip colours. HTML, SVG and plain-text exports read
    the recording, so they keep their colours, as upstream's do.
  - In a terminal, `NO_COLOR` now keeps bold and underline, as it does in
    upstream rich-cli (which honours it through `Console`); our `--no-color`,
    documented as equivalent, does the same. Piped output is still plain.
  - A `TERM` of `dumb` or `unknown` now gets no colour system, as upstream's
    `is_dumb_terminal` does. `NO_COLOR` had hidden this gap.
  - New `Style::without_color` and `Segment::remove_color`.
- **Spinners animate by themselves.** A `Spinner` or `Status` rendered as a
  renderable now shows the frame for the console's clock
  (`Spinner.__rich_console__` → `render(console.get_time())`), so one placed in
  a `Live` display moves. Before, it always drew frame 0.
  - New `Console::get_time` and `ConsoleBuilder::get_time`, ports of
    `Console(get_time=…)`, default monotonic. This is the same clock as
    `Progress`: `progress::GetTime` is now `console::GetTime`, re-exported.
  - `Spinner` measures as `render(0)`, as upstream's `__rich_measure__` does,
    which also fixes the start time at 0.
- **Prompt questions go through the console.** `Prompt`, `Confirm`,
  `IntPrompt` and `FloatPrompt` wrote the question with `print!`, so capture and
  export missed it. New `Console::input` / `input_from` port `Console.input`:
  the prompt is printed through the console with `end=""`, then a line is read.

### Fixes: print macro captures, flag suggestions, diagnostic locations, SVG titles

- **Print macros capture locals.** `rich_println!("[bold]{x}[/]")`,
  `rich_eprintln!` and `rich_trace!` failed with "cannot find value `x`":
  `richf!` gave implicit captures the call-site span, which inside the print
  macros' `macro_rules!` wrapper is the wrapper's hygiene. Captures now take
  the template literal's span, as `format!` does.
- **Like-for-like flag suggestions.** `cli_doc::suggest` compares a `--long`
  typo only with long flags, a `-s` typo only with short flags and a bare word
  only with bare words, so `--paralel` no longer also suggests `-r`.
- **One location line.** A diagnostic with a location and a snippet of the same
  file no longer repeats `--> file` above the snippet; `DataError` parse errors
  show only `  --> file:line:col`. Snippet-only diagnostics are unchanged.
- **SVG titles.** `rich print '[b]Hi[/] there' --export-svg …` and `--rule` with
  markup titled the SVG with a fragment split on `/`; literal print text and
  rule titles now get the default title `rich`. Path, URL and stdin titles are
  unchanged.

### Core: Columns and Tree fit narrow widths

Both were found by the ext fuzzer and checked against rich 15.0.0.

- **Columns:** an item wider than the width sized its column to the whole item
  and showed only its first line, so `Columns(["supercalifragilistic"])` at 8
  cells printed 20. Item widths are now capped by `Measurement.get`, and the
  items are laid out in upstream's `Table.grid`, which wraps them or ends them
  with `…` (`superca…`). `Columns::equal` and `Columns::expand` are ported too.
- **Tree:** guides overflowed a narrow width, so a nested tree printed 8-cell
  lines at 3 cells. A label with no width left after its guides is now not
  rendered, guides included, as upstream renders nothing there.
- Goldens: `columns_long_*`, `columns_wrap_w13`, `columns_mixed_*`,
  `columns_equal*`, `columns_expand_w20`, `tree_deep_w{3,4,6,10,20}` and
  `tree_multiline_w{6,12}`.

### Core: markup in cells, labels and columns; measurement parity

Both were found while writing the user guide and checked against rich 15.0.0.

- **Markup in plain strings.** Upstream renders a `str` through
  `Console.render_str`, so `add_row("[b]x")`, `add_column("[b]Name")`,
  `Tree("[b]root")`, `tree.add(...)` and `Columns([...])` items parse console
  markup and emoji codes. The port printed `[b]x[/b]` literally. Strings are now
  a new `Cell::Markup` (also what `From<&str>`/`From<String>` produce), rendered
  through the new `Console::render_str`. A `Text` is never re-parsed, so
  `add_row_text`, `add_column_text` and `Cell::Text` stay literal.
- **Highlighting follows upstream:** off in table cells unless
  `Table::highlight(true)` or `column_highlight(true)`, off in tree labels
  unless `Tree::highlight(true)`, and the console default for `Columns` items.
- **Measurement.** `Table`, `Tree`, `Panel`, `Padding`, `Constrain`, `Align`,
  `Rule` (`1, 1`) and `Bar` now port their `__rich_measure__`. Before, they
  measured as the full width, so a nested table stretched its column and
  `Align` could not move a table. `Styled` and `Align` measure through
  `Measurement::get`. `Table` measures every cell, header included, with
  `Measurement.get` inside its padding, and uses upstream's `_get_padding_width`
  for fixed, clamped and ratio columns.
- **Panel:** `expand(false)`, `Panel::fit` and `width` are ported, so a fitted
  panel shrinks to its measured content and title.
- **Columns** take any cell: `Columns::from_cells` accepts markup strings,
  literal `Text` and renderables, with `equal` constraining renderables.
- **Table text cells** render their `Text` unstyled and apply the cell style
  afterwards, as `render_lines(style=...)` does. A span that matches the cell
  style now keeps its own segment (`[b]Name` in a bold header).
- **Migration:** a plain-string header, cell or tree label that must stay
  literal should be passed as `Text` or escaped with `markup::escape`.
  `Tree::new`/`add` take `impl Into<Cell>`. Ext's `derive` headers, precedence
  layer names, `rich_table!` and `rich_tree!` now pass `Text` and stay literal.
  CLI CSV headers and cells now parse markup and emoji codes, as rich-cli 1.8.1
  does (`table.add_row(*row)` with `str` cells). Malformed markup prints
  literally rather than raising (DIVERGENCES §2).
  The 0.0.11 ext views that show data (ANSI explain, capability, contrast,
  test and matrix reports) pass `Text` too, so `[info]` or `test_x[a]` stays
  literal.
- **QA measure checks** follow upstream's contract now that containers
  measure. An expanding `Panel` or `Table` takes the whole width whatever it
  measures (and an expanding table can give a column more than its
  `max_width`), as upstream's do. So when a render is wider than its measured
  maximum, fuzz, stress and lint render it again at that maximum and report a
  mismatch if it panics, overflows, or wraps onto more lines while it does
  not widen with more room.
- Goldens: `table_markup_*`, `tree_markup`, `tree_highlight`, `columns_markup`,
  `table_nested`, `table_renderable_cells*`, `table_tree_cell`,
  `columns_panels*`, `columns_tables`, `panel_fit_*`, `panel_width`,
  `align_table`, `align_panel_fit`, plus the new `highlight_renderables.tsv`
  (5 cases, highlighting console) and `measure_renderables.tsv` (18
  container measurements).


### Capabilities and accessibility (0.0.11 workstream 9)

- **Capabilities (#209).** `rich_ext::capabilities` detects colour depth,
  Unicode, OSC 8 hyperlinks, graphics protocol (kitty, iTerm, Sixel), size,
  interactivity and whether animation suits the output. It reads an
  `Environment` (the real one, or a deterministic map for tests), records where
  every value came from, and honours `RICH_COLOR`, `RICH_UNICODE`,
  `RICH_HYPERLINKS`, `RICH_GRAPHICS`, `RICH_SIXEL`, `RICH_ANIMATION`,
  `RICH_WIDTH` and `RICH_HEIGHT` overrides. It converts to the
  `TargetCapabilities` that `RenderTarget` uses. `rich doctor` now reports
  through it: a capability table with sources, and `capabilities` in
  `--report json`.
- **Graceful degradation (#218).** `Fidelity` runs Animated, Rich, Styled,
  Plain, Ascii and is selected from capabilities and a policy. `Degradable` and
  `Adaptive` let a renderable offer levels, and `Degrade` strips colour, styles
  or non-ASCII glyphs from any renderable while keeping cell widths.
- **Semantic text (#229).** `AccessibleText` gives screen readers and logs the
  content of a `Table`, `Tree`, `Panel`, `Rule`, `Text` or `Diagnostic` in
  reading order, without decoration.
- **Policies (#422).** `AccessibilityPolicy` (compact, screen reader, no
  animation, reduced motion, high contrast, monochrome) from `RICH_A11Y` and
  `NO_COLOR`, with themes, fidelity ceilings and status symbols that do not
  rely on colour (`✔ ok`, `[OK]`, `ok:`).
- **Theme checks (#307).** `check_theme` reports WCAG contrast failures
  against light and dark backgrounds, styles that become identical without
  colour, and pairs confusable under protanopia, deuteranopia or tritanopia
  (Viénot/Brettel simulation, CIEDE2000), with suggested colours and a JSON
  form behind the `serde` feature.
- **ANSI explain (#219).** `rich_ext::ansi_explain` decodes SGR, CSI, OSC
  (including OSC 8 links), DCS, ESC and control characters with their meanings
  and the visible text. `rich ansi explain [FILE|-]` prints it.

### Diff engine and `rich diff` for text (0.0.11 workstream 8)

- **Engine (#208).** `rich_ext::diff`: a linear-space Myers diff whose unified
  output matches GNU `diff -u` hunk for hunk, with word-level emphasis.
  `DiffView` renders text, ANSI captures (style-only changes shown with `~`) and
  render snapshots, unified or side by side, readable with colour off.
- **Source diffs (#223).** `SourceDiff` highlights both sides with `Syntax`,
  emphasises changed tokens, numbers lines and can link them to an editor.
- **Patches (#236).** `git::parse_unified` reads `git diff` output, including
  renames, copies, binary files and mode changes. `PatchView` shows a file tree
  with counts, highlighted hunks, inline annotations and links from a pluggable
  `LinkProvider`.
- **Test results (#237).** Behind `test-report`: JUnit XML (Surefire, pytest,
  jest-junit) and libtest JSON into one model, and `TestReport` renders failures
  first with a diff of expected against actual.
- **Assertions (#217).** With `testing`: `assert_rich_eq!`,
  `assert_rich_json_eq!`, `assert_render_eq!` and `assert_snapshot_eq!` panic
  with a rendered diff (side by side with `RICH_ASSERT_LAYOUT=side-by-side`).
  `RenderSnapshot::diff` now returns a unified diff.
- **QA tooling (#306, #308–#313, #225, #230).** `rich_ext::qa`, with `testing`:
  - `screenshot`: renders a fixture across widths, colour depths and Unicode
    modes and checks it against approved files (`RICH_APPROVE=1` accepts
    changes; `.new` files hold what changed).
  - `stress`: many widths and heights, reporting overflow, clipped content,
    unstable wrapping, panics and measure mismatches.
  - `lint`: clipped text, unknown style names in markup, broken links, status
    shown only by colour, and colours, glyphs or links the target cannot show.
  - `explain`: why a render wrapped, truncated, lost colour (`#ff8700 → 208 →
    …`), fell back to ASCII or dropped links.
  - `profile`: measure, render and frame times, with an opt-in counting
    allocator for allocation counts.
  - `fuzz`: seeded, reproducible random renderables checked for panics, width,
    determinism and measure bounds, with shrinking and a Rust reproduction.
  - `matrix`: the same fixtures across 16 capability profiles (colour depths,
    Unicode, links, dumb, CI, Windows Terminal, screen reader).
  - `bench`: a benchmark harness, a JSON run format (also read from criterion
    output) and `compare`, with sparklines. `rich bench compare BASE CAND`
    prints it and exits 5 on a regression.
  - The fuzzer found two core bugs, confirmed against rich 15.0.0: `Columns`
    overflowed with items wider than the width, and `Tree` guides overflowed
    below about 8 columns. Both are fixed below, and the fuzz test now covers
    every core renderable at every width.
- **CLI.** `rich diff` compares anything that is not an image pair as text, or
  renders one patch (`git diff | rich diff -`), with `--side-by-side`,
  `--context` and `--language`. `--threshold` counts changed lines and exits 5
  above it. Image pairs are compared perceptually as before.

### Ext: CLI authoring (0.0.11 workstream 6)

- **One description, many outputs.** `rich_ext::cli_doc::CommandSpec` describes a
  command line without depending on any parser: options with value hints,
  choices, defaults, environment variables and config keys, headings, examples
  and extra sections.
- **Help and errors (#38, #407).** `HelpView` renders grouped options with their
  hints, examples and sections, in two columns from 60 columns wide and stacked
  below that. `CliError` renders a diagnostic with "a similar argument exists"
  suggestions (Jaro-Winkler, as clap does).
- **Completions (#403).** `completion::generate` writes Bash, Zsh, Fish and
  PowerShell scripts with descriptions and groups. `CompletionCatalog` exposes
  the same metadata to other tools.
- **Markdown and man pages (#408).** `docs::to_markdown` and `docs::to_man` /
  `to_man_pages`; the man pages pass `mandoc -T lint` and `groff -ww`.
- **Configuration (#409, #413).** `ConfigReference` renders keys, types,
  defaults, environment variables and flags, with its sources in precedence
  order. `Precedence` shows every layer's value for each key, marks the winner
  and explains a single key's chain.
- **clap (#38).** The optional `clap` feature maps a `clap::Command` onto the
  model (`CommandSpec::from_clap`), maps `clap::Error` onto `CliError`, and
  offers parse helpers that render help, version and errors through rich.
- **CLI.** `rich --help` is now rendered from a `CommandSpec` of the whole CLI:
  usage first, options under headings (Render mode, Input, Layout, Image,
  Inspect, Export, Paging, Watch, Batch, Config & theme, …) with their
  defaults, choices, environment variables and config keys, wrapped to the
  terminal. Metavars read `<N>`. It is plain when piped and colour-free with
  `--no-color`.
  - `rich completions bash|zsh|fish|powershell` prints a completion script.
  - `rich docs markdown`, `rich docs man [--output DIR]` and `rich docs config`
    print reference pages; `docs/cli-reference.md` is generated from them.
  - `rich config reference` lists every config key. `rich config explain [KEY]`
    shows each key's value in every layer, in the order the binary applies
    them (defaults, `NO_COLOR`, config file, profile, command line), and marks
    the one that wins.
  - Drift tests keep the help and the parser in step, in both directions.
  - `completions` and `docs` are command words when they come first, like
    `config` and `doctor`; `rich -p docs` still prints the word.

### Structured data and `rich inspect` (0.0.11 workstream 4)

- **Ext: `rich_ext::data` (#37, #211, #384).** One insertion-ordered document
  tree for JSON, YAML, TOML, XML, INI and dotenv, built from text or from any
  `Serialize` value. Features: `data` (JSON, INI, dotenv and every view),
  `yaml` (saphyr-parser; anchors and aliases are kept), `toml`, `xml` and
  `jsonpath`. Parse errors carry a line and column and convert to a
  `Diagnostic`.
  - `Explorer` draws a tree or table with depth, length and string limits,
    folding and optional paths. `print_json`, `print_table` and `print_tree`
    render serde values with no setup, and `TableOptions` overrides columns,
    headers, justification and row limits.
  - YAML (#394), TOML (#395) and XML (#396) keep their structure: anchors,
    datetimes, and attributes as `@name` keys. INI and dotenv (#397) render as a
    key table with comments; `Redaction::secrets()` and the `Redactor` trait
    mask secret-looking values.
  - Selection (#398): the `Selector` / `SelectorBackend` traits and a
    `Selectors` registry, with an optional JSONPath backend (`$`, keys, indices,
    slices, wildcards, recursive descent and filters).
  - Flatten and unflatten (#399), with errors for ambiguous input; key, path
    and value search with highlighted matches and context (#400); and `diff`.
  - `Format::detect` is conservative: prose, Markdown, CSV and single lines are
    not mistaken for YAML, TOML, INI or dotenv.
- **CLI: `rich inspect` (#211).** Explores a file, URL or stdin as a tree; INI and
  dotenv as a key table. `--select`, `--find`, `--flatten`, `--table`,
  `--max-depth`, `--max-length`, `--show-paths`, `--redact` and `--compare`
  change the view. A parse error reports `file:line:column` and exits 4.
- **CLI: `--format` (#402).** `--format auto` detects piped or extensionless
  input and routes it: JSON to the JSON renderer, other formats to highlighting,
  anything else to plain text as before. A named format overrides the
  extension. Detection is opt-in, so upstream's default is unchanged. `format`
  is also a config key, and is ignored when another mode is chosen explicitly.

### Progress: pulse, format columns and live display (#6)

- **Pulse bar.** `ProgressBar` pulses when `pulse` is set or the total is
  unknown: `ProgressBar::indeterminate`, `.pulse`, `.animation_time`. It uses
  upstream's cosine fade, including the quirk that 256-colour consoles get the
  two-tone fallback. It also has the ASCII (legacy/`ascii_only`) glyphs and
  no-colour fallbacks. The bar column pulses for unstarted and indeterminate
  tasks.
- **Determinate bar fixes.** A zero total draws a full finished bar, and the
  background is left out without colour, as upstream does.
- **Text and renderable columns.** `TextColumn` formats strings against the
  task (`{task.completed:>6.1f}`, `{task.fields[name]}`) through the new
  `rich::pyformat`, a port of Python's format mini-language. It also takes
  style, justify and markup. Per-task fields come through
  `Progress::add_task_with` and `TaskUpdate::field`. `ProgressColumn::Renderable`
  shows any renderable, and rows grow to fit multi-line cells.
- **Live display.** `Progress::start(console, writer, refresh_per_second)`
  returns a `LiveProgress`. It redraws on the `Live` thread and refreshes
  where upstream does: `add_task`, `reset`, and `update(refresh=True)`.
  `stop()` commits the final frame.
- **`track()`.** `LiveProgress::track` and the module-level `rich::track`
  advance a task per item, after the loop body, as upstream does.
- **Live changes.** `Live::spawn` waits for the first frame. The new
  `AutoLive::refresh_wait` redraws synchronously. The final newline is written
  only after a non-empty render.
- **API changes.** `TaskUpdate` gains `fields` and `refresh` (struct literals
  need `..TaskUpdate::default()`). `ProgressColumn` gains `TextFormat` and
  `Renderable`. New `Console::no_color()`.
- **Goldens.** New `progress_bar.tsv` (15 cases) and `progress_live.tsv`
  (3 cases), plus 4 more `progress_time.tsv` cases: pulse, format fields and
  renderable columns. All match rich 15.0.0.

### Progress: the grid, expand, column options, transient and file reading (#6)

- **Rendered as upstream's grid.** `Progress::make_tasks_table` builds the
  `Table::grid` that upstream's `make_tasks_table` does, and the display renders
  it. Columns wrap at narrow widths instead of cropping.
- **`expand`**, **`transient`** and **`disable`** on `Progress`.
- **Column options.** `ProgressColumn::with_table_column(ColumnOptions)` is
  upstream's `table_column=Column(...)`: width, min/max width, ratio, justify,
  `no_wrap`, overflow and style. `ProgressColumn::BarWith(BarColumn)` sets the
  bar width (`None` fills the column) and its styles.
- **Reading files.** `LiveProgress::wrap_read` and `LiveProgress::open` port
  `wrap_file` and `open`: the returned `ProgressReader` advances its task by
  the bytes read.
- **Table.** Cells may be any renderable (`Cell`, `Table::add_row_cells`), and
  `Table::add_column_with(header, ColumnOptions)` sets every column option.
  `ProgressBar` measures as upstream's does and gains style setters.
- **Live.** `Live::transient` and `Live::spawn_with(…, transient)`. A
  non-terminal stop no longer adds a newline; `LiveProgress::stop` writes it,
  as `Progress.stop` does.
- **Migration.** Exhaustive matches on `ProgressColumn` need the `BarWith` and
  `WithTableColumn` variants.
- **Goldens.** 6 new `progress_time.tsv` cases (expand, flexible bar, column
  options, crop and style, narrow wrapping) and 4 new `progress_live.tsv` cases
  (transient, disabled, non-terminal) match rich 15.0.0.

### New crate `rs-rich-macros` 0.0.1 and ext macros (0.0.11 workstream 5)

- **`rs-rich-macros`** is a proc-macro crate, used through `rs-rich-ext`'s new
  `macros` feature. It checks markup and styles with the core's own parsers.
  - **Checked markup (#36, #282, #283).** `richf!` builds a `Text` from
    `format!`-style markup and escapes the values. Unbalanced, mismatched or
    unclosed tags, unknown style names and unknown theme keys are compile
    errors, as are malformed or unused placeholders. `style!`, `theme_key!` and
    `markup!` check literals.
  - **Derive (#280, #281).** `#[derive(Rich)]` implements `RichRecord` and
    `Renderable`: labelled fields, a panel, or a table row, with field options
    `skip`, `label`, `style` (checked), `display`, `format`, `justify` and
    `order`. `rich_ext::derive::table` renders many records as rows.
- **Print and debug macros (#284, #285).** `rich_dbg!` is `dbg!` rendered through
  `Pretty`. With `macros`, `rich_println!`, `rich_eprintln!` and `rich_trace!`
  print checked markup.
- **Convenience macros (#385).** `rich_table!`, `rich_panel!`, `rich_tree!` and
  `rich_progress!` build the ordinary core types.
- **Tests.** `trybuild` compile-fail cases pin the error messages.
- **Release tooling.** The tooling knows the fifth crate: `release.py`, the
  release tag triggers, the CI feature matrix and the release tests.
  Publication order is core, macros, ext and art, then CLI.

### Ext: diagnostics, hyperlinks, stack traces and a dashboard (0.0.11 workstream 3)

- **Diagnostics (#210).** `Diagnostic` gains a `Level` and code
  (`error[E0308]: …`, the code optionally linked), a `Location` row, labelled
  primary and secondary spans on `SourceSnippet`, `Suggestion`s that show the
  edited line, and an attached `StackTrace`. Without them it renders exactly as
  before.
- **Error adapters (#386, #387).** `DiagnosticInfo` lets any error type, such as a
  `thiserror` enum, supply its level, code, help, notes and location through
  `Diagnostic::from_info` / `to_diagnostic`. The new `anyhow` feature adds
  `Diagnostic::from_anyhow`, which maps the context chain and a captured
  backtrace.
- **Hyperlinks (#212).** `hyperlink::Hyperlinker` links URLs, paths,
  `path:line:column` and `#123` / `owner/repo#123` references, as `file://` URLs
  or through an editor template. It is a `Highlighter`, so `RichHandler` can use
  it. Non-terminals keep the plain text.
- **Stack traces (#239).** `stacktrace::parse` normalises Rust, Python, Java and
  JavaScript traces, with chained causes, into one shape. Parsers are pluggable.
  Rendering dims and collapses library frames and links locations.
  `stacktrace::capture` and `panic_hook` cover Rust panics. The tests parse traces
  captured from real Python 3.11, Node 22, OpenJDK and Rust runs.
- **Dashboard (#329).** `dashboard::DiagnosticsDashboard` summarises counts by
  level, lists the most frequent codes, and groups diagnostics by file in
  location order.
- `rs-rich-ext` now depends on `fancy-regex` directly. It was already in the tree
  through core.

### Logging: `LogRender` port and `RichHandler` (#10)

- **`LogRender` is now a port of `_log_render.py`.** It lays a record out as a
  `Table::grid` row: time (blanked when it repeats the previous record's),
  level, message (ratio 1, folding) and `path:line` linked to the file. It
  remembers the last time across calls, as upstream's instance does.
  `rich::level_text` builds upstream's padded `logging.level.<name>` column, and
  `LogLevel` spells `WARN` as Python's `WARNING`.
- **Migration.** The old one-line `LogRender::new(level, message).time(..).path(..)`
  is now `LogRecord`, with the same builders plus `.line(n)`. `LogRender::new()`
  takes no arguments; call `.render(console, message, time, level, path, line,
  link_path)` for each record.
- **Table.** `Table::grid()` now has upstream's defaults: no padding and
  `collapse_padding`. New `Table::without_box()` (upstream `box=None`),
  `Table::padding(top, right, bottom, left)` and `column_overflow`; a
  column's overflow applies to cells that set none.
- **JSON floats** are written with Python's `float.__repr__` (`1e+20`, `1e-07`,
  `10000000000.0`), as `json.dumps` does. This resolves DIVERGENCES §8.
- **Ext: `RichHandler`** (`rich_ext::RichHandler`) is the counterpart of
  upstream's `logging.RichHandler`. It prints events with the level column,
  `ReprHighlighter`, HTTP-method keywords in `logging.keyword`, optional
  markup, the file name linked to its full path, and structured fields as
  `key=value`. With the `log` or `tracing` feature it is an `EventSink`, so
  `LogAdapter` and `EventLayer` print through it. See the `rich_handler` example.
- **Goldens.** New `log_render.tsv` (4 cases) and `json_python_floats` match
  rich 15.0.0. The differential fuzzer now generates `box=None` tables.

### Markdown: styled table cells and constructor options (#9)

- `Table` headers and cells can be styled `Text`, via the new
  `Table::add_column_text` and `Table::add_row_text`. A cell's own `justify`,
  `overflow` and `no_wrap` override the column's, as `Text.__rich_console__`
  prefers them.
- Markdown table cells now carry their strong, emphasis, code, strike and link
  runs, as upstream's `TableDataElement` appends them under the current style.
  Headers keep `markdown.table.header` over their inline spans.
- A single `~` inside a table cell is appended to the cell, like other literal text.
- `Markdown` gains upstream's constructor options: `justify`, `style`,
  `code_theme`, `inline_code_lexer` and `inline_code_theme`. `style` is the root
  of the style stack, as upstream's context uses it: under every text run,
  under list-item content and padding, and under the block-quote colour. Code
  themes are `syntect` names (DIVERGENCES #18).
- New `Syntax::highlight` returns the highlighted code as `Text` (upstream
  `Syntax.highlight`); inline code uses it when a lexer is set.
- Fix: a wrapped list item's continuation rows are padded in the marker's own
  style (bold bullet, cyan number), as upstream's `render_bullet` and
  `render_number` do.
- New goldens: `markdown_table_inline.tsv` (8 cases) and
  `markdown_options.tsv` (8 cases: justify, style, and wrapped items) match
  rich 15.0.0.

### Core parity fixes (0.0.11 workstream 1)

This fixes every divergence family the 0.0.10 differential fuzzer found, each
verified against rich 15.0.0. The known-divergence queue is now empty, and every
repro is in the pull-request corpus.

- **Padding (#442):** empty content keeps its line, and the padding style sits
  under the content, as upstream's `render_lines(style=…)` does. The same fix
  gives `Panel("")` its blank body row.
- **Align (#443):** the rendered block is aligned as a whole
  (`Constrain` + `Segment.set_shape`), not line by line.
- **Rule (#444):** a right-aligned title with a multi-cell fill is dropped, as
  upstream drops it.
- **Table (#445):**
  - header cells are bottom-aligned, and row shaping follows upstream's
    `align_cell` + `set_shape`
  - cells render through `Text` wrap, justify and truncate
  - `_measure_column` is ported, including columns with no cells, the
    `maximum or 1` floor, the re-measure after collapsing, and no expand once a
    table has collapsed
- **Printed `Text` (#446):** `Console::print` renders at the full width and
  applies upstream's `Text.join` semantics, so a printed `Text`'s own `justify`,
  `overflow` and `no_wrap` defer to the print options. The top-level
  shrink-to-measurement that stood in for this is gone.
  - `Renderable::fit_to_measurement` now defaults to `false`.
  - New `Console::print_with` and `render_export_with` take explicit options, the
    equivalent of `console.print(…, overflow=…, no_wrap=…)`.
- **Tabs (#447):** `Text` measures the raw string, as upstream does. A tab
  measures zero cells until render.
- **Emoji (#448):** scanning resumes after every `:…:` match, including unknown
  codes and `::`, as upstream's single `re.sub` does.
- **Zero-width content (#449):** a renderable given less than one cell renders
  nothing (upstream `Console.render`), so a squeezed `Panel` has no body rows.
- **API:** `Console::render_lines_styled` ports `render_lines(style=…)`.

Before the fix, three seeds of 2,000 generated cases had 1,193 mismatches.
After it, 40,000 generated cases across 20 seeds match.

## Core 0.0.6 / ext 0.0.8 / art 0.0.8 / CLI 0.0.10 — published 2026-09-23

Every workstream in the [0.0.10 plan](docs/plans/0.0.10.md) is merged to `main`, and the
release test passed on the integrated tree; see the
[0.0.10 release notes](docs/releases/0.0.10.md). All four crates were published through
Trusted Publishing, each with a passing exact-version registry consumer:
[core](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35818053978), [ext](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35818015980), [art](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35818038760) and
[CLI](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35818067984). The tags were first pushed out of order; the release notes
record the recovery. Each version was uploaded once.

- Release tooling: `check_packages.py` drops Cargo's cached copies of staged-only
  versions before verifying. Cargo reused a core 0.0.6 unpacked and built before
  `fit_to_measurement` existed, which failed a good tree and could pass a bad one.
- CLI: a multi-file `--watch` with `--watch-exit-on-error` no longer promises a retry
  in the failing region.
- Demo: the tour shows a pushed theme, `~~~` strikethrough and a two-file watch that
  ends through `--watch-exit-on-error`; it is re-recorded from the 0.0.10 build.

- Release: crates.io Trusted Publishing replaces the stored registry token; the
  OIDC exchange runs only after preflight and dry run (`docs/BRANCHING.md`).
- CI: GitHub Actions moved to Node 24 majors (`checkout` v7, `github-script` v9,
  `upload-pages-artifact` v5, `deploy-pages` v5).
- Art: `icy_sixel` 0.7 for the optional Sixel backend.
- CLI: `toml` 1.1 for configuration parsing; strict-config behaviour unchanged.
- Core parity (#15): `Spinner` follows upstream's animation state. The first
  `render` fixes the start time, `Spinner::update` changes text, style or speed
  (a new speed continues from the current frame), the text is console markup,
  and the frame style may be a theme name. `Status` keeps one stateful spinner,
  parses its message as markup, defaults to the `status.spinner` theme style and
  gains `update`; `Status::renderable` now returns `&Spinner`.
  `ProgressColumn::Spinner` delegates its start time to the spinner, as
  upstream's `SpinnerColumn` does. The new golden `live_status.tsv` has 8 step
  programs covering spinner and status frames and LiveRender control sequences;
  PORTING gains parity cells for Live and progress.
- Core: Progress time, rate and spinner columns and upstream's task model (#6):
  an injectable clock, `add_task` returning a `TaskId`, `update`/`advance`/
  `reset`/`start_task`/`stop_task`/`remove_task`, the 30 s speed window, and
  `TimeElapsed`, `TimeRemaining`, `TransferSpeed`, `FileSize`, `TotalFileSize`,
  `Spinner`, `TaskProgress` and binary `Download` columns. Golden
  `progress_time.tsv` replays identical step programs against rich 15.0.0.
  `Progress::new()` now uses upstream's default columns (adds time remaining);
  `add_task` takes `impl Into<Option<f64>>` totals and returns a `TaskId`.
  Progress cell styles now resolve against the console theme.
- CLI: the capability demo's progress section shows speed, ETA, elapsed and a
  spinner from a simulated clock.
- Ext: `Diagnostic::from_error` no longer reports an ordinary error chain as a
  `[cycle]` when a wrapper stores its source as its first field (same address);
  cause identity now compares address *and* type (#146, #151). New regressions
  cover a three-level chain, and checked-in `RenderSnapshot` fixtures cover
  multiline, chained and source-context diagnostics. A nested-panel layout
  regression evidences #134.
- Ext (behaviour change): `LayoutNode` leaves now receive their region's height,
  as upstream `Layout` passes it, so height-aware renderables such as `Panel`
  fill their region instead of rendering at natural height above blank rows.
  The nested-panel regression is byte-identical to rich 15.0.0's `Layout`.
  Wrap a leaf in `.content_height()` to keep a panel at its natural height.
- Core: `Syntax` and `Json` port upstream `__rich_measure__` (Syntax measures its
  raw source plus padding; JSON measures as its `Text`), with `Measurement::get`,
  `normalize` and `with_maximum`, parity-tested by the new `measure.tsv` golden.
  A printed `Syntax` still renders at the full console width, as upstream does:
  the new `Renderable::fit_to_measurement` (default `true`) opts it out of the
  top-level shrink that stands in for upstream's `str`/`Text` joining.
- Ext: `layout::Overflowing` applies one explicit `OverflowPolicy` (wrap, fold,
  crop, ellipsis, visible) to Syntax, JSON or Text lines; fitting output is
  unchanged byte for byte, and padded Syntax rows keep their background (#149).
- Art 0.0.8 image modes (#125, #126, #199) and CLI routing (#144):
  - `ImageColorMode::Ansi16` (rich's standard palette) and `Grayscale` (neutral
    ANSI256 entries by luma). Floyd–Steinberg and Bayer 4×4 now work with every
    quantized mode.
  - `ImageMode::Quadrants` / `QuadrantArt`: 2×2 pixels per cell, choosing the
    cheapest two-colour split. Also available for `--diff` heatmaps.
  - `ImageFit::Stretch`, `ImageArt::max_width`/`max_height`, and brightness,
    contrast and gamma in `ImageTransforms`, applied in a documented fixed order.
  - CLI flags `--image-color ansi16|grayscale`, `--image-mode quadrants`,
    `--image-fit stretch`, `--image-max-width`, `--image-max-height`,
    `--image-brightness`, `--image-contrast` and `--image-gamma`, plus matching
    config keys. Invalid values and unsupported combinations are usage errors.
  - Unset options leave output byte-identical: 149 pre-existing mode, colour,
    dither, fit and transform invocations compared equal against the previous
    binary, stdout plus HTML and SVG exports.
  - Migration: exhaustive matches need `ImageMode::Quadrants`,
    `ImageFit::Stretch`, `ImageColorMode::{Ansi16, Grayscale}` and
    `ImageArtError::InvalidAdjustment`. `ImageTransforms` gained three `f32`
    fields, so it is no longer `Eq`, and struct literals need `..Default::default()`.
  - The guided demo's art section and a same-source comparison image
    (`docs/media/cli-010-image-modes.png`) show the new modes from actual output.
- CLI: `--watch` accepts several local files; a change re-renders only that
  file, in its own `rich-ext` Live region, with errors shown per file until it
  recovers. File events come from `notify` (parent-directory watches, so atomic
  rename-over saves and delete-and-recreate are seen), debounced by
  `--watch-debounce` (default 0.1 s); `--watch-poll` and watcher failures use
  the polling loop. `--watch-exit-on-error` ends the watch non-zero on a failed
  render. New config keys: `watch_debounce`, `watch_poll`,
  `watch_exit_on_error`. Redirected output and URL watching are unchanged (#139).
- Parity tooling (#34): `scripts/diff_rich.py` generates Table, Rule, Padding
  and Align cases alongside markup, text and panels. The Python oracle renders
  each colour system in its own interpreter, because rich memoises a Style's
  escape codes and a shared process misreported colours. Markup compares the
  strict parser on both sides. The shrinker keeps the failure kind and reduces
  rows, cells, columns and options. A nightly workflow runs 20,000 generated
  cases on `main`. First findings are filed as #442–#449, with repros in
  `scripts/fixtures/diff_rich_known.jsonl`; triage steps are in `docs/parity.md`.
- Core: Markdown strikethrough pairs tilde runs as upstream's markdown-it does, so
  runs of three or more (`a ~~~x~~~ b` → `a ~` + struck `x` + `~ b`) and uneven
  runs match rich 15.0.0. Golden `markdown_strike.tsv` (24 cases); DIVERGENCES §21
  narrowed to tilde pairs crossing a later emphasis span (#9).
- Core: upstream's theme stack — `Console::push_theme`, `pop_theme` and a
  `use_theme` guard (`ThemeContext`) that pops on drop — plus `Theme::from_styles`,
  `config`, `from_file` and `read` for upstream theme files. Golden
  `theme_stack.tsv` checks them against rich 15.0.0; DIVERGENCES §14 resolved (#3).
  `RichError` gains `ThemeStack` and `ThemeConfig`; exhaustive matches need them.

## CLI 0.0.9 / art 0.0.7 — published with the cohort above

- Named TOML themes, default/profile/CLI selection, explicit style overrides and
  resolved theme bindings for batch workers and exports.
- Human-report stderr-terminal batch progress and `--no-progress`; Ctrl+C stops
  scheduling, kills/reaps workers, exits 130 and preserves one machine report.
- Listable `core`, `workflows`, `art` demo sections and selectable playback.
- Read-only `rich doctor` diagnostics with JSON stdout, selected config/pager
  details and explicit inferred capability reporting.
- Public art `ImageColorMode`/`Dither` builders and CLI opt-in ANSI256/
  Floyd–Steinberg for ASCII/half-block still images. Default truecolor/no-dither
  and the `ImageOptions` struct shape remain unchanged.

Local validation and review passed; see [0.0.9 preparation notes](docs/releases/0.0.9.md).
The expanded scope above also moves core to 0.0.5 and ext to 0.0.7; all four published on 2026-09-22.

## CLI 0.0.8 / art 0.0.6 — published

- Guided `rich --demo` suite tour with adjustable pacing, real CLI workflow and
  art examples, finite redirected output and clean Ctrl+C interruption.
- Batch planning also rejects hard-linked input/output aliases before writing.

- Full TOML configuration with strict validation, default/profile/CLI precedence,
  explicit boolean overrides and JSON `config show` / `config validate`.
  Inspection lists configured settings, not every built-in default.
- Batch dry-run without output writes; bounded subprocess concurrency for file
  exports with disk-spooled output replayed in input order. Fail-fast stops new
  scheduling while in-flight workers finish; terminal batches remain serial.
- Opt-in terminal-height automatic paging and explicit no-pager controls.
- Nine public `ImageAnchor` positions and an `ImageArt::anchor` builder, exposed
  by `--image-anchor` for cover fitting. Center remains the default and contain
  behavior is unchanged.
- Registry package-content checks and staged sibling package verification;
  staged success does not establish publication readiness. Updated recipes,
  benchmark/demo work and release handoff documentation.

Independent publication succeeded for
[art 0.0.6](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35541438821)
and [CLI 0.0.8](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35542000274),
including exact-version registry consumers. Ext 0.0.6 and core 0.0.4 stayed
unchanged. Evidence is recorded in [0.0.8 notes](docs/releases/0.0.8.md).

## CLI 0.0.7 / art 0.0.5 / ext 0.0.6 — published

The art release succeeded first. The initial CLI package dry run failed because
registry ext 0.0.5 lacked the workspace `CliExtensions: Clone` implementation.
Publishing ext 0.0.6 and correcting the CLI dependency completed recovery:
[art workflow](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35531841822),
[ext workflow](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35533288812),
[CLI workflow](https://github.com/buchochelliq-labs/rs-rich-cli/actions/runs/35533304887).
Core remained 0.0.4.

### CLI finishing

- Fix watch refresh after same-size edits/atomic saves with preserved timestamps;
  read local contents using bounded memory on each poll. Repaint the terminal
  viewport instead of appending each changed frame. Redirected watch stays finite.
- Add `ImageFit::{Contain,Cover}` and `ImageArt::background` for still-image
  letterboxing, centre-cropping and alpha compositing; expose CLI fit/background
  options with positive-dimension and allocation-limit validation.
- Add real CLI workflow snapshots and terminal regressions, fresh demo capture
  tooling, workflow recipes, and truthful serial `--jobs` help.

### Release scope

CLI 0.0.7 and art 0.0.5 shipped still-image fitting, background compositing,
watch fixes, deterministic serial batch conversion and scalar config profiles.
The [0.0.7 release notes](docs/releases/0.0.7.md) record validation and dependency
recovery. Serial `--jobs` and limited TOML/boolean semantics below describe that
release; the published 0.0.8 release above replaces them.

### Added

- **Confidence tooling:** added `scripts/snapshot_cli.py`, a deterministic CLI
  snapshot runner with injectable width and terminal capability profiles,
  PTY terminal forcing for color profiles, option-terminator (`--`) positioning,
  newline normalization, and readable unified diffs. The differential corpus and
  generator now cover box renderables (`Panel`) and safe-box capability
  profiles, and `library_bench` accepts explicit `--width` and `--color-system`
  values.
- **`rs-rich-cli` 0.0.7 workstream:** deterministic `--batch` conversion for
  files, directories, and globs with deterministic serial execution, explicit collision and
  overwrite policy, fail-fast/continue-on-error modes, and aggregate JSON
  status reports. `--jobs` reserves a future concurrency limit; it does not
  enable parallel workers. Added TOML-style scalar config profiles with local/home discovery,
  explicit `--config`/`--profile`/`--no-config`, and CLI-over-config precedence.

- **`rs-rich-cli`:** added task-oriented subcommands (`rich json`,
  `rich markdown`, `rich syntax`, `rich csv`, `rich ipynb`, `rich jsonl`,
  `rich log`, `rich gif`, `rich diff`, `rich rule`) while preserving existing
  flat flags without deprecation warnings.
- **`rs-rich-cli`:** added streaming JSONL / NDJSON and structured-log rendering
  from stdin or files with bounded per-record processing and fail-fast malformed
  record handling.
- **`rs-rich-cli`:** added stable automation exit-code classes plus
  `--report json` / `--machine-json` for a common result/error envelope on
  stderr, keeping rendered stdout separate.
- **`rs-rich-cli`:** added non-busy `--watch` polling for local files and
  fetch-enabled URLs, including atomic-save/disappearance recovery, parse-error
  frames, configurable intervals, URL response caching, and deterministic
  one-shot behavior when stdout is redirected.
- **`rs-rich-art`:** added an `ImageArt` facade (`ImageMode`, `ImageOptions`,
  `RenderCapabilities`) plus a reusable Braille renderer so consumers can
  render a single still image without duplicating renderer selection logic.
- **`rs-rich-cli`:** added a first-class `--image` flag / `rich image` command
  for rendering a single still image (reusing `rich-art`'s `ImageArt` facade),
  plus a new `--height N` option; `--image-mode` now also applies to `--image`
  and accepts `braille`.

## [0.0.5] — 2026-09-13

Prepared for independent `rs-rich-ext-v0.0.5` and `rs-rich-cli-v0.0.5` tags.
`rs-rich` and `rs-rich-art` remain at 0.0.4.

### Changed

- **Parity tooling:** `scripts/capture_golden.py` now verifies that the
  installed Python `rich` version matches the exact `UPSTREAM.toml` pin before
  regenerating fixtures, preventing silent captures from the wrong oracle.

- **Differential testing:** added a replayable `scripts/diff_rich.py` corpus
  harness and Rust `diff_render` probe that compare selected markup/style/Text
  cases against pinned Python `rich`, with bounded mismatch shrinking and CI
  coverage for the deterministic corpus.

- **Benchmarking:** added a `library_bench` Rust probe for repeatable markup
  parsing, Text wrap/justify, Table layout and Console-render timings, including
  setup timing, output hashes and active feature metadata.

### Added

- **`rs-rich-ext`, `rs-rich-cli`:** added an opt-in `--sanitize` path that
  neutralizes terminal controls from decoded input, JSON/notebook strings,
  titles and captions by rendering them as visible inert text while preserving
  default upstream-compatible output.

## [0.0.4] — 2026-09-10

All four independently versioned packages published at 0.0.4 for their code and
internal dependency changes. The annotated tag is on main; the protected release
workflow passed full source checks and exact-version registry verification.

- `rs-rich`, `rs-rich-cli`: optional, off-by-default `syntax-cache` for repeated
  lines matching a small initial parser state. No per-entry capture copies;
  grammars and output remain unchanged, with workload-dependent gains (#45).

- `rs-rich`: ownership-transfer protocol for table rows, preserving existing rendering.
- `rs-rich-cli`: move parsed CSV rows into the table and compact completed rows;
  measured 100k-row peak RSS falls about 46%, with identical output (#74).

- `rs-rich`: translate successive Text wrapping breaks incrementally, removing
  repeated scans of long UTF-8 line prefixes while preserving wrapping output (#74).

- `rs-rich-art`: explicit capability-aware half-block GIF frames and mixed-renderer stages.
- `rs-rich-cli`: `--gif-mode ascii|blocks`, preserving ASCII defaults and safe
  ASCII fallback without color and first-frame output when redirected (#65).

- `rs-rich-ext`: explicit strict UTF-8/UTF-16 decoder with BOM and byte-order validation.
- `rs-rich-cli`: wire `--encoding` for text files, stdin and URLs; preserve default
  decoding, add UTF-16 hints and clean actionable image errors (#62).

## [0.0.3] — 2026-09-10

### Fixed
- **`rs-rich-cli` diff thresholds:** compare both percentages with the same
  one-decimal formatting used by the report, including fractional limits and
  rounding ties, so the displayed verdict agrees with the process exit status.
- **`rs-rich-cli`:** render diff HTML/SVG using destination color capabilities
  while preserving plain piped stdout and threshold exit codes. Apply notebook
  decorators to the whole cell group; reject ignored demo options, explain
  interactive stdin, and document environment, paging and GIF repeat defaults.
  Honor non-empty `NO_COLOR` in all CLI modes.
- **`rs-rich`:** parse Panel, Rule and Table title/caption markup with visible
  width measurement; preserve styled Unicode truncation without invalid span
  offsets. New fixtures are captured from pinned Python Rich 15.0.0.
- **`rs-rich`:** select `more.com` as the Windows pager fallback. Required Windows
  CI launches the native pager and verifies its output as well as env precedence.
- **`rs-rich`, `rs-rich-cli`:** expose table streaming through the `LineRenderable`
  protocol trait; treat an early-closing CSV consumer as successful termination.
  Regression test covers a 10,000-row producer whose pipe is closed after one byte.
- **`rs-rich`, `rs-rich-cli`:** preserve arbitrary-size JSON integers and render
  overflowing exponents as signed Infinity, matching Python. Repair #98 escape
  folding without losing suffix bytes; keep the divergence behind the off-default
  `json-escape-safe` feature. Default golden fixtures are captured from Python.
- **`rs-rich-art`, `rs-rich-cli`:** piped GIFs emit their first frame once, including
  infinite repeats, without cursor controls. Reject unsupported GIF decorators,
  paging and exports; explain image-mode downgrades on stderr (#59, #72).
- **`rs-rich`, `rs-rich-cli`:** stream undecorated CSV rows to reduce peak memory;
  preserve upstream alignment-flag priority and notebook display-data omission.
  Fix empty Markdown output, HTML/quoted-rule spacing, and adjacent images in one
  table cell (#72, #74). Empty text and CSV retain their newline.
- **`rs-rich` (and rendering through `rs-rich-cli`): Markdown image hoisting is byte-parity in nested and adjacent containers**
  (`markdown.rs`): images inside table cells are now hoisted ahead of the table
  and leave their cells empty, multiple images in one container share a row, and
  images hoisted from consecutive containers occupy adjacent rows without a blank
  row. Goldens cover a table cell, a multi-image paragraph, and a README-style
  badge table against Python `rich` 15.0.0.

### Changed
- **`rs-rich`, `rs-rich-ext`, `rs-rich-art`, `rs-rich-cli`:** set 0.0.3
  manifests and internal requirements together. Core includes the Markdown fix;
  ext needs a new package version for its dependency on core 0.0.3. Art and CLI
  retain their 0.0.3 versions. This dependency closure is specific to this release;
  independent per-crate versioning remains supported.
- **Release tooling (all four crates):** retain coordinated `v*` releases and
  independent `<crate>-v*` releases, with selection-scoped exact-version
  verification. Protect registry consumer builds with the same `crates-io`
  environment as upload, with a manual verification-only recovery mode. Reject
  lightweight tags and require the tag's exact checked-out commit on main. Align the
  release checklist and skill with both paths.
- **CI and docs (all four crates):** parse the upstream pin as TOML, check
  generated manifest versions and CLI help, and apply main-ancestry checks to
  `rc/*`, `release/*`, and `releases/*` integration branches.

## [0.0.2] — 2026-08-11

### Fixed
- **`Color::downgrade` was wrong for 8-bit and standard targets** (`color.rs`) —
  found by the new colour-system goldens, which is exactly the coverage gap that
  let it survive:
  - **8-bit:** we searched the 256 palette for the nearest colour. Upstream
    instead maps into the 6×6×6 cube *by formula* (with a greyscale-ramp branch
    under 15% saturation), which picks different entries: `#00ff00` is cube index
    46, not the exactly-matching system green 10. The axis is also non-linear
    (first step 0–95, then 40 per step), and Python's banker's rounding is
    reproduced with `round_ties_even`.
  - **standard:** we matched against the 128-based ANSI table. Upstream matches
    against a *separate* 170/85-based `STANDARD_PALETTE`, so `#ff0000` gives 1
    (maroon), not 9. The two tables are now distinct: `ANSI_BASE_PALETTE` (the
    first 16 of the 8-bit palette, and the default terminal theme's ANSI set)
    versus `STANDARD_PALETTE` (matching only).

  A unit test had been asserting the buggy values; it now asserts upstream's,
  captured from real rich 15.0.0.

### Fixed
- **Four more defects**, from a second adversarial review (of the change above):
  - **Tabs were never expanded on the render path.** `Text::expand_tabs` existed
    but nothing called it, so a tab occupied one cell in every layout calculation
    and eight on the terminal. `Text::measurement` now measures the *expanded*
    text too — upstream gets away with measuring raw (a tab counts as zero cells
    there) only because its `print` never narrows to the measurement.
  - **A bare `link` was accepted** and emitted an empty OSC 8 hyperlink; upstream
    raises `"URL expected after 'link'"`. This one was mine, from the `link`
    keyword added in the previous commit.
  - **Colour names kept their original case**, so `on #FF0000` did not normalize
    to `on #ff0000` and a mixed-case open/close tag pair raised.
  - **`Text::new` did not strip control codes** (BEL, backspace, vertical tab,
    form feed, carriage return), which upstream removes on construction.
- **Six markup and `Style::parse` defects**, all found by an adversarial review of
  the named-span change and each verified against real rich 15.0.0:
  - **The highlighter beat markup.** `[green]123[/]` rendered `repr.number` cyan
    instead of green, because markup spans were pushed first and the highlighter
    decorated that `Text` in place. Upstream highlights the plain string and
    appends the markup spans last, so an explicit tag wins.
  - **Two tags over the exact same range combined backwards**, so `[red][blue]x[/][/]`
    came out red. Upstream's `sorted(spans[::-1], key=start)` needs both the
    reversal *and* a start-only key; sorting by `(start, end desc)` makes an
    identical range compare equal and hands the win to the outer tag.
  - **`expand_tabs` kept one span** where upstream splits per tab-part, so a span
    crossing several tabs rendered as one segment instead of several — same
    colours, different bytes.
  - **Tag names weren't normalized.** `Style::normalize` is now ported (with
    `Style::definition`, the port of `Style.__str__`), so `[b]` opens what
    `[/bold]` closes and a mixed-case name reaches the theme lowercased.
  - **`[link=url]` was unusable**: parameters weren't split from the tag name, so
    `[/link]` raised a markup error. `Style::parse` also had no `link` keyword,
    which meant such a style resolved to null and dropped the hyperlink.
  - **`not BOLD` was accepted** and silently cancelled an enclosing bold;
    upstream's `not` operand is case-sensitive and raises.

### Changed
- **Style names on spans now resolve at render time, against the rendering
  console's theme** (`StyleType` in `style.rs`, `Theme::get_style`,
  `Console::get_style`; closes the main half of #14). Previously a span held a
  concrete `Style`, resolved eagerly — highlighters looked names up in a
  *process-global* default theme, so a console's own theme could not restyle
  their output at all, and an unknown markup tag was an error rather than the
  no-op upstream produces.
  - `Span.style` and `Text`'s base style are now `StyleType { Name | Style }`,
    the port of upstream's `Union[str, "Style"]`. The render path resolves them
    once per render into a vector parallel to the spans (upstream's `style_map`).
  - Two user-visible behaviour changes, both verified against real rich 15.0.0
    before shipping: highlight colours follow the console's theme, and an unknown
    markup **tag name** renders as a no-op (a genuine *syntax* error still fails).
  - `Text::highlight_regex` gains its named-group half — each group is styled
    with `{style_prefix}{name}` as a *name*, retiring the deferral noted when it
    was first ported. This is also how `RegexHighlighter` now works, so a custom
    highlighter with novel group names finally gets colours rather than just span
    boundaries.
  - `Text::stylize` takes its arguments in upstream's order
    (`stylize(style, start, end)`), and the render family takes a `&Theme`.
    `Text::render_wrapped` and `render_joined_justified` are gone (no callers,
    not upstream). `Text::render` drops its unused `ColorSystem` argument.
  - Covered by `golden/highlight.tsv` — the **only** fixture set rendered with
    `highlight=true`, which is why this path had no byte-parity net before. All
    12 pre-existing fixture sets are byte-identical.
  - Still deferred: upstream's theme *stack* (`push_theme`/`pop_theme`). See
    `docs/DIVERGENCES.md` §14 for why it needs its own design pass.

### Added
- **Prompts** (`prompt.rs`, closes the `prompt.py` half of #11): `Prompt`,
  `Confirm`, `IntPrompt`, `FloatPrompt` — a styled question, a read, and a
  re-ask until the answer validates.
  - Reading sits behind an `InputSource` trait (upstream's `stream=` argument),
    so the whole loop — including the re-ask path and the empty-answer-takes-the-
    default rule — is testable without a terminal. `ScriptedInput` is provided
    for that; `StdinInput` is the default.
  - The question line is byte-parity tested (13 fixtures in `golden/prompts.tsv`)
    across choices, defaults, `show_choices`/`show_default`, markup prompts and
    `Confirm`'s `(y)`/`(n)` rendering.
  - Case-insensitive choices return the choice as spelled in the list, not as
    typed, which upstream is explicit about.
  - Diverges in one place: exhausted input returns the default instead of
    looping forever (Python raises `EOFError`).
- **The rest of the `Text` manipulation API** (`text.rs`): `divide`, `split`,
  `pad`/`pad_left`/`pad_right`, `right_crop`, `rstrip`, `rstrip_end`,
  `expand_tabs`, `join`, `blank_copy`, `highlight_words` and `highlight_regex`.
  All carry spans correctly through the transformation — re-based and clipped by
  `divide`, shifted by `pad_left`, extended over the spaces a tab expands into.
  - `highlight_regex` is ported in its **explicit-style** form only. Upstream also
    styles each named group with the group's *name* as a style name, which needs
    a span that can hold an unresolved name; that waits on the named-span
    refactor (#14). `RegexHighlighter` already covers the named-group case by
    resolving against a theme as it goes.
  - Covered by 24 new byte-parity fixtures (`golden/text_ops.tsv`). Each case
    renders its result, so one comparison pins the plain text, the span
    boundaries and the styles together.
- **Text overflow** (`Overflow` in `console.rs`, `text.rs`, `segment.rs`). `Text`
  now carries `overflow` and `no_wrap`, as do `ConsoleOptions`, resolved with
  upstream's `self.x or options.x or DEFAULT` precedence. The wrap pipeline is
  now a full port of `Text.wrap`: split on newlines, wrap (folding an unbreakable
  word **only** under `fold`), justify, then truncate each line.
  - `Overflow::Ignore` neither wraps nor truncates, which upstream keeps
    survivable via `Console.print(crop=True)` — ported as `Segment::crop_lines`
    and applied on the print path. Without it, `ignore` would run off the side of
    the terminal rather than being cut at the edge.
  - `Text::truncate(max_width, overflow, pad)` is exposed directly.
  - Covered by 22 new byte-parity fixtures (`golden/overflow.tsv`) spanning every
    method, `no_wrap`, double-width characters straddling the cut, and the three
    positions an ellipsis can land relative to a span — the last of these caught
    a real bug, since the marker inherits the style of the run the cut *entered*,
    not the one it left.
- **Paging** (`pager.rs` + `Console::page`, `rich-cli --pager`). A `Pager` trait
  with a `SystemPager` that
  ports `pydoc.get_pager`'s selection order (`MANPAGER`, then `PAGER` — split
  into program + args — then `less -R`/`more`), including its guards: no tty on
  stdin/stdout or `TERM=dumb`/`emacs` falls back to writing straight to stdout,
  as does a pager that can't be spawned. `Console::page(styles, f)` buffers a
  closure's output and shows it, stripping styles unless `styles` is set
  (matching `Console.pager(styles=False)`); `page_with` takes a custom `Pager`,
  upstream's `Console.pager(pager=…)` seam. `rich --pager` pages *styled*
  output.
- **`rich-cli` URL fetch** (`rich-cli/main.rs`, completes the common CLI surface):
  `rich <url>` fetches an `http(s)` resource and renders it — the render mode
  comes from a flag, else the URL extension, else the response `Content-Type`
  (markdown/json/csv), else syntax-highlighting (with a lexer guessed from the
  Content-Type; upstream's behavior). Fetching uses `ureq` (rustls +
  bundled webpki roots — self-contained TLS), bounded by a 30 s timeout and a
  16 MiB **decoded**-body limit, behind a **default `fetch` feature**
  (`--no-default-features` gives a lean, network-free build). Like
  `requests.get`, a non-2xx status still renders the response body. The size
  limit is applied to the post-decompression stream (ureq's own `limit()` sits
  *under* the gzip decoder and would only bound compressed bytes, leaving a
  decompression-bomb hole); oversized and non-UTF-8 bodies get their own clear
  errors. The only deliberate divergences from upstream are the timeout and size
  bound, which upstream lacks.
- **`Live` auto-refresh thread** (`live.rs`, advances DIVERGENCES #17):
  `Live::spawn(renderable, console, writer, refresh_per_second)` returns an
  `AutoLive` handle whose background thread redraws on an interval (and on each
  `update`) — a port of upstream's `refresh_per_second`. The thread constructs and
  owns the `Live` internally (only `Send` inputs cross the boundary), so
  `Console` is now `Send` (highlighter boxes are `dyn Highlighter + Send`; the
  rich-ext registry factory matches). Deterministically tested to emit the same
  byte stream through the thread; `Drop` finalizes if `stop()` is skipped.
- **`rich-cli` `.ipynb` (Jupyter notebook) rendering** (`rich-cli/main.rs`, port
  of rich-cli's `render_ipynb`): `--ipynb` (and `.ipynb` auto-detect) renders a
  notebook — markdown cells as `Markdown`, code cells as an `In [n]:` label + a
  dim `Panel` of `Syntax`, and cell outputs (stream / error traceback /
  execute_result `text/plain`) decoded via `AnsiDecoder`, blank-line separated.
  Adds a `serde_json` dependency for notebook parsing. (Rich outputs — images,
  HTML — are deferred; text is handled.)
- **`rich-cli` panel `--title` / `--caption` / `--style` + `none` box**
  (`rich-cli/main.rs`, `box.rs`): completes the `--panel` decorator to full
  rich-cli fidelity — the panel gets a title (top border), caption (bottom
  border), and border style (e.g. `--style "bold red"`); `--panel none` uses the
  new blank `box.NONE`. Composes the already-byte-parity `Panel` builders.
- **`rich-cli` `--panel` / `--padding` decorators** (`rich-cli/main.rs`): wrap any
  render mode's output in a `Panel` (`--panel ascii|ascii2|square|rounded|heavy|
  double`) and/or `Padding` (`--padding` with 1, 2, or 4 comma-separated ints,
  unpacked like upstream's `Padding.unpack`), composing the byte-parity Panel/
  Padding renderables. Port of rich-cli's decorator flow (padding inside, panel
  outside). `Console::build_text` is now public so the `--print` markup can be
  wrapped. (`none` box + `--title`/`--caption`/`--style` are follow-ups.)
- **`rich-cli` `--export-svg`** (`rich-cli/main.rs`): exposes the new SVG export
  through the CLI (any render mode → a self-contained SVG), alongside the existing
  `--export-html`. The two are mutually exclusive; the SVG title is the resource's
  basename (else `rich`) and it uses a fixed `unique_id` (the default can't be
  byte-parity — DIVERGENCES #15). `emit` refactored to an `Export` enum.
- **`Console::export_svg`** (`svg.rs`, resolves DIVERGENCES #15): exports recorded
  output as a self-contained SVG image of a terminal window (Fira Code font-face,
  window chrome + traffic-light circles, per-line clip-paths, a generated CSS
  class table, and the styled text matrix), using `SVG_EXPORT_THEME`.
  **Byte-parity** with real rich 15.0.0 (golden `tests/golden/svg_export.svg`).
  Built on the groundwork primitives `SVG_EXPORT_THEME`, `blend_rgb`, and
  `Style::get_svg_style`, plus new `Style::attr` / `Color::is_default` accessors.
  Takes an **explicit `unique_id`** (upstream's default hashes Python `repr()`
  output, not reproducible in Rust — the auto-default is the only residual, #15).
- **`rich-cli` CSV/TSV rendering** (`rich-cli/main.rs`, advances the CLI port):
  `--csv` (and `.csv`/`.tsv` auto-detection) renders a delimited file as a table,
  a port of rich-cli's `render_csv` — blue-bordered `HEAVY_HEAD` table with the
  first row as the header and any all-numeric column right-justified + bold-green
  (body & header). Includes an RFC-4180-ish parser (double-quoted fields with
  `""` escaping, embedded delimiters/newlines, `\r\n`, leading-BOM stripping) and
  the numeric-column heuristic. **Byte-parity** with the Table real rich-cli
  builds (unit-tested). Deferred: `csv.Sniffer` dialect/has-header heuristics and
  title/caption. Demo shows a CSV table.
- **`Table` `border_style` + per-column header cell fill** (`table.rs`): a
  `border_style` builder (tints the box edges/dividers, composed over the table
  style) and `column_header_fill` (a per-column header *cell* style — content +
  padding — combined over the table `header_style`, distinct from the existing
  content-only span). Together they reproduce **rich-cli's `render_csv` styling**
  byte-parity (HEAVY_HEAD, blue border, numeric columns right-justified + bold
  green), verified by the new golden `table_csv_style`. Groundwork for the CLI's
  CSV rendering.
- **`Progress` `Download` column + `filesize::pick_unit_and_suffix`** (`progress.rs`,
  `filesize.rs`, advances DIVERGENCES #16): the `DownloadColumn` renders
  `completed`/`total` in a shared SI byte unit (`0.5/1.0 kB`), byte-parity with
  rich 15.0.0. Added `pick_unit_and_suffix` (the unit/suffix picker upstream's
  download/transfer columns share). This completes the *deterministic* Progress
  columns; only the wall-clock columns (spinner/speed/time) + the `Live` loop
  remain.
- **`Progress` custom columns** (`progress.rs`, advances DIVERGENCES #16):
  generalized from a hard-coded 3-column layout to a configurable
  `ProgressColumn` list (`Progress::columns`). Deterministic columns ported
  byte-parity — description, static text, bar, percentage, and **M-of-N**
  (`{completed}/{total}`, `progress.download` green). The inline grid matches
  upstream's `Table.grid(padding=(0,1))`. Non-deterministic columns (spinner,
  speed, time) and the download byte-unit formatting stay deferred (need the
  `Live` loop / filesize units). Default layout unchanged (byte-parity).
- **`Table` per-column `ratio` / `min_width` / `max_width`** (`table.rs`, the
  substantive half of DIVERGENCES #7): `column_ratio`, `column_min_width`, and
  `column_max_width` builders. `min_width`/`max_width` clamp the measured content
  width (over-wide cells wrap); `ratio` columns share the free width in proportion
  when the table is `expand`ed, via a faithful port of upstream's flexible/fixed
  split (`ratio_distribute` now takes per-slot minimums). Byte-parity goldens
  `table_ratio`, `table_min_width`, `table_max_width`. Only the rare width-0
  padding edge remains open on #7.
- **`ISO8601Highlighter` covers the full upstream pattern set** (`highlighter.rs`,
  resolves DIVERGENCES #13): added compact/basic calendar dates (`20230615`),
  ordinal dates, week dates, basic times, standalone timezones, and space-separated
  date-times. Upstream's one PCRE-conditional pattern is rewritten as two
  non-conditional alternatives (all-hyphen/colon + all-basic) that match the same
  strings. Byte-parity with rich 15.0.0 (unit-tested); the extended forms are
  unchanged.
- **`AnsiDecoder` decodes OSC 8 hyperlinks** (`ansi.rs`, resolves DIVERGENCES
  #10): `\x1b]8;<params>;<url>\x1b\` sequences now attach the URL to the running
  `Style` (via the new `Style::update_link`), with the empty closing sequence
  clearing it and `id=`/other params ignored — matching upstream. Re-rendering is
  byte-identical to rich except the random `id=` field we omit for determinism
  (same deviation as #20). Round-trip unit tests added.

### Fixed / clarified
- **`chop_cells` over-long-word fold is byte-parity** (`cells.rs`, resolves
  DIVERGENCES #5): dropped the `!line.is_empty()` guard so folding a character
  wider than the fold width (e.g. a 2-cell CJK char to width 1) emits upstream's
  empty leading chunk (`["", "宽", …]`), and `_wrap.divide_line` therefore yields
  the same break positions (and the same empty line) as rich. Normal text is
  unaffected (`cw <= width` never triggers the empty push). Unit-tested against
  real rich 15.0.0. Only a rare multi-codepoint-grapheme residual remains (needs a
  grapheme table).
- **Markdown trailing thematic-break blank line** (`markdown.rs`): a document that
  *ends* with a `---` rule now emits the extra trailing blank line upstream does
  (upstream's hr element yields a trailing break, observable only when the rule is
  the last block — a mid-document rule merges with the normal block separator).
  Byte-parity golden `markdown_hr_end` + unit test; the mid-document case
  (`markdown_quote_hr`) is unchanged. Closes the last Markdown-block divergence.
- **`Json` non-ASCII is byte-parity** (`json.rs`): confirmed and locked in with a
  golden (`json_unicode`) + unit test. `rich.json.JSON` defaults to
  `ensure_ascii=False`, so our UTF-8 output already matched upstream (accented
  characters/symbols render literally, not `\uXXXX`), and `serde_json`'s
  `preserve_order` keeps input key order. Corrected the stale DIVERGENCES #8,
  which wrongly claimed an escaping mismatch; the only remaining JSON caveat is
  exotic **number formatting** (exponent notation like `1e+20`/`1e-07`, and
  integers beyond i64/u64) differing from CPython's `repr`.

### Added
- **Markdown tables** (`markdown.rs`, port of upstream's `TableElement`): GFM
  tables now parse (`pulldown-cmark` `ENABLE_TABLES`) and render via [`Table`]
  built exactly as upstream — `box=SIMPLE`, `pad_edge=false`,
  `collapse_padding=true`, `markdown.table.border` (cyan) table style, and
  `markdown.table.header` (`not bold cyan`) header-content styling — with
  per-column justify read from the alignment row. **Byte-parity** with real rich
  15.0.0 (new golden `markdown_table`). Inline styling *within* a table cell is a
  documented follow-up. Enabled by a new Table capability: a per-column
  **header-content style span** (`Table::column_header_style`) that styles the
  visible header characters while the header padding keeps `header_style`.
- **Table-level `style`** (`table.rs`, port of `Table(style=…)`): a default style
  for the whole table, composed as the base of the border style
  (`border_style = style + border_style`) so it tints the box glyphs and dividers
  while cell content keeps its own styles — matching upstream exactly. Byte-parity
  golden `table_style`. With `pad_edge`/`show_edge`/`collapse_padding`/per-column
  justify already in place, the only remaining Markdown-tables step is wiring the
  `TableElement` (#9).
- **Table `collapse_padding`** (`table.rs`, port of `_get_padding_width`):
  `Table::collapse_padding(true)` merges adjacent cell padding — an interior
  column's left pad is reduced by the previous column's right pad
  (`max(0, pad_left - pad_right)`), so a default-padded grid collapses to a
  single space between columns. Byte-parity-tested (new golden `table_collapse`).
  The last per-cell Table feature Markdown tables need before wiring the element.
- **Table `pad_edge` + `show_edge`** (`table.rs`, port of upstream's flags):
  `Table::pad_edge(false)` drops the first column's left pad and the last
  column's right pad; `Table::show_edge(false)` removes the outer box edges
  (top/bottom borders + left/right glyphs), leaving only the internal dividers +
  content. `Box::get_top/get_row/get_bottom` gained an `edge` parameter.
  Byte-parity-tested (new goldens `table_pad_edge`, `table_no_edge`). These are
  two of the features Markdown tables need (`SIMPLE` box + `pad_edge=False`).
- **OSC 8 hyperlinks** (`Style::with_link`): a `Style` can now carry a link URL,
  rendered as an OSC 8 hyperlink around its styled text. **Markdown links**
  (`[text](url)`) render as the `markdown.link_url` style (underline blue) + the
  hyperlink. Byte-identical to real rich 15.0.0 **except** upstream's random `id=`
  field, which we omit for determinism (DIVERGENCES #20).

### Added (functional, non-byte-parity)
- **Markdown code blocks** (`markdown.rs`): fenced and indented code blocks now
  render via the `Syntax` renderable (language from the fence info string), so
  they're syntax-highlighted. Not byte-parity (syntect ≠ Pygments — DIVERGENCES
  #9/#18). Markdown now covers everything but links and tables.
- **`LogRender`** (`log_render.rs`, a Rust-native reimagining of `rich/_log_render.py`
  + `rich/logging.py`): formats a log record — optional time, a severity-colored
  `LogLevel`, message, optional path — into a styled line, using the same column
  styles (`log.time`, `logging.level.*`, `log.path`). Takes a `LogLevel` enum +
  strings, so the core stays dependency-light; a `log::Log` handler on top is a
  `rich-ext` follow-up (DIVERGENCES #19).
- **`Traceback`** (`traceback.rs`, a Rust-native reimagining of `rich/traceback.py`):
  `Traceback::new(&error)` walks an error's `Error::source()` chain and renders the
  message + `Caused by:` chain in a red-bordered `HEAVY` panel; `from_message` takes
  a plain string (e.g. a captured panic). No stack frames — Rust errors don't carry
  them (DIVERGENCES #19).
- **`Pretty`** (`pretty.rs`, a Rust-native reimagining of `rich/pretty.py`):
  `Pretty::new(&value)` / `Pretty::compact(&value)` format a value with its
  `Debug` impl (`{:#?}` / `{:?}`) and colorize the result with the built-in
  `ReprHighlighter` (numbers, strings, `None`/`Some`, paths, …). Rust has no
  reflection, so this replaces upstream's Python-object introspection; a few
  Rust spellings (`true`/`false`) are left unstyled (DIVERGENCES #19).
- **Syntax highlighting** (`syntax.rs`, port of `rich/syntax.py`'s renderable via
  the `syntect` crate): `Syntax::new(code, language)` highlights a code block into
  a solid colored panel (fg/bg + bold/italic/underline, each line padded to width
  with the theme background), with a configurable theme. Language is resolved by
  name or file extension; unknown languages render as plain text. **Not**
  byte-parity with upstream — `syntect` uses different grammars/themes than
  Pygments (DIVERGENCES #18); the first renderable that is functionally, not
  byte-, verified. `syntect` uses its pure-Rust `fancy-regex` backend (no C
  `onig`).

### Fixed / verified
- **Wrapping matches upstream for combining sequences** (DIVERGENCES #5 corrected):
  confirmed `cells::chop_cells` is byte-parity with real rich 15.0.0 — current
  rich folds over-long words char-by-char (not by grapheme), and 0-width combining
  marks stay attached to their base char in both. Added unit tests and a golden
  (`wrap_combining`, a decomposed `base + U+0301` fold). The old "we don't do
  grapheme wrapping" note was stale; the only residual difference is an empty
  leading chunk for a char wider than the fold width, which we suppress.

### Added
- **All built-in boxes** (`box.rs`): added the remaining upstream box constants —
  `ASCII2`, `ASCII_DOUBLE_HEAD`, `SQUARE_DOUBLE_HEAD`, `MINIMAL_HEAVY_HEAD`,
  `MINIMAL_DOUBLE_HEAD`, `SIMPLE`, `SIMPLE_HEAD`, `SIMPLE_HEAVY`, `HORIZONTALS`,
  `HEAVY_EDGE`, `DOUBLE_EDGE`, `MARKDOWN` — so `box.py` is complete. Byte-parity-
  tested (new goldens `table_simple`, `table_double_edge`). `SIMPLE`/`MARKDOWN`
  are the boxes Markdown tables will use.
- **Box substitution** (`Box::substitute`, port of `rich/box.py`'s `substitute`):
  on a legacy Windows console the fancy boxes (`ROUNDED`/`HEAVY`/`HEAVY_HEAD`) fall
  back to `SQUARE`, and on a non-UTF-8 terminal any non-ASCII box falls back to
  `ASCII`. Added the `legacy_windows`/`safe_box`/`ascii_only` `Console` flags
  (default off/on/off); `Panel` and `Table` apply the substitution. Byte-parity-
  tested against real rich 15.0.0. Resolves the mechanism half of DIVERGENCES #6
  (runtime auto-detection of legacy terminals is still deferred).
- **Table `no_wrap`** (`table.rs`, toward `rich/table.py`): `Table::column_no_wrap`
  marks a column whose cells crop to a single line with ellipsis instead of
  wrapping, and which doesn't shrink during collapse (only yielding via the
  last-resort even reduce). Byte-parity-tested (golden `table_nowrap`). Narrows
  DIVERGENCES #7 (only per-column ratio/min/max and the width-0 padding edge
  remain).
- **`Live`** (`live.rs`, port of `rich/live.py`'s manual-refresh core): drives a
  `LiveRender` over a generic `Write` sink — `start` hides the cursor and draws,
  `update`/`refresh` reposition and redraw in place, `stop` commits the final
  frame and shows the cursor. The emitted byte stream is byte-parity with real
  rich 15.0.0 (`auto_refresh=False`, `transient=False`). The background
  auto-refresh thread + transient/alt-screen modes are deferred (DIVERGENCES #17).
- **`LiveRender`** (`live_render.rs`, port of `rich/live_render.py`): wraps a
  renderable, records its rendered shape, and emits the `position_cursor` /
  `restore_cursor` control-code sequences (`\r` + erase-line + cursor-up×N) that
  redraw in place — the byte-parity-testable core of a `Live` display. Byte-parity
  vs real rich 15.0.0. (The full `Live` refresh loop + vertical-overflow cropping
  are deferred — issue #6.)
- **`Progress`** (`progress.rs`, port of `rich/progress.py`'s default display):
  `Progress::add_task(description, total, completed)` renders a static grid of
  tasks — each a description, a bar that flexes to fill the width, and a magenta
  percentage — reproducing the default `TextColumn`/`BarColumn`/`TaskProgressColumn`
  layout. Byte-parity-tested against real rich 15.0.0 (golden `progress_three`).
  Custom columns, the time/rate columns, and the `Live` refresh loop are deferred
  (DIVERGENCES #16 / issue #6).
- **`Status`** (`status.rs`, port of `rich/status.py`) + **styled spinner frames**:
  `Status::new("…")` renders a spinner (default `dots`, green) followed by a
  message; `Spinner::style` styles the frame while the trailing text stays plain.
  Byte-parity-tested against real rich 15.0.0 (static `t = 0` frame). Live-loop
  animation is still deferred (DIVERGENCES / issue #6).
- **HTML export** (`export.rs` + `terminal_theme.rs`, port of
  `Console.export_html` + `_export_format`): `Console::export_html(|c| …)`
  captures printed output and returns a self-contained HTML document with inline
  styles, resolving colors through a `TerminalTheme` (`DEFAULT_TERMINAL_THEME`).
  Added `Style::get_html_style` and `blend_rgb`. Byte-parity-tested against real
  rich 15.0.0 (full document + `get_html_style`). Also `export_html_classes`
  (the default `.r1 {…}` CSS-class stylesheet form, byte-parity). The `rich-cli`
  `--export-html` flag emits the class form (rich-cli's default). (SVG export
  deferred — DIVERGENCES #15.)
- **Generic `RegexHighlighter` base + `ISO8601Highlighter`** (`highlighter.rs`,
  toward `rich/highlighter.py`): extracted the shared regex-highlight loop into a
  public `RegexHighlighter` (base-style prefix + named-group patterns), refactored
  `ReprHighlighter` onto it, and added `ISO8601Highlighter` for standard extended
  date/time strings (`iso8601.date`/`time`/`timezone`). Highlighters now stylize
  **every** matched named group (unknown names → null style), reproducing
  upstream's per-field segment boundaries. Byte-parity-tested vs real rich 15.0.0
  (repr unchanged; ISO8601 new). See DIVERGENCES #13/#14 for the remaining gaps.
- **Table per-column width, style + ellipsis overflow** (`table.rs`, toward
  `rich/table.py`): `Table::column_width` pins a column's content width (content
  wraps to it instead of the column shrinking), `Table::column_style` styles a
  column's body cells, and table cells now wrap with **ellipsis overflow** (a
  word wider than the column is cropped with `…`), matching upstream's default.
  Added the last-resort even `ratio_reduce` when fixed columns overflow.
  Byte-parity-tested against real rich 15.0.0. (Per-column `ratio`/min/max and
  `no_wrap` still deferred — DIVERGENCES #7.)
- **Markup fidelity** (`markup.rs`, toward `rich/markup.py`): a `[…]` is now only
  a tag when it starts with `[a-z#/@]` (so `[Hello]`/`[42]` are literal text),
  unmatched closing tags return `RichError::Markup`, `@`-tags apply no visible
  style, and a public `markup::escape` implements upstream's backslash-doubling
  escape. Byte-parity-tested (literal brackets, meta tags) + negative tests for
  the error cases. Narrows DIVERGENCES #2.
- **`Styled`** (`styled.rs`, port of `rich/styled.py`) and **`Screen`**
  (`screen.rs`, port of `rich/screen.py`): `Styled` lays a `Style` under an
  entire renderable (each segment's own style still wins); `Screen` fills the
  console region (width × height) with a child, cropping/padding to an exact
  rectangle with an optional background style. Added `Segment::apply_style` and
  `Segment::set_shape` (now shared with `Layout`). `Styled` is byte-parity-tested
  against real rich 15.0.0; `Screen` has a known trailing-newline edge
  (DIVERGENCES #12).
- **`rich-cli` render modes** (`crates/rich-cli`): the binary now implements a
  slice of upstream rich-cli's flags — `-p/--print` (console markup),
  `-m/--markdown`, `-j/--json`, and `--rule` — plus `-w/--width`,
  `--left/--center/--right` justification, stdin (`-`), and file-extension
  auto-detection (`.md`/`.json`). Covered by CLI integration tests. (Syntax, CSV,
  panel, ipynb, URL fetch, and HTML export remain roadmap items.)
- **Height-aware `Panel`** (+ `Console::render_lines` height handling): a `Panel`
  now consumes `options.height`, expanding its content to fill the imposed height
  (`child_height = height − 2 − padding`) so a `Panel` used as a `Layout` leaf
  fills its region instead of being blank-padded. `render_lines` crops/pads any
  renderable to an explicit height. Byte-parity-tested against real rich 15.0.0
  (Panel-in-layout goldens). Resolves the height half of DIVERGENCES #11.
- **Layout** (`layout.rs` + `ratio.rs`, port of `rich/layout.py` +
  `_ratio.ratio_resolve`): split a region into ratioed/sized rows (`split_row`)
  and columns (`split_column`), recursively; each leaf is rendered to an exact
  `(width, height)` block (`Segment.set_shape`) and tiled. Added console
  `height` (builder + detection) and `ConsoleOptions::update_dimensions`.
  Byte-parity-tested against real rich 15.0.0 (column/row/nested, incl. `size`).
  (Empty-leaf placeholder renders blank; height-aware container leaves like
  `Panel` don't yet expand to fill — DIVERGENCES #11.)
- **Console capture + export** (`Console::capture` / `export_text`, port of
  `Console.capture()` + `export_text(styles=False)`): run a closure with output
  recorded to an internal segment buffer instead of stdout, returning either the
  rendered ANSI (`capture`) or plain style-stripped text (`export_text`). Captures
  nest. Byte-parity-tested against real rich 15.0.0. (HTML/SVG export deferred.)
- **ANSI decoder** (`ansi.rs`, port of `rich/ansi.py`): `AnsiDecoder` tokenizes a
  terminal string and turns SGR sequences back into styled `Text` — attributes,
  16/256/truecolor foreground + background, lenient parsing, and cross-line style
  persistence. Byte-parity-tested against real rich 15.0.0 (re-render round-trip).
  Added `Color::from_ansi`/`from_rgb` and `Style::from_color`. (OSC hyperlinks are
  skipped, pending `Style` link support — DIVERGENCES #10.)
- **Control codes** (`control.rs`, port of `rich/control.py` + `ControlType`):
  the `Control` renderable and typed `ControlType` sequences — screen clear,
  cursor home/move/move_to/move_to_column, show/hide cursor, alt-screen toggle,
  bell, erase-in-line — plus `Console::control`/`clear`/`show_cursor`/`bell`.
  Control segments are written only to a real terminal (matching upstream's
  `_render_buffer`). Byte-parity-tested against real rich 15.0.0.
- **Markdown** (`markdown.rs`, port of `rich/markdown.py` core): renders
  paragraphs, ATX headings (h1–h6, centered h1), **bullet + ordered lists**,
  **block quotes**, **thematic breaks (hr)**, and inline strong/emphasis/code via
  `pulldown-cmark`, as justified full-width blocks separated by blank lines.
  Byte-parity-tested against real rich 15.0.0. (Code blocks, links, and tables
  deferred — DIVERGENCES #9.)
- **Full spinner table**: all 73 built-in spinners are vendored
  (`spinner_data.rs`) from `_spinners.py`, replacing the curated subset.
- **ReprHighlighter** (`highlighter.rs`, port of `rich/highlighter.py`): the
  built-in highlighter that auto-colors numbers, bools, `None`, strings, paths,
  URLs, braces, calls, IPs, UUIDs, and tags. Patterns vendored verbatim
  (`repr_patterns.rs`) and compiled with `fancy-regex`; enabled via
  `ConsoleBuilder::highlight(true)`. Byte-parity-tested against real rich 15.0.0
  across many pattern types.
- **Full emoji table**: the complete ~3600-entry `_emoji_codes` table is vendored
  (`emoji_codes.rs`, binary-searched), so every `:shortcode:` resolves. Resolves
  the former curated-subset divergence.
- **Full 256-color names**: the complete `ANSI_COLOR_NAMES` table is vendored
  (`color_names.rs`), so markup/style names like `[orange1]`, `[grey37]`,
  `[deep_sky_blue1]` resolve to the correct 8-bit colors. Byte-parity-tested;
  resolves the former partial-names divergence.
- **Table title / caption / show_lines**: `Table::title` and `caption` render
  centered above/below the table (italic / dim-italic), and `show_lines` draws a
  separator between body rows. Byte-parity-tested against real rich 15.0.0.
- **Table `expand` + per-column justify**: `Table::expand` fills the available
  width (distributing leftover space via `ratio_distribute`), and
  `Table::add_column_justify` justifies a column's cells left/center/right.
  Byte-parity-tested against real rich 15.0.0.
- **Table flexible widths**: when a table's natural width exceeds the console
  width, the widest columns now shrink and their cells wrap to fit (port of
  `Table._calculate_column_widths` + `_collapse_widths` + `_ratio.ratio_reduce`,
  with banker's rounding). Byte-parity-tested against real rich 15.0.0.
- **Measurement** (`Renderable::measure` + top-level measurement-fit): the print
  path now shrinks the width to a renderable's measured content width when no
  explicit justify is set. This makes a bare `Text::justify(...)` shrink to its
  content (matching upstream, resolving the former DIVERGENCES #9), while
  `print(justify=…)` and container-embedded justify still pad to full width.
  Byte-parity-tested against real rich 15.0.0.
- **Bar** (`bar.rs`, port of `rich/bar.py`): a horizontal bar spanning
  `[begin, end]` within `[0, size]`, with eighth-block sub-cell edges. Byte-parity-
  tested against real rich 15.0.0.
- **Emoji** (`emoji.rs`, port of `_emoji_replace` + a curated `_emoji_codes`
  subset): `:name:` shortcodes (with `-emoji`/`-text` variants) expand in the
  `Console` print path (default on, `ConsoleBuilder::emoji`). Byte-parity-tested
  against real rich 15.0.0; replacement logic is complete, code table curated.
- **Text justify** (`Text::justify` + `Console::print_justified`): left/center/
  right justification, padded to the render width. Byte-parity-tested against
  real rich 15.0.0 inside a `Panel` and via `print(justify=…)`. (Bare top-level
  measurement-fit is deferred — see DIVERGENCES #9.)
- **JSON** (`json.rs`, port of `rich/json.py`): parses a JSON string (via
  `serde_json` with `preserve_order`) and pretty-prints it with 2-space indent
  and the default highlight colors (bold braces, bold-blue keys, green strings,
  bold-cyan numbers, italic bools/null). Byte-parity-tested against real rich
  15.0.0 for ASCII documents. First core third-party content dependency.
- **Panel subtitle**: `Panel::subtitle` / `subtitle_align` (drawn into the bottom
  border, mirroring title alignment). Byte-parity-tested against real rich 15.0.0.
- **Spinner** (`spinner.rs`, port of `rich/spinner.py` + a subset of
  `_spinners.py`): `Spinner::render(time)` picks the animation frame for an
  elapsed time (dots/line/dots2/arrow/simpleDots). Frames verified against real
  rich 15.0.0. (Live-loop animation deferred.)
- **ProgressBar** (`progress_bar.rs`, port of `rich/progress_bar.py` static
  render): determinate bars with half-cell resolution and the default
  `bar.complete`/`bar.finished`/`bar.back` styles. Byte-parity-tested against
  real rich 15.0.0. (Indeterminate "pulse" deferred.)
- **Title alignment** for `Rule` and `Panel` (`HorizontalAlign::{Left,Center,Right}`,
  shared with `Align`): `Rule::align` and `Panel::title_align`. Byte-parity-tested
  against real rich 15.0.0.
- **Columns** (`columns.rs`, port of `rich/columns.py`): packs items into as many
  equal-gap columns as fit the width, filling row by row (ports the column-count
  fitting algorithm). Byte-parity-tested against real rich 15.0.0.
- **Constrain** (`constrain.rs`, port of `rich/constrain.py`): render a child
  within a reduced max width. Byte-parity-tested against real rich 15.0.0.
- **filesize** (`filesize.rs`, port of `rich/filesize.py`): `decimal()` SI byte
  formatting, unit-tested against real rich reference values.
- **Align** (`align.rs`, port of `rich/align.py` horizontal axis): left/center/
  right alignment of a child within the available width. Byte-parity-tested
  against real rich 15.0.0.
- **Tree** (`tree.rs`, port of `rich/tree.py`): renders a hierarchy with the
  `├──`/`└──` guide lines. Byte-parity-tested against real rich 15.0.0.
- **Table** (`table.rs`, port of `rich/table.py` core): columns, rows, box choice
  (default `HEAVY_HEAD`), per-cell padding, bold headers, and multi-line/wrapped
  cells. Byte-parity-tested against real rich 15.0.0 (SQUARE and HEAVY_HEAD).
  Added `Box::get_row`/`RowLevel`, `Segment::simplify`, and the `HEAVY_HEAD` box.
- **Word wrapping** (`wrap.rs`, port of `_wrap.divide_line` + `cells.chop_cells`):
  `Text` now wraps to the available width — breaking on words and folding
  over-long words — so `Panel`/`Padding` reflow long content instead of cropping.
  Byte-parity-tested against real rich 15.0.0.
- **Layout primitives**: width-aware render model (`ConsoleOptions`,
  `Console::render_lines`, `Segment::split_lines`/`adjust_line_length`) plus the
  first composite renderables — `box` (ROUNDED/SQUARE/HEAVY/DOUBLE/MINIMAL/ASCII),
  `Rule`, `Padding`, and `Panel` — all byte-parity-tested against real rich 15.0.0.
- Golden harness extended with a renderables fixture (`renderables.tsv`), captured
  in Python UTF-8 mode for deterministic box glyphs.

### Fixed
- **`Color::parse("color(N)")` for N < 16**: now returns a `Standard` color (SGR
  30–37/90–97) instead of an 8-bit palette color, matching `Color.parse`. This
  makes ANSI round-trips of the standard colors byte-identical to upstream.

### Foundation
- **Workspace scaffold**: `rich` (core, mirroring upstream **15.0.0**),
  `rich-ext` (`0.1.0`), and `rich-cli` (mirroring upstream **1.8.1**).
- **First vertical slice** of the `rich` core, parity-tested against real
  `rich` 15.0.0: `color`, `style`, `cells`, `segment`, `markup`, `text`, `theme`,
  `console`, plus the `protocol` extension points (`Renderable`, `Highlighter`).
- **Internal plugin registry** in `rich-ext` (`ExtensionRegistry`, `ConsoleExt`)
  with an example `NumberHighlighter`, demonstrating the core/ext boundary.
- **`rich-cli` binary**: argument handling, plain-file printing, and a capability
  demo. Rich rendering subcommands are tracked as roadmap issues.
- **Governance**: `AGENTS.md`, `docs/{ARCHITECTURE,PORTING,PLUGINS,DIVERGENCES}.md`,
  the `sync-upstream` and `port-module` skills, `UPSTREAM.toml` pins, and the
  golden parity harness (`scripts/capture_golden.py`).
