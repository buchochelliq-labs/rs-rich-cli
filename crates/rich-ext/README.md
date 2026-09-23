# rich-ext

Extensions and the internal plugin registry for the [`rich`](../rich) Rust port.

## Why this crate exists

[`rich`](../rich) is a **faithful mirror** of Python `rich`: it ships upstream's
behaviour and nothing else, so that absorbing a new upstream release is a
diff-and-port rather than a merge conflict.

Everything we add on top lives here instead. Like every crate in this
repository, this crate follows an **independent SemVer** that started at
`0.0.1`; its version is bumped only when `rs-rich-ext` is selected for a release
and never mirrors the Python projects' release numbers.

The rule, in one line: *never edit the core to add a feature.*

## What's here

- The **extension registry** — the seam that installs extra highlighters (and
  in future, boxes, themes and renderables) into a `Console` by explicit
  registration rather than compile-time magic.
- `ConsoleExt`, which adds `install_extensions()` to a `Console`.
- Example extensions, including a custom highlighter that proves the plugin
  boundary works without touching the core.

```rust
use rich::Console;
use rich_ext::ConsoleExt;

let mut console = Console::builder().build();
console.install_extensions();
```

## Status

The registry surface is currently **internal** — `rich-ext` is its only
registrant. Promoting it to a stable public API, and evaluating dynamic
third-party plugin loading, is tracked as its own roadmap issue.

## Licence

MIT.

### Explicit render destinations

`target::RenderTarget` constructs a fully configured console from
`rich::protocol::TargetCapabilities` and a theme. It never detects the process
terminal. `segments` and `text` apply destination policy: plain streams strip
colour, links and controls; capture and HTML/SVG disable interactive protocols.
A zero-sized viewport returns empty without rendering children. An attached
`ConsoleEnvironment` carries this immutable context through nested renderables;
legacy consoles remain unchanged. Custom writers declare their own capabilities.

### Bounded layouts

`layout::LayoutNode` composes horizontal/vertical splits with `Constraint`
(min/max/preferred/flex), intrinsic `content_width`/`content_height`, alignment,
and explicit Wrap/Fold/Crop/Ellipsis/Visible overflow. Allocation shrinks requested
sizes under viewport pressure, reports relaxed requests, and keeps capped surplus
as padding. Zero cells skip children. Containers always clip at their boundary,
including Visible overflow. Core Layout and ratio behaviour remain unchanged.

### Structured events

`event::StructuredEvent` retains typed `Value` fields until render time. Field
replacement preserves insertion position; explicit ordering and hiding are opt-in.
Messages are literal unless constructed as `Message::Markup`. Compact/expanded
views share bounded overflow and resolve `event.message`, `event.field`,
`event.value` and `event.severity.*` against the supplied theme. EventContext
never reads a clock, thread or environment implicitly.

### Diagnostics

`diagnostic::Diagnostic` supports causes, labels, supplied source snippets,
metadata, notes and help. `from_error` bounds the cause walk and marks cycles or
truncation. `SourceSnippet::new` validates UTF-8 byte spans. Source/underline rows
are cropped together at narrow widths to preserve column meaning; other content
uses the selected overflow policy. No source files are opened implicitly.
Attach multiple diagnostic blocks with `StructuredEvent::diagnostic`.

For compiler-style output, give a diagnostic a `Level` and code
(`Diagnostic::error(…).code("E0308")` renders `error[E0308]: …`) and a
`Location`. Snippets take labelled primary (`^^^`) and secondary (`---`) spans
(`primary_label`, `primary`, `secondary`), and `Suggestion::replace` shows the
edited line. Implement `DiagnosticInfo` on an error type, such as a `thiserror`
enum, to supply its level, code, help and location to `Diagnostic::from_info`.
The `anyhow` feature adds `Diagnostic::from_anyhow`, which maps the context chain
and any captured backtrace. `dashboard::DiagnosticsDashboard` groups many
diagnostics by file, level and code. See the `diagnostic` and `dashboard`
examples.

`hyperlink::Hyperlinker` turns URLs, paths, `path:line:column` and (given a
repository) `#123` references into OSC 8 links, or into editor URLs through a
template. Non-terminals get the plain text. It is a `Highlighter`, so
`RichHandler::highlighter` accepts it. Diagnostics, the dashboard and stack
traces link their locations through it.

`stacktrace::parse` normalises Rust, Python, Java and JavaScript traces, including
chained causes, into one `StackTrace` with most recent call last. Add formats
with `Parsers::with_parser`. `stacktrace::capture` and `stacktrace::panic_hook`
build a trace for the current thread; installing the hook is left to you.

Enable optional `log` or `tracing` features for `adapters::LogAdapter` or
`adapters::EventLayer`. Supply an `Arc<dyn EventSink>` and install/filter the
facade yourself. Sink errors are not recursively logged. Typed tracing visitors
retain signed/unsigned 64/128-bit numbers; Debug-only fields become strings.
See examples `log_adapter` and `tracing_adapter`. Layer integration follows
[tracing-subscriber Layer](https://docs.rs/tracing-subscriber/0.3.23/tracing_subscriber/layer/trait.Layer.html).

### Coordinated Live regions

`live::LiveCoordinator` owns one writer and opaque region IDs. Call `refresh`
explicitly; `handle().print` coordinates ordinary messages with the display.
Do not share that writer with another Live loop or bypass it with Console.print.
The inline viewport reserves one row for insertion and one guard column to avoid
terminal auto-wrap. Control/raster content is rejected. Noninteractive output
emits a finite final snapshot. Call `finish` to observe cleanup errors; Drop is
best effort and cannot restore state after process abort or SIGKILL.

Live interactivity and writer ownership are fixed for a coordinator lifetime.
Finish and create a new coordinator to switch between terminal and pipe. Resize
updates the safe viewport; applications must rerender their region content for
new dimensions. Cursor cleanup covers ordinary errors and Rust unwinding, not
process termination that prevents destructors from running.

### Downstream render snapshots

Enable `testing`, construct an explicit `RenderTarget`, then use
`testing::RenderSnapshot::capture(&target, &renderable)`. `to_json()` serializes
schema version, dimensions, plain text, ANSI and styled segment metadata;
`expected.diff(&actual)` reports the first changed line or metadata path. Raw
renderable line endings are preserved, without adding Console.print's newline.
The `snapshot` example demonstrates a downstream test fixture. No framework is
required. New `Value` unsigned and 128-bit variants must be handled by exhaustive
matches before upgrading.
