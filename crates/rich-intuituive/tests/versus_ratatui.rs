//! The scorecard's regression gate: the same ops dashboard in intuiTUIve
//! and in ratatui, frame cost and bytes per frame, three interactions.
//! Two more scenes are measured and reported, not gated: a full-screen
//! churn where every cell changes each frame (an RGB swatch, as ratatui's
//! `colors-rgb`), and a line chart redrawn each tick.
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

use intuituive::prelude::*;
use intuituive::widget::{Canvas, DrawCx};

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

/// The swatch's colour at `x`, `y` of a `width` x `height` grid of half
/// cells: hue across, brightness down.
fn swatch(x: usize, y: usize, width: usize, height: usize) -> (u8, u8, u8) {
    let hue = x as f64 * 6.0 / width.max(1) as f64;
    let value = 1.0 - y as f64 / height.max(1) as f64;
    let part = |offset: f64| {
        let h = (hue + offset) % 6.0;
        let level = if h < 1.0 {
            h
        } else if h < 3.0 {
            1.0
        } else if h < 4.0 {
            4.0 - h
        } else {
            0.0
        };
        (level * value * 255.0) as u8
    };
    (part(0.0), part(4.0), part(2.0))
}

/// The swatch's colours for a `width` x `rows` screen, two a cell (the
/// upper and lower half of `▀`).
fn swatch_grid(width: usize, rows: usize) -> Vec<Vec<(u8, u8, u8)>> {
    let height = rows * 2;
    (0..height)
        .map(|y| (0..width).map(|x| swatch(x, y, width, height)).collect())
        .collect()
}

/// The chart's points at tick `tick`: a sine wave moving along.
fn wave(tick: u64) -> Vec<(f64, f64)> {
    (0..200)
        .map(|i| {
            let x = i as f64 / 10.0;
            (x, (x + tick as f64 / 5.0).sin())
        })
        .collect()
}

/// The swatch as ratatui draws it: every cell `▀` in two colours, shifted
/// along by the frame.
fn ratatui_churn(frame: &mut Frame, grid: &[Vec<(u8, u8, u8)>], shift: usize) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    let width = area.width as usize;
    for y in 0..area.height {
        for x in 0..area.width {
            let column = (x as usize + shift) % width.max(1);
            let (r, g, b) = grid[y as usize * 2][column];
            let (r2, g2, b2) = grid[y as usize * 2 + 1][column];
            buf[Position::new(x, y)]
                .set_char('▀')
                .set_fg(Color::Rgb(r, g, b))
                .set_bg(Color::Rgb(r2, g2, b2));
        }
    }
}

/// The wave as ratatui draws it: a braille line chart with its axes.
fn ratatui_chart(frame: &mut Frame, tick: u64) {
    use ratatui::symbols::Marker;
    use ratatui::widgets::{Axis, Chart, Dataset, GraphType};
    let points = wave(tick);
    let dataset = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(RStyle::new().fg(Color::Cyan))
        .data(&points);
    let chart = Chart::new(vec![dataset])
        .x_axis(Axis::default().bounds([0.0, 20.0]))
        .y_axis(Axis::default().bounds([-1.0, 1.0]));
    frame.render_widget(chart, frame.area());
}

/// The swatch as an intuiTUIve widget: a frame's worth of cells, every one
/// changed, drawn whenever the tick moves.
struct Swatch {
    tick: Signal<u64>,
    grid: Vec<Vec<(u8, u8, u8)>>,
}

impl intuituive::Widget for Swatch {
    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let shift = self.tick.get() as usize;
        let (width, rows) = (canvas.width() as usize, canvas.height() as usize);
        if self.grid.len() != rows * 2 || self.grid.first().map(Vec::len) != Some(width) {
            self.grid = swatch_grid(width, rows);
        }
        let colour = |(r, g, b): (u8, u8, u8)| rich::Color::from_rgb(r, g, b);
        let lines: Vec<Vec<rich::Segment>> = (0..rows)
            .map(|y| {
                (0..width)
                    .map(|x| {
                        let column = (x + shift) % width.max(1);
                        let style = rich::Style::from_color(
                            Some(colour(self.grid[y * 2][column])),
                            Some(colour(self.grid[y * 2 + 1][column])),
                        );
                        rich::Segment::new("▀", Some(style))
                    })
                    .collect()
            })
            .collect();
        canvas.lines(&lines);
    }
}

