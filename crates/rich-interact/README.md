# rs-rich-interact

Interactive terminal components for
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli), the Rust port of
Python's `rich`: the layer between printing and a full TUI framework. This
crate is an rs-rich addition, not a port: `rich` has no interactive
components.

A **component** is a state machine: it handles one event and renders one
view. Two drivers run it, and a component behaves the same under both:

- `run(component, &options)` takes the terminal, drives the component to an
  outcome and gives the terminal back, like `Prompt.ask`;
- an `EventLoop` runs several components at once, with ticks and timers, and
  repaints only when something changed.

```rust
use rich_interact::{headless, Component, Context, Event, Flow, KeyCode, Outcome, View};

/// Counts Up presses; Enter returns the count.
struct Counter(u32);

impl Component for Counter {
    type Output = u32;
    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<u32> {
        match event.key().map(|key| key.code) {
            Some(KeyCode::Up) => self.0 += 1,
            Some(KeyCode::Enter) => return Flow::Done(self.0),
            _ => {}
        }
        Flow::Continue
    }
    fn render(&self, context: &Context<'_>) -> View {
        View::new(context.markup(&format!("count: [bold]{}[/]", self.0)))
    }
}

// Tests drive it headless: scripted keys in, frames out.
let (outcome, record) = headless::run(Counter(0), headless::Script::new().keys("up up enter"), 40, 5);
assert_eq!(outcome.unwrap(), Outcome::Done(2));
assert_eq!(record.last_frame(), "count: 2");
```

What else is in the box:

- **Painting cell by cell.** Views are painted through
  `rich_ext::frame::Frame::diff`: only changed cells are written, so an idle
  loop writes nothing.
- **A session that always restores the terminal.** Raw mode, the alternate
  screen, mouse and bracketed paste are undone on every way out: finishing,
  `?`, Ctrl+C and a panic. `Flow::Handoff(command)` gives the terminal to
  `$EDITOR` or a pager and takes it back.
- **A viewport**: a scrollable window over rendered lines, and a minimal pager
  on its own.
- **One item model** (`Item<T>`: label, description, metadata, preview,
  actions, keywords) behind every picker.
- **A degradation policy.** With no terminal on stdin or stdout, under `CI`,
  or with `TERM=dumb`, `run` starts no session. Instead it asks line by line,
  returns the component's default, or fails, as the caller chooses. Nothing
  blocks on a pipe.

Linux, macOS and Windows are supported through `crossterm`. The PTY tests run
on Unix.

Licensed under MIT.
