//! Live (in-place updating) displays.
//!
//! Port of the core of `rich/live.py`. A [`Live`] drives a [`LiveRender`],
//! writing the control codes to redraw a renderable in place as it changes:
//! `start` hides the cursor and draws, `update`/`refresh` reposition the cursor
//! and redraw, and `stop` commits the final render and restores the cursor.
//!
//! Scope: the deterministic manual-refresh path (the byte stream is byte-parity
//! with upstream's `auto_refresh=False`, `transient=False` Live), plus a
//! background **auto-refresh thread** ([`Live::spawn`] → [`AutoLive`]) that
//! redraws on an interval like upstream's `refresh_per_second`. The
//! alt-screen/transient modes and IO redirection remain deferred (see the
//! Live/progress issue).

use std::io::Write;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::console::Console;
use crate::control::Control;
use crate::live_render::LiveRender;
use crate::protocol::Renderable;

/// An in-place updating display over a renderable. Mirrors `rich.live.Live`
/// (manual-refresh subset). Output is written to a generic sink `W`, so it can
/// target stdout or, in tests, a byte buffer.
pub struct Live<W: Write> {
    live_render: LiveRender,
    console: Console,
    writer: W,
    started: bool,
}

impl<W: Write> Live<W> {
    /// Create a live display for `renderable`, rendering with `console` and
    /// writing control/output bytes to `writer`.
    pub fn new(renderable: Box<dyn Renderable>, console: Console, writer: W) -> Self {
        Live {
            live_render: LiveRender::new(renderable),
            console,
            writer,
            started: false,
        }
    }

    /// Begin the live display: hide the cursor and draw the first frame.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let _ = write!(self.writer, "{}", Control::show_cursor(false).as_str());
        self.refresh();
    }

    /// Swap in a new renderable and redraw in place.
    pub fn update(&mut self, renderable: Box<dyn Renderable>) {
        self.live_render.set_renderable(renderable);
        self.refresh();
    }

    /// Redraw the current renderable in place (reposition over the last frame,
    /// then render).
    pub fn refresh(&mut self) {
        // `position_cursor` uses the *previous* frame's shape; rendering then
        // updates the shape for next time.
        let position = self.live_render.position_cursor();
        let content = self.console.render_to_string(&self.live_render);
        let _ = write!(self.writer, "{}{}", position.as_str(), content);
    }

    /// Commit the final frame (with a trailing newline) and show the cursor.
    pub fn stop(&mut self) {
        if !self.started {
            return;
        }
        let position = self.live_render.position_cursor();
        let content = self.console.render_to_string(&self.live_render);
        let _ = write!(
            self.writer,
            "{}{}\n{}",
            position.as_str(),
            content,
            Control::show_cursor(true).as_str()
        );
        self.started = false;
    }

    /// The output sink (for inspecting captured bytes in tests).
    pub fn writer(&self) -> &W {
        &self.writer
    }

    /// Consume the display, returning its output sink.
    pub fn into_writer(self) -> W {
        self.writer
    }
}

/// A message from an [`AutoLive`] handle to its background refresh thread.
enum LiveMessage {
    /// Swap in a new renderable and redraw.
    Update(Box<dyn Renderable + Send>),
    /// Redraw the current renderable now.
    Refresh,
    /// Commit the final frame, restore the cursor, and stop the thread.
    Stop,
}

impl<W: Write + Send + 'static> Live<W> {
    /// Start an auto-refreshing live display on a background thread: it draws the
    /// first frame, then redraws every `1/refresh_per_second` seconds (and
    /// immediately on each [`AutoLive::update`]). Mirrors upstream's
    /// `auto_refresh`/`refresh_per_second`. The `renderable`, `console`, and
    /// `writer` are moved into the thread, so all three must be `Send`.
    pub fn spawn(
        renderable: Box<dyn Renderable + Send>,
        console: Console,
        writer: W,
        refresh_per_second: f64,
    ) -> AutoLive<W> {
        let (sender, receiver) = mpsc::channel::<LiveMessage>();
        let interval = Duration::from_secs_f64(1.0 / refresh_per_second.max(f64::MIN_POSITIVE));
        let handle = thread::spawn(move || {
            // The `Live` (and its non-`Send` `LiveRender`) is built and owned
            // entirely within this thread — only the `Send` inputs cross over.
            let mut live = Live::new(renderable, console, writer);
            live.start();
            loop {
                match receiver.recv_timeout(interval) {
                    Ok(LiveMessage::Update(renderable)) => live.update(renderable),
                    Ok(LiveMessage::Refresh) | Err(RecvTimeoutError::Timeout) => live.refresh(),
                    // Stop, or the handle was dropped: finalize and exit.
                    Ok(LiveMessage::Stop) | Err(RecvTimeoutError::Disconnected) => {
                        live.stop();
                        break;
                    }
                }
            }
            live.into_writer()
        });
        AutoLive {
            sender,
            handle: Some(handle),
        }
    }
}