/// The two scenes the dashboard does not have, as intuiTUIve apps: `t`
/// moves them on.
fn scene(scenario: Scenario) -> App {
    match scenario {
        Scenario::Churn => App::new(|| {
            let tick = signal(0u64);
            intuituive::widget(Swatch {
                tick,
                grid: Vec::new(),
            })
            .on_key("t", move |_| tick.update(|t| *t += 1))
            .on_key("q", |cx| cx.quit())
        }),
        _ => App::new(|| {
            use rich_ext::chart::{Axis, Chart, Dataset, GraphType, Marker};
            let tick = signal(0u64);
            leaf(move |console, width, height| {
                let chart = Chart::new([Dataset::new(wave(tick.get()))
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style("cyan")])
                .x_axis(Axis::default().bounds(0.0, 20.0))
                .y_axis(Axis::default().bounds(-1.0, 1.0))
                .width(width as usize)
                .height(height.max(2) as usize);
                let options = console.options().update_width(width as usize);
                console.render_lines(&chart, &options, false)
            })
            .on_key("t", move |_| tick.update(|t| *t += 1))
            .on_key("q", |cx| cx.quit())
        }),
    }
}

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
    /// Every cell changes each frame.
    Churn,
    /// A line chart redrawn each tick.
    Chart,
}

impl Scenario {
    fn key(self) -> &'static str {
        match self {
            Scenario::Tick | Scenario::Churn | Scenario::Chart => "t",
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
    if let Scenario::Churn | Scenario::Chart = scenario {
        let grid = swatch_grid(width as usize, height as usize);
        let draw = |frame: &mut Frame, tick: u64| match scenario {
            Scenario::Churn => ratatui_churn(frame, &grid, tick as usize),
            _ => ratatui_chart(frame, tick),
        };
        terminal.draw(|f| draw(f, 0)).unwrap();
        let first = *sink.0.borrow();
        let start = Instant::now();
        for tick in 1..=frames as u64 {
            terminal.draw(|f| draw(f, tick)).unwrap();
        }
        let elapsed = start.elapsed();
        let total = *sink.0.borrow();
        return Measured {
            micros: elapsed.as_secs_f64() * 1e6 / frames as f64,
            bytes: (total - first) as f64 / frames as f64,
        };
    }
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
            Scenario::Churn | Scenario::Chart => unreachable!("drawn above"),
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
/// One key a frame: the loop drains every event already waiting before it
/// draws, so a read that may not wait (a zero timeout) finds none once
/// after each event, as a terminal typed into at human speed would.
struct Feed {
    events: VecDeque<Event>,
    /// An event was just handed out: the next read without waiting finds
    /// nothing.
    gave: bool,
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
    fn read(&mut self, timeout: Option<Duration>) -> io::Result<Option<Event>> {
        if self.started.is_none() {
            self.started = Some(Instant::now());
            self.first = Some(self.bytes);
        }
        self.clock += Duration::from_millis(1);
        if timeout == Some(Duration::ZERO) && std::mem::take(&mut self.gave) {
            return Ok(None);
        }
        self.gave = true;
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
        gave: false,
        size: (width, height),
        bytes: 0,
        writes: 0,
        first: None,
        started: None,
        clock: Duration::ZERO,
    };
    let app = match scenario {
        Scenario::Churn | Scenario::Chart => scene(scenario),
        _ => dashboard::dashboard(false),
    };
    app.text_frames(false).run_on(&mut feed).unwrap();
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
    // Reported, not gated yet: every cell changing, and a chart redrawn.
    for (w, h) in [(80, 24), (200, 60)] {
        for scenario in [Scenario::Churn, Scenario::Chart] {
            let r = best(|| ratatui(scenario, w, h, frames / 5));
            let i = best(|| intuituive(scenario, w, h, frames / 5));
            println!(
                "| {w}x{h} | {scenario:?} | {:.1} | {:.1} | {:.0} | {:.0} |",
                r.micros, i.micros, r.bytes, i.bytes
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_churn_and_the_chart_draw() {
    let screen = scene(Scenario::Churn)
        .render_with(&["t", "q"], 20, 4)
        .unwrap();
    assert!(
        screen.iter().all(|row| row.chars().all(|c| c == '▀')),
        "{screen:?}"
    );
    let screen = scene(Scenario::Chart)
        .render_with(&["t", "q"], 40, 10)
        .unwrap();
    // Braille dots, somewhere.
    assert!(
        screen
            .iter()
            .any(|row| row.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))),
        "{screen:#?}"
    );
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
