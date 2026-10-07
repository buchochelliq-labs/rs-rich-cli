//! One ops dashboard, written twice: with ratatui, and with the prototype's
//! signals and retained tree. Both draw the same screen:
//!
//! ```text
//!  ops · 20 services
//! ╭ Services ───────╮╭ Detail ──────────────────────╮
//! │● svc-00    12ms ││svc-03                        │
//! │● svc-01    40ms ││status   degraded             │
//! │...              │╰──────────────────────────────╯
//! │                 │╭ Log ─────────────────────────╮
//! │                 ││12:00:01 svc-03 restarted     │
//! ╰─────────────────╯╰──────────────────────────────╯
//!  tick 42 · ↑↓ select · q quit
//! ```
//!
//! The code between the `// app:` markers is what an author writes; the
//! benchmark counts its lines.

use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style as RStyle};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::reactive::{signal, Signal};
use crate::tree::{bordered, column, keyed, leaf, row, text, App, Size};

pub const SERVICES: usize = 20;

pub fn service(i: usize) -> (String, &'static str, u32) {
    let status = ["up", "up", "degraded", "up", "down"][i % 5];
    (format!("svc-{i:02}"), status, 10 + (i as u32 * 37) % 90)
}

pub fn log_line(n: usize) -> String {
    let (name, _, _) = service(n % SERVICES);
    format!(
        "12:{:02}:{:02} {name} heartbeat #{n}",
        (n / 60) % 60,
        n % 60
    )
}

// app: ratatui
/// The ratatui dashboard: state in a struct, everything drawn every frame.
pub struct RatatuiDash {
    pub selected: usize,
    pub tick: u64,
    pub log: Vec<String>,
}

impl RatatuiDash {
    pub fn draw(&self, frame: &mut Frame) {
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(body);
        let [detail, log] =
            Layout::vertical([Constraint::Length(5), Constraint::Min(0)]).areas(right);
        let bold = RStyle::new().add_modifier(Modifier::BOLD);
        let block = |title: &'static str| {
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(RStyle::new().fg(Color::Blue))
                .title(Span::styled(format!(" {title} "), bold.fg(Color::Blue)))
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ops", bold),
                Span::raw(format!(" · {SERVICES} services")),
            ])),
            header,
        );
        let items: Vec<ListItem> = (0..SERVICES)
            .map(|i| {
                let (name, status, ms) = service(i);
                let colour = match status {
                    "up" => Color::Green,
                    "degraded" => Color::Yellow,
                    _ => Color::Red,
                };
                ListItem::new(Line::from(vec![
                    Span::styled("● ", RStyle::new().fg(colour)),
                    Span::raw(format!("{name:<8} {ms:>4}ms")),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(block("Services"))
            .highlight_style(RStyle::new().add_modifier(Modifier::REVERSED));
        let mut state = ListState::default().with_selected(Some(self.selected));
        frame.render_stateful_widget(list, left, &mut state);
        let (name, status, ms) = service(self.selected);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(name, bold),
                Line::raw(format!("status   {status}")),
                Line::raw(format!("latency  {ms}ms")),
            ])
            .block(block("Detail")),
            detail,
        );
        let rows = log.height.saturating_sub(2) as usize;
        let tail = &self.log[self.log.len().saturating_sub(rows)..];
        frame.render_widget(
            Paragraph::new(
                tail.iter()
                    .map(|l| Line::raw(l.as_str()))
                    .collect::<Vec<_>>(),
            )
            .block(block("Log")),
            log,
        );
        frame.render_widget(
            Paragraph::new(format!(" tick {} · ↑↓ select · q quit", self.tick))
                .style(RStyle::new().add_modifier(Modifier::DIM)),
            footer,
        );
    }
}
// app: end

/// The prototype dashboard's state: signals the handlers write.
#[derive(Clone, Copy)]
pub struct Signals {
    pub selected: Signal<usize>,
    /// One flag per row, so a move re-renders two rows, not twenty (what a
    /// memo would derive from `selected` in a full implementation).
    pub marks: Signal<Vec<Signal<bool>>>,
    pub tick: Signal<u64>,
    pub log: Signal<Vec<String>>,
}

