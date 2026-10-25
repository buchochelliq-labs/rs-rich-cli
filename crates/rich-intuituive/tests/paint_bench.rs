//! The paint benchmark (docs/design/render-performance.md, "Benchmarks"):
//! scenes from the smallest change to every cell changing, at 80x24 and
//! 200x60, under each paint strategy. Each reports time and bytes a frame,
//! and the painter's counts: cells compared, written and elided, and bytes
//! of moves, styles and text.
//!
//!     cargo test --release -p rs-rich-intuituive --test paint_bench -- --ignored --nocapture
//!
//! The benchmark is ignored by default: timings mean something only in
//! release builds. What runs by default is the check every number rests
//! on: each scene's bytes, under each strategy, replayed through vt100,
//! reproduce the screen a full repaint draws, cell for cell, so a strategy
//! can never be faster by being wrong.

mod bench;

/// Counts allocations, for the allocations a frame takes.
#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::system();

/// The counts are the process's: the tests here take turns, so none
/// allocates while another counts.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

use std::time::Duration;

use intuituive::interact::headless::Script;
use intuituive::prelude::*;
use intuituive::screen::{PaintStats, Painter, Screen};
use intuituive::widget::{Canvas, DrawCx};
use intuituive::PaintStrategy;
use rich_ext::qa::profile::{AllocStats, CountingAllocator};
use rich_interact::{Event, Key};

/// A scene: an app, and the key that moves it on a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scene {
    /// One cell blinks: the smallest change, and the cost of choosing.
    Blink,
    /// A character typed into a field: a short run, and the caret.
    Typing,
    /// A clock at the top and a status line at the bottom, far apart.
    StatusClock,
    /// A line appended to a full log pane: the scroll case.
    LogAppend,
    /// A long list scrolled a page: every row changes.
    ListPage,
    /// A long list scrolled one row: every row moves by one.
    ListRow,
    /// A table re-sorted: every row changes, its styles stay.
    TableSort,
    /// Every cell in a new colour.
    Churn,
    /// A braille chart redrawn.
    Chart,
    /// Text of many scripts redrawn: combining accents, wide CJK and
    /// Hangul, Greek. What takes the grapheme-splitting path.
    Unicode,
}

const SCENES: [Scene; 10] = [
    Scene::Blink,
    Scene::Typing,
    Scene::StatusClock,
    Scene::LogAppend,
    Scene::ListPage,
    Scene::ListRow,
    Scene::TableSort,
    Scene::Churn,
    Scene::Chart,
    Scene::Unicode,
];

const STRATEGIES: [PaintStrategy; 5] = [
    PaintStrategy::Cells,
    PaintStrategy::Rows,
    PaintStrategy::Full,
    PaintStrategy::Scroll,
    PaintStrategy::Best,
];

