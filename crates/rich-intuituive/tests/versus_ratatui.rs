//! The scorecard's regression gate: the same ops dashboard in intuiTUIve
//! and in ratatui, frame cost and bytes per frame, three interactions.
//!
//!     cargo test --release -p rs-rich-intuituive --test versus_ratatui -- --ignored --nocapture
//!
//! Ignored by default: timings mean something only in release builds. It
//! asserts the bar the design note commits to: no slower than ratatui on a
//! tick or a selection move, within 1.5 times on a log append (1.2 is the
//! target; CI machines are noisy), and never more bytes.

#[path = "../examples/dashboard.rs"]
#[allow(dead_code)]
mod dashboard;

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use dashboard::{log_line, service, SERVICES};
use ratatui::backend::{ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Constraint, Layout, Position, Rect, Size};
use ratatui::style::{Color, Modifier, Style as RStyle};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};
use rich_interact::{Backend, Event, Key, KeyCode};

// app: ratatui
struct RatatuiDash {
    selected: usize,
    tick: u64,
    log: Vec<String>,
}

impl RatatuiDash {
    fn draw(&self, frame: &mut Frame) {
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
            Paragraph::new(format!(" tick {} · ↑↓ select · l log · q quit", self.tick))
                .style(RStyle::new().add_modifier(Modifier::DIM)),
            footer,
        );
    }
}
// app: end

/// Bytes written, shared with the benchmark.
#[derive(Clone, Default)]
struct Sink(std::rc::Rc<std::cell::RefCell<usize>>);

impl io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        *self.0.borrow_mut() += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// ratatui's crossterm backend writing to memory at a fixed size.
struct Counting {
    inner: CrosstermBackend<Sink>,
    size: Size,
}

impl ratatui::backend::Backend for Counting {
    type Error = io::Error;
    fn draw<'a, I: Iterator<Item = (u16, u16, &'a Cell)>>(&mut self, content: I) -> io::Result<()> {
        self.inner.draw(content)
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(Position::ORIGIN)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }
    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }
    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }
    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::new(0, 0),
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        ratatui::backend::Backend::flush(&mut self.inner)
    }
}

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Tick,
    Select,
    Log,
}

