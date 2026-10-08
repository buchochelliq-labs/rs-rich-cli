//! The terminal session: raw mode, the alternate screen, mouse and paste
//! modes, the kitty keyboard protocol, and their restoration on every way
//! out (#489).
//!
//! Everything a [`Session`] turns on is recorded in a process-wide flag
//! set, and undone by whichever comes first: [`Session::leave`], dropping
//! the session (an early return, `?`), or a panic, through a hook installed
//! on first use that restores the terminal before the panic message prints
//! (a panic caught by [`catch_panic`], which the session survives, leaves
//! it alone).
//! Ctrl+C arrives as a key in raw mode, so it ends the event loop the
//! ordinary way. On Unix, SIGTERM, SIGHUP and SIGQUIT restore the terminal
//! too, from a thread that then takes the signal's default action. Ctrl+Z
//! (a key in raw mode) and SIGTSTP suspend: the terminal is given back, the
//! process stops as the shell expects, and on `fg` the modes come back and
//! the view repaints. [`Session::handoff`] gives the terminal to another program
//! (`$EDITOR`, a pager) and takes it back.
//!
//! The terminal's modes are process-wide, so only one session exists at a
//! time: starting a second while the first is alive (a component that
//! calls [`run`](crate::run) from inside another) fails with
//! [`io::ErrorKind::ResourceBusy`] and changes nothing.

use std::cell::Cell;
use std::io::{self, Write};
use std::panic::UnwindSafe;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

use crate::event::{from_crossterm, from_crossterm_kitty, Event};

const RAW: u8 = 1;
const ALTERNATE: u8 = 2;
const MOUSE: u8 = 4;
const PASTE: u8 = 8;
const KITTY: u8 = 16;

/// The kitty keyboard protocol's flags a session pushes: disambiguate the
/// keys a legacy terminal sends alike (1) and report releases and repeats
/// (2). The terminal keeps a stack of them for each screen, so they are
/// pushed after entering the alternate screen and popped before leaving it.
const PUSH_KITTY: &str = "\x1b[>3u";
const POP_KITTY: &str = "\x1b[<1u";

/// Whether the terminal answered the kitty keyboard protocol's query: 0 not
/// asked yet, 1 no, 2 yes. Asked once a process, since asking can wait for
/// a terminal that does not answer.
static KITTY_ANSWER: AtomicU8 = AtomicU8::new(0);

/// What is currently turned on, for the panic hook.
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static HOOK: Once = Once::new();
/// Whether a [`Session`] is alive. Held from `start` until drop.
static OWNED: AtomicBool = AtomicBool::new(false);
/// Whether the live session paints to standard error, for the panic hook.
static ON_STDERR: AtomicBool = AtomicBool::new(false);

/// Where a session paints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Output {
    /// Standard output.
    #[default]
    Stdout,
    /// Standard error, which leaves standard output to the answer a script
    /// captures: `choice=$(rich choose a b c)`. Keys still come from the
    /// terminal, which crossterm opens itself when standard input is a pipe.
    Stderr,
}

impl Output {
    /// Whether the stream is a terminal.
    pub fn is_terminal(self) -> bool {
        use std::io::IsTerminal;
        match self {
            Output::Stdout => io::stdout().is_terminal(),
            Output::Stderr => io::stderr().is_terminal(),
        }
    }

    fn write(self, text: &str) -> io::Result<()> {
        match self {
            Output::Stdout => {
                let mut out = io::stdout().lock();
                out.write_all(text.as_bytes())?;
                out.flush()
            }
            Output::Stderr => {
                let mut out = io::stderr().lock();
                out.write_all(text.as_bytes())?;
                out.flush()
            }
        }
    }
}

/// Whether keys can be read from a terminal: standard input, or the
/// process's controlling terminal when standard input is a pipe.
pub(crate) fn keys_available() -> bool {
    use std::io::IsTerminal;
    if io::stdin().is_terminal() {
        return true;
    }
    #[cfg(unix)]
    let terminal = "/dev/tty";
    #[cfg(windows)]
    let terminal = "CONIN$";
    #[cfg(any(unix, windows))]
    return std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(terminal)
        .is_ok();
    #[cfg(not(any(unix, windows)))]
    false
}

