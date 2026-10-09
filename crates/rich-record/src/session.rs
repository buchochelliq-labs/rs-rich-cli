//! A shell on a PTY, followed by a VT emulator, with a recording.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::screen::{Snapshot, Theme};
use crate::terminal::Terminal;

/// Frames are kept at most this often, per second: the video's frame rate.
/// Output that arrives faster is gathered into one event and one frame.
pub const FRAME_RATE: f64 = 12.0;
/// Frames are kept for this much recorded time. A longer recording still
/// has its screenshots and cast, but no video.
pub const MAX_VIDEO: Duration = Duration::from_secs(300);
/// Output gathered within one frame beyond this many bytes is recorded as a
/// repaint of the screen instead, so a program that floods the terminal
/// (`seq 1 1000000000`) costs a bounded amount of memory per frame.
const MAX_BATCH: usize = 32 * 1024;
/// The screens kept for [`Session::seen`], in bytes: past it the oldest go.
const MAX_SEEN: usize = 16 * 1024 * 1024;
/// Answers to the program's queries waiting to be written, in bytes: past
/// it, more are dropped, as by a terminal whose reply queue is full.
const MAX_QUEUED_REPLIES: usize = 4096;

/// What happened, and when, in the visible parts of a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Bytes the program wrote.
    Output(String),
    /// Bytes typed into the terminal.
    Input(String),
    /// The terminal was resized.
    Resize { columns: u16, rows: u16 },
}

/// The recorded timeline: seconds from the start, with hidden stretches cut.
#[derive(Clone, Debug, Default)]
pub struct Timeline {
    pub events: Vec<(f64, Event)>,
    /// The screen after each change.
    pub frames: Vec<(f64, Snapshot)>,
    /// Keys pressed, for the key overlay.
    pub keys: Vec<(f64, String)>,
    /// Whether frames after [`MAX_VIDEO`] were dropped: the timeline is too
    /// long for video.
    pub truncated: bool,
}

struct State {
    terminal: Terminal,
    theme: Theme,
    hidden: bool,
    hidden_since: Instant,
    hidden_total: Duration,
    start: Instant,
    utf8: Vec<u8>,
    /// Every screen shown since the last [`Session::mark`], so a `Wait`
    /// sees text that scrolled away between polls; at most [`MAX_SEEN`]
    /// bytes of them.
    seen: VecDeque<String>,
    seen_bytes: usize,
    /// Output decoded since the last frame, and when it last arrived.
    batch: String,
    /// Whether `batch` passed [`MAX_BATCH`] and was dropped for a repaint.
    overflow: bool,
    pending: Option<f64>,
    /// When the last frame was kept.
    last_frame: Option<f64>,
    timeline: Timeline,
    alive: bool,
    /// Why the emulator failed, if it did.
    error: Option<String>,
}

impl State {
    /// A hidden session's state, on a `columns` x `rows` screen.
    fn new(rows: u16, columns: u16, theme: Theme) -> State {
        let now = Instant::now();
        State {
            terminal: Terminal::new(rows, columns),
            theme,
            hidden: true,
            hidden_since: now,
            hidden_total: Duration::ZERO,
            start: now,
            utf8: Vec::new(),
            seen: VecDeque::new(),
            seen_bytes: 0,
            batch: String::new(),
            overflow: false,
            pending: None,
            last_frame: None,
            timeline: Timeline::default(),
            alive: true,
            error: None,
        }
    }

    /// Resize the screen, and record it. Output still queued was drawn at
    /// the old size, so it is recorded, with its frame, first.
    fn resize(&mut self, columns: u16, rows: u16) -> std::io::Result<()> {
        if !self.hidden {
            self.flush();
        }
        if let Err(error) = self.terminal.set_size(rows, columns) {
            self.error = Some(error.to_string());
            return Err(error);
        }
        if !self.hidden {
            let t = self.now();
            self.timeline
                .events
                .push((t, Event::Resize { columns, rows }));
            self.frame(t);
        }
        Ok(())
    }

    fn now(&self) -> f64 {
        (self.start.elapsed() - self.hidden_total).as_secs_f64()
    }

