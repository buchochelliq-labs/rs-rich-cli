# The ext crate

`rs-rich-ext` holds everything this port adds on top of Python `rich`. The core
crate, `rs-rich`, stays a faithful mirror of upstream; this crate builds on its
public API and never changes how a plain `rich::Console` behaves. Read
[Extensions](extensions.md) for why the split exists and how to opt in.

```bash
cargo add rs-rich rs-rich-ext
cargo add rs-rich-ext --features macros,log    # add features as needed
```

Every module is listed below with the feature it needs. "none" means it is
always available. The `use` path is `rich_ext::<module>`.

## Output and diagnostics

| Module | What it gives you | Feature | Guide |
|---|---|---|---|
| `ConsoleExt`, `registry` | `install_extensions()` and `ExtensionRegistry`: explicit installation of highlighters | none | [Extensions](extensions.md) |
| `theme` | `extended_theme()`: upstream's theme plus `error`/`warning`/`info`/`success`, CLI help, diff and workflow styles (`STYLE_TABLES`) | none | [Extensions](extensions.md#the-extended-theme) |
| `highlighter` | `NumberHighlighter`, the default extension | none | [Extensions](extensions.md#installing-the-default-extensions) |
| `hyperlink` | `Hyperlinker`: OSC 8 links for URLs, paths, `path:line:col`, `#123`; editor URL templates | none | [Extensions](extensions.md#hyperlinks) |
| `sanitize` | `sanitize_terminal_controls`: make untrusted text inert | none | [Extensions](extensions.md#sanitizing-untrusted-text) |
| `encoding` | `Encoding`: strict, explicit UTF-8/UTF-16 decoding | none | [Extensions](extensions.md#decoding-input-explicitly) |
| `diagnostic` | `Diagnostic`: compiler-style errors with snippets, labels, suggestions; `DiagnosticInfo` for error types; `from_anyhow` | none; `anyhow` for `from_anyhow` | [Diagnostics](diagnostics.md) |
| `stacktrace` | Parse Rust, Python, Java and JavaScript traces into one `StackTrace`; `panic_hook` | none | [Diagnostics](diagnostics.md#stack-traces) |
| `dashboard` | `DiagnosticsDashboard`: counts, top codes and diagnostics grouped by file | none | [Diagnostics](diagnostics.md#many-diagnostics-at-once) |
| `event` | `StructuredEvent`: typed log records that render compact or expanded | none | [Logging](logging.md#structured-events) |
| `log_handler` | `RichHandler`: upstream's `logging.RichHandler` layout | none | [Logging](logging.md) |
| `adapters` | `LogAdapter` (the `log` facade) and `EventLayer` (a `tracing` layer), both feeding an `EventSink` | `log`, `tracing` | [Logging](logging.md) |
| `target` | `RenderTarget` and `TargetKind`: explicit, deterministic destinations | none | [Live and layout](live-and-layout.md#render-targets) |
| `layout` | `LayoutNode`, `Constraint`, `allocate`, `OverflowPolicy`, `Overflowing`: bounded layouts | none | [Live and layout](live-and-layout.md#bounded-layouts) |
| `live` | `LiveCoordinator`: several live regions and printed lines through one writer | none | [Live and layout](live-and-layout.md#coordinated-live-regions) |
| `macros` | `rich_table!`, `rich_panel!`, `rich_tree!`, `rich_progress!`, `rich_dbg!`; with `macros`, `rich_println!`, `rich_eprintln!`, `rich_trace!` | none; `macros` for the print macros | [Macros](macros.md) |
| `richf!`, `style!`, `theme_key!`, `markup!`, `#[derive(Rich)]` | Compile-time checked markup and styles; derive rendering (re-exported from `rs-rich-macros`) | `macros` | [Macros](macros.md) |
| `derive` | `RichRecord`, `Field`, `render` and `table`: the runtime behind the derive | none | [Macros](macros.md#many-records-as-a-table) |

## Workflows, tables and status

| Module | What it gives you | Feature | Guide |
|---|---|---|---|
| `workflow` | `CommandRecord`/`CommandView`/`CommandRunner`: a process's output, exit status and duration, folded, with a live spinner; `TaskTree`: nested tasks with aggregate status and cancellation; `CompletionSummary` | none | [Workflows](workflows.md) |
| `transfer` | `Transfer`, `Transfers`, `transfer_columns()`, `TransferReader`/`TransferWriter`: download and upload progress with rate, ETA, retries and cancellation | none | [Transfers and status](transfers-and-status.md#transfers) |
| `countdown` | `Backoff`, `RetryStatus`, `RateLimit`, `CountdownBar`, `CountdownWait`: retry and rate-limit countdowns | none | [Transfers and status](transfers-and-status.md#retries-and-rate-limits) |
| `notify` | `Notification`, `Notifications`: transient toasts with expiry | none | [Transfers and status](transfers-and-status.md#notifications) |
| `cancel` | `CancelToken`: one cancellation flag, with child tokens, shared by the modules above | none | [Workflows](workflows.md#cancellation) |
| `table` | `TableData`: stable multi-column sort, grouping and aggregates; `StreamingTable`: keyed rows that re-render only what changed | none | [Tables](tables.md) |
| `badge`, `size_bar` | `Badge`/`Badges`: status, label, link and metadata chips; `SizeBar`: a size against a total or limit | none | [Badges and redaction](badges-and-redaction.md) |
| `format` | Sizes, rates, durations, relative times, timestamps, percentages and numbers as people read them | none | [Badges and redaction](badges-and-redaction.md#formatters) |
| `redact` | `Redactor`: mask secrets in strings, ANSI text and rendered segments before they are exported or recorded | none | [Badges and redaction](badges-and-redaction.md#redaction) |

## Structured data and CLI authoring

| Module | What it gives you | Feature | Guide |
|---|---|---|---|
| `data` | One document tree for JSON, INI and dotenv; `Explorer`, tables, flatten, search, diff, redaction; `print_json`/`print_table`/`print_tree` for `serde` values | `data`; `yaml`, `toml`, `xml` for those formats; `jsonpath` for JSONPath selectors | [Structured data](structured-data.md) |
| `cli_doc` | From one `CommandSpec`: help, errors as diagnostics, shell completions, Markdown and man pages, config reference and precedence | none; `clap` for `CommandSpec::from_clap` and `cli_doc::clap` | [CLI authoring](cli-authoring.md) |
| `cli` | The `rich` binary's extension options (`--encoding`, `--gif-mode`) | none | [CLI authoring](cli-authoring.md) |

## Testing and QA

| Module | What it gives you | Feature | Guide |
|---|---|---|---|
| `diff` | Myers diff engine, `DiffView` (unified or side by side, text or ANSI), `SourceDiff`, `git` patch rendering | none | [Diffs and test reports](diffs-and-test-reports.md) |
| `diff::test_report` | `TestReport` from JUnit XML or libtest JSON | `test-report` | [Diffs and test reports](diffs-and-test-reports.md) |
| `diff::assert` | `assert_rich_eq!` and friends: assertions that panic with a rendered diff | `testing` | [Diffs and test reports](diffs-and-test-reports.md) |
| `testing` | `RenderSnapshot`: deterministic render snapshots as JSON for downstream tests | `testing` | [QA](qa.md) |
| `qa` | Approved screenshots, layout stress, render lint, explain, profiling, seeded fuzzing, capability matrix, benchmarks | `testing` | [QA](qa.md) |

## Capabilities and accessibility

| Module | What it gives you | Feature | Guide |
|---|---|---|---|
| `capabilities` | `Capabilities::system()` / `detect`: what the terminal supports, and why | none; `serde` to serialize reports | [Capabilities](capabilities.md) |
| `fidelity` | `Fidelity` levels and `Degrade`: render anything without colour, without styles, or ASCII only | none | [Capabilities](capabilities.md) |
| `a11y` | `semantic::AccessibleText` for screen readers; `policy::AccessibilityPolicy` (reduced motion, high contrast, monochrome); `contrast::check_theme` | none; `serde` to serialize findings | [Accessibility](accessibility.md) |
| `ansi_explain` | `explain`: decode escape sequences into words | none; `serde` to serialize explanations | [ANSI explained](ansi.md) |
| `source_view` | `SourceView`: source with line numbers and search highlights | none | [CLI viewers](../cli/walkthrough.md#viewing-and-inspecting-anything) |
| `hex`, `unicode_inspect`, `env_inspect` | `HexView`, `UnicodeView`, `EnvView` / `PathView`: bytes, graphemes and environment | none | [CLI viewers](../cli/walkthrough.md#viewing-and-inspecting-anything) |

## Feature flags

All features are off by default.

| Feature | Enables | Extra dependencies |
|---|---|---|
| `macros` | `richf!`, `style!`, `theme_key!`, `markup!`, `#[derive(Rich)]`, print macros | `rs-rich-macros` |
| `anyhow` | `Diagnostic::from_anyhow` | `anyhow` |
| `log` | `adapters::LogAdapter`; `RichHandler` as an `EventSink` | `log` |
| `tracing` | `adapters::EventLayer`; `RichHandler` as an `EventSink` | `tracing`, `tracing-subscriber` |
| `data` | `rich_ext::data` with JSON, INI and dotenv | `serde`, `serde_json` |
| `yaml`, `toml`, `xml` | Those formats in `data` (each implies `data`) | `saphyr-parser`, `toml`, `quick-xml` |
| `jsonpath` | JSONPath selectors in `data` | none |
| `clap` | `CommandSpec::from_clap`, `cli_doc::clap::parse_or_exit` | `clap` |
| `testing` | `testing`, `qa`, `diff::assert` | `serde`, `serde_json` |
| `test-report` | `diff::test_report` | `serde`, `serde_json`, `quick-xml` |
| `serde` | `Serialize` for capability reports, contrast findings and ANSI explanations | `serde` |

## Examples

Every guide page has a runnable example in `crates/rich-ext/examples`, named
`guide_<topic>`. Each prints to the terminal, or with `-- --svg DIR` writes the
screenshots used on these pages:

```bash
cargo run -p rs-rich-ext --example guide_extensions
cargo run -p rs-rich-ext --example guide_diagnostics --features anyhow
cargo run -p rs-rich-ext --example guide_logging --features log,tracing
cargo run -p rs-rich-ext --example guide_live_layout
cargo run -p rs-rich-ext --example guide_macros --features macros
```

## See also

- [Tutorial](../../tutorial/index.md): the core library, from the first
  `Console` to live output.
- [Extending](../../PLUGINS.md): extension points and the plugin roadmap.
- [Divergences](../../DIVERGENCES.md): where the core deliberately differs
  from upstream.
- API: [`rs-rich-ext` on docs.rs](https://docs.rs/rs-rich-ext/latest/rich_ext/).
