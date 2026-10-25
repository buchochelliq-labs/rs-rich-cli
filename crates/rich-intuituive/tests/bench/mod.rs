//! What the benchmarks share: the scenes the ratatui gate and the paint
//! benchmark both draw, a backend that feeds keys and counts bytes, and the
//! best of three runs.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use rich_interact::{Backend, Event};

use intuituive::prelude::*;
use intuituive::widget::{Canvas, DrawCx};

/// The swatch's colour at `x`, `y` of a `width` x `height` grid of half
/// cells: hue across, brightness down.
pub fn swatch(x: usize, y: usize, width: usize, height: usize) -> (u8, u8, u8) {
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
pub fn swatch_grid(width: usize, rows: usize) -> Vec<Vec<(u8, u8, u8)>> {
    let height = rows * 2;
    (0..height)
        .map(|y| (0..width).map(|x| swatch(x, y, width, height)).collect())
        .collect()
}

/// The chart's points at tick `tick`: a sine wave moving along.
pub fn wave(tick: u64) -> Vec<(f64, f64)> {
    (0..200)
        .map(|i| {
            let x = i as f64 / 10.0;
            (x, (x + tick as f64 / 5.0).sin())
        })
        .collect()
}

/// The swatch as an intuiTUIve widget: a frame's worth of cells, every one
/// changed, drawn whenever the tick moves.
pub struct Swatch {
    pub tick: Signal<u64>,
    pub grid: Vec<Vec<(u8, u8, u8)>>,
    /// Drawn through rich's path (a `Segment` and a `Style` a cell, written
    /// with `Canvas::lines`) rather than rs-rich-ext's fast path
    /// (`Canvas::put_rgb`): the same cells, for the two-path comparison.
    pub rich: bool,
}

impl intuituive::Widget for Swatch {
    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let shift = self.tick.get() as usize;
        let (width, rows) = (canvas.width() as usize, canvas.height() as usize);
        if self.grid.len() != rows * 2 || self.grid.first().map(Vec::len) != Some(width) {
            self.grid = swatch_grid(width, rows);
        }
        if !self.rich {
            let rgb = |(r, g, b): (u8, u8, u8)| Some([r, g, b]);
            for y in 0..rows {
                let (upper, lower) = (&self.grid[y * 2], &self.grid[y * 2 + 1]);
                for x in 0..width {
                    let column = (x + shift) % width.max(1);
                    canvas.put_rgb(
                        x as u16,
                        y as u16,
                        '▀',
                        rgb(upper[column]),
                        rgb(lower[column]),
                    );
                }
            }
            return;
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

/// Every cell in a new colour each frame (an RGB swatch, as ratatui's
/// `colors-rgb`): `t` moves it on. Drawn with rs-rich-ext's fast path.
pub fn churn_app() -> App {
    swatch_app(false)
}

/// [`churn_app`] drawn through rich's `Segment`s and `Style`s.
pub fn churn_rich_app() -> App {
    swatch_app(true)
}

fn swatch_app(rich: bool) -> App {
    App::new(move || {
        let tick = signal(0u64);
        intuituive::widget(Swatch {
            tick,
            grid: Vec::new(),
            rich,
        })
        .on_key("t", move |_| tick.update(|t| *t += 1))
        .on_key("q", |cx| cx.quit())
    })
}

/// The benchmark's chart: a braille line of the wave at `tick`, `width`
/// x `height`.
pub fn wave_chart(tick: u64, width: u16, height: u16) -> rich_ext::chart::Chart {
    use rich_ext::chart::{Axis, Chart, Dataset, GraphType, Marker};
    Chart::new([Dataset::new(wave(tick))
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style("cyan")])
    .x_axis(Axis::default().bounds(0.0, 20.0))
    .y_axis(Axis::default().bounds(-1.0, 1.0))
    .width(width as usize)
    .height(height.max(2) as usize)
}

/// A braille line chart of a moving sine wave: `t` moves it on. Drawn with
/// rs-rich-ext's fast path.
pub fn chart_app() -> App {
    App::new(|| {
        let tick = signal(0u64);
        intuituive::widget(WaveChart { tick })
            .on_key("t", move |_| tick.update(|t| *t += 1))
            .on_key("q", |cx| cx.quit())
    })
}

/// The chart as a widget drawing through the fast path.
struct WaveChart {
    tick: Signal<u64>,
}

impl intuituive::Widget for WaveChart {
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let chart = wave_chart(self.tick.get(), canvas.width(), canvas.height());
        canvas.draw(cx.console(), &chart);
    }
}

/// [`chart_app`] rendered to rich segments and written as lines.
pub fn chart_rich_app() -> App {
    App::new(|| {
        let tick = signal(0u64);
        leaf(move |console, width, height| {
            let chart = wave_chart(tick.get(), width, height);
            let options = console.options().update_width(width as usize);
            console.render_lines(&chart, &options, false)
        })
        .on_key("t", move |_| tick.update(|t| *t += 1))
        .on_key("q", |cx| cx.quit())
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Measured {
    pub micros: f64,
    pub bytes: f64,
}

/// A backend that feeds a run of key presses and counts what is written.
/// One key a frame: the loop drains every event already waiting before it
/// draws, so a read that may not wait (a zero timeout) finds none once
/// after each event, as a terminal typed into at human speed would.
pub struct Feed {
    pub events: VecDeque<Event>,
    /// An event was just handed out: the next read without waiting finds
    /// nothing.
    pub gave: bool,
    pub size: (u16, u16),
    pub bytes: usize,
    pub writes: usize,
    pub first: Option<usize>,
    pub started: Option<Instant>,
    pub clock: Duration,
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

impl Feed {
    /// `events`, then `q`, at `width` x `height`.
    pub fn new(mut events: VecDeque<Event>, width: u16, height: u16) -> Feed {
        events.push_back(Event::Key(rich_interact::Key::new(
            rich_interact::KeyCode::Char('q'),
        )));
        Feed {
            events,
            gave: false,
            size: (width, height),
            bytes: 0,
            writes: 0,
            first: None,
            started: None,
            clock: Duration::ZERO,
        }
    }
}

/// The fastest of three runs.
pub fn best(f: impl Fn() -> Measured) -> Measured {
    (0..3)
        .map(|_| f())
        .min_by(|a, b| a.micros.total_cmp(&b.micros))
        .unwrap()
}
