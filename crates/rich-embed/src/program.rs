//! [`ProgramEngine`]: a terminal browser (Carbonyl, Browsh, Chawan, w3m,
//! lynx) run in a terminal pane, as the web view's engine.

use std::ffi::OsString;
use std::io;

use crate::host::{Command, LocalPty, Notify, PtyHost};
use crate::pane::DEFAULT_SCROLLBACK;
use crate::term::TermCore;
use crate::web::{PageState, WebEngine, WebFrame, WebInput};

/// The environment variable [`ProgramEngine::detect`] reads first: a
/// browser and its arguments, separated by spaces (`"w3m -o
/// confirm_qq=false"`).
pub const BROWSER_VARIABLE: &str = "RICH_EMBED_BROWSER";

/// Terminal browsers [`ProgramEngine::detect`] looks for on `PATH`, in
/// order: (program, arguments before the address).
pub const KNOWN_BROWSERS: &[(&str, &[&str])] = &[
    ("carbonyl", &[]),
    ("cha", &[]),
    ("browsh", &["--startup-url"]),
    ("w3m", &[]),
    ("lynx", &[]),
];

type Hosts = Box<dyn FnMut(&str) -> Box<dyn PtyHost>>;

/// A terminal browser, named by the app, run in a terminal pane: its
/// screen is the page. Nothing of ours ships with it, and nothing is
/// downloaded; install the browser you want.
///
/// The browser is started with the address as its last argument. Its own
/// keys work as they do in a terminal (following links, its own history).
/// The view's back, forward and reload restart it at an address from the
/// engine's own history of what was opened; the address shown is the last
/// one opened, since a terminal browser does not say where its links led.
pub struct ProgramEngine {
    program: Option<(OsString, Vec<OsString>)>,
    hosts: Option<Hosts>,
    core: Option<TermCore>,
    notify: Option<Notify>,
    size: (u16, u16),
    history: Vec<String>,
    at: usize,
    /// Output has arrived since the browser was last started.
    shown: bool,
    error: Option<String>,
}

impl ProgramEngine {
    /// Run `program` with the address as its argument.
    pub fn new(program: impl Into<OsString>) -> ProgramEngine {
        ProgramEngine::with_program(Some((program.into(), Vec::new())))
    }

    fn with_program(program: Option<(OsString, Vec<OsString>)>) -> ProgramEngine {
        ProgramEngine {
            program,
            hosts: None,
            core: None,
            notify: None,
            size: (80, 24),
            history: Vec::new(),
            at: 0,
            shown: false,
            error: None,
        }
    }

    /// Arguments before the address.
    pub fn args<I: IntoIterator<Item = S>, S: Into<OsString>>(mut self, args: I) -> ProgramEngine {
        if let Some((_, own)) = &mut self.program {
            own.extend(args.into_iter().map(Into::into));
        }
        self
    }

    /// The browser in [`BROWSER_VARIABLE`], else the first of
    /// [`KNOWN_BROWSERS`] on `PATH`. With none, the view says so.
    pub fn detect() -> ProgramEngine {
        if let Some(value) = std::env::var_os(BROWSER_VARIABLE) {
            let value = value.to_string_lossy().into_owned();
            let mut words = value.split_whitespace().map(OsString::from);
            if let Some(program) = words.next() {
                return ProgramEngine::with_program(Some((program, words.collect())));
            }
        }
        for (program, args) in KNOWN_BROWSERS {
            if on_path(program) {
                return ProgramEngine::new(*program).args(args.iter().copied());
            }
        }
        ProgramEngine::with_program(None)
    }

    /// Run each page in a host `hosts` makes from its address, instead of
    /// a local program: a browser on another machine, or a test.
    pub fn with_hosts(hosts: impl FnMut(&str) -> Box<dyn PtyHost> + 'static) -> ProgramEngine {
        let mut engine = ProgramEngine::with_program(None);
        engine.hosts = Some(Box::new(hosts));
        engine
    }