impl Scenario {
    fn key(self) -> &'static str {
        match self {
            Scenario::Tick => "t",
            Scenario::Select => "down",
            Scenario::Log => "l",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Measured {
    micros: f64,
    bytes: f64,
}

fn ratatui(scenario: Scenario, width: u16, height: u16, frames: usize) -> Measured {
    let sink = Sink::default();
    let backend = Counting {
        inner: CrosstermBackend::new(sink.clone()),
        size: Size::new(width, height),
    };
    let options = TerminalOptions {
        viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
    };
    let mut terminal = Terminal::with_options(backend, options).unwrap();
    let mut dash = RatatuiDash {
        selected: 0,
        tick: 0,
        log: (0..50).map(log_line).collect(),
    };
    terminal.draw(|f| dash.draw(f)).unwrap();
    let first = *sink.0.borrow();
    let start = Instant::now();
    for n in 0..frames {
        match scenario {
            Scenario::Tick => dash.tick += 1,
            Scenario::Select => dash.selected = (dash.selected + 1) % SERVICES,
            Scenario::Log => {
                dash.log.push(log_line(50 + n));
                dash.tick += 1;
            }
        }
        terminal.draw(|f| dash.draw(f)).unwrap();
    }
    let elapsed = start.elapsed();
    let total = *sink.0.borrow();
    Measured {
        micros: elapsed.as_secs_f64() * 1e6 / frames as f64,
        bytes: (total - first) as f64 / frames as f64,
    }
}

/// A backend that feeds a run of key presses and counts what is written.
struct Feed {
    events: VecDeque<Event>,
    size: (u16, u16),
    bytes: usize,
    writes: usize,
    first: Option<usize>,
    started: Option<Instant>,
    clock: Duration,
}

impl Backend for Feed {
    fn size(&self) -> (u16, u16) {
        self.size
    }
    fn read(&mut self, _: Option<Duration>) -> io::Result<Option<Event>> {
        if self.started.is_none() {
            self.started = Some(Instant::now());
            self.first = Some(self.bytes);
        }
        self.clock += Duration::from_millis(1);
        Ok(self.events.pop_front())
    }
    fn write(&mut self, text: &str) -> io::Result<()> {
        self.bytes += text.len();
        self.writes += 1;
        Ok(())
    }
    fn elapsed(&self) -> Duration {
        self.clock
    }
    fn handoff(&mut self, _: &mut std::process::Command) -> io::Result<Option<i32>> {
        Ok(None)
    }
    fn alternate_screen(&self) -> bool {
        true
    }
}

fn intuituive(scenario: Scenario, width: u16, height: u16, frames: usize) -> Measured {
    let key = Key::parse(scenario.key()).unwrap();
    let mut events: VecDeque<Event> = std::iter::repeat_n(Event::Key(key), frames).collect();
    events.push_back(Event::Key(Key::new(KeyCode::Char('q'))));
    let mut feed = Feed {
        events,
        size: (width, height),
        bytes: 0,
        writes: 0,
        first: None,
        started: None,
        clock: Duration::ZERO,
    };
    let app = dashboard::dashboard(false).text_frames(false);
    app.run_on(&mut feed).unwrap();
    let elapsed = feed.started.unwrap().elapsed();
    // The closing write (reset, cursor shown) is not a frame.
    let bytes = feed.bytes - feed.first.unwrap() - "\x1b[0m\x1b[?25h".len();
    Measured {
        micros: elapsed.as_secs_f64() * 1e6 / frames as f64,
        bytes: bytes as f64 / frames as f64,
    }
}

fn best(f: impl Fn() -> Measured) -> Measured {
    (0..3)
        .map(|_| f())
        .min_by(|a, b| a.micros.total_cmp(&b.micros))
        .unwrap()
}

#[test]
#[ignore = "a benchmark: run it in release with --ignored"]
fn intuituive_meets_the_scorecard_against_ratatui() {
    let frames = 500;
    println!("| Size | Scenario | ratatui µs | intuiTUIve µs | ratatui B | intuiTUIve B |");
    println!("|---|---|---:|---:|---:|---:|");
    let mut failures = Vec::new();
    for (w, h) in [(80, 24), (200, 60)] {
        for scenario in [Scenario::Tick, Scenario::Select, Scenario::Log] {
            let r = best(|| ratatui(scenario, w, h, frames));
            let i = best(|| intuituive(scenario, w, h, frames));
            println!(
                "| {w}x{h} | {scenario:?} | {:.1} | {:.1} | {:.0} | {:.0} |",
                r.micros, i.micros, r.bytes, i.bytes
            );
            let allowed = match scenario {
                Scenario::Log => 1.5,
                _ => 1.0,
            };
            if i.micros > r.micros * allowed {
                failures.push(format!(
                    "{w}x{h} {scenario:?}: {:.1} µs against ratatui's {:.1}",
                    i.micros, r.micros
                ));
            }
            if i.bytes > r.bytes * 1.05 {
                failures.push(format!(
                    "{w}x{h} {scenario:?}: {:.0} bytes against ratatui's {:.0}",
                    i.bytes, r.bytes
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_dashboard_draws() {
    let screen = dashboard::dashboard(false)
        .render_with(&["down", "down", "l", "q"], 80, 24)
        .unwrap();
    assert!(screen[0].starts_with(" ops · 20 services"), "{screen:#?}");
    assert!(screen.iter().any(|l| l.contains("svc-02")), "{screen:#?}");
    assert!(
        screen.iter().any(|l| l.contains("status   degraded")),
        "{screen:#?}"
    );
    assert!(screen[23].contains("tick 1"), "{screen:#?}");
}
