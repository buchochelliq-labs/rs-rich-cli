# 5. Progress and live output

## Progress bars

```rust
use rich::{Console, Progress, ProgressColumn};

let mut progress = Progress::new().columns(vec![
    ProgressColumn::Description,
    ProgressColumn::Bar,
    ProgressColumn::Percentage,
]);

progress.add_task("Downloading", 100.0, 60.0);   // description, total, completed
progress.add_task("Extracting", 100.0, 30.0);

console.print(&progress);
```

![Progress](../assets/progress-animated.svg)

`Progress` renders a **snapshot** — it draws the bars at the values you give it.
To animate, update the values and redraw, which is what `Live` is for.

Available columns: `Description`, `Bar`, `Percentage`, and `Text(String, Style)`
for a fixed cell.

## Spinners

```rust
use rich::Spinner;

let spinner = Spinner::new("dots");
let frame = spinner.render(elapsed_seconds);
console.print(&frame);
```

![Spinner](../assets/spinner-animated.svg)

`render` takes a time in seconds and returns the frame for that moment, so the
animation is a pure function of elapsed time — no internal clock, and easy to
test.

## Live displays

`Live` redraws a renderable in place, moving the cursor back over its previous
output rather than scrolling:

```rust
use rich::{Console, Live, Spinner, Text};
use std::time::{Duration, Instant};

let console = Console::new();
let started = Instant::now();
let mut live = Live::new(&console);

for _ in 0..50 {
    let frame = Spinner::new("dots").render(started.elapsed().as_secs_f64());
    live.update(&Text::new("  ").append_text(&frame));
    std::thread::sleep(Duration::from_millis(80));
}
live.finish();
```

!!! warning "One live display at a time"

    `Live` owns the cursor while it runs. Printing to the same console from
    elsewhere — including from another thread — will interleave with its
    redraws and corrupt the display.

## Status

`Status` is a `Live` with a spinner and a message, for the common
"working on it" case:

```rust
use rich::Status;

let status = Status::new("Fetching…");
```

Next: [The CLI →](06-cli.md)
