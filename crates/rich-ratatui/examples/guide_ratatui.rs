//! Guide: Using rich with ratatui — run: cargo run -p rs-rich-ratatui --example guide_ratatui --features interact [-- --svg docs/media/guide]
//!
//! The snippets and pictures on `docs/guide/ratatui.md` come from this file.
//! Each frame is drawn on ratatui's `TestBackend`, read back with
//! `buffer_to_lines` and printed through a rich console, so a picture is the
//! ratatui buffer cell for cell, colours included.

use std::path::PathBuf;

use ratatui_core::backend::TestBackend;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::{Constraint, Layout, Rect};
use ratatui_core::style::{Color, Style};
use ratatui_core::terminal::Terminal;
use ratatui_core::widgets::Widget;
use ratatui_widgets::block::Block;
use ratatui_widgets::paragraph::Paragraph;
use rich::markdown::Markdown;
use rich::{ColorSystem, Console, ConsoleOptions, Renderable, Segment, Table};
use rich_interact::headless::{self, Script, Step};
use rich_interact::{Component, Context, Event, Flow, KeyCode, Outcome};
use rich_ratatui::{buffer_to_lines, RatatuiComponent, RichWidget};

// --8<-- [start:widget]
fn widget() -> Buffer {
    let mut table = Table::new();
    table.add_column("service");
    table.add_column("status");
    table.add_row(&["api", "[green]ok[/]"]);
    table.add_row(&["worker", "[bold red]failed[/]"]);

    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal
        .draw(|frame| {
            // A borrowed renderable: the table outlives the frame.
            frame.render_widget(RichWidget::new(&table), Rect::new(0, 0, 40, 6));
            // An owned one: markup, drawn in the last row.
            frame.render_widget(RichWidget::markup("[b]q[/] quit"), Rect::new(0, 7, 40, 1));
        })
        .unwrap();
    terminal.backend().buffer().clone()
}
// --8<-- [end:widget]

// --8<-- [start:side-by-side]
const NOTES: &str = "\
# Release notes

- **Tables**, panels and trees draw as *ratatui widgets*.
- Markdown, too: `RichWidget::new(&markdown)`.

> Styles patch the cells beneath them.
";

fn side_by_side() -> Buffer {
    let mut table = Table::new().title("Services");
    table.add_column("service");
    table.add_column("p99");
    table.add_column("status");
    table.add_row(&["api", "35 ms", "[green]ok[/]"]);
    table.add_row(&["worker", "120 ms", "[yellow]slow[/]"]);
    table.add_row(&["billing", "-", "[bold red]down[/]"]);
    let markdown = Markdown::new(NOTES);

    let mut terminal = Terminal::new(TestBackend::new(76, 8)).unwrap();
    terminal
        .draw(|frame| {
            let [left, right] = Layout::horizontal([Constraint::Length(34), Constraint::Fill(1)])
                .areas(frame.area());
            frame.render_widget(RichWidget::new(&table), left);
            frame.render_widget(RichWidget::new(&markdown), right);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}
// --8<-- [end:side-by-side]

// --8<-- [start:patch]
fn patch() -> Buffer {
    let mut table = Table::new();
    table.add_column("queue");
    table.add_column("depth");
    table.add_row(&["emails", "[bold]12[/]"]);
    table.add_row(&["exports", "[red]340[/]"]);

    let area = Rect::new(0, 0, 40, 7);
    let mut buffer = Buffer::empty(area);
    // A ratatui block paints a background first...
    let block = Block::bordered()
        .title(" queues ")
        .style(Style::default().bg(Color::Rgb(30, 40, 70)));
    let inner = block.inner(area);
    block.render(area, &mut buffer);
    // ...and the table's unstyled cells keep it.
    RichWidget::new(&table).render(inner, &mut buffer);
    buffer
}
// --8<-- [end:patch]

// --8<-- [start:component]
fn counter() -> RatatuiComponent<u32, u32> {
    RatatuiComponent::with_state(0u32, |n, area, buf| {
        Paragraph::new(format!("pressed {n}"))
            .block(Block::bordered().title("counter"))
            .render(area, buf);
    })
    .on_event(|n, event| match event {
        Event::Key(key) if key.code == KeyCode::Enter => Flow::Done(*n),
        Event::Key(_) => {
            *n += 1;
            Flow::Continue
        }
        _ => Flow::Ignored,
    })
    .height(3)
}

fn component(console: &Console) {
    // Under the headless driver; `rich_interact::run` drives a terminal.
    let (outcome, record) = headless::run(counter(), Script::new().keys("a b enter"), 30, 10);
    assert!(matches!(outcome.unwrap(), Outcome::Done(2)));
    assert!(record.last_frame().contains("pressed 2"));

    // The same keys by hand, then the component's view, as the loop paints it.
    let context = Context {
        console,
        width: 30,
        height: 10,
    };
    let mut counter = counter();
    for step in Script::new().keys("a b").0 {
        if let Step::Event(event) = step {
            counter.handle(&event, &context);
        }
    }
    console.print(&Lines(counter.render(&context).lines));
}
// --8<-- [end:component]

/// Rich lines (a buffer read back with `buffer_to_lines`, or a view) as a
/// renderable, so a console can print or export them.
struct Lines(Vec<Vec<Segment>>);

impl Renderable for Lines {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        for line in &self.0 {
            segments.extend(line.iter().cloned());
            segments.push(Segment::line());
        }
        segments
    }
}

fn main() {
    let shots = Shots::from_args("guide_ratatui");
    shots.shot("widget", 40, |c| {
        c.print(&Lines(buffer_to_lines(&widget())))
    });
    shots.shot("side-by-side", 76, |c| {
        c.print(&Lines(buffer_to_lines(&side_by_side())))
    });
    shots.shot("patch", 40, |c| c.print(&Lines(buffer_to_lines(&patch()))));
    shots.shot("component", 30, component);
}

/// `--svg DIR` writes each shot as `DIR/<topic>-<shot>.svg` from a pinned
/// console; without it, shots print to the terminal.
struct Shots {
    topic: &'static str,
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args(topic: &'static str) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { topic, dir }
    }

    fn shot(&self, name: &str, width: usize, f: impl FnOnce(&Console)) {
        let Some(dir) = &self.dir else {
            return f(&Console::new());
        };
        let console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        let id = format!("{}-{name}", self.topic);
        let svg = console.export_svg(&id, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        let path = dir.join(format!("{id}.svg"));
        std::fs::write(&path, svg).expect("write the SVG");
        eprintln!("wrote {}", path.display());
    }
}