/// Undo whatever `ACTIVE` records. Safe to call twice.
fn restore() -> io::Result<()> {
    let active = ACTIVE.swap(0, Ordering::SeqCst);
    if active == 0 {
        return Ok(());
    }
    let out = undo(active);
    let output = if ON_STDERR.load(Ordering::SeqCst) {
        Output::Stderr
    } else {
        Output::Stdout
    };
    let written = output.write(&out);
    if active & RAW != 0 {
        crossterm::terminal::disable_raw_mode()?;
    }
    written
}

/// The sequences that turn off what `active` records.
fn undo(active: u8) -> String {
    let mut out = String::new();
    if active & KITTY != 0 {
        out.push_str(POP_KITTY);
    }
    if active & MOUSE != 0 {
        out.push_str("\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l");
    }
    if active & PASTE != 0 {
        out.push_str("\x1b[?2004l");
    }
    out.push_str("\x1b[?25h");
    if active & ALTERNATE != 0 {
        out.push_str("\x1b[?1049l");
    }
    out
}

/// Raw mode is on for a line read without echo
/// ([`StdLineIo::read_secret`](crate::policy::StdLineIo)), outside any
/// session: the signal thread turns it off too.
static RAW_LINE: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static SIGNALS: Once = Once::new();

/// Record that a line is being read in raw mode (`on`), or no longer is.
pub(crate) fn raw_line(on: bool) {
    if on {
        watch_signals();
    }
    RAW_LINE.store(on, Ordering::SeqCst);
}

/// SIGTERM, SIGHUP and SIGQUIT end the process without unwinding, so
/// neither `Drop` nor the panic hook would give the terminal back. From
/// the first session on, a thread waits for them: it restores whatever is
/// on, then takes the signal's default action (the process ends as it
/// would have). The handlers stay installed once the session ends, since
/// removing them would leave the signals ignored; with nothing on they
/// only take the default action.
fn watch_signals() {
    #[cfg(unix)]
    SIGNALS.call_once(|| {
        use signal_hook::consts::{SIGHUP, SIGQUIT, SIGTERM, SIGTSTP};
        let Ok(mut signals) =
            signal_hook::iterator::Signals::new([SIGTERM, SIGHUP, SIGQUIT, SIGTSTP])
        else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("rich-interact-signals".into())
            .spawn(move || {
                for signal in signals.forever() {
                    if signal == SIGTSTP {
                        suspend();
                        RESUMED.store(true, Ordering::SeqCst);
                    } else {
                        restore_for_signal();
                        let _ = signal_hook::low_level::emulate_default_handler(signal);
                    }
                }
            });
    });
}

