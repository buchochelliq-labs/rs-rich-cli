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

`Progress::new()` uses upstream's default columns: description, bar, percentage
and time remaining.

### Time, rate and spinner columns

`add_task` returns a `TaskId`; `advance` and `update` move a task along and record
speed samples, exactly as upstream's `Progress` does. Time-based columns read the
progress clock, which you can replace — handy in tests, where a fixed clock makes
every frame reproducible:

```rust
use rich::{Progress, ProgressColumn, TaskUpdate};
use std::sync::{Arc, Mutex};

let now = Arc::new(Mutex::new(0.0_f64));
let clock = now.clone();
let mut progress = Progress::new()
    .clock(move || *clock.lock().unwrap())
    .columns(vec![
        ProgressColumn::spinner(),
        ProgressColumn::Description,
        ProgressColumn::Bar,
        ProgressColumn::Download,
        ProgressColumn::TransferSpeed,
        ProgressColumn::TimeElapsed,
        ProgressColumn::time_remaining(),
    ]);
let iso = progress.add_task("ubuntu.iso", 4_700_000_000.0, 0.0);
*now.lock().unwrap() = 4.0;
progress.advance(iso, 400_000_000.0);
*now.lock().unwrap() = 8.0;
progress.update(iso, TaskUpdate::default().advance(500_000_000.0));
console.print(&progress);   // ⠋ ubuntu.iso ━━━━╸━━━ 0.9/4.7 GB 125.0 MB/s 0:00:08 0:00:31
```

Columns: `Description`, `Bar`, `Percentage`, `TaskProgress { show_speed }`,
`MofN`, `Download`, `BinaryDownload`, `TimeElapsed`, `TimeRemaining`
(`ProgressColumn::time_remaining()` or `TimeRemainingColumn::new(compact,
elapsed_when_finished)`), `TransferSpeed`, `FileSize`, `TotalFileSize`,
`Spinner` (`ProgressColumn::spinner()` or `SpinnerColumn::new(name,
finished_text)`) and `Text(String, Style)` for a fixed cell. A task with a total
of `None` is indeterminate; `add_unstarted_task` adds one whose clock has not
started.

## Spinners

```rust
use rich::Spinner;

let spinner = Spinner::new("dots");
let frame = spinner.render(elapsed_seconds);
console.print(&frame);
```

![Spinner](../assets/spinner-animated.svg)

`render` takes a time in seconds and returns the frame for that moment. As in
upstream rich, the first `render` marks the start of the animation, so keep one
spinner for the whole animation rather than building a new one per frame (a new
spinner always starts on its first frame). `spinner.update(text, style, speed)`
changes the text, style or speed mid-animation; a new speed continues from the
current frame. `Spinner::text` is console markup.

## Live displays

`Live` redraws a renderable in place, moving the cursor back over its previous
output rather than scrolling:

```rust
use rich::{Console, Live, Spinner, Text};
use std::time::{Duration, Instant};

let console = Console::new();
let started = Instant::now();
let mut live = Live::new(&console);
let spinner = Spinner::new("dots");

for _ in 0..50 {
    let frame = spinner.render(started.elapsed().as_secs_f64());
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

let mut status = Status::new("Fetching [b]index[/]…");
let frame = status.renderable().render(elapsed_seconds);
status.update(Some("Unpacking…"), None, None, None);
```

`Status::update` follows upstream: a new spinner name replaces the spinner and
restarts its animation, while a new message, style or speed updates it in
place.

Next: [The CLI →](06-cli.md)