    fn frame(&mut self, t: f64) {
        self.last_frame = Some(t);
        if t > MAX_VIDEO.as_secs_f64() {
            self.timeline.truncated = true;
            return;
        }
        let snapshot = self.terminal.snapshot(&self.theme);
        if self
            .timeline
            .frames
            .last()
            .is_some_and(|(_, last)| *last == snapshot)
        {
            return;
        }
        self.timeline.frames.push((t, snapshot));
    }

    /// Remember a screen for [`Session::seen`].
    fn see(&mut self) {
        let screen = self.terminal.contents();
        if self.seen.back() == Some(&screen) {
            return;
        }
        self.seen_bytes += screen.len();
        self.seen.push_back(screen);
        while self.seen_bytes > MAX_SEEN && self.seen.len() > 1 {
            let old = self.seen.pop_front().expect("more than one screen");
            self.seen_bytes -= old.len();
        }
    }

    /// Queue output that arrived at `t`; it is recorded, with a frame, once
    /// a frame interval has passed since the last one, or before the next
    /// input, resize or hide.
    fn output(&mut self, t: f64, bytes: &[u8]) {
        self.utf8.extend_from_slice(bytes);
        let text = self.decode();
        if !self.overflow {
            self.batch.push_str(&text);
            if self.batch.len() > MAX_BATCH {
                self.overflow = true;
                self.batch = String::new();
            }
        }
        self.pending = Some(t);
        if self
            .last_frame
            .is_none_or(|last| t - last >= 1.0 / FRAME_RATE)
        {
            self.flush();
        }
    }

    /// Record the queued output and its frame.
    fn flush(&mut self) {
        let Some(t) = self.pending.take() else {
            return;
        };
        if self.overflow {
            // The screen after the flood, drawn from scratch: what a player
            // shows at this frame, without every line that scrolled past.
            self.overflow = false;
            let snapshot = self.terminal.snapshot(&self.theme);
            let repaint = crate::render::cast::repaint(&snapshot, &self.theme);
            self.timeline.events.push((t, Event::Output(repaint)));
        } else if !self.batch.is_empty() {
            let text = std::mem::take(&mut self.batch);
            self.timeline.events.push((t, Event::Output(text)));
        }
        self.frame(t);
    }

    /// Decode as much of `utf8` as forms whole characters.
    fn decode(&mut self) -> String {
        let valid = match std::str::from_utf8(&self.utf8) {
            Ok(_) => self.utf8.len(),
            // An incomplete sequence at the end waits for the next read.
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => self.utf8.len(),
        };
        let text = String::from_utf8_lossy(&self.utf8[..valid]).into_owned();
        self.utf8.drain(..valid);
        text
    }
}

/// What waits to be written to the program, by the writer thread: a
/// program that does not read its input blocks that thread only, never the
/// reader (whose output would stop) or [`Session::send`].
#[derive(Default)]
struct Outbox {
    /// Typed bytes, in order.
    input: Vec<u8>,
    /// Answers to the program's queries, at most [`MAX_QUEUED_REPLIES`].
    replies: Vec<u8>,
    /// Why a write failed: the writer has stopped, and sends report it.
    error: Option<(std::io::ErrorKind, String)>,
    /// The session is gone: the writer thread ends.
    closed: bool,
}

/// The [`Outbox`], shared by the session, the reader and the writer
/// thread, which waits on `ready`.
#[derive(Default)]
struct Outgoing {
    outbox: Mutex<Outbox>,
    ready: Condvar,
}

impl Outgoing {
    fn lock(&self) -> MutexGuard<'_, Outbox> {
        self.outbox.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queue answers to the program's queries; dropped when too many wait.
    fn reply(&self, replies: &[u8]) {
        let mut outbox = self.lock();
        if outbox.replies.len() + replies.len() <= MAX_QUEUED_REPLIES {
            outbox.replies.extend_from_slice(replies);
            self.ready.notify_one();
        }
    }

    /// Queue typed bytes; fails once a write has.
    fn send(&self, data: &[u8]) -> std::io::Result<()> {
        let mut outbox = self.lock();
        if let Some((kind, message)) = &outbox.error {
            return Err(std::io::Error::new(*kind, message.clone()));
        }
        outbox.input.extend_from_slice(data);
        self.ready.notify_one();
        Ok(())
    }

    fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_one();
    }

    /// The writer thread: typed bytes first, then answers, each written
    /// whole, until the session is gone or a write fails.
    fn run(&self, mut writer: Box<dyn Write + Send>) {
        loop {
            let bytes = {
                let mut outbox = self.lock();
                loop {
                    if outbox.closed {
                        return;
                    }
                    if !outbox.input.is_empty() {
                        break std::mem::take(&mut outbox.input);
                    }
                    if !outbox.replies.is_empty() {
                        break std::mem::take(&mut outbox.replies);
                    }
                    outbox = self
                        .ready
                        .wait(outbox)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            };
            if let Err(error) = writer.write_all(&bytes).and_then(|()| writer.flush()) {
                self.lock().error = Some((error.kind(), error.to_string()));
                return;
            }
        }
    }
}

/// A running shell session.
pub struct Session {
    state: Arc<Mutex<State>>,
    master: Box<dyn MasterPty + Send>,
    outgoing: Arc<Outgoing>,
    child: Box<dyn Child + Send + Sync>,
    /// Whether the child has been killed and waited for.
    reaped: bool,
}

/// Lock the state, even after a panic elsewhere left the lock poisoned.
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn check_size(columns: u16, rows: u16) -> std::io::Result<()> {
    if crate::tape::size_allowed(columns, rows) {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "a terminal of {columns}x{rows} is outside {}x{} to {}x{}",
            crate::tape::MIN_COLUMNS,
            crate::tape::MIN_ROWS,
            crate::tape::MAX_COLUMNS,
            crate::tape::MAX_ROWS
        ),
    ))
}

