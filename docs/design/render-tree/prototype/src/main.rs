//! Measures a cell frame against the segment stream on the benchmark cases.
//!
//! For each case: render once through core (`rich_render` + the print path's
//! crop), then check that the frame encodes to the same ANSI as the merged
//! segment stream and to the same plain text, then time and weigh both
//! representations. Finally, a live-update case compares a cell diff with the
//! row diff `rich_ext::live::LiveCoordinator` does today.

mod frame;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use frame::{Frame, Runs};
use rich::markdown::Markdown;
use rich::protocol::Renderable;
use rich::r#box::SQUARE;
use rich::segment::Segment;
use rich::syntax::Syntax;
use rich::{ColorSystem, Console, Json, Justify, Panel, Style, Table, Text};

/// Counts live heap bytes, so a representation's retained size is the
/// difference before and after building it.
struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(
            new_size as isize - layout.size() as isize,
            Ordering::Relaxed,
        );
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

const WIDTH: usize = 100;
const RUNS: usize = 15;

fn console() -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(WIDTH)
        .build()
}

fn render(console: &Console, renderable: &dyn Renderable) -> Vec<Segment> {
    // What `Console::print` writes, minus the top-level `Text` join.
    let segments = renderable.rich_render(console, &console.options());
    Segment::crop_lines(&segments, console.width())
}