impl Signals {
    pub fn select(&self, to: usize) {
        let from = self.selected.get_untracked();
        let marks = self.marks.get_untracked();
        marks[from].set(false);
        marks[to].set(true);
        self.selected.set(to);
    }
}

// app: prototype
/// The prototype dashboard: a retained tree whose leaves read signals.
pub fn prototype() -> (App<()>, Signals) {
    let s = Signals {
        selected: signal(0),
        marks: signal((0..SERVICES).map(|i| signal(i == 0)).collect()),
        tick: signal(0),
        log: signal(Vec::new()),
    };
    let services = keyed(signal((0..SERVICES).collect::<Vec<_>>()), move |&i| {
        let mark = s.marks.get_untracked()[i];
        text(move || {
            let (name, status, ms) = service(i);
            let colour = match status {
                "up" => "green",
                "degraded" => "yellow",
                _ => "red",
            };
            let row = format!("[{colour}]●[/] {name:<8} {ms:>4}ms");
            if mark.get() {
                format!("[reverse]{row}[/]")
            } else {
                row
            }
        })
    });
    let detail = text(move || {
        let (name, status, ms) = service(s.selected.get());
        format!("[bold]{name}[/]\nstatus   {status}\nlatency  {ms}ms")
    });
    let log = leaf(move |console, width, height| {
        s.log.with(|log| {
            let tail = &log[log.len().saturating_sub(height)..];
            let text = rich::Text::new(tail.join("\n"));
            console.render_lines(&text, &console.options().update_width(width.max(1)), false)
        })
    });
    let root = column(vec![
        text(|| format!("[bold] ops[/] · {SERVICES} services")).size(Size::Fixed(1)),
        row(vec![
            bordered("Services", services).size(Size::Flex(40)),
            column(vec![
                bordered("Detail", detail).size(Size::Fixed(5)),
                bordered("Log", log),
            ])
            .size(Size::Flex(60)),
        ]),
        text(move || format!("[dim] tick {} · ↑↓ select · q quit[/]", s.tick.get()))
            .size(Size::Fixed(1)),
    ]);
    (App::new(root, |_| rich_interact::Flow::Continue), s)
}
// app: end

/// Lines of author code between the `// app: NAME` and `// app: end`
/// markers in this file, blank lines and comments excluded.
pub fn author_lines(name: &str) -> usize {
    let source = include_str!("dashboard.rs");
    let start = format!("// app: {name}");
    let body = source.split(&start).nth(1).unwrap_or("");
    let body = body.split("// app: end").next().unwrap_or("");
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &crate::tree::Lines) -> String {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect::<String>() + "\n")
            .collect()
    }

    #[test]
    fn the_prototype_draws_the_dashboard() {
        let (app, s) = prototype();
        let console = rich::Console::builder()
            .width(80)
            .color_system(None)
            .build();
        s.log.update(|l| l.extend((0..30).map(log_line)));
        let screen = plain(&app.frame(&console, 80, 24));
        assert!(screen.starts_with(" ops · 20 services"), "{screen}");
        assert!(screen.contains("svc-19"), "{screen}");
        assert!(screen.contains(&log_line(29)), "{screen}");
        assert_eq!(screen.lines().count(), 24);
        assert!(
            screen.lines().all(|l| rich::cells::cell_len(l) == 80),
            "{screen}"
        );
        s.select(3);
        s.tick.set(7);
        let screen = plain(&app.frame(&console, 80, 24));
        assert!(
            screen.contains("status   up") && screen.contains("svc-03"),
            "{screen}"
        );
        assert!(screen.contains("tick 7"), "{screen}");
        // A move renders: two rows, the detail, and their ancestors.
        assert!(app.stats().rendered < 12, "{:?}", app.stats());
    }
}
