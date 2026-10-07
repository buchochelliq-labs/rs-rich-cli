//! Draw a rich `Table` and rich `Markdown` in a ratatui frame, side by side,
//! and print the frame. No real terminal is needed: the frame is drawn on
//! ratatui's `TestBackend`, and its buffer is printed through a rich console
//! (read back with `buffer_to_lines`), colours included.
//!
//!     cargo run -p rs-rich-ratatui --example rich_in_ratatui

use ratatui_core::backend::TestBackend;
use ratatui_core::layout::{Constraint, Layout};
use ratatui_core::terminal::Terminal;
use rich::markdown::Markdown;
use rich::{Console, Segment, Table};
use rich_ratatui::{buffer_to_lines, RichWidget};

const NOTES: &str = "\
# Release notes

- **Tables**, panels and trees draw as *ratatui widgets*.
- Markdown, too: `RichWidget::new(&markdown)`.

> Styles patch the cells beneath them.
";

fn main() {
    let mut table = Table::new().title("Services");
    table.add_column("service");
    table.add_column("p99");
    table.add_column("status");
    table.add_row(&["api", "35 ms", "[green]ok[/]"]);
    table.add_row(&["worker", "120 ms", "[yellow]slow[/]"]);
    table.add_row(&["billing", "-", "[bold red]down[/]"]);
    let markdown = Markdown::new(NOTES);

    let mut terminal = Terminal::new(TestBackend::new(76, 12)).expect("a test backend");
    terminal
        .draw(|frame| {
            let [left, right] = Layout::horizontal([Constraint::Length(34), Constraint::Fill(1)])
                .areas(frame.area());
            frame.render_widget(RichWidget::new(&table), left);
            frame.render_widget(RichWidget::new(&markdown), right);
        })
        .expect("drawing on a test backend");

    // Print the frame: its buffer, back as rich lines, through a console
    // (which drops the colours when stdout is not a terminal).
    let console = Console::new();
    for mut line in buffer_to_lines(terminal.backend().buffer()) {
        line.push(Segment::line());
        print!("{}", console.segments_to_string(&line));
    }
}
