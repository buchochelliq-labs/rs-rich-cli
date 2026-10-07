//! Widgets of your own: a board of meters, each a custom [`Widget`] that
//! draws itself cell by cell, the way the framework's own widgets do.
//!
//!     cargo run -p rs-rich-intuituive --example meters [-- --frozen]
//!
//! Tab moves between the meters · ←/→ set the focused meter's alarm · r
//! resets its peak · Space pauses the board · the pointer over a sparkline
//! reads that sample · q quits. `--frozen` fills the meters once and stops
//! the clock, for screenshots that do not move.
//!
//! What each part shows of the trait:
//!
//! - **Retained drawing.** A meter keeps what it drew: its border is drawn
//!   when it must be ([`DrawCx::repaint`], or the focus came or went), and
//!   a new sample redraws only the three rows inside it.
//! - **Lifecycle events.** `Focus` redraws the border in the accent style;
//!   `Resize` sets how many samples the sparkline shows (one per column).
//! - **Hover and the pointer.** [`DrawCx::pointer`] says which column of
//!   the sparkline is under the mouse; the meter draws again only when the
//!   pointer moves inside it.
//! - **Keys before the focus.** The board sees keys before the meter inside
//!   it ([`Widget::previews_keys`]): while paused, it keeps ←/→ and `r` from
//!   the meters and says why in a toast.
//! - **Animation and toasts.** The gauge eases to each new sample, and a
//!   sample over a meter's alarm raises a toast.

use std::time::Duration;

use intuituive::interact::{Key, KeyCode};
use intuituive::node::Axis;
use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};
use intuituive::Easing;

/// One meter's data, shared by the clock that feeds it and the widget that
/// draws it.
#[derive(Clone, Copy)]
pub struct Feed {
    /// The samples, oldest first, each 0 to 1.
    pub samples: Signal<Vec<f64>>,
    /// The gauge's value: the newest sample, eased towards.
    pub shown: Signal<f64>,
    /// The level over which a sample raises a toast.
    pub alarm: Signal<f64>,
    /// The highest sample since the last reset.
    pub peak: Signal<f64>,
}

impl Feed {
    fn new(alarm: f64) -> Feed {
        Feed {
            samples: signal(Vec::new()),
            shown: signal(0.0),
            alarm: signal(alarm),
            peak: signal(0.0),
        }
    }
}

/// The most samples a meter keeps.
const HISTORY: usize = 240;

/// The `n`th sample of meter `which`: a slow wave with some noise, the same
/// every run.
pub fn sample(which: usize, n: usize) -> f64 {
    let t = n as f64 / 12.0 + which as f64 * 1.7;
    let mut x =
        (n as u64 + 1).wrapping_mul(6364136223846793005) ^ (which as u64 * 1442695040888963407);
    x ^= x >> 29;
    let noise = (x % 1000) as f64 / 1000.0 - 0.5;
    (0.5 + 0.32 * t.sin() + 0.12 * (t * 2.3).cos() + 0.12 * noise).clamp(0.0, 1.0)
}

/// A meter: a gauge, a sparkline of its history, and a status row, in a
/// border with its name.
pub struct Meter {
    name: String,
    feed: Feed,
    /// How many samples the sparkline shows: its width, set on `Resize`.
    window: usize,
    /// The border needs drawing again (the focus came or went).
    border: bool,
    focused: bool,
}

impl Meter {
    pub fn new(name: &str, feed: Feed) -> Meter {
        Meter {
            name: name.to_string(),
            feed,
            window: 0,
            border: true,
            focused: false,
        }
    }
}

/// Eight levels of a sparkline, from empty to full.
const BARS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

