//! Spinners.
//!
//! Port of upstream `rich/spinner.py` and the full `rich/_spinners.py` table. A
//! [`Spinner`] picks an animation frame for a point in time;
//! [`Spinner::render`] is the testable surface. Like upstream, the first render
//! fixes the start of the animation, and [`Spinner::update`] can change the
//! text, style or speed mid-animation (a new speed takes effect from the next
//! render, continuing from the current frame). Rendered as a renderable, a
//! spinner shows the frame for the console's clock ([`Console::get_time`]), so
//! redrawing it in a `Live` display animates it; `ProgressColumn::Spinner`
//! animates one from the progress clock.
//!
//! Scope: all built-in spinners (vendored in `spinner_data.rs`), trailing text
//! as console markup ([`Spinner::text`]), as a literal [`Text`]
//! ([`Spinner::with_text`]) or as any renderable ([`Spinner::renderable`],
//! upstream's `Table.grid` of frame and renderable), and a frame style.

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::StyleType;
use crate::text::Text;

/// A named terminal spinner. Mirrors `rich.spinner.Spinner`.
pub struct Spinner {
    frames: &'static [&'static str],
    /// Frame interval in milliseconds.
    interval: f64,
    // Boxed so a spinner stays small inside `ProgressColumn`.
    text: Option<Box<Text>>,
    /// Upstream's `text` when it is neither a `str` nor a `Text`: shown in a
    /// grid beside the frame. Shared so a table cell can render it.
    renderable: Option<Arc<Mutex<Box<dyn Renderable + Send>>>>,
    style: Option<StyleType>,
    speed: Cell<f64>,
    /// Upstream's `start_time`: set by the first render.
    start_time: Cell<Option<f64>>,
    /// Upstream's `frame_no_offset`, carried across a speed change.
    frame_no_offset: Cell<f64>,
    /// Upstream's `_update_speed`: a pending speed (0.0 = none).
    update_speed: Cell<f64>,
}

/// Console markup as upstream's `Text.from_markup(text)`; malformed markup is
/// kept literally rather than failing a spinner.
fn markup(text: &str) -> Text {
    Text::from_markup(text).unwrap_or_else(|_| Text::new(text))
}

/// Every built-in spinner name, in upstream's `rich._spinners.SPINNERS`
/// order. Look each up with [`spinner_frames`].
pub fn spinner_names() -> &'static [&'static str] {
    crate::spinner_data::SPINNER_NAMES
}

/// A built-in spinner's `(interval in milliseconds, frames)`, or `None` for
/// an unknown name. Port of a `SPINNERS[name]` lookup (`interval`, `frames`).
pub fn spinner_frames(name: &str) -> Option<(f64, &'static [&'static str])> {
    crate::spinner_data::spinner_data(name)
}

impl Spinner {
    /// Look up a built-in spinner by name (falls back to `dots`).
    pub fn new(name: &str) -> Self {
        let (interval, frames) = crate::spinner_data::spinner_data(name)
            .or_else(|| crate::spinner_data::spinner_data("dots"))
            .expect("dots spinner exists");
        Spinner {
            frames,
            interval,
            text: None,
            renderable: None,
            style: None,
            speed: Cell::new(1.0),
            start_time: Cell::new(None),
            frame_no_offset: Cell::new(0.0),
            update_speed: Cell::new(0.0),
        }
    }

    /// Look up a built-in spinner by name, or `None` where upstream's
    /// constructor raises `KeyError("no spinner called 'name'")`.
    /// [`new`](Self::new) falls back to `dots` instead.
    pub fn try_new(name: &str) -> Option<Self> {
        crate::spinner_data::spinner_data(name).map(|_| Spinner::new(name))
    }

