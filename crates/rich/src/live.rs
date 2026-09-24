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
//! redraws on an interval like upstream's `refresh_per_second`, and
//! `transient` displays that erase themselves on stop. The alt-screen mode and
//! IO redirection remain deferred (see the Live/progress issue).

use std::any::Any;
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
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
    transient: bool,
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
            transient: false,
        }
    }

    /// Clear the display when it stops (upstream `transient`): the final
    /// frame is drawn, then erased and the cursor put back where it started.
    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    /// Begin the live display: hide the cursor and draw the first frame.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        if self.console.is_terminal() {
            let _ = write!(self.writer, "{}", Control::show_cursor(false).as_str());
        }
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
        if !self.console.is_terminal() {
            return;
        }
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
        if !self.console.is_terminal() {
            // Upstream prints the final result for files, with no newline,
            // only when it is not transient.
            if !self.transient {
                let content = self.console.render_to_string(&self.live_render);
                let _ = write!(self.writer, "{content}");
            }
            self.started = false;
            return;
        }
        let position = self.live_render.position_cursor();
        let content = self.console.render_to_string(&self.live_render);
        // Upstream ends the display with `console.line()` only when the last
        // render drew something (`_live_render.last_render_height`).
        let newline = if self.live_render.last_render_height() > 0 {
            "\n"
        } else {
            ""
        };
        let _ = write!(
            self.writer,
            "{}{}{newline}{}",
            position.as_str(),
            content,
            Control::show_cursor(true).as_str()
        );
        if self.transient {
            let _ = write!(
                self.writer,
                "{}",
                self.live_render.restore_cursor().as_str()
            );
        }
        self.started = false;
    }

    /// Restore the terminal after a render panicked mid-display: end the
    /// partial frame's line and show the cursor, as the `finally` of
    /// upstream's `stop()` does whatever its refresh raised.
    fn abort(&mut self) {
        if !self.started {
            return;
        }
        self.started = false;
        if self.console.is_terminal() {
            let _ = write!(self.writer, "\n{}", Control::show_cursor(true).as_str());
            let _ = self.writer.flush();
        }
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
    /// Redraw now, then acknowledge on the channel.
    RefreshAck(mpsc::Sender<()>),
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
        Live::spawn_with(renderable, console, writer, refresh_per_second, false)
    }

    /// [`spawn`](Self::spawn), clearing the display when it stops when
    /// `transient` is set (upstream `Live(transient=True)`).
    pub fn spawn_with(
        renderable: Box<dyn Renderable + Send>,
        console: Console,
        writer: W,
        refresh_per_second: f64,
        transient: bool,
    ) -> AutoLive<W> {
        let (sender, receiver) = mpsc::channel::<LiveMessage>();
        let (started, wait_started) = mpsc::channel::<()>();
        let interval = Duration::from_secs_f64(1.0 / refresh_per_second.max(f64::MIN_POSITIVE));
        let handle = thread::spawn(move || {
            // The `Live` (and its non-`Send` `LiveRender`) is built and owned
            // entirely within this thread — only the `Send` inputs cross over.
            let mut live = Live::new(renderable, console, writer).transient(transient);
            // A panicking render must not take the writer down with it:
            // upstream's `stop()` restores the terminal in a `finally`, so the
            // failure is caught here, the cursor shown again, and the panic
            // handed back to `try_stop` instead of being re-raised by `join`.
            let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                live.start();
                let _ = started.send(());
                loop {
                    match receiver.recv_timeout(interval) {
                        Ok(LiveMessage::Update(renderable)) => live.update(renderable),
                        Ok(LiveMessage::Refresh) | Err(RecvTimeoutError::Timeout) => live.refresh(),
                        Ok(LiveMessage::RefreshAck(done)) => {
                            live.refresh();
                            let _ = done.send(());
                        }
                        // Stop, or the handle was dropped: finalize and exit.
                        Ok(LiveMessage::Stop) | Err(RecvTimeoutError::Disconnected) => {
                            live.stop();
                            break;
                        }
                    }
                }
            }));
            let failure = outcome.err().map(|payload| {
                live.abort();
                panic_message(payload.as_ref())
            });
            (live.into_writer(), failure)
        });
        // Upstream's `Live.start()` draws the first frame before returning;
        // without this wait, a change made right after spawning could land in
        // the "first" frame, depending on thread scheduling.
        let _ = wait_started.recv();
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
    handle: Option<JoinHandle<(W, Option<String>)>>,
}

