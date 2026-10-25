//! A status indicator with a spinner.
//!
//! Port of `rich/status.py`. A [`Status`] shows a spinner animation followed
//! by a status message, parsed as console markup. It is a renderable whose
//! spinner reads the console's clock ([`Console::get_time`]), and
//! [`Status::start`] runs it in its own transient live display, as
//! upstream's `with Status(...)` does ([`LiveStatus`]).
//! [`Status::update`] follows upstream: a new spinner name replaces the
//! spinner (restarting its animation) and redraws, anything else updates it
//! in place for the next refresh.

use std::io::Write;
use std::sync::{Arc, Mutex};

use crate::console::{Console, ConsoleOptions};
use crate::file_proxy::{FileProxy, ProxyStream};
use crate::live::{AutoLive, Live, LiveHandle, LiveOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::spinner::{Spinner, SpinnerText};
use crate::style::StyleType;
use crate::text::Text;

/// A spinner + message status indicator. Mirrors `rich.status.Status`.
pub struct Status {
    status: SpinnerText,
    spinner_style: StyleType,
    speed: f64,
    spinner: Spinner,
    refresh_per_second: f64,
}

impl Status {
    /// A status showing `message` with the default `dots` spinner, styled
    /// `status.spinner` (green in the default theme).
    pub fn new(message: impl Into<String>) -> Self {
        Status::from_content(SpinnerText::Markup(message.into()))
    }

    /// A status showing `message` as it is (upstream's `Status(Text(...))`).
    pub fn from_text(message: Text) -> Self {
        Status::from_content(SpinnerText::Text(message))
    }

    /// A status showing any renderable beside the spinner (upstream's
    /// `Status(renderable)`), in a `Table.grid` as [`Spinner::renderable`].
    pub fn from_renderable(renderable: Box<dyn Renderable + Send>) -> Self {
        Status::from_content(SpinnerText::Renderable(Arc::new(Mutex::new(renderable))))
    }

    fn from_content(status: SpinnerText) -> Self {
        let spinner_style = StyleType::Name("status.spinner".to_string());
        Status {
            spinner: Spinner::new("dots")
                .content(&status)
                .style(spinner_style.clone()),
            status,
            spinner_style,
            speed: 1.0,
            refresh_per_second: 12.5,
        }
    }

    /// How often [`start`](Self::start)'s display redraws (upstream
    /// `refresh_per_second`, default 12.5).
    pub fn refresh_per_second(mut self, refresh_per_second: f64) -> Self {
        self.refresh_per_second = refresh_per_second;
        self
    }

    fn rebuild(mut self, name: &str) -> Self {
        self.spinner = Spinner::new(name)
            .content(&self.status)
            .style(self.spinner_style.clone())
            .speed(self.speed);
        self
    }

    /// Choose the spinner animation by name (default `dots`).
    pub fn spinner(self, name: &str) -> Self {
        self.rebuild(name)
    }

    /// Style applied to the spinner frame (default `status.spinner`).
    pub fn spinner_style(mut self, style: impl Into<StyleType>) -> Self {
        self.spinner_style = style.into();
        self.spinner = self.spinner.style(self.spinner_style.clone());
        self
    }

    /// Set the spinner animation speed multiplier (default 1.0).
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self.spinner = self.spinner.speed(speed);
        self
    }

    /// Port of `Status.update`. `None` (and, as upstream, a zero speed) leaves
    /// a field unchanged. A new spinner name builds a fresh spinner; otherwise
    /// the current one is updated in place, a speed change continuing from its
    /// current frame.
    pub fn update(
        &mut self,
        status: Option<&str>,
        spinner: Option<&str>,
        spinner_style: Option<StyleType>,
        speed: Option<f64>,
    ) {
        self.update_content(
            status.map(|status| SpinnerText::Markup(status.to_string())),
            spinner,
            spinner_style,
            speed,
        );
    }

    /// [`update`](Self::update) with the status as a [`Text`], used as it is.
    pub fn update_text(&mut self, status: Text) {
        self.update_content(Some(SpinnerText::Text(status)), None, None, None);
    }

    /// [`update`](Self::update) with the status as any renderable.
    pub fn update_renderable(&mut self, status: Box<dyn Renderable + Send>) {
        let status = SpinnerText::Renderable(Arc::new(Mutex::new(status)));
        self.update_content(Some(status), None, None, None);
    }

    fn update_content(
        &mut self,
        status: Option<SpinnerText>,
        spinner: Option<&str>,
        spinner_style: Option<StyleType>,
        speed: Option<f64>,
    ) {
        if let Some(status) = status {
            self.status = status;
        }
        if let Some(style) = spinner_style {
            self.spinner_style = style;
        }
        if let Some(speed) = speed.filter(|s| *s != 0.0) {
            self.speed = speed;
        }
        if let Some(name) = spinner {
            self.spinner = Spinner::new(name)
                .content(&self.status)
                .style(self.spinner_style.clone())
                .speed(self.speed);
        } else {
            self.spinner.update_content(&self.status);
            self.spinner
                .update(None, Some(self.spinner_style.clone()), Some(self.speed));
        }
    }

    /// The underlying spinner. Mirrors upstream's `Status.renderable`.
    pub fn renderable(&self) -> &Spinner {
        &self.spinner
    }
}

