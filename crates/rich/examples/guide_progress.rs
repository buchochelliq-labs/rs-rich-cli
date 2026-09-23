//! Guide: Progress and live displays — run: cargo run -p rs-rich --example guide_progress [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/progress-and-live.md are cut from this file.
//! Screenshots are static frames driven by a fixed clock; without `--svg` the
//! program also runs each live display for a moment in the terminal.

#[path = "guide_support/mod.rs"]
mod guide_support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use guide_support::Shots;

// --8<-- [start:imports]
use rich::progress::TaskUpdate;
use rich::{
    BarColumn, ColumnOptions, Console, Justify, Live, Progress, ProgressBar, ProgressColumn,
    Spinner, SpinnerColumn, Status, Text, TextColumn, TimeRemainingColumn,
};
// --8<-- [end:imports]

fn main() {
    let shots = Shots::from_args("guide_progress");
    shots.shot("default", 72, default_columns);
    shots.shot("columns", 90, all_columns);
    shots.shot("custom", 72, custom_columns);
    shots.shot("bars", 60, bars);
    shots.shot("spinners", 60, spinners);
    shots.shot("status", 60, status);

    if !shots.is_svg() {
        live_progress();
        track_iterator();
        live_display();
        auto_live();
    }
}

// --8<-- [start:clock]
/// A clock you control. Time-based columns read it, so frames are reproducible.
fn fixed_clock() -> (Arc<Mutex<f64>>, impl Fn() -> f64 + Send + Sync + 'static) {
    let now = Arc::new(Mutex::new(0.0_f64));
    let reader = now.clone();
    (now, move || *reader.lock().unwrap())
}
// --8<-- [end:clock]

// --8<-- [start:default]
fn default_columns(console: &Console) {
    let (now, clock) = fixed_clock();
    // Description, bar, percentage and time remaining — upstream's defaults.
    let mut progress = Progress::new().clock(clock);

    let download = progress.add_task("Downloading", 100.0, 0.0);
    let extract = progress.add_task("Extracting", 100.0, 0.0);
    let verify = progress.add_task("Verifying", None, 0.0); // indeterminate

    *now.lock().unwrap() = 5.0;
    progress.advance(download, 30.0);
    *now.lock().unwrap() = 10.0;
    progress.advance(download, 34.0);
    progress.update(extract, TaskUpdate::default().completed(100.0));
    progress.advance(verify, 1.0);

    console.print(&progress);
}
// --8<-- [end:default]

// --8<-- [start:columns]
fn all_columns(console: &Console) {
    let (now, clock) = fixed_clock();
    let mut progress = Progress::new().clock(clock).columns(vec![
        ProgressColumn::spinner(),
        ProgressColumn::Description,
        ProgressColumn::Bar,
        ProgressColumn::Download,
        ProgressColumn::TransferSpeed,
        ProgressColumn::TimeElapsed,
        ProgressColumn::time_remaining(),
    ]);
    let iso = progress.add_task("ubuntu.iso", 4_700_000_000.0, 0.0);
    let deb = progress.add_task("rich.deb", 18_000_000.0, 0.0);

    *now.lock().unwrap() = 4.0;
    progress.advance(iso, 400_000_000.0);
    progress.advance(deb, 18_000_000.0);
    *now.lock().unwrap() = 8.0;
    progress.advance(iso, 500_000_000.0);

    console.print(&progress);
}
// --8<-- [end:columns]

// --8<-- [start:custom]
fn custom_columns(console: &Console) {
    let (now, clock) = fixed_clock();
    let mut progress = Progress::new().clock(clock).columns(vec![
        // Format strings see the task, like upstream's "{task.description}".
        ProgressColumn::TextFormat(TextColumn::new("[bold blue]{task.fields[stage]}")),
        ProgressColumn::Description,
        ProgressColumn::BarWith(
            BarColumn::new()
                .bar_width(Some(20))
                .complete_style("magenta")
                .finished_style("bold green"),
        ),
        ProgressColumn::MofN,
        ProgressColumn::TextFormat(
            TextColumn::new("{task.percentage:>3.0f}%").justify(Justify::Right),
        ),
        ProgressColumn::TimeRemaining(TimeRemainingColumn::new(true, true)),
        ProgressColumn::Spinner(SpinnerColumn::new("line", "✓")).with_table_column(ColumnOptions {
            width: Some(2),
            ..ColumnOptions::default()
        }),
    ]);

    let fields = |stage: &str| [("stage", stage.to_string())];
    let build = progress.add_task_with("compile", 120.0, 0.0, true, fields("1/2"));
    let test = progress.add_task_with("test", 40.0, 0.0, true, fields("2/2"));

    *now.lock().unwrap() = 30.0;
    progress.advance(build, 120.0);
    progress.update(test, TaskUpdate::default().advance(9.0));

    console.print(&progress);
}
// --8<-- [end:custom]