impl Widget for Meter {
    fn name(&self) -> &'static str {
        "meter"
    }

    fn describe(&self) -> Option<String> {
        Some(self.name.clone())
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            // A border round a gauge, a sparkline and a status row; a
            // taller meter gives the sparkline the rows.
            Axis::Vertical => 5,
            Axis::Horizontal => width,
        }
    }

    fn retained(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        if width < 4 || height < 5 {
            return;
        }
        if cx.repaint() || self.border {
            let edge = if self.focused {
                cx.style("accent", "cyan")
            } else {
                cx.style("muted", "dim")
            };
            canvas.border(&self.name, &edge, &cx.style("bold", "bold"));
            self.border = false;
        }
        let inner = width - 2;
        // `Resize` comes when the size changes after the first layout.
        if self.window == 0 {
            self.window = inner as usize;
        }
        let console = cx.console().clone();
        // The gauge: the eased value, coloured by how near the alarm it is.
        let shown = self.feed.shown.get();
        let alarm = self.feed.alarm.get();
        let label = format!(" {:>3.0}%", shown * 100.0);
        let bar = inner.saturating_sub(label.len() as u16);
        let filled = ((shown * bar as f64).round() as u16).min(bar);
        let colour = if shown >= alarm {
            "red"
        } else if shown >= alarm - 0.15 {
            "yellow"
        } else {
            "green"
        };
        let alarm_at = ((alarm * bar as f64).round() as u16).min(bar.saturating_sub(1));
        let mut gauge = String::new();
        for x in 0..bar {
            let cell = match (x < filled, x == alarm_at) {
                (_, true) => "[bold]│[/]".to_string(),
                (true, false) => format!("[{colour}]█[/]"),
                (false, false) => "[dim]░[/]".to_string(),
            };
            gauge.push_str(&cell);
        }
        // Inside the border: the gauge, the sparkline's rows, the status.
        let rows = height - 4;
        let status_row = height - 2;
        canvas.clear(1, 1, inner, height - 2);
        canvas.markup(&console, 1, 1, inner, &format!("{gauge}{label}"), None);
        // The sparkline: the newest samples, one per column, as tall as
        // the meter has rows for, each row an eighth-block step.
        let pointer = cx.pointer();
        self.feed.samples.with(|samples| {
            let start = samples
                .len()
                .saturating_sub(self.window.min(inner as usize));
            let shown = &samples[start..];
            let spark = cx.style("accent", "cyan");
            for row in 0..rows {
                // Eighths filled below this row, from the bottom one up.
                let below = (rows - 1 - row) as f64 * 8.0;
                let line: String = shown
                    .iter()
                    .map(|v| {
                        let level = (v * rows as f64 * 8.0).round() - below;
                        BARS[level.clamp(0.0, 8.0) as usize]
                    })
                    .collect();
                canvas.print(1, 2 + row, &line, Some(&spark));
            }
            // What is under the pointer, else the peak and the alarm.
            let under = pointer
                .filter(|(x, y)| (2..2 + rows).contains(y) && *x >= 1)
                .and_then(|(x, _)| shown.get(x as usize - 1).map(|v| (x, *v)));
            let status = match under {
                Some((x, value)) => {
                    canvas.restyle(x, 2, 1, rows, &cx.style("reverse", "reverse"));
                    let ago = shown.len() - x as usize;
                    format!("[b]{:.0}%[/] [muted]{ago} samples ago", value * 100.0)
                }
                None => format!(
                    "[muted]peak[/] {:.0}%  [muted]alarm[/] {:.0}%",
                    self.feed.peak.get() * 100.0,
                    alarm * 100.0
                ),
            };
            canvas.markup(&console, 1, status_row, inner, &status, None);
        });
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Focus(on) => {
                self.focused = *on;
                self.border = true;
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Resize { width, .. } => {
                self.window = width.saturating_sub(2) as usize;
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Key(key) if key.code == KeyCode::Left => {
                self.feed.alarm.update(|a| *a = (*a - 0.05).max(0.05));
                Used::Yes
            }
            WidgetEvent::Key(key) if key.code == KeyCode::Right => {
                self.feed.alarm.update(|a| *a = (*a + 0.05).min(1.0));
                Used::Yes
            }
            WidgetEvent::Key(key) if *key == Key::char('r') => {
                let latest = self.feed.shown.get_untracked();
                self.feed.peak.set(latest);
                cx.app().toast(format!("{}: peak reset", self.name));
                Used::Yes
            }
            _ => Used::No,
        }
    }
}

