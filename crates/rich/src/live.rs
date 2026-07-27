//! Live (in-place updating) displays.
//!
//! Port of the core of `rich/live.py`. A [`Live`] drives a [`LiveRender`],
//! writing the control codes to redraw a renderable in place as it changes:
//! `start` hides the cursor and draws, `update`/`refresh` reposition the cursor
//! and redraw, and `stop` commits the final render and restores the cursor.
//!
//! Scope: the deterministic manual-refresh path (the byte stream is byte-parity
//! with upstream's `auto_refresh=False`, `transient=False` Live). The background
//! auto-refresh thread, alt-screen/transient modes, and IO redirection are
//! deferred (see the Live/progress issue).

use std::io::Write;

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
}