    /// Trailing text after the frame, parsed as console markup (upstream
    /// passes a `str` through `Text.from_markup`).
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(Box::new(markup(&text.into())));
        self.renderable = None;
        self
    }

    /// Trailing text after the frame, used as it is (upstream's `text=`
    /// given a `Text`).
    pub fn with_text(mut self, text: Text) -> Self {
        self.text = Some(Box::new(text));
        self.renderable = None;
        self
    }

    /// Any renderable after the frame (upstream's `text=` given another
    /// renderable): the frame and the renderable are laid out in a
    /// `Table.grid(padding=1)`.
    pub fn renderable(mut self, renderable: Box<dyn Renderable + Send>) -> Self {
        self.renderable = Some(Arc::new(Mutex::new(renderable)));
        self.text = None;
        self
    }

    /// Set the animation speed multiplier (default 1.0).
    pub fn speed(self, speed: f64) -> Self {
        self.speed.set(speed);
        self
    }

    /// Style applied to the spinner *frame* (not the trailing text): a style
    /// or a theme name such as `"status.spinner"`.
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Port of `Spinner.update`: replace the text or style when given, and
    /// schedule a speed change for the next render. Like upstream, empty text
    /// and a zero speed mean "unchanged".
    pub fn update(&mut self, text: Option<&str>, style: Option<StyleType>, speed: Option<f64>) {
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.text = Some(Box::new(markup(text)));
        }
        if let Some(style) = style {
            self.style = Some(style);
        }
        if let Some(speed) = speed.filter(|s| *s != 0.0) {
            self.update_speed.set(speed);
        }
    }

    /// Render the spinner as it appears at `time` seconds. Port of
    /// `Spinner.render`: the first call fixes the start time, the frame carries
    /// the spinner style and `Text.assemble(frame, " ", text)` adds the text.
    pub fn render(&self, time: f64) -> Text {
        let frame = self.frame(time);
        match &self.text {
            // `Text.assemble` turns the frame's style into a span over the
            // frame alone, so the trailing text keeps its own styling.
            Some(text) if !text.plain().is_empty() => {
                let mut assembled = Text::new("");
                assembled.append(frame.plain(), self.style.clone());
                assembled.append(" ", None);
                assembled.append_text(text)
            }
            _ => frame,
        }
    }

    /// [`render`](Self::render) for any trailing renderable: upstream's
    /// `render` returns a `Table.grid(padding=1)` of the frame and the
    /// renderable set with [`renderable`](Self::renderable), and the
    /// assembled text otherwise.
    pub fn render_renderable(&self, time: f64) -> Box<dyn Renderable> {
        let Some(renderable) = &self.renderable else {
            return Box::new(self.render(time));
        };
        let frame = self.frame(time);
        let mut table = crate::table::Table::grid().padding(1, 1, 1, 1);
        table.add_row_cells(vec![
            crate::table::Cell::Text(frame),
            crate::table::Cell::Renderable(Arc::new(Shared(renderable.clone()))),
        ]);
        Box::new(table)
    }

    /// The styled frame at `time`, moving the animation on as upstream's
    /// `render` does.
    fn frame(&self, time: f64) -> Text {
        let start = self.start_time.get().unwrap_or(time);
        self.start_time.set(Some(start));
        let frame_no = (time - start) * self.speed.get() / (self.interval / 1000.0)
            + self.frame_no_offset.get();
        // Python's `int()` truncates toward zero and `%` is non-negative.
        let index = (frame_no.trunc() as i64).rem_euclid(self.frames.len() as i64) as usize;
        let frame_str = self.frames[index];
        let pending = self.update_speed.get();
        if pending != 0.0 {
            self.frame_no_offset.set(frame_no);
            self.start_time.set(Some(time));
            self.speed.set(pending);
            self.update_speed.set(0.0);
        }
        let mut frame = Text::new(frame_str);
        if let Some(style) = &self.style {
            frame.set_base_style(style.clone());
        }
        frame
    }
}

/// A spinner's trailing renderable as a table cell.
struct Shared(Arc<Mutex<Box<dyn Renderable + Send>>>);

impl Renderable for Shared {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .measure(console, options)
    }
}

impl Renderable for Spinner {
    /// Port of `Spinner.__rich_console__`: the frame for the console's clock
    /// ([`Console::get_time`]), so each redraw inside a `Live` display moves
    /// the animation on.
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.renderable.is_some() {
            return self
                .render_renderable(console.get_time())
                .rich_render(console, options);
        }
        self.render(console.get_time())
            .rich_render(console, options)
    }

    /// Port of `Spinner.__rich_measure__`, which measures `self.render(0)`.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        if self.renderable.is_some() {
            return self.render_renderable(0.0).measure(console, options);
        }
        self.render(0.0).measure(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_table_is_public_in_upstream_order() {
        let names = spinner_names();
        assert_eq!(names.len(), 73);
        assert_eq!(names[0], "dots");
        assert!(names.iter().all(|name| spinner_frames(name).is_some()));
        assert!(spinner_frames("nope").is_none());
    }

    #[test]
    fn an_unknown_name_is_refused_by_try_new() {
        // Upstream raises `KeyError("no spinner called 'nope'")`.
        assert!(Spinner::try_new("nope").is_none());
        assert!(Spinner::try_new("line").is_some());
    }
    use crate::color::ColorSystem;

    /// The frame at `time` of a spinner whose first render was at 0.
    fn frame_at(name: &str, time: f64) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        let spinner = Spinner::new(name);
        spinner.render(0.0);
        console.render_to_string(&spinner.render(time))
    }

    #[test]
    fn dots_frames_match_upstream() {
        // Captured from real rich 15.0.0 (start time 0).
        assert_eq!(frame_at("dots", 0.0), "⠋");
        assert_eq!(frame_at("dots", 0.1), "⠙");
        assert_eq!(frame_at("dots", 0.25), "⠸");
    }

    #[test]
    fn line_frames_match_upstream() {
        assert_eq!(frame_at("line", 0.0), "-");
        assert_eq!(frame_at("line", 0.1), "-");
        assert_eq!(frame_at("line", 0.25), "\\");
    }

    #[test]
    fn full_table_covers_more_spinners() {
        // "moon"/"bounce" weren't in the original curated subset. moon: 80ms.
        assert_eq!(frame_at("moon", 0.0), "\u{1f311} ");
        assert_eq!(frame_at("moon", 0.08), "\u{1f312} ");
        assert_eq!(frame_at("bounce", 0.0), "\u{2801}");
    }

    #[test]
    fn arrow_and_dots2_match_upstream() {
        // arrow: interval 100ms → frame advances each 0.1s.
        assert_eq!(frame_at("arrow", 0.0), "←");
        assert_eq!(frame_at("arrow", 0.1), "↖");
        assert_eq!(frame_at("dots2", 0.0), "⣾");
    }

    #[test]
    fn text_follows_frame() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        let out = console.render_to_string(&Spinner::new("dots").text("Working").render(0.0));
        assert_eq!(out, "⠋ Working");
    }

    #[test]
    fn styled_frame_only() {
        // Captured from real rich 15.0.0: the frame is green, " Working" plain.
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(30)
            .no_color(false)
            .build();
        let spinner = Spinner::new("dots")
            .text("Working")
            .style(crate::style::Style::parse("green").unwrap());
        assert_eq!(
            console.render_to_string(&spinner.render(0.0)),
            "\x1b[32m⠋\x1b[0m Working"
        );
    }
}