/// A [`Status`] shared with its display's refresh thread.
struct StatusView(Arc<Mutex<Status>>);

impl Renderable for StatusView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lock(&self.0).rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        lock(&self.0).measure(console, options)
    }
}

fn lock(status: &Mutex<Status>) -> std::sync::MutexGuard<'_, Status> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Status {
    /// Start the status animation in a transient live display, redrawn
    /// [`refresh_per_second`](Self::refresh_per_second) times a second on a
    /// background thread. Port of `Status.start` (upstream's
    /// `Live(transient=True)`, started without a first refresh, so the first
    /// frame comes with the first tick); stop it with [`LiveStatus::stop`].
    pub fn start<W: Write + Send + 'static>(self, console: Console, writer: W) -> LiveStatus<W> {
        let refresh_per_second = self.refresh_per_second;
        let shared = Arc::new(Mutex::new(self));
        let live = Live::spawn_options(
            Box::new(StatusView(shared.clone())),
            console,
            writer,
            refresh_per_second,
            LiveOptions {
                transient: true,
                refresh_on_start: false,
                ..LiveOptions::default()
            },
        );
        LiveStatus {
            status: shared,
            live,
        }
    }
}

/// A running [`Status`] display, returned by [`Status::start`].
pub struct LiveStatus<W: Write + Send + 'static> {
    status: Arc<Mutex<Status>>,
    live: AutoLive<W>,
}

impl<W: Write + Send + 'static> LiveStatus<W> {
    /// [`Status::update`] on the running status. As upstream, a new spinner
    /// redraws at once (and returns once drawn); other changes show at the
    /// next refresh.
    pub fn update(
        &self,
        status: Option<&str>,
        spinner: Option<&str>,
        spinner_style: Option<StyleType>,
        speed: Option<f64>,
    ) {
        lock(&self.status).update(status, spinner, spinner_style, speed);
        if spinner.is_some() {
            self.live.refresh_wait();
        }
    }

    /// Run `f` on the running status (for [`Status::update_text`] and
    /// [`Status::update_renderable`]); the change shows at the next refresh.
    pub fn with<R>(&self, f: impl FnOnce(&mut Status) -> R) -> R {
        f(&mut lock(&self.status))
    }

    /// Redraw now, returning once the frame is written (upstream's
    /// `status._live.refresh()`).
    pub fn refresh(&self) {
        self.live.refresh_wait();
    }

    /// Print `renderable` above the status (upstream's `status.console.print`).
    pub fn print(&self, renderable: Box<dyn Renderable + Send>) {
        self.live.print(renderable);
    }

    /// [`print`](Self::print) a string as console markup.
    pub fn print_str(&self, content: impl Into<String>) {
        self.live.print_str(content);
    }

    /// A writer standing in for stdout while the status runs; see
    /// [`AutoLive::stdout`].
    pub fn stdout(&self) -> FileProxy<LiveHandle<W>, std::io::Stdout> {
        self.live.stdout()
    }

    /// [`stdout`](Self::stdout) for stderr.
    pub fn stderr(&self) -> FileProxy<LiveHandle<W>, std::io::Stderr> {
        self.live.stderr()
    }

    /// A proxy for `stream` over any `file`.
    pub fn file_proxy<F: Write>(
        &self,
        stream: ProxyStream,
        file: F,
    ) -> FileProxy<LiveHandle<W>, F> {
        self.live.file_proxy(stream, file)
    }

    /// Stop the animation, erase the status, and return the output sink.
    /// Port of `Status.stop`.
    pub fn stop(self) -> W {
        self.live.stop()
    }
}

/// Upstream's `Status.__rich__` returns the spinner, so a status renders (and
/// measures) exactly as its spinner: the frame for the console's clock.
impl Renderable for Status {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.spinner.rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.spinner.measure(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(status: &Status) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(30)
            .no_color(false)
            .build()
            .render_to_string(status)
    }

    #[test]
    fn default_status_frame() {
        // Captured from real rich 15.0.0 (dots spinner, status.spinner=green, t=0).
        assert_eq!(
            render(&Status::new("Loading data")),
            "\x1b[32m⠋\x1b[0m Loading data"
        );
    }

    #[test]
    fn custom_spinner() {
        assert_eq!(
            render(&Status::new("Building").spinner("line")),
            "\x1b[32m-\x1b[0m Building"
        );
    }
}
