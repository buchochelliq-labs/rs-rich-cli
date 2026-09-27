//! A shell on a PTY, followed by a VT emulator, with a recording.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::screen::{Snapshot, Theme};

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
}

struct State {
    parser: vt100::Parser,
    theme: Theme,
    hidden: bool,
    hidden_since: Instant,
    hidden_total: Duration,
    start: Instant,
    utf8: Vec<u8>,
    timeline: Timeline,
    alive: bool,
}

impl State {
    fn now(&self) -> f64 {
        (self.start.elapsed() - self.hidden_total).as_secs_f64()
    }

    fn frame(&mut self, t: f64) {
        let snapshot = Snapshot::from_screen(self.parser.screen(), &self.theme);
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

/// A running shell session.
pub struct Session {
    state: Arc<Mutex<State>>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Session {
    /// Start `bash` (interactive, no profile or rc) in `workspace`, with
    /// exactly the environment `env`.
    pub fn start(
        workspace: &Path,
        columns: u16,
        rows: u16,
        env: &[(String, String)],
        theme: Theme,
    ) -> std::io::Result<Session> {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(std::io::Error::other)?;
        let mut command = CommandBuilder::new("bash");
        command.args(["--noprofile", "--norc", "-i"]);
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
        let now = Instant::now();
        let state = Arc::new(Mutex::new(State {
            parser: vt100::Parser::new(rows, columns, 0),
            theme,
            hidden: true,
            hidden_since: now,
            hidden_total: Duration::ZERO,
            start: now,
            utf8: Vec::new(),
            timeline: Timeline::default(),
            alive: true,
        }));
        let shared = Arc::clone(&state);
        thread::spawn(move || {
            let mut buffer = [0u8; 65536];
            loop {
                let read = match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                let mut state = shared.lock().expect("session state");
                state.parser.process(&buffer[..read]);
                if !state.hidden {
                    let t = state.now();
                    state.utf8.extend_from_slice(&buffer[..read]);
                    let text = state.decode();
                    if !text.is_empty() {
                        state.timeline.events.push((t, Event::Output(text)));
                    }
                    state.frame(t);
                }
            }
            shared.lock().expect("session state").alive = false;
        });
        Ok(Session {
            state,
            master: pty.master,
            writer,
            child,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("session state")
    }

    /// Type `data`; `label` is shown by the key overlay.
    pub fn send(&mut self, data: &str, label: Option<String>) -> std::io::Result<()> {
        {
            let mut state = self.lock();
            if !state.hidden {
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
        self.writer.write_all(data.as_bytes())?;
        self.writer.flush()
    }

    pub fn resize(&mut self, columns: u16, rows: u16) -> std::io::Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(std::io::Error::other)?;
        let mut state = self.lock();
        state.parser.screen_mut().set_size(rows, columns);
        if !state.hidden {
            let t = state.now();
            state
                .timeline
                .events
                .push((t, Event::Resize { columns, rows }));
            state.frame(t);
        }
        Ok(())
    }

    /// The screen's text, rows joined by line breaks.
    pub fn contents(&self) -> String {
        self.lock().parser.screen().contents()
    }

    pub fn alive(&self) -> bool {
        self.lock().alive
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.lock();
        Snapshot::from_screen(state.parser.screen(), &state.theme)
    }

    pub fn hide(&self) {
        let mut state = self.lock();
        if !state.hidden {
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
            let t = state.now();
            let snapshot = Snapshot::from_screen(state.parser.screen(), &state.theme);
            let repaint = crate::render::cast::repaint(&snapshot, &state.theme);
            state.timeline.events.push((t, Event::Output(repaint)));
            state.frame(t);
        }
    }

    /// Stop the shell and return what was recorded.
    pub fn finish(mut self) -> Timeline {
        self.hide();
        let _ = self.child.kill();
        let _ = self.child.wait();
        std::mem::take(&mut self.lock().timeline)
    }
}
