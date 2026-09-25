# rs_rich.ext

`rs_rich.ext` is the port's own extension crate, `rs-rich-ext`, from Python.
Rich has no counterpart: this is what the port adds on top of it. Everything
renders in Rust; the Python classes store what you give them and build the
Rust value when printed, so they work wherever a renderable does:
`Console.print`, table cells, panels.

```python
from rs_rich.console import Console
from rs_rich.ext.diagnostic import Diagnostic, Location

console = Console(width=60)
console.print(Diagnostic.error("mismatched types", code="E0308",
                               location=Location("src/main.rs", 12, 5),
                               causes=["expected `u16`, found `&str`"]))
```

```text
error[E0308]: mismatched types
  --> src/main.rs:12:5
caused by: expected `u16`, found `&str`
```

## Modules

Each Rust module `rich_ext::<name>` is `rs_rich.ext.<name>`, and
`rs_rich.ext` itself re-exports every class and function.

| Guide | Modules |
|---|---|
| [Diagnostics and logs](diagnostics.md) | `diagnostic`, `stacktrace`, `dashboard`, `hyperlink`, `highlighter`, `event`, `log_handler` |
| [Structured data](data.md) | `data` |
| [Diffs and test reports](diffs.md) | `diff` |
| [Transforms](transforms.md) | `transform` (and the data, table and patch transforms) |
| [Workflows and status](workflows.md) | `workflow`, `cancel`, `transfer`, `countdown`, `notify` |
| [Tables, badges and formatting](tables.md) | `table`, `badge`, `size_bar`, `format`, `redact`, `derive` |
| [Terminals and accessibility](terminals.md) | `capabilities`, `fidelity`, `a11y`, `ansi_explain`, `sanitize`, `encoding`, `target`, `theme` |
| [Inspectors](inspectors.md) | `source_view`, `hex`, `unicode_inspect`, `env_inspect` |
| [Layout and live output](layout.md) | `layout`, `live` |
| [CLI authoring](cli-authoring.md) | `cli_doc` |

The extension registry (`ExtensionRegistry`, `install_defaults`) is the plugin
host, documented with the plugin API; `rs_rich.ext.registry` re-exports it.

## Conventions

- **Names, not enums.** Where Rust takes an enum, Python takes its name as a
  string: `level="warning"`, `view="expanded"`, `layout="side_by_side"`,
  `symbols="ascii"`. A wrong name is a `ValueError` listing the right ones.
- **Characters, not bytes.** Offsets into text (source spans, links,
  diff ranges, ANSI tokens) are Python string indices. Rust's are UTF-8 byte
  offsets; the bindings convert both ways.
- **Seconds.** Durations and times are `int`/`float` seconds (or a
  `datetime.timedelta`); `now=` arguments are seconds on a clock you choose.
  `ManualClock` makes task trees and transfers deterministic.
- **Keyword arguments.** Rust's builder chains become keyword arguments, and
  the `add_*` methods of mutable classes return the object so calls chain.
- **Errors.** Every `rs_rich.ext` exception derives from `ExtError`:
  `DataError`, `SelectError`, `TransformError` (and `PipelineError`),
  `PatchParseError`, `TestParseError`, `DiagnosticSpanError`,
  `ConstraintError`, `LiveCoordinatorError`, `RedactPatternError`,
  `EncodingError`, `TransferCancelled`.

## Not available from Python

| Rust | Why |
|---|---|
| `macros` (`richf!`, `rich_println!`, `#[derive(Rich)]`) | Compile-time Rust macros; `rs_rich.ext.derive.Record` is the runtime they expand to |
| `adapters` (`log`, `tracing`) | Rust logging facades; Python's `logging` goes through `rs_rich.logging` |
| `cli` (`CliExtensions`) and `cli_doc::clap` | The `rich` binary's own options and clap integration |
| `stacktrace::capture`, `stacktrace::panic_hook` | Rust backtraces and panics; `StackTrace.from_exception` is the Python equivalent |
| `data::from_serialize` and the serde helpers | `DataNode.from_python` takes Python values directly |
| `TraceParser`, `SelectorBackend`, `Degradable`, `LinkProvider`, `FsProbe` as traits | Generic plumbing: parsers, probes and link providers are accepted as Python objects where it matters (`StackTrace.parse(parsers=...)`, `PathView(probe=...)`) |
| `transfer_columns()` | Progress columns for Rust's `Progress`; use `Transfer.task_fields()` with `rs_rich.progress` |
| `testing`, `qa`, `diff::assert` | Behind `rs-rich-ext`'s `testing` feature, which the wheel does not build: the functions in `rs_rich.ext.testing` and `rs_rich.ext.qa` raise `NotImplementedError` |
