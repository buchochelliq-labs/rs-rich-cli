//! A shell on a PTY, followed by a VT emulator, with a recording.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::screen::{Packed, Snapshot, Theme};
use crate::terminal::Terminal;

/// The video's frame rate unless a tape sets one (`Set Framerate`), and how
/// often output is gathered into one event of the cast.
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
/// How far back, in seconds, the updates of one read are spread when the
/// read holds several: the longest the reader is taken to have been late.
/// A busy machine wakes the reader late, and the PTY then hands over every
/// update written meanwhile in one read; macOS's CI runners did, and kept
/// a fifth of the frames. A program that wrote a burst after a pause is
/// drawn over at most this long.
const MAX_CATCH_UP: f64 = 0.25;

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
    /// The screen after each settled update (output that read in full, no
    /// synchronized update open), packed. Of the updates within one frame
    /// of the video's rate only the last is kept, as only it would be
    /// shown.
    pub frames: Vec<(f64, Packed)>,
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
    /// When the last batch of output was recorded.
    last_frame: Option<f64>,
    /// When the last read arrived, while visible.
    last_read: Option<f64>,
    /// Frames kept a second, at most: the video's frame rate.
    frame_rate: f64,
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
            last_read: None,
            frame_rate: FRAME_RATE,
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
        self.capture(t);
    }

    /// The frame interval `at` falls in, counted from the first frame: the
    /// `n`th covers the `n`th frame time after it and the moment before.
    fn interval(&self, at: f64) -> f64 {
        let first = self.timeline.frames.first().map_or(at, |(first, _)| *first);
        ((at - first) * self.frame_rate - 1e-9).ceil()
    }

    /// Keep the screen as it is at `t` as a frame, unless it is the frame
    /// before. A frame within the same frame interval as the one before
    /// (counted from the first) replaces it, since only the later of the
    /// two would be shown; one at another size never does.
    fn capture(&mut self, t: f64) {
        if t > MAX_VIDEO.as_secs_f64() {
            self.timeline.truncated = true;
            return;
        }
        let snapshot = self.terminal.snapshot(&self.theme);
        let frames = &self.timeline.frames;
        let packed = Packed::pack(&snapshot, frames.last().map(|(_, last)| last));
        let Some((last_t, last)) = frames.last() else {
            self.timeline.frames.push((t, packed));
            return;
        };
        if *last == packed {
            return;
        }
        let resized = (last.columns(), last.height()) != (packed.columns(), packed.height());
        let replace = frames.len() > 1 && !resized && self.interval(*last_t) == self.interval(t);
        let frames = &mut self.timeline.frames;
        if replace {
            *frames.last_mut().expect("a frame") = (t, packed);
        } else {
            frames.push((t, packed));
        }
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

    /// Follow one read of the program's output, which arrived at `t` (none
    /// while hidden); `more` is whether more may have been waiting (the
    /// read filled the buffer). A read can hold several updates when the
    /// reader was late: each is drawn and kept as a frame of its own, the
    /// last at `t` and the others a frame apart before it, within the time
    /// since the last read (and [`MAX_CATCH_UP`]). An update is settled,
    /// and kept, when no synchronized update is open.
    fn receive(&mut self, t: Option<f64>, bytes: &[u8], more: bool) -> std::io::Result<()> {
        let Some(t) = t else {
            self.terminal.process(bytes)?;
            self.see();
            return Ok(());
        };
        let pieces = updates(bytes);
        // The frame intervals (counted from the first frame, as
        // [`State::capture`] counts them) that began since the last read,
        // or since [`MAX_CATCH_UP`] ago: one frame for each, the updates
        // there are allowing, at each interval's end and the last at `t`.
        let since = self
            .last_read
            .into_iter()
            .chain(self.timeline.frames.last().map(|(at, _)| *at))
            .fold(t - MAX_CATCH_UP, f64::max);
        let (first, now) = match self.timeline.frames.first() {
            Some((first, _)) => (*first, self.interval(t)),
            None => (t, 0.0),
        };
        let open = (now - self.interval(since)).max(0.0) as usize;
        let kept = open.clamp(1, pieces.len());
        // The pieces after which a frame is kept: the last of each of
        // `kept` even groups, so the last piece always is.
        let mut next = 0;
        for (index, piece) in pieces.iter().enumerate() {
            self.terminal.process(piece)?;
            self.see();
            let last = index + 1 == pieces.len();
            let frame = (next + 1) * pieces.len() / kept - 1;
            if index < frame {
                continue;
            }
            next += 1;
            if (last && more) || self.terminal.in_synchronized_update() {
                continue;
            }
            let at = if next == kept {
                t
            } else {
                first + (now - (kept - next) as f64) / self.frame_rate
            };
            self.capture(at);
        }
        self.output(t, bytes);
        self.last_read = Some(t);
        Ok(())
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
/// One read of output split into the updates it holds. A program starts
/// each update the same way: a carriage return that redraws a line (not
/// one ending it), the cursor sent home, the screen cleared, the cursor
/// hidden, or a synchronized update begun. The first of these in the read
/// is taken as the program's, and each later copy of it starts an update.
/// Bytes before the first copy finish the update the last read left.
fn updates(bytes: &[u8]) -> Vec<&[u8]> {
    let Some((start, opener)) = (0..bytes.len()).find_map(|at| opener(bytes, at).map(|o| (at, o)))
    else {
        return vec![bytes];
    };
    let mut cuts = Vec::new();
    if start > 0 {
        cuts.push(start);
    }
    let mut at = start + opener.len();
    while at < bytes.len() {
        if bytes[at..].starts_with(opener) && opener_at(bytes, at, opener) {
            cuts.push(at);
            at += opener.len();
        } else {
            at += 1;
        }
    }
    let mut pieces = Vec::with_capacity(cuts.len() + 1);
    let mut from = 0;
    for cut in cuts {
        pieces.push(&bytes[from..cut]);
        from = cut;
    }
    pieces.push(&bytes[from..]);
    pieces
}

/// The update-starting sequence at `at`, if one is there (see [`updates`]).
fn opener(bytes: &[u8], at: usize) -> Option<&[u8]> {
    const OPENERS: [&[u8]; 8] = [
        b"\x1b[H",
        b"\x1b[;H",
        b"\x1b[1H",
        b"\x1b[1;1H",
        b"\x1b[2J",
        b"\x1b[?25l",
        b"\x1b[?2026h",
        b"\r",
    ];
    OPENERS
        .into_iter()
        .find(|seq| bytes[at..].starts_with(seq) && opener_at(bytes, at, seq))
}

/// Whether `seq`, found at `at`, starts an update there: a carriage
/// return does only when another character, not a line feed, follows it.
fn opener_at(bytes: &[u8], at: usize, seq: &[u8]) -> bool {
    seq != b"\r" || bytes.get(at + 1).is_some_and(|next| *next != b'\n')
}

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
                let t = (!state.hidden).then(|| state.now());
                // Settled only if the read did not fill the buffer (no more
                // was waiting).
                if let Err(error) = state.receive(t, &buffer[..read], read == buffer.len()) {
                    // The session fails; the tape runner reports it.
                    state.error = Some(error.to_string());
                    break;
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

    /// Keep at most `fps` frames a second (the video's frame rate): of the
    /// screen updates within one frame only the last is kept.
    pub fn set_frame_rate(&self, fps: f64) {
        self.lock().frame_rate = fps.clamp(1.0, 240.0);
    }

    /// Report the kitty keyboard protocol to the program, so that it can
    /// turn it on ([`Terminal::set_kitty_keyboard`]).
    pub fn set_kitty_keyboard(&self, on: bool) {
        self.lock().terminal.set_kitty_keyboard(on);
    }

    /// The kitty keyboard protocol's flags the program has in force
    /// ([`Terminal::keyboard_flags`]).
    pub fn keyboard_flags(&self) -> u8 {
        self.lock().terminal.keyboard_flags()
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
        let before = frames[frames.len() - 2].1.snapshot();
        assert_eq!((before.rows.len(), before.rows[0].len()), (4, 20));
        assert!(row_text(&before, 0).starts_with("hello"));
        let after = frames.last().unwrap().1.snapshot();
        assert_eq!((after.rows.len(), after.rows[0].len()), (6, 30));
    }

    #[test]
    fn a_read_is_split_into_the_updates_it_holds() {
        let split = |bytes: &[u8]| -> Vec<Vec<u8>> {
            updates(bytes).into_iter().map(<[u8]>::to_vec).collect()
        };
        assert_eq!(
            split(b"\r001\r002\r003"),
            [&b"\r001"[..], b"\r002", b"\r003"]
        );
        // The end of the update the last read left comes first.
        assert_eq!(split(b"1\r002"), [&b"1"[..], b"\r002"]);
        // Lines are one update: a carriage return ending a line starts none.
        assert_eq!(split(b"a\r\nb\r\nc\r\n"), [&b"a\r\nb\r\nc\r\n"[..]]);
        // The program's own opener: here the cursor home, not the CR.
        let frames = b"\x1b[Hab\r\ncd\x1b[Hef\r\ngh";
        assert_eq!(split(frames), [&b"\x1b[Hab\r\ncd"[..], b"\x1b[Hef\r\ngh"]);
        let sync = b"\x1b[?2026h1\x1b[?2026l\x1b[?2026h2\x1b[?2026l";
        assert_eq!(split(sync).len(), 2);
        assert_eq!(
            split(b"plain \x1b[1mtext\x1b[0m"),
            [&b"plain \x1b[1mtext\x1b[0m"[..]]
        );
    }

    /// Updates written every `every` seconds, read in chunks every `wake`
    /// seconds (a reader woken late), at `fps`: the frames kept.
    fn late_reads(updates: usize, every: f64, wake: f64, fps: f64) -> Vec<f64> {
        let mut state = State::new(2, 10, Theme::default());
        state.hidden = false;
        state.frame_rate = fps;
        state.frame(0.0);
        let mut written = 0;
        let mut now = wake;
        while written < updates {
            let mut chunk = Vec::new();
            while written < updates && (written + 1) as f64 * every <= now {
                written += 1;
                chunk.extend_from_slice(format!("\r{written:03}").as_bytes());
            }
            if !chunk.is_empty() {
                state.receive(Some(now), &chunk, false).unwrap();
            }
            now += wake;
        }
        state.timeline.frames[1..].iter().map(|(t, _)| *t).collect()
    }

    #[test]
    fn late_reads_keep_the_frames_they_hold() {
        // The frame-rate test's tape: 75 updates 20ms apart at 35 frames a
        // second. A prompt reader keeps about 35 a second...
        let prompt = late_reads(75, 0.02, 0.02, 35.0);
        // ...and so does one woken only every 130ms (6 or 7 updates a
        // read), as on macOS's CI runners, which kept 7 frames over 0.8s.
        for wake in [0.05, 0.13, 0.2] {
            let late = late_reads(75, 0.02, wake, 35.0);
            let span = late.last().unwrap() - late.first().unwrap();
            let rate = (late.len() - 1) as f64 / span;
            assert!(
                late.len() as f64 >= 0.8 * prompt.len() as f64 && rate <= 35.0 * 1.1,
                "woken every {wake}s: {} frames over {span:.2}s, {} when prompt",
                late.len(),
                prompt.len()
            );
            // In order, and every one a different screen.
            assert!(late.windows(2).all(|w| w[0] < w[1]), "{late:?}");
        }
        // Never more frames than the rate: 75 updates at 12 a second.
        let slow = late_reads(75, 0.02, 0.13, 12.0);
        let span = slow.last().unwrap() - slow.first().unwrap();
        assert!((slow.len() - 1) as f64 / span <= 12.0 * 1.1, "{slow:?}");
    }

    #[test]
    fn a_burst_after_a_pause_is_drawn_briefly() {
        let mut state = State::new(2, 10, Theme::default());
        state.hidden = false;
        state.frame_rate = 10.0;
        state.frame(0.0);
        // A hundred updates in one read, 5s after the last: at most
        // MAX_CATCH_UP of them, before the read.
        let burst: String = (1..=100).map(|i| format!("\r{i:03}")).collect();
        state.receive(Some(5.0), burst.as_bytes(), false).unwrap();
        let times: Vec<f64> = state.timeline.frames[1..].iter().map(|f| f.0).collect();
        assert_eq!(times.len(), 2, "{times:?}");
        assert!(
            times[0] >= 5.0 - MAX_CATCH_UP && times[1] == 5.0,
            "{times:?}"
        );
        assert!(
            row_text(&state.timeline.frames.last().unwrap().1.snapshot(), 0).starts_with("100")
        );
        // Neither a read that filled the buffer (more was waiting) nor a
        // synchronized update still open is settled: no frame until the
        // rest arrives.
        let before = state.timeline.frames.len();
        state.receive(Some(5.01), b"\rabc", true).unwrap();
        state
            .receive(Some(5.02), b"\x1b[?2026h\rxyz", false)
            .unwrap();
        assert_eq!(state.timeline.frames.len(), before);
        state.receive(Some(5.03), b"\x1b[?2026l", false).unwrap();
        assert_eq!(state.timeline.frames.len(), before + 1);
        assert!(
            row_text(&state.timeline.frames.last().unwrap().1.snapshot(), 0).starts_with("xyz")
        );
    }

    #[test]
    fn updates_within_a_frame_keep_only_the_last() {
        let mut state = State::new(2, 10, Theme::default());
        state.hidden = false;
        state.frame_rate = 10.0;
        let draw = |state: &mut State, t: f64, text: &str| {
            state.terminal.process(text.as_bytes()).unwrap();
            state.capture(t);
        };
        draw(&mut state, 0.0, "a");
        draw(&mut state, 0.01, "b");
        draw(&mut state, 0.05, "c");
        draw(&mut state, 0.15, "d");
        // An update that changes nothing is no frame.
        draw(&mut state, 0.16, "");
        let times: Vec<f64> = state.timeline.frames.iter().map(|(t, _)| *t).collect();
        assert_eq!(times, [0.0, 0.05, 0.15]);
        assert!(row_text(&state.timeline.frames[1].1.snapshot(), 0).starts_with("abc"));
    }
}