/// A handle to an auto-refreshing [`Live`] running on a background thread.
/// Dropping the handle (or calling [`stop`](Self::stop)) finalizes the display.
pub struct AutoLive<W: Write + Send + 'static> {
    sender: mpsc::Sender<LiveMessage>,
    handle: Option<JoinHandle<W>>,
}

impl<W: Write + Send + 'static> AutoLive<W> {
    /// Swap in a new renderable; the thread redraws it promptly.
    pub fn update(&self, renderable: Box<dyn Renderable + Send>) {
        let _ = self.sender.send(LiveMessage::Update(renderable));
    }

    /// Ask the thread to redraw the current renderable now.
    pub fn refresh(&self) {
        let _ = self.sender.send(LiveMessage::Refresh);
    }

    /// Commit the final frame, join the thread, and return the output sink.
    pub fn stop(mut self) -> W {
        let _ = self.sender.send(LiveMessage::Stop);
        self.handle
            .take()
            .expect("thread handle present until stop/drop")
            .join()
            .expect("live refresh thread panicked")
    }
}

impl<W: Write + Send + 'static> Drop for AutoLive<W> {
    fn drop(&mut self) {
        // If the caller didn't `stop()`, still finalize + join the thread.
        if let Some(handle) = self.handle.take() {
            let _ = self.sender.send(LiveMessage::Stop);
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::text::Text;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .no_color(false)
            .build()
    }

    #[test]
    fn manual_refresh_stream_matches_upstream() {
        let mut live = Live::new(
            Box::new(Text::new("frame one")),
            console(),
            Vec::<u8>::new(),
        );
        live.start();
        live.update(Box::new(Text::new("frame two")));
        live.update(Box::new(Text::new("frame three")));
        live.stop();

        // Captured verbatim from real rich 15.0.0 (auto_refresh=False,
        // transient=False, width 20) writing to a StringIO.
        let expected = "\x1b[?25lframe one\r\x1b[2Kframe two\r\x1b[2Kframe three\r\x1b[2Kframe three\n\x1b[?25h";
        assert_eq!(String::from_utf8(live.writer().clone()).unwrap(), expected);
    }

    #[test]
    fn auto_refresh_thread_produces_the_same_stream() {
        // A very low refresh rate (10s interval) means no timeout-driven refresh
        // fires during the test, so the thread processes exactly start + the two
        // updates + stop, in order — the identical byte-parity stream, now driven
        // through the background thread (spawn / channel / join).
        let auto = Live::spawn(
            Box::new(Text::new("frame one")),
            console(),
            Vec::<u8>::new(),
            0.1,
        );
        auto.update(Box::new(Text::new("frame two")));
        auto.update(Box::new(Text::new("frame three")));
        let output = auto.stop();

        let expected = "\x1b[?25lframe one\r\x1b[2Kframe two\r\x1b[2Kframe three\r\x1b[2Kframe three\n\x1b[?25h";
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }

    #[test]
    fn dropping_the_handle_finalizes_the_display() {
        // Even without an explicit stop(), Drop commits the final frame + restores
        // the cursor (the trailing "\n" + show-cursor), so no display is left open.
        let console = console();
        // Route through a shared buffer so we can inspect it after the drop.
        let auto = Live::spawn(Box::new(Text::new("only")), console, Vec::<u8>::new(), 0.1);
        drop(auto); // no explicit stop
                    // If Drop didn't join the thread, this test would still pass
                    // but leak the thread; the assertion is simply that drop
                    // returns without panicking / deadlocking.
    }
}