/// The meters in two columns. It sees keys before the meter that has the
/// focus, so pausing can keep them from changing anything.
pub struct Board {
    meters: Vec<Node>,
    paused: Signal<bool>,
}

impl Widget for Board {
    fn name(&self) -> &'static str {
        "board"
    }

    fn children(&self) -> &[Node] {
        &self.meters
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => self.meters.len().div_ceil(2) as u16 * 5,
            Axis::Horizontal => width,
        }
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        // Two columns, the rows sharing the height (five each at least).
        let half = rect.width / 2;
        let rows = self.meters.len().div_ceil(2).max(1) as u16;
        let tall = (rect.height / rows).max(5);
        (0..self.meters.len())
            .map(|i| {
                let (column, row) = ((i % 2) as u16, (i / 2) as u16);
                let width = if column == 0 { half } else { rect.width - half };
                Rect::new(rect.x + column * half, rect.y + row * tall, width, tall)
            })
            .collect()
    }

    fn retained(&self) -> bool {
        true
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn previews_keys(&self) -> bool {
        true
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let WidgetEvent::Preview(key) = event else {
            return Used::No;
        };
        let changes = matches!(key.code, KeyCode::Left | KeyCode::Right) || *key == Key::char('r');
        if changes && self.paused.get_untracked() {
            cx.app().toast("[yellow]Paused[/]: Space resumes");
            return Used::Yes;
        }
        Used::No
    }
}

/// The board of four meters; `frozen` fills them once and stops the clock.
pub fn meters_app(frozen: bool) -> App {
    App::new(move || {
        let names = ["cpu", "memory", "disk", "network"];
        let alarms = [0.8, 0.85, 0.9, 0.75];
        let feeds: Vec<Feed> = alarms.iter().map(|a| Feed::new(*a)).collect();
        let paused = signal(false);
        let tick = signal(0usize);

        // A new sample for every meter: the gauge eases to it, and one over
        // the alarm raises a toast as it crosses.
        let step = {
            let feeds = feeds.clone();
            move |cx: Option<&mut Ctx>, n: usize| {
                let mut cx = cx;
                for (which, feed) in feeds.iter().enumerate() {
                    let value = sample(which, n);
                    let before = feed.samples.with_untracked(|s| s.last().copied());
                    feed.samples.update(|s| {
                        s.push(value);
                        if s.len() > HISTORY {
                            s.remove(0);
                        }
                    });
                    feed.peak.update(|p| *p = p.max(value));
                    let alarm = feed.alarm.get_untracked();
                    match cx.as_deref_mut() {
                        Some(cx) => {
                            cx.animate(
                                feed.shown,
                                value,
                                Duration::from_millis(180),
                                Easing::EaseOut,
                            );
                            if value >= alarm && before.is_some_and(|b| b < alarm) {
                                cx.toast(format!(
                                    "[red]{}[/] over {:.0}%",
                                    names[which],
                                    alarm * 100.0
                                ));
                            }
                        }
                        None => feed.shown.set(value),
                    }
                }
            }
        };
        if frozen {
            for n in 0..HISTORY {
                step(None, n);
            }
        } else {
            every(Duration::from_millis(250), move |cx| {
                if !paused.get_untracked() {
                    let n = tick.get_untracked();
                    tick.set(n + 1);
                    step(Some(cx), n);
                }
            });
        }

        let meters = names
            .iter()
            .zip(&feeds)
            .map(|(name, feed)| widget(Meter::new(name, *feed)))
            .collect();
        let board = widget(Board { meters, paused });
        let status = text(move || {
            let state = if paused.get() {
                "[yellow]paused[/]"
            } else {
                "[green]live[/]"
            };
            format!("{state}  [dim]tab meter · ←/→ alarm · r peak · space pause · q quit[/]")
        });
        column([
            label("[bold]meters[/] [muted]· widgets of your own[/]").fixed(1),
            board.flex(1),
            status.fixed(1),
        ])
        .bind("space", "pause or resume", move |_| {
            paused.update(|p| *p = !*p)
        })
        .bind("q", "quit", |cx| cx.quit())
    })
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let frozen = std::env::args().any(|a| a == "--frozen");
    meters_app(frozen).run()
}
