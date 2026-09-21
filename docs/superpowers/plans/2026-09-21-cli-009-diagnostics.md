# Events and Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render typed events and diagnostic chains consistently across explicit destinations.

**Architecture:** Owned data models implement Renderable through target context and shared overflow. Optional adapters translate external records into the same model.

**Tech Stack:** Rust 2021, Rust 1.90, Cargo and the existing public Rich APIs.

**Spec:** [Approved design](../specs/2026-09-20-cli-0.0.9-expanded-design.md).

## Global Constraints

Read the [release index](2026-09-21-cli-009-index.md) and AGENTS.md before execution. Preserve default output; keep new library behaviour in rich-ext/rich-art. Do not add required Renderable methods or fields to ConsoleOptions/ImageOptions. No implicit ambient probes, source-file reads or global logger installation. Rust 1.90, Rust 2021 and `unsafe_code = "deny"` apply. Before each commit run `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, and `env -u NO_COLOR cargo test`; commit only after all pass. Publication is a separate handoff.

## Review Focus

1. Duplicate and hidden field ordering stays stable (C1).
2. Literal brackets and nonfinite floats cannot become markup or invalid output (C1).
3. Invalid UTF-8 spans and cyclic errors fail or truncate visibly (C2).
4. Narrow Unicode snippets retain correct underline positions (C2).
5. Adapter write errors cannot recurse; lean builds exclude adapters (C3).

---

### C1: Typed events and themed views

**Files:** Create `crates/rich-ext/src/event.rs`, `crates/rich-ext/tests/events.rs`; modify `crates/rich-ext/src/lib.rs`, `crates/rich-ext/src/theme.rs`, `crates/rich-ext/README.md`.

**Interfaces:** Define `Value::{Null,Bool(bool),Integer(i64),Float(f64),String(String),List(Vec<Value>),Map(Vec<(String,Value)>)}`, `Message::{Literal(String),Markup(String)}`, `Severity::{Trace,Debug,Info,Warn,Error,Fatal}`, `EventView::{Compact,Expanded}`. `StructuredEvent::new(message: Message) -> Self`; consuming builders `field(self,key:impl Into<String>,value:Value)->Self`, `field_order(self,keys:Vec<String>)->Self`, `hide_fields(self,keys:Vec<String>)->Self`, `view(self,view:EventView)->Self`, `context(self,context:EventContext)->Self`, `diagnostic(self,diagnostic:Diagnostic)->Self`. The last builder is added in C2. EventContext derives Default with optional String fields timestamp,target,module,thread,task,correlation_id and optional Severity severity, optional SourceLocation source. SourceLocation owns `path:String,line:usize,column:Option<usize>`. Implements Renderable; consumes B2's `fit_segments` and A1's attached context. Theme keys `event.message`, `event.field`, `event.value`, `event.severity.<lowercase severity>` resolve through Console theme with neutral/message, cyan/field, neutral/value and severity fallback colours.

- [ ] Write an events test building literal `[red]hello[/red]`, fields a=1,b=2,a=3, explicit order b, hidden b. At width80 assert a=3 appears once, a=1 and b=2 do not, and literal bracket text remains. Construct target using A1's exact RenderTarget constructor. Core assertions:

```rust
assert!(output.contains("[red]hello[/red]"));
assert_eq!(output.matches("a=3").count(), 1);
assert!(!output.contains("a=1"));
assert!(!output.contains("b=2"));
```

  Add widths1/2/20/80, multiline ordered maps/lists, negative integers and NaN/±infinity (display as `NaN`, `inf`, `-inf` literal strings). Assert capture/HTML/SVG retain identical field ordering with severity styles.
- [ ] Run `env -u NO_COLOR cargo test -p rs-rich-ext --test events`; expect unresolved event APIs.
- [ ] Implement ordered replacement and selection before formatting. Do not format fields into ANSI strings: build styled Text/Segments, then fit using B2. Compact uses message plus ` key=value`; expanded uses one labelled field per line, with recursively indented lists/maps. No clock is captured.

```rust
if let Some((_, old)) = fields.iter_mut().find(|(name, _)| name == &key) {
    *old = value;
} else {
    fields.push((key, value));
}
```

- [ ] Run events tests and A3 snapshots; run global gates. Commit `feat: add typed themed event renderables` with the listed files.

### C2: Diagnostics with bounded chains and source spans

**Files:** Create `crates/rich-ext/src/diagnostic.rs`, `crates/rich-ext/tests/diagnostics.rs`, `crates/rich-ext/examples/diagnostic.rs`; modify event.rs, lib.rs and README.md in rich-ext.

**Interfaces:** `Diagnostic::new(message:impl Into<String>)->Self`, `from_error(error:&(dyn std::error::Error+'static),max_depth:usize)->Self`; consuming builders `cause(self,message:impl Into<String>)->Self`, `note`, `help`, `label` with same argument/return types; `snippet(self,snippet:SourceSnippet)->Self`, `metadata(self,key:impl Into<String>,value:Value)->Self`, `view(self,view:EventView)->Self`. `SourceSnippet::new(name:String,source:String,span:std::ops::Range<usize>,context_lines:usize)->Result<Self,DiagnosticError>`. Both diagnostic and event render with the target's policy. DiagnosticError::InvalidSpan reports bounds/boundary failures without reading a file.

- [ ] Add constructor boundary tests:

```rust
assert!(SourceSnippet::new("x".into(), "é界".into(), 1..2, 1).is_err());
assert!(SourceSnippet::new("x".into(), "é界".into(), 2..5, 1).is_ok());
assert!(SourceSnippet::new("x".into(), "abc".into(), 2..1, 1).is_err());
```

  Test empty spans at EOF, multiline spans, tabs/combining/wide glyph underlines at widths1/2/20/80. Define a test Error whose source returns Some(self); assert visible cycle marker and bounded output. A chain longer than2 with max_depth2 must show a truncation marker. max_depth0 shows primary message plus truncation, never a blank diagnostic. Attach two diagnostics to an event and assert distinct ordered blocks.
- [ ] Run `env -u NO_COLOR cargo test -p rs-rich-ext --test diagnostics`; expect missing APIs.
- [ ] Validate ranges before slicing; map byte offsets to display-cell offsets using core cell helpers with tab expansion. Keep caller-provided source only, show line numbers and bounded context. Walk sources iteratively with pointer-identity visited set and depth limit; emit `[cycle]` or `[truncated]` once. Render annotations as segments through shared overflow, preserving primary/causes/snippets/metadata/notes/help order.

```rust
if span.start > span.end || span.end > source.len()
    || !source.is_char_boundary(span.start) || !source.is_char_boundary(span.end) {
    return Err(DiagnosticError::InvalidSpan);
}
```

- [ ] Run diagnostics/events snapshots and example, then global gates. Commit `feat: add bounded diagnostic chains and source snippets`.

### C3: Explicit adapters and CLI presentation

**Files:** Create `crates/rich-ext/src/adapters/mod.rs`, `log.rs`, `tracing.rs` alongside it, `crates/rich-ext/tests/adapters.rs`, `crates/rich-ext/examples/log_adapter.rs`, `crates/rich-ext/examples/tracing_adapter.rs`; modify ext Cargo.toml/lib.rs, CLI `src/main.rs`, `src/config.rs`, `src/demo.rs`, `tests/cli.rs`, `docs/cli.md`, `docs/PORTING.md`.

**Interfaces:** `EventSink: Send+Sync { fn emit(&self,event:StructuredEvent)->std::io::Result<()>; }`; `LogAdapter::new(sink:std::sync::Arc<dyn EventSink>,level:log::LevelFilter)->Self` implements log::Log without installation; `EventLayer::new(sink:Arc<dyn EventSink>)->Self` implements tracing_subscriber::Layer for compatible Subscriber. Optional ext Cargo features `log` and `tracing` gate their facade dependencies. CLI flag/config `--log-presentation plain|rich`/`log_presentation` defaults plain and only selects structured-log presentation; machine envelopes stay unchanged.

- [ ] Add a counting sink returning io::Error on every call. Send one facade record and assert exactly one call. Use a scoped tracing dispatcher; emit i64/bool/string/debug fields and assert Value variants, target/level and field order. Assert constructing either adapter leaves global facade ownership untouched:

```rust
assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
assert!(matches!(recorded_value, Value::Bool(true)));
```

  Test CLI default fixture equality, explicit rich mode and config/flag precedence.
- [ ] Run `env -u NO_COLOR cargo test -p rs-rich-ext --all-features --test adapters`; expect missing adapter features/APIs.
- [ ] Map facade fields directly; tracing Debug-only values become Value::String. Never report a sink failure through log/tracing; drop the failed delivery without recursion. Implement flush with no hidden worker. Add CLI rendering branch only after existing input parsing and before destination rendering.

```rust
// Adapter emission path: deliberately no facade call in the failure branch.
let _ = self.sink.emit(event);
```

- [ ] Run all-feature adapters tests, CLI tests and `cargo check -p rs-rich-ext --no-default-features`; inspect `cargo tree -p rs-rich-ext --no-default-features` for absence of log/tracing additions, then global gates. Commit `feat: add optional event adapters and rich log presentation`.