/// Prose to fill a screen with, a row at a time.
fn prose(row: usize) -> String {
    const WORDS: [&str; 12] = [
        "the",
        "quick",
        "[b]brown[/b]",
        "fox",
        "jumps",
        "over",
        "[cyan]lazy[/]",
        "dogs",
        "while",
        "[i]seven[/i]",
        "wizards",
        "hex",
    ];
    (0..40)
        .map(|i| WORDS[(row * 7 + i * 5) % WORDS.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

/// A long list's row `i`.
fn item(i: usize) -> String {
    let colour = ["green", "yellow", "red", "blue"][i % 4];
    format!(
        "[{colour}]●[/] [b]item {i:>5}[/b]  [dim]{:08x}[/dim]  {}",
        i.wrapping_mul(2_654_435_761) as u32,
        prose(i)
    )
}

/// The scenes' apps.
fn app(scene: Scene, rows: u16) -> App {
    match scene {
        Scene::Blink => App::new(|| {
            let on = signal(false);
            column([
                label((0..60).map(prose).collect::<Vec<_>>().join("\n")).flex(1),
                text(move || if on.get() { "[reverse]█[/]" } else { " " }.to_string()).fixed(1),
            ])
            .on_key("t", move |_| on.update(|on| *on = !*on))
        }),
        Scene::Typing => App::new(|| {
            let name = signal(String::new());
            column([
                label((0..60).map(prose).collect::<Vec<_>>().join("\n")).flex(1),
                input(name).fixed(1),
            ])
            .on_key("enter", move |_| name.set(String::new()))
        }),
        Scene::StatusClock => App::new(|| {
            let tick = signal(0u64);
            column([
                text(move || {
                    format!(
                        "[b]clock[/b] 12:{:02}:{:02}",
                        tick.get() / 60 % 60,
                        tick.get() % 60
                    )
                })
                .fixed(1),
                label((0..60).map(prose).collect::<Vec<_>>().join("\n")).flex(1),
                text(move || format!("[dim]status: {} jobs done[/dim]", tick.get())).fixed(1),
            ])
            .on_key("t", move |_| tick.update(|t| *t += 1))
        }),
        Scene::LogAppend => App::new(|| {
            let log = Log::new(5000);
            let n = signal(0usize);
            (0..200).for_each(|i| log.push(item(i)));
            log.view().on_key("t", move |_| {
                n.update(|n| *n += 1);
                log.push(item(200 + n.get_untracked()));
            })
        }),
        Scene::ListPage | Scene::ListRow => App::new(move || {
            let top = signal(0usize);
            let page = rows as usize;
            intuituive::widget(Rows {
                top,
                lines: Vec::new(),
            })
            .on_key("down", move |_| top.update(|t| *t += 1))
            .on_key("pagedown", move |_| top.update(|t| *t += page))
        }),
        Scene::TableSort => App::new(move || {
            let up = signal(true);
            let rows = rows as usize;
            text(move || {
                let mut order: Vec<usize> = (0..rows).collect();
                if !up.get() {
                    order.reverse();
                }
                order
                    .into_iter()
                    .map(|i| {
                        format!(
                            "[b cyan]{:<16}[/] [green]{:>8}[/] [yellow]{:>10.2}[/] {}",
                            format!("host-{i:03}"),
                            i * 37 % 1000,
                            i as f64 * 1.5,
                            prose(i % 7)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .on_key("t", move |_| up.update(|up| *up = !*up))
        }),
        Scene::Unicode => App::new(move || {
            let tick = signal(0usize);
            let rows = rows as usize;
            text(move || {
                // No emoji with selectors or joiners: their width is one
                // terminals (and vt100) disagree on with rich's table.
                const WORDS: [&str; 8] = [
                    "cafe\u{301}",
                    "[b]日本語[/b]",
                    "Ελληνικά",
                    "[cyan]naïve[/]",
                    "e\u{301}te\u{301}",
                    "한국어",
                    "[i]Zürich[/i]",
                    "ä\u{308}",
                ];
                let t = tick.get();
                (0..rows)
                    .map(|row| {
                        (0..12)
                            .map(|i| WORDS[(row * 3 + i + t) % WORDS.len()])
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .on_key("t", move |_| tick.update(|t| *t += 1))
        }),
        Scene::Churn => bench::churn_app(),
        Scene::Chart => bench::chart_app(),
    }
}

/// A long list as a widget that draws the rows in view, from `top`,
/// wrapping round at the end: what a virtual list draws, without a list's
/// own work, so the scene measures the paint.
struct Rows {
    top: Signal<usize>,
    lines: Vec<Vec<rich::Segment>>,
}

impl intuituive::Widget for Rows {
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let top = self.top.get();
        if self.lines.is_empty() {
            let console = cx.console();
            let options = console.options().update_width(canvas.width() as usize);
            self.lines = (0..1000)
                .map(|i| {
                    let text = rich::Text::from_markup(&item(i)).unwrap();
                    let mut lines = console.render_lines(&text, &options, false);
                    lines.truncate(1);
                    lines.pop().unwrap_or_default()
                })
                .collect();
        }
        let len = self.lines.len();
        let shown: Vec<Vec<rich::Segment>> = (0..canvas.height() as usize)
            .map(|row| self.lines[(top + row) % len].clone())
            .collect();
        canvas.lines(&shown);
    }
}

impl Scene {
    fn key(self) -> &'static str {
        match self {
            Scene::Typing => "a",
            Scene::ListPage => "pagedown",
            Scene::ListRow => "down",
            _ => "t",
        }
    }
}

/// One scene, measured.
#[derive(Clone, Copy, Debug, Default)]
struct Run {
    micros: f64,
    /// Of which the widgets' draw, and the painter's encode.
    draw: f64,
    paint: f64,
    bytes: f64,
    painted: PaintStats,
    frames: usize,
}

/// `scene` at `width` x `height` under `strategy`, `frames` frames.
fn measure(scene: Scene, strategy: PaintStrategy, width: u16, height: u16, frames: usize) -> Run {
    let mut script = Script::new();
    for _ in 0..frames {
        script = script.keys(scene.key());
    }
    let demo = app(scene, height)
        .paint_strategy(strategy)
        .timedemo(script, width, height);
    // The first frame draws everything: it is not measured.
    let skip = demo.frames.len().saturating_sub(frames);
    let measured = &demo.frames[skip..];
    let n = measured.len().max(1) as f64;
    let mut painted = PaintStats::default();
    for frame in measured {
        painted.add(&frame.painted);
    }
    Run {
        micros: measured.iter().map(|f| f.time.as_secs_f64()).sum::<f64>() * 1e6 / n,
        draw: measured.iter().map(|f| f.draw.as_secs_f64()).sum::<f64>() * 1e6 / n,
        paint: measured.iter().map(|f| f.paint.as_secs_f64()).sum::<f64>() * 1e6 / n,
        bytes: measured.iter().map(|f| f.bytes).sum::<usize>() as f64 / n,
        painted,
        frames: measured.len(),
    }
}

/// Allocations a frame of `scene` takes under `strategy`, in a steady
/// state (after a few frames), and its bytes: both exact, so a gate on
/// them does not depend on the machine.
fn allocations(scene: Scene, strategy: PaintStrategy, width: u16, height: u16) -> (f64, f64) {
    const FRAMES: usize = 20;
    let key = Event::Key(Key::parse(scene.key()).unwrap());
    let mut driver = app(scene, height)
        .paint_strategy(strategy)
        .driver(width, height);
    let step = |driver: &mut intuituive::Driver| {
        driver.event(key.clone());
        driver.update(Duration::ZERO);
        driver.render().map_or(0, |out| out.len())
    };
    driver.update(Duration::ZERO);
    driver.render();
    for _ in 0..3 {
        step(&mut driver);
    }
    let before = AllocStats::now().expect("counting");
    let bytes: usize = (0..FRAMES).map(|_| step(&mut driver)).sum();
    let made = AllocStats::now().expect("counting").since(&before);
    (
        made.allocations as f64 / FRAMES as f64,
        bytes as f64 / FRAMES as f64,
    )
}

/// The checked-in numbers the regression gate holds `best` to: bytes and
/// allocations a frame, per size and scene.
const BASELINE: &str = include_str!("paint_bench_baseline.txt");

/// The regression gate: `best`'s bytes and allocations a frame on every
/// scene, against the checked-in baseline, 10% over failing (so a
/// regression of 20% always does). Exact counts, so it holds on any
/// machine. `PAINT_BENCH_RECORD=1` prints a new baseline instead.
#[test]
fn bytes_and_allocations_hold_to_the_baseline() {
    let _serial = serial();
    let record = std::env::var_os("PAINT_BENCH_RECORD").is_some();
    let mut failures = Vec::new();
    for (w, h) in [(80u16, 24u16), (200, 60)] {
        for scene in SCENES {
            let (allocs, bytes) = allocations(scene, PaintStrategy::Best, w, h);
            if record {
                println!("{w}x{h} {scene:?} {bytes:.0} {allocs:.0}");
                continue;
            }
            let key = format!("{w}x{h} {scene:?} ");
            let Some(line) = BASELINE.lines().find(|l| l.starts_with(&key)) else {
                failures.push(format!("{key}: not in the baseline"));
                continue;
            };
            let mut numbers = line[key.len()..]
                .split_whitespace()
                .map(|n| n.parse::<f64>().unwrap());
            let (want_bytes, want_allocs) = (numbers.next().unwrap(), numbers.next().unwrap());
            if bytes > want_bytes * 1.1 + 2.0 {
                failures.push(format!(
                    "{key}: {bytes:.0} bytes a frame, the baseline {want_bytes}"
                ));
            }
            if allocs > want_allocs * 1.1 + 2.0 {
                failures.push(format!(
                    "{key}: {allocs:.0} allocations a frame, the baseline {want_allocs}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// How many runs of each strategy a scene takes, the fastest kept.
const ROUNDS: usize = 5;

/// Each of `strategies` on `scene`, in rounds that take each in turn, the
/// fastest of each kept: a machine that slows for a while slows them all.
fn measure_all(
    scene: Scene,
    strategies: &[PaintStrategy],
    width: u16,
    height: u16,
    frames: usize,
) -> Vec<Run> {
    let mut runs: Vec<Option<Run>> = vec![None; strategies.len()];
    for _ in 0..ROUNDS {
        for (i, &strategy) in strategies.iter().enumerate() {
            let run = measure(scene, strategy, width, height, frames);
            if runs[i].is_none_or(|r| run.micros < r.micros) {
                runs[i] = Some(run);
            }
        }
    }
    runs.into_iter().map(Option::unwrap).collect()
}

/// Where `best` did worse than the best fixed strategy: more than 5% in
/// bytes, or in time (with a floor of 10 µs for timer noise on frames of
/// a few microseconds).
fn judge(strategies: &[PaintStrategy], runs: &[Run]) -> Vec<String> {
    let Some(at) = strategies.iter().position(|&s| s == PaintStrategy::Best) else {
        return Vec::new();
    };
    let best = runs[at];
    let fixed: Vec<Run> = (runs.iter().enumerate())
        .filter(|&(i, _)| i != at)
        .map(|(_, r)| *r)
        .collect();
    if fixed.is_empty() {
        return Vec::new();
    }
    let least_bytes = fixed.iter().map(|r| r.bytes).fold(f64::MAX, f64::min);
    let least_time = fixed.iter().map(|r| r.micros).fold(f64::MAX, f64::min);
    let mut wrong = Vec::new();
    if best.bytes > least_bytes * 1.05 + 1.0 {
        wrong.push(format!(
            "best sent {:.0} B, the best fixed {least_bytes:.0} B",
            best.bytes
        ));
    }
    if best.micros > least_time * 1.05 + 10.0 {
        wrong.push(format!(
            "best took {:.1} µs, the best fixed {least_time:.1} µs",
            best.micros
        ));
    }
    wrong
}

/// What `bytes` leave a `width` x `height` vt100 terminal showing.
fn replay(bytes: &str, width: u16, height: u16) -> vt100::Parser {
    let mut parser = vt100::Parser::new(height, width, 0);
    parser.process(bytes.as_bytes());
    parser
}

/// Where two terminals differ, cell by cell: text, colours and attributes.
fn differences(a: &vt100::Screen, b: &vt100::Screen, width: u16, height: u16) -> Vec<String> {
    let mut found = Vec::new();
    for row in 0..height {
        for col in 0..width {
            let (Some(x), Some(y)) = (a.cell(row, col), b.cell(row, col)) else {
                continue;
            };
            let text = |c: &vt100::Cell| match c.contents() {
                "" => " ".to_string(),
                s => s.to_string(),
            };
            let look = |c: &vt100::Cell| {
                (
                    text(c),
                    c.fgcolor(),
                    c.bgcolor(),
                    c.bold(),
                    c.dim(),
                    c.italic(),
                    c.underline(),
                    c.inverse(),
                )
            };
            if look(x) != look(y) {
                found.push(format!("{col},{row}: {:?} against {:?}", look(x), look(y)));
            }
        }
    }
    found
}

/// What a terminal shows after a full paint of `screen`.
fn reference(screen: &Screen) -> vt100::Parser {
    let area = screen.area();
    let out = Painter::new(Some(rich::ColorSystem::Truecolor)).paint(screen, &[]);
    replay(&out, area.width, area.height)
}

/// Run `scene` under `strategy` for `frames` frames, replaying every
/// frame's bytes through vt100 and comparing with a full paint after each.
fn check(scene: Scene, strategy: PaintStrategy, width: u16, height: u16, frames: usize) {
    let mut driver = app(scene, height)
        .paint_strategy(strategy)
        .driver(width, height);
    let mut terminal = vt100::Parser::new(height, width, 0);
    let keys = std::iter::repeat_n(scene.key(), frames);
    for (n, key) in std::iter::once(None).chain(keys.map(Some)).enumerate() {
        if let Some(key) = key {
            driver.event(Event::Key(Key::parse(key).unwrap()));
        }
        driver.update(Duration::ZERO);
        if let Some(out) = driver.render() {
            terminal.process(out.as_bytes());
        }
        let wanted = reference(driver.screen());
        let wrong = differences(terminal.screen(), wanted.screen(), width, height);
        assert!(
            wrong.is_empty(),
            "{scene:?} under {strategy:?} at {width}x{height}, frame {n}: {} cells differ, \
             first {:?}",
            wrong.len(),
            &wrong[..wrong.len().min(5)]
        );
    }
}

#[test]
fn every_strategy_reproduces_every_scene() {
    let _serial = serial();
    for scene in SCENES {
        for strategy in STRATEGIES {
            check(scene, strategy, 80, 24, 3);
        }
    }
}

#[test]
fn the_scenes_change_what_they_say() {
    let _serial = serial();
    for scene in SCENES {
        let mut driver = app(scene, 24).driver(80, 24);
        driver.settle();
        let before = driver.screen().lines();
        driver.press(scene.key());
        assert_ne!(driver.screen().lines(), before, "{scene:?}");
    }
}

/// The table the design note keeps: per scene and size, each strategy's
/// time and bytes a frame; then the counts for each scene under `Cells`.
#[test]
#[ignore = "a benchmark: run it in release with --ignored"]
fn paint_strategies_benchmark() {
    let _serial = serial();
    let frames = std::env::var("PAINT_BENCH_FRAMES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(200);
    // `PAINT_BENCH_STRATEGIES=cells,best` measures only those.
    let strategies: Vec<PaintStrategy> = match std::env::var("PAINT_BENCH_STRATEGIES") {
        Ok(names) => names
            .split(',')
            .map(|n| n.trim().parse().unwrap())
            .collect(),
        Err(_) => STRATEGIES.to_vec(),
    };
    for (w, h) in [(80, 24), (200, 60)] {
        for &strategy in &strategies {
            for scene in SCENES.into_iter().filter(|s| {
                let only = std::env::var("PAINT_BENCH_SCENE").ok();
                only.is_none_or(|o| format!("{s:?}").eq_ignore_ascii_case(&o))
            }) {
                check(scene, strategy, w, h, 3);
            }
        }
    }
    let mut failures = Vec::new();
    let mut counts = Vec::new();
    print!("| Size | Scene |");
    for strategy in &strategies {
        print!(" {strategy:?} µs | {strategy:?} B |");
    }
    println!();
    println!("|---|---|{}", "---:|---:|".repeat(strategies.len()));
    // `PAINT_BENCH_SCENE=Churn` measures only that scene.
    let only = std::env::var("PAINT_BENCH_SCENE").ok();
    let scenes: Vec<Scene> = SCENES
        .into_iter()
        .filter(|s| {
            only.as_ref()
                .is_none_or(|o| format!("{s:?}").eq_ignore_ascii_case(o))
        })
        .collect();
    for (w, h) in [(80, 24), (200, 60)] {
        for &scene in &scenes {
            let frames = match scene {
                Scene::Churn | Scene::Chart | Scene::TableSort | Scene::Unicode => frames / 4,
                // The field holds a screen's width of typing.
                Scene::Typing => frames.min(60),
                _ => frames,
            };
            let mut runs = measure_all(scene, &strategies, w, h, frames);
            let mut wrong = judge(&strategies, &runs);
            if !wrong.is_empty() {
                // A timing that lost to a busy moment gets a second look.
                runs = measure_all(scene, &strategies, w, h, frames);
                wrong = judge(&strategies, &runs);
            }
            print!("| {w}x{h} | {scene:?} |");
            for run in &runs {
                print!(" {:.1} | {:.0} |", run.micros, run.bytes);
            }
            println!();
            counts.push((w, h, scene, runs[0]));
            failures.extend(
                wrong
                    .into_iter()
                    .map(|why| format!("{w}x{h} {scene:?}: {why}")),
            );
        }
    }
    println!();
    println!(
        "| Size | Scene | frame µs | draw µs | paint µs | compared | written | elided | moves B \
         | styles B | text B | other B |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (w, h, scene, run) in counts {
        let n = run.frames.max(1);
        let p = run.painted;
        println!(
            "| {w}x{h} | {scene:?} | {:.1} | {:.1} | {:.1} | {} | {} | {} | {} | {} | {} | {} |",
            run.micros,
            run.draw,
            run.paint,
            p.compared / n,
            p.written / n,
            p.elided / n,
            p.move_bytes / n,
            p.style_bytes / n,
            p.text_bytes / n,
            p.other_bytes / n,
        );
    }
    if std::env::var_os("PAINT_BENCH_GATE").is_some() {
        assert!(failures.is_empty(), "{failures:#?}");
    } else if !failures.is_empty() {
        println!("\nNot gated (set PAINT_BENCH_GATE): {failures:#?}");
    }
}

/// One scene under one strategy for a profiler to watch:
/// `PAINT_BENCH_SCENE=Churn PAINT_BENCH_STRATEGIES=cells valgrind
/// --tool=callgrind <the test binary> --ignored profile_one_scene`.
#[test]
#[ignore = "a profiling harness: run it under a profiler"]
fn profile_one_scene() {
    let _serial = serial();
    let name = std::env::var("PAINT_BENCH_SCENE").unwrap_or_else(|_| "Churn".into());
    let scene = *SCENES
        .iter()
        .find(|s| format!("{s:?}").eq_ignore_ascii_case(&name))
        .expect("a scene");
    let strategy = std::env::var("PAINT_BENCH_STRATEGIES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_default();
    let frames = std::env::var("PAINT_BENCH_FRAMES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(50);
    // `PAINT_BENCH_SIZE=200x60` profiles the larger screen.
    let (w, h) = std::env::var("PAINT_BENCH_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((80, 24));
    let run = measure(scene, strategy, w, h, frames);
    println!("{scene:?} {strategy:?} {w}x{h}: {run:?}");
}