/// The merged stream's ANSI, one line per row: what the frame must match.
fn reference_ansi(console: &Console, segments: &[Segment]) -> String {
    Segment::split_lines(segments)
        .iter()
        .map(|line| console.segments_to_string(&Segment::simplify(line)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn reference_plain(segments: &[Segment]) -> String {
    Segment::split_lines(segments)
        .iter()
        .map(|line| {
            line.iter()
                .filter(|s| !s.control)
                .map(|s| s.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn median_ms(mut f: impl FnMut()) -> f64 {
    for _ in 0..3 {
        f();
    }
    let mut samples: Vec<f64> = (0..RUNS)
        .map(|_| {
            let started = Instant::now();
            f();
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn table(rows: usize, changed: Option<usize>) -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("id");
    table.add_column("name");
    table.add_column("note");
    for index in 0..rows {
        let id = index.to_string();
        let name = if changed == Some(index) {
            // Same text, new style: the layout does not move.
            format!("[bold red]item-{index}")
        } else {
            format!("item-{index}")
        };
        let note = format!("note {} wraps through the table layout engine", index % 17);
        table.add_row(&[&id, &name, &note]);
    }
    table
}

fn repo(path: &str) -> String {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../");
    std::fs::read_to_string(format!("{root}{path}")).expect("a repository file")
}

fn big_json() -> String {
    let items: Vec<String> = (0..400)
        .map(|i| {
            format!(
                r#"{{"id": {i}, "name": "item-{i}", "active": {}, "score": {}, "tags": ["t{}", "u{}"], "meta": {{"depth": {}, "note": null}}}}"#,
                i % 3 == 0,
                i as f64 * 1.5,
                i % 7,
                i % 11,
                i % 5
            )
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn main() {
    let console = console();
    let markup = "[bold red]error[/]: [#ff8800]value[/] ".repeat(128);
    let cases: Vec<(&str, Box<dyn Renderable>)> = vec![
        (
            "text_wrap_justify",
            Box::new(
                Text::styled(
                    "hello world ".repeat(512),
                    Style::parse("bold blue").unwrap(),
                )
                .justify(Justify::Full),
            ),
        ),
        ("table_layout (500 rows)", Box::new(table(500, None))),
        (
            "console_render (panel)",
            Box::new(Panel::new(Box::new(Text::from_markup(&markup).unwrap())).box_set(SQUARE)),
        ),
        (
            "markdown (DIVERGENCES.md)",
            Box::new(Markdown::new(&repo("docs/DIVERGENCES.md"))),
        ),
        (
            "syntax (text.rs)",
            Box::new(Syntax::new(repo("crates/rich/src/text.rs"), "rust")),
        ),
        (
            "json (400 records)",
            Box::new(Json::new(&big_json()).unwrap()),
        ),
        (
            "wide (CJK + emoji)",
            Box::new(Text::styled(
                "日本語のテキスト 🦀 emoji 👨‍👩‍👧 mixed ".repeat(200),
                Style::parse("green").unwrap(),
            )),
        ),
    ];

    println!("width {WIDTH}, truecolor, median of {RUNS} runs (ms); heap bytes retained\n");
    println!(
        "| case | segments | runs | cells | styles | render | → runs | → cells | encode segments | encode runs | encode cells | segments heap | exact runs heap | runs heap | cells heap | ANSI bytes: segments → runs |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for (name, renderable) in &cases {
        let segments = render(&console, renderable.as_ref());
        let frame = Frame::from_segments(&segments);
        assert_eq!(
            frame.to_ansi(Some(ColorSystem::Truecolor)),
            reference_ansi(&console, &segments),
            "{name}: ANSI differs"
        );
        assert_eq!(
            frame.to_plain(),
            reference_plain(&segments),
            "{name}: text differs"
        );
        let runs = Runs::from_segments(&segments);
        let exact = Runs::from_segments_exact(&segments);
        // Unmerged runs are byte-identical to today's direct output.
        let direct: String = Segment::split_lines(&segments)
            .iter()
            .map(|line| console.segments_to_string(line))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            exact.to_ansi(Some(ColorSystem::Truecolor)),
            direct,
            "{name}: exact runs differ"
        );
        assert_eq!(
            runs.to_ansi(Some(ColorSystem::Truecolor)),
            reference_ansi(&console, &segments),
            "{name}: runs ANSI differs"
        );

        let before = live();
        let held = render(&console, renderable.as_ref());
        let segment_heap = live() - before;
        let before = live();
        let built = Frame::from_segments(&held);
        let frame_heap = live() - before;
        drop(built);
        let before = live();
        let built = Runs::from_segments(&held);
        let runs_heap = live() - before;
        drop(built);
        let before = live();
        let built = Runs::from_segments_exact(&held);
        let exact_heap = live() - before;
        drop(built);
        drop(held);

        let render_ms = median_ms(|| {
            black_box(render(&console, renderable.as_ref()));
        });
        let build_ms = median_ms(|| {
            black_box(Frame::from_segments(&segments));
        });
        let encode_segments_ms = median_ms(|| {
            black_box(console.segments_to_string(&segments));
        });
        let encode_frame_ms = median_ms(|| {
            black_box(frame.to_ansi(Some(ColorSystem::Truecolor)));
        });
        let runs_build_ms = median_ms(|| {
            black_box(Runs::from_segments(&segments));
        });
        let encode_runs_ms = median_ms(|| {
            black_box(runs.to_ansi(Some(ColorSystem::Truecolor)));
        });
        println!(
            "| {name} | {} | {} | {} | {} | {render_ms:.2} | {runs_build_ms:.2} | {build_ms:.2} | {encode_segments_ms:.2} | {encode_runs_ms:.2} | {encode_frame_ms:.2} | {} | {} | {} | {} | {} → {} |",
            segments.len(),
            runs.run_count(),
            frame.cell_count(),
            frame.styles.len(),
            kb(segment_heap),
            kb(exact_heap),
            kb(runs_heap),
            kb(frame_heap),
            console.segments_to_string(&segments).len(),
            runs.to_ansi(Some(ColorSystem::Truecolor)).len(),
        );
    }

    // A live update: one table cell changes between two frames.
    let before = render(&console, &table(50, None));
    let after = render(&console, &table(50, Some(20)));
    let (old, new) = (Frame::from_segments(&before), Frame::from_segments(&after));
    let runs = new.diff(&old);
    let cells: usize = runs.iter().map(|(_, a, b)| b - a + 1).sum();
    let cell_bytes = new.repaint_bytes(&runs, Some(ColorSystem::Truecolor));
    // LiveCoordinator repaints each changed row whole, with a cursor move.
    let old_rows: Vec<String> = Segment::split_lines(&before)
        .iter()
        .map(|l| console.segments_to_string(l))
        .collect();
    let new_rows: Vec<String> = Segment::split_lines(&after)
        .iter()
        .map(|l| console.segments_to_string(l))
        .collect();
    let changed: Vec<&String> = new_rows
        .iter()
        .zip(&old_rows)
        .filter(|(a, b)| a != b)
        .map(|(a, _)| a)
        .collect();
    let row_bytes: usize = changed.iter().map(|r| r.len() + 8).sum();
    let full_bytes: usize = new_rows.iter().map(|r| r.len() + 1).sum();
    println!(
        "\nlive update (50-row table, one cell changed): {} changed row(s), {} bytes; \
         {} changed cell run(s), {cells} cells, {cell_bytes} bytes; full repaint {full_bytes} bytes",
        changed.len(),
        row_bytes,
        runs.len(),
    );
    println!(
        "size_of: Segment {} B, Style {} B, Run {} B, Cell {} B",
        std::mem::size_of::<Segment>(),
        std::mem::size_of::<Style>(),
        std::mem::size_of::<frame::Run>(),
        std::mem::size_of::<frame::Cell>()
    );
}

fn kb(bytes: isize) -> String {
    format!("{:.0} KB", bytes as f64 / 1024.0)
}