    fn host(&mut self, url: &str) -> Option<Box<dyn PtyHost>> {
        if let Some(hosts) = &mut self.hosts {
            return Some(hosts(url));
        }
        let (program, args) = self.program.as_ref()?;
        let command = Command::new(program.clone()).args(args.clone()).arg(url);
        Some(Box::new(LocalPty::new(command)))
    }

    /// Start the browser at the history's current address.
    fn launch(&mut self) -> io::Result<()> {
        let url = self.history[self.at].clone();
        self.core = None;
        self.shown = false;
        let Some(host) = self.host(&url) else {
            let names: Vec<&str> = KNOWN_BROWSERS.iter().map(|(name, _)| *name).collect();
            self.error = Some(format!(
                "no terminal browser: install one of {}, or name one in {BROWSER_VARIABLE}",
                names.join(", ")
            ));
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                self.error.clone().unwrap_or_default(),
            ));
        };
        self.error = None;
        let mut core = TermCore::new(host, DEFAULT_SCROLLBACK);
        if let Some(notify) = &self.notify {
            core.set_notify(notify.clone());
        }
        core.fit(self.size.0, self.size.1);
        self.error = core.error.clone();
        self.core = Some(core);
        if let Some(notify) = &self.notify {
            notify();
        }
        Ok(())
    }
}

fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(program);
        candidate.is_file() || (cfg!(windows) && candidate.with_extension("exe").is_file())
    })
}

impl WebEngine for ProgramEngine {
    fn open(&mut self, url: &str) -> io::Result<()> {
        if !self.history.is_empty() {
            self.history.truncate(self.at + 1);
        }
        self.history.push(url.to_string());
        self.at = self.history.len() - 1;
        self.launch()
    }

    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        self.size = (columns, rows);
        if let Some(core) = &mut self.core {
            core.fit(columns, rows);
        }
        Ok(())
    }

    fn input(&mut self, input: WebInput) -> io::Result<()> {
        let Some(core) = &mut self.core else {
            return Ok(());
        };
        match input {
            WebInput::Key(key) => core.key(key),
            WebInput::Paste(text) => core.paste(&text),
            WebInput::Mouse(mouse) => {
                core.mouse(mouse);
            }
        }
        Ok(())
    }

    fn back(&mut self) -> io::Result<()> {
        if self.at == 0 || self.history.is_empty() {
            return Ok(());
        }
        self.at -= 1;
        self.launch()
    }

    fn forward(&mut self) -> io::Result<()> {
        if self.at + 1 >= self.history.len() {
            return Ok(());
        }
        self.at += 1;
        self.launch()
    }

    fn reload(&mut self) -> io::Result<()> {
        if self.history.is_empty() {
            return Ok(());
        }
        self.launch()
    }

    fn poll(&mut self) -> Option<WebFrame> {
        let core = self.core.as_mut()?;
        let pumped = core.pump();
        if !pumped.output && pumped.exit.is_none() {
            return None;
        }
        self.shown |= pumped.output;
        if let Some(exit) = pumped.exit {
            if !exit.success() {
                self.error = Some(format!("the browser {exit}"));
            }
        }
        Some(WebFrame::Cells {
            lines: core.lines(),
            cursor: core.cursor(),
        })
    }

    fn state(&self) -> PageState {
        let title = match &self.program {
            Some((program, _)) => program.to_string_lossy().into_owned(),
            None => String::new(),
        };
        PageState {
            url: self.history.get(self.at).cloned().unwrap_or_default(),
            title,
            loading: self.core.is_some() && !self.shown && self.error.is_none(),
            can_go_back: self.at > 0,
            can_go_forward: self.at + 1 < self.history.len(),
            error: self.error.clone(),
        }
    }

    fn set_notify(&mut self, notify: Notify) {
        if let Some(core) = &mut self.core {
            core.set_notify(notify.clone());
        }
        self.notify = Some(notify);
    }
}