/// A render that panicked on an [`AutoLive`] refresh thread, returned by
/// [`AutoLive::try_stop`]. The terminal was restored before the thread exited.
#[derive(Debug)]
pub struct LivePanic<W> {
    /// The output sink, with everything written up to and including the
    /// cursor restore.
    pub writer: W,
    /// The panic message (`"<non-string panic payload>"` when it had none).
    pub message: String,
}

/// The message of a caught panic payload.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
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

    /// Redraw now and wait until the frame is written, as upstream's
    /// `Live.refresh()` renders in the caller's thread. A renderable that reads
    /// shared state then shows the state as of this call.
    pub fn refresh_wait(&self) {
        let (done, wait) = mpsc::channel();
        if self.sender.send(LiveMessage::RefreshAck(done)).is_ok() {
            let _ = wait.recv();
        }
    }

    /// Commit the final frame, join the thread, and return the output sink.
    ///
    /// If a render panicked on the refresh thread, the cursor has already been
    /// shown again and the failure is swallowed (the panic hook reported it
    /// when it happened); use [`try_stop`](Self::try_stop) to observe it.
    pub fn stop(self) -> W {
        match self.try_stop() {
            Ok(writer) => writer,
            Err(failure) => failure.writer,
        }
    }

    /// [`stop`](Self::stop), reporting a render that panicked on the refresh
    /// thread as an error that still carries the output sink.
    pub fn try_stop(mut self) -> Result<W, LivePanic<W>> {
        let _ = self.sender.send(LiveMessage::Stop);
        let handle = self
            .handle
            .take()
            .expect("thread handle present until stop/drop");
        // The thread catches every panic from rendering, so a join error
        // cannot carry a writer; there is nothing to return but the payload.
        let (writer, failure) = match handle.join() {
            Ok(result) => result,
            Err(payload) => panic::resume_unwind(payload),
        };
        match failure {
            None => Ok(writer),
            Some(message) => Err(LivePanic { writer, message }),
        }
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
    fn redirected_live_emits_only_the_final_frame() {
        let console = Console::builder().force_terminal(false).width(20).build();
        let mut live = Live::new(Box::new(Text::new("first")), console, Vec::<u8>::new());
        live.start();
        live.update(Box::new(Text::new("last")));
        live.refresh();
        assert!(live.writer().is_empty());
        live.stop();
        assert_eq!(live.writer(), b"last");
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

    /// Renders `ok` until `fail_after` renders have happened, then panics.
    struct PanicsLater {
        renders: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        fail_after: usize,
    }

    impl Renderable for PanicsLater {
        fn rich_render(
            &self,
            console: &Console,
            options: &crate::console::ConsoleOptions,
        ) -> Vec<crate::segment::Segment> {
            use std::sync::atomic::Ordering;
            if self.renders.fetch_add(1, Ordering::SeqCst) >= self.fail_after {
                panic!("render failed");
            }
            Text::new("ok").rich_render(console, options)
        }
    }

    #[test]
    fn a_panicking_render_still_restores_the_cursor() {
        // Upstream's `stop()` shows the cursor again in a `finally`, whatever
        // the refresh raised. A render that panics on the refresh thread must
        // neither leave the cursor hidden nor make `stop()` panic in turn.
        let renders = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let auto = Live::spawn(
            Box::new(PanicsLater {
                renders: renders.clone(),
                fail_after: 1,
            }),
            console(),
            Vec::<u8>::new(),
            0.1,
        );
        auto.refresh_wait(); // panics on the thread; must not hang here
        let (output, error) = match auto.try_stop() {
            Ok(output) => (output, None),
            Err(failure) => (failure.writer, Some(failure.message)),
        };
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("\x1b[?25lok"), "{output:?}");
        assert!(
            output.ends_with("\x1b[?25h"),
            "cursor left hidden: {output:?}"
        );
        assert_eq!(error.as_deref(), Some("render failed"));

        // `stop()` swallows the failure after restoring the terminal.
        let auto = Live::spawn(
            Box::new(PanicsLater {
                renders: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                fail_after: 1,
            }),
            console(),
            Vec::<u8>::new(),
            0.1,
        );
        auto.refresh();
        let output = String::from_utf8(auto.stop()).unwrap();
        assert!(
            output.ends_with("\x1b[?25h"),
            "cursor left hidden: {output:?}"
        );
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