/// [`restore`] from the signal thread. The sequences go straight to the
/// terminal rather than through the standard stream's lock, which the
/// interrupted thread may hold.
#[cfg(unix)]
fn restore_for_signal() {
    let active = ACTIVE.swap(0, Ordering::SeqCst);
    if active != 0 {
        write_direct(&undo(active));
    }
    let raw_line = RAW_LINE.swap(false, Ordering::SeqCst);
    if active & RAW != 0 || raw_line {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Set when the process came back from a suspend it did not start itself
/// (SIGTSTP from outside): [`Session::read`] then asks for a repaint.
static RESUMED: AtomicBool = AtomicBool::new(false);

/// Give the terminal back, stop the process as a shell's job control
/// expects (Ctrl+Z, `kill -TSTP`), and when it continues (`fg`, SIGCONT)
/// turn back on what was on. SIGSTOP cannot be caught, so it still stops
/// with the modes on.
/// Held for the whole of a suspend, from giving the terminal back until its
/// modes are on again, and by [`Session::leave`]. A resume from an outside
/// SIGTSTP runs on the signal thread while the event loop runs on, so
/// without it a session that ended during the stop (a key queued, a timer
/// due) would have its modes turned back on after it restored them.
static SUSPENDING: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(unix)]
fn suspend() {
    let _suspending = SUSPENDING.lock().unwrap_or_else(|e| e.into_inner());
    let active = ACTIVE.load(Ordering::SeqCst);
    let raw_line = RAW_LINE.load(Ordering::SeqCst);
    restore_for_signal();
    // Stops here until SIGCONT.
    let _ = signal_hook::low_level::emulate_default_handler(signal_hook::consts::SIGTSTP);
    if active & RAW != 0 || raw_line {
        let _ = crossterm::terminal::enable_raw_mode();
    }
    RAW_LINE.store(raw_line, Ordering::SeqCst);
    let mut out = String::new();
    if active & ALTERNATE != 0 {
        out.push_str("\x1b[?1049h\x1b[H");
    }
    if active & KITTY != 0 {
        out.push_str(PUSH_KITTY);
    }
    if active & MOUSE != 0 {
        out.push_str("\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h");
    }
    if active & PASTE != 0 {
        out.push_str("\x1b[?2004h");
    }
    if active != 0 {
        out.push_str("\x1b[?25l");
    }
    ACTIVE.fetch_or(active, Ordering::SeqCst);
    write_direct(&out);
}

/// `text` straight to the terminal, else to the session's stream.
#[cfg(unix)]
fn write_direct(text: &str) {
    if text.is_empty() {
        return;
    }
    let direct = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .and_then(|mut tty| tty.write_all(text.as_bytes()));
    if direct.is_err() {
        let output = if ON_STDERR.load(Ordering::SeqCst) {
            Output::Stderr
        } else {
            Output::Stdout
        };
        let _ = output.write(text);
    }
}

thread_local! {
    /// How many [`catch_panic`] calls are running on this thread.
    static CATCHING: Cell<usize> = const { Cell::new(0) };
}

/// [`std::panic::catch_unwind`] for code that runs inside a live session
/// and carries on after a panic (a plugin component's
/// [`PluginView`](crate::plugin::PluginView)): the session's panic hook
/// neither gives the terminal back nor prints the panic message over the
/// view for a panic caught here, since the session goes on. A panic nothing
/// catches still restores the terminal before its message prints.
pub fn catch_panic<R>(f: impl FnOnce() -> R + UnwindSafe) -> std::thread::Result<R> {
    CATCHING.with(|depth| depth.set(depth.get() + 1));
    let result = std::panic::catch_unwind(f);
    CATCHING.with(|depth| depth.set(depth.get() - 1));
    result
}

fn install_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Caught on this thread while a session is live: the session
            // runs on, and the catcher reports the panic itself.
            if CATCHING.with(Cell::get) > 0 && ACTIVE.load(Ordering::SeqCst) != 0 {
                return;
            }
            let _ = restore();
            previous(info);
        }));
    });
}

/// What a session turns on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionOptions {
    /// Take over the whole screen, and give it back as it was on exit.
    /// Without it, components paint inline, below the cursor.
    pub alternate_screen: bool,
    /// Report mouse clicks and the wheel.
    pub mouse: bool,
    /// Deliver pasted text as one [`Event::Paste`].
    pub bracketed_paste: bool,
    /// Where to paint.
    pub output: Output,
    /// Read keys as a legacy terminal sends them, even from a terminal with
    /// the kitty keyboard protocol. Without it, a terminal that answers the
    /// protocol's query has it turned on: Ctrl+I and Tab are told apart
    /// (see [`Key::matches`](crate::Key::matches)), and key releases arrive
    /// as [`Event::KeyUp`].
    pub legacy_keys: bool,
}