// --8<-- [start:bars]
fn bars(console: &Console) {
    // The bar on its own, as a renderable.
    console.print(&ProgressBar::new(100.0, 25.0).width(40));
    console.print(
        &ProgressBar::new(100.0, 70.0)
            .width(40)
            .complete_style("yellow"),
    );
    console.print(&ProgressBar::new(100.0, 100.0).width(40));
    // No total: a pulsing bar. `animation_time` picks the frame.
    console.print(&ProgressBar::indeterminate().width(40).animation_time(0.4));
}
// --8<-- [end:bars]

// --8<-- [start:spinners]
fn spinners(console: &Console) {
    for name in [
        "dots",
        "line",
        "arc",
        "bouncingBar",
        "moon",
        "earth",
        "clock",
    ] {
        let spinner = Spinner::new(name).style("green");
        spinner.render(0.0); // the first render starts the animation clock
                             // Six frames, 0.1 s apart, then the name.
        let mut row = Text::new("");
        for step in 0..6 {
            row = row.append_text(&spinner.render(step as f64 * 0.1));
            row.append("  ", None);
        }
        row.append(name, Some("dim".into()));
        console.print(&row);
    }
}
// --8<-- [end:spinners]

// --8<-- [start:status]
fn status(console: &Console) {
    let mut status = Status::new("Fetching [bold]index[/]…").spinner("dots");
    console.print(&status.renderable().render(0.0));

    // Change the message, the spinner, its style or speed.
    status.update(
        Some("Unpacking…"),
        Some("line"),
        Some("yellow".into()),
        None,
    );
    console.print(&status.renderable().render(0.0));
}
// --8<-- [end:status]

// --8<-- [start:live_progress]
fn live_progress() {
    let progress = Progress::new().transient(false).expand(false);
    // `start` moves the progress onto a refresh thread: 10 redraws a second.
    let live = progress.start(Console::new(), std::io::stdout(), 10.0);

    let task = live.add_task("Working", 50.0, 0.0);
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(20));
        live.advance(task, 1.0);
    }
    // Draw the final frame, join the thread, get the Progress back.
    let (progress, _stdout) = live.stop();
    assert!(progress.finished());
}
// --8<-- [end:live_progress]

// --8<-- [start:track]
fn track_iterator() {
    // The one-liner: a live bar on stdout for any iterator.
    let mut total = 0;
    for n in rich::track(0..40, "Summing") {
        std::thread::sleep(Duration::from_millis(10));
        total += n;
    }
    assert_eq!(total, 780);

    // The same through a running Progress, sharing its display with other tasks.
    let live = Progress::new().start(Console::new(), std::io::stdout(), 10.0);
    for _item in live.track(vec!["a", "b", "c"], None, "Items") {
        std::thread::sleep(Duration::from_millis(50));
    }
    live.stop();
}
// --8<-- [end:track]

// --8<-- [start:live]
fn live_display() {
    // Live redraws a renderable in place. Manual mode: you call update/refresh.
    let spinner = Spinner::new("dots").text("counting…");
    let mut live = Live::new(Box::new(Text::new("")), Console::new(), std::io::stdout());
    live.start();
    let started = Instant::now();
    for n in 0..20 {
        let frame = spinner.render(started.elapsed().as_secs_f64());
        live.update(Box::new(frame.append_text(&Text::new(format!(" {n}")))));
        std::thread::sleep(Duration::from_millis(50));
    }
    live.stop(); // leaves the last frame on screen
}
// --8<-- [end:live]

// --8<-- [start:auto_live]
fn auto_live() {
    // Auto-refresh mode: a background thread redraws; you send new renderables.
    let live = Live::spawn(
        Box::new(Text::new("starting")),
        Console::new(),
        std::io::stdout(),
        20.0,
    );
    let status = Status::new("Downloading…");
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(800) {
        // Spinners animate by rendering at the current time.
        let frame = status.renderable().render(started.elapsed().as_secs_f64());
        live.update(Box::new(frame));
        std::thread::sleep(Duration::from_millis(50));
    }
    live.update(Box::new(Text::styled("✓ downloaded", "green")));
    let _stdout = live.stop();
}
// --8<-- [end:auto_live]