impl Session {
    /// Start `command` (an interactive shell) in `workspace`, with exactly
    /// the environment `env`.
    pub fn start(
        command: &[String],
        workspace: &Path,
        columns: u16,
        rows: u16,
        env: &[(String, String)],
        theme: Theme,
    ) -> std::io::Result<Session> {
        check_size(columns, rows)?;
        let pty = native_pty_system()
            .openpty(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(std::io::Error::other)?;
        let (program, args) = command.split_first().expect("a shell command");
        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.env_clear();
        for (key, value) in env {
            command.env(key, value);
        }
        command.cwd(workspace);
        let child = pty
            .slave
            .spawn_command(command)
            .map_err(std::io::Error::other)?;
        drop(pty.slave);
        let mut reader = pty
            .master
            .try_clone_reader()
            .map_err(std::io::Error::other)?;
        let writer = pty.master.take_writer().map_err(std::io::Error::other)?;
        let outgoing = Arc::new(Outgoing::default());
        let writing = Arc::clone(&outgoing);
        thread::spawn(move || writing.run(writer));
        let state = Arc::new(Mutex::new(State::new(rows, columns, theme)));
        let shared = Arc::clone(&state);
        let answers = Arc::clone(&outgoing);
        thread::spawn(move || {
            let mut buffer = [0u8; 65536];
            loop {
                let read = match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                let mut state = lock(&shared);
                if let Err(error) = state.terminal.process(&buffer[..read]) {
                    // The session fails; the tape runner reports it.
                    state.error = Some(error.to_string());
                    break;
                }
                state.see();
                if !state.hidden {
                    let t = state.now();
                    state.output(t, &buffer[..read]);
                }
                let replies = state.terminal.take_replies();
                drop(state);
                if !replies.is_empty() {
                    // Answers are not typing: they stay off the timeline.
                    answers.reply(&replies);
                }
            }
            lock(&shared).alive = false;
        });
        Ok(Session {
            state,
            master: pty.master,
            outgoing,
            child,
            reaped: false,
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Why the session failed (the terminal emulator panicked), if it did.
    pub fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Type `data`; `label` is shown by the key overlay. It is written to
    /// the program in order, without waiting for a program that is not
    /// reading; a write that failed is reported by the next send.
    pub fn send(&mut self, data: &str, label: Option<String>) -> std::io::Result<()> {
        {
            let mut state = self.lock();
            if !state.hidden {
                state.flush();
                let t = state.now();
                state
                    .timeline
                    .events
                    .push((t, Event::Input(data.to_string())));
                if let Some(label) = label {
                    state.timeline.keys.push((t, label));
                }
            }
        }
        self.outgoing.send(data.as_bytes())
    }

    pub fn resize(&mut self, columns: u16, rows: u16) -> std::io::Result<()> {
        check_size(columns, rows)?;
        self.master
            .resize(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(std::io::Error::other)?;
        self.lock().resize(columns, rows)
    }

    /// Forget the screens seen so far: the next [`Session::seen`] starts
    /// from the current screen.
    pub fn mark(&self) {
        let mut state = self.lock();
        let screen = state.terminal.contents();
        state.seen_bytes = screen.len();
        state.seen = VecDeque::from([screen]);
    }

    /// Whether `test` holds for the current screen or any screen shown since
    /// the last [`Session::mark`].
    pub fn seen(&self, test: impl Fn(&str) -> bool) -> bool {
        let state = self.lock();
        test(&state.terminal.contents()) || state.seen.iter().any(|screen| test(screen))
    }

    /// The screen's text, rows joined by line breaks.
    pub fn contents(&self) -> String {
        self.lock().terminal.contents()
    }

    pub fn alive(&self) -> bool {
        self.lock().alive
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.lock();
        state.terminal.snapshot(&state.theme)
    }

    pub fn hide(&self) {
        let mut state = self.lock();
        if !state.hidden {
            state.flush();
            state.hidden = true;
            state.hidden_since = Instant::now();
        }
    }

    /// Resume recording. The cast gets a repaint of the current screen, so
    /// a player shows what the hidden steps left behind.
    pub fn show(&self) {
        let mut state = self.lock();
        if state.hidden {
            let hidden = state.hidden_since.elapsed();
            state.hidden_total += hidden;
            state.hidden = false;
            state.utf8.clear();
            state.batch.clear();
            state.overflow = false;
            let t = state.now();
            let snapshot = state.terminal.snapshot(&state.theme);
            let repaint = crate::render::cast::repaint(&snapshot, &state.theme);
            state.timeline.events.push((t, Event::Output(repaint)));
            state.frame(t);
        }
    }

    /// Kill the shell, if it is still running, and reap it.
    fn stop(&mut self) {
        if !self.reaped {
            self.reaped = true;
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Stop the shell and return what was recorded.
    pub fn finish(mut self) -> Timeline {
        self.hide();
        self.stop();
        std::mem::take(&mut self.lock().timeline)
    }
}

impl Drop for Session {
    /// A session dropped on an error path still stops and reaps its shell.
    fn drop(&mut self) {
        self.stop();
        self.outgoing.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_text(snapshot: &Snapshot, row: usize) -> String {
        snapshot.rows[row]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect()
    }

    #[test]
    fn output_queued_before_a_resize_is_recorded_at_the_old_size() {
        let mut state = State::new(4, 20, Theme::default());
        state.hidden = false;
        let t = state.now();
        state.frame(t);
        // Within a frame interval of the last frame: queued, not recorded.
        state.terminal.process(b"hello").unwrap();
        let t = state.now();
        state.output(t, b"hello");
        assert!(state.pending.is_some());
        state.resize(30, 6).unwrap();

        let events = &state.timeline.events;
        let output = events
            .iter()
            .position(|(_, event)| *event == Event::Output("hello".into()))
            .expect("the output is recorded");
        let resize = events
            .iter()
            .position(|(_, event)| matches!(event, Event::Resize { .. }))
            .expect("the resize is recorded");
        assert!(output < resize, "{events:?}");
        assert!(events[output].0 <= events[resize].0, "{events:?}");

        // The output's frame is the old 20x4 screen; the resize's, 30x6.
        let frames = &state.timeline.frames;
        let (_, before) = &frames[frames.len() - 2];
        assert_eq!((before.rows.len(), before.rows[0].len()), (4, 20));
        assert!(row_text(before, 0).starts_with("hello"));
        let (_, after) = frames.last().unwrap();
        assert_eq!((after.rows.len(), after.rows[0].len()), (6, 30));
    }
}