/// Where the event loop reads events and writes paints: the terminal, or
/// the [headless](crate::headless) driver.
pub trait Backend {
    /// (columns, rows).
    fn size(&self) -> (u16, u16);
    /// The next event, or `None` when `timeout` passes first. `None` for
    /// `timeout` waits as long as it takes.
    fn read(&mut self, timeout: Option<Duration>) -> io::Result<Option<Event>>;
    /// Write and flush.
    fn write(&mut self, text: &str) -> io::Result<()>;
    /// Time since the backend started (virtual in the headless driver).
    fn elapsed(&self) -> Duration;
    /// Run `command` with the terminal as it was before the session, then
    /// take the terminal back. Returns the exit code.
    fn handoff(&mut self, command: &mut Command) -> io::Result<Option<i32>>;
    /// Whether the region is the alternate screen (painted from the top).
    fn alternate_screen(&self) -> bool;
    /// The terminal row an inline region started on, for placing mouse
    /// events; 0 when unknown. The alternate screen always starts at 0.
    fn origin(&self) -> u16 {
        0
    }
    /// The plain text of a view just painted: the headless driver records
    /// it.
    fn painted(&mut self, text: &str) {
        let _ = text;
    }
    /// Whether Ctrl+Z suspends (a real terminal on Unix), rather than
    /// reaching the component as a key.
    fn can_suspend(&self) -> bool {
        false
    }
    /// Give the terminal back, stop until the shell continues the process,
    /// then take the terminal back. Only called when
    /// [`can_suspend`](Backend::can_suspend).
    fn suspend(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// Whether the process was suspended from outside (SIGTSTP) and has
    /// continued since the last call: the screen is then the shell's.
    fn take_resumed(&mut self) -> bool {
        false
    }
    /// Whether [`copy`](Backend::copy) reaches a clipboard (#488): `Ok`
    /// when it does, or why not. The default: it does not.
    fn clipboard(&self) -> Result<(), String> {
        Err("this backend has no clipboard".into())
    }
    /// Put `text` on the clipboard. Only called when
    /// [`clipboard`](Backend::clipboard) is `Ok`.
    fn copy(&mut self, text: &str) -> io::Result<()> {
        let _ = text;
        Ok(())
    }
}

/// The real terminal: keys from it, paints to standard output or standard
/// error ([`SessionOptions::output`]).
pub struct Session {
    options: SessionOptions,
    start: Instant,
    active: bool,
    /// The row the cursor was on when the session started (inline, with
    /// the mouse on).
    origin: u16,
    /// Whether the terminal takes OSC 52 copies, detected at the start.
    clipboard: rich_ext::clipboard::Clipboard,
    /// Whether keys are read with the kitty keyboard protocol.
    kitty: bool,
}

/// Whether the terminal has the kitty keyboard protocol. Asked with raw
/// mode on, once a process: a terminal that answers neither this query nor
/// the device attributes one after it keeps the session waiting (crossterm
/// gives up after two seconds).
fn kitty_answered() -> bool {
    match KITTY_ANSWER.load(Ordering::SeqCst) {
        0 => {
            let yes = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
            KITTY_ANSWER.store(if yes { 2 } else { 1 }, Ordering::SeqCst);
            yes
        }
        answer => answer == 2,
    }
}

impl Session {
    /// Turn on raw mode and the options. Fails, changing nothing, when the
    /// terminal cannot do raw mode or another session is alive.
    pub fn start(options: SessionOptions) -> io::Result<Session> {
        if OWNED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::ResourceBusy,
                "a terminal session is already running",
            ));
        }
        install_hook();
        watch_signals();
        // From here on, dropping the session releases ownership, including
        // when `enter` fails part way.
        let mut session = Session {
            options,
            start: Instant::now(),
            active: false,
            origin: 0,
            clipboard: rich_ext::clipboard::Clipboard::detect(
                &crate::clipboard::SessionEnvironment,
            ),
            kitty: false,
        };
        session.enter()?;
        Ok(session)
    }

    fn enter(&mut self) -> io::Result<()> {
        ON_STDERR.store(self.options.output == Output::Stderr, Ordering::SeqCst);
        crossterm::terminal::enable_raw_mode()?;
        ACTIVE.fetch_or(RAW, Ordering::SeqCst);
        // Active as soon as anything is on, so a failure below still
        // restores on drop.
        self.active = true;
        let mut out = String::new();
        if self.options.alternate_screen {
            out.push_str("\x1b[?1049h\x1b[H");
            ACTIVE.fetch_or(ALTERNATE, Ordering::SeqCst);
        }
        self.kitty = !self.options.legacy_keys && kitty_answered();
        if self.kitty {
            out.push_str(PUSH_KITTY);
            ACTIVE.fetch_or(KITTY, Ordering::SeqCst);
        }
        if self.options.mouse {
            out.push_str("\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h");
            ACTIVE.fetch_or(MOUSE, Ordering::SeqCst);
        }
        if self.options.bracketed_paste {
            out.push_str("\x1b[?2004h");
            ACTIVE.fetch_or(PASTE, Ordering::SeqCst);
        }
        out.push_str("\x1b[?25l");
        self.options.output.write(&out)?;
        // Where an inline region starts, so clicks land on the right rows.
        // Asking writes a query to standard output, so only when that is
        // the terminal being painted.
        if self.options.mouse
            && !self.options.alternate_screen
            && self.options.output == Output::Stdout
        {
            self.origin = crossterm::cursor::position().map_or(0, |(_, row)| row);
        }
        Ok(())
    }

    /// Restore the terminal. Also done on drop and on panic.
    pub fn leave(&mut self) -> io::Result<()> {
        if !std::mem::take(&mut self.active) {
            return Ok(());
        }
        // After any suspend under way has turned the modes back on, so
        // this restores them rather than being undone by it.
        let _suspending = SUSPENDING.lock().unwrap_or_else(|e| e.into_inner());
        restore()
    }

    pub fn options(&self) -> SessionOptions {
        self.options
    }

    /// Whether keys are read with the kitty keyboard protocol: the terminal
    /// has it and [`SessionOptions::legacy_keys`] is off. Every key is then
    /// [`exact`](crate::Key::exact), and releases arrive.
    pub fn kitty_keys(&self) -> bool {
        self.kitty
    }

    /// A resize to the current size, which repaints the whole view: after a
    /// suspend, what is on the screen is the shell's.
    fn repaint(&mut self) -> Event {
        if self.options.mouse
            && !self.options.alternate_screen
            && self.options.output == Output::Stdout
        {
            self.origin = crossterm::cursor::position().map_or(0, |(_, row)| row);
        }
        let (columns, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        Event::Resize { columns, rows }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.leave();
        OWNED.store(false, Ordering::SeqCst);
    }
}

impl Backend for Session {
    fn size(&self) -> (u16, u16) {
        crossterm::terminal::size().unwrap_or((80, 24))
    }

    fn read(&mut self, timeout: Option<Duration>) -> io::Result<Option<Event>> {
        let end = timeout.map(|timeout| Instant::now() + timeout);
        loop {
            // Back from a SIGTSTP sent from outside: the screen is the
            // shell's, so repaint all of it.
            if RESUMED.load(Ordering::SeqCst) {
                return Ok(Some(self.repaint()));
            }
            // Short waits, so a resume is noticed without a key.
            let wait = match end {
                Some(end) => end.saturating_duration_since(Instant::now()),
                None => Duration::from_secs(3600),
            }
            .min(Duration::from_millis(250));
            if !crossterm::event::poll(wait)? {
                if end.is_some_and(|end| Instant::now() >= end) {
                    return Ok(None);
                }
                continue;
            }
            // Events crossterm reports that components do not see (focus)
            // are skipped, not returned as a timeout.
            let event = crossterm::event::read()?;
            let event = if self.kitty {
                from_crossterm_kitty(event)
            } else {
                from_crossterm(event)
            };
            if let Some(event) = event {
                return Ok(Some(event));
            }
        }
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        self.options.output.write(text)
    }

    fn clipboard(&self) -> Result<(), String> {
        let field = self.clipboard.field();
        if field.value {
            Ok(())
        } else {
            Err(field.reason.clone())
        }
    }

    fn copy(&mut self, text: &str) -> io::Result<()> {
        let sequence = rich_ext::clipboard::osc52(text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        self.options.output.write(&sequence)
    }

    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    fn handoff(&mut self, command: &mut Command) -> io::Result<Option<i32>> {
        self.leave()?;
        let status = command.status();
        self.enter()?;
        Ok(status?.code())
    }

    fn alternate_screen(&self) -> bool {
        self.options.alternate_screen
    }

    fn origin(&self) -> u16 {
        self.origin
    }

    fn can_suspend(&self) -> bool {
        cfg!(unix)
    }

    fn suspend(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            suspend();
            self.repaint();
        }
        Ok(())
    }

    fn take_resumed(&mut self) -> bool {
        RESUMED.swap(false, Ordering::SeqCst)
    }
}
