//! Cornacai as an intuiTUIve app: [`app`] builds it from [`Options`].
//!
//! The screen, top to bottom: the sessions' tabs and the agent's name; the
//! active session's conversation, which follows its end while the agent
//! writes until the reader scrolls up; its plan; the prompt editor; the
//! key hints. The shell pane (Ctrl+T) opens on the right.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use rich_acp::replay::{FakeAgent, ReplayError, Transcript};
use rich_acp::{
    AuthMethod, ClientCapabilities, Connection, CreateTerminalRequest, Implementation,
    InitializeRequest, LoadSessionRequest, NewSessionRequest, NewSessionResponse,
    PermissionOptionKind, PlanEntryStatus, PromptRequest, RequestPermissionRequest,
    RequestPermissionResponse, SessionId, SessionNotification, StopReason, ToolCall,
    ToolCallContent, ToolCallStatus, ToolKind, Workspace,
};
use rich_embed::TerminalPane;
use rich_intuituive::files::path_completer;
use rich_intuituive::prelude::*;
use rich_intuituive::widgets::{Decision, DiffOptions, accepted_text, diff_view_with};
use rich_intuituive::{
    Driver, Origin, Status, button, collapsible_with, history_session, markdown, switch,
};

use crate::bridge::{Bridge, Review, display};
use crate::clean::{clean, markup, reveal};
use crate::mention::prompt_request;
use crate::session::{Entry, Session};
use crate::store::{SessionStore, StoredSession, ago, now};
use crate::terminals::AgentTerminals;

/// How Cornacai reaches its agent.
pub enum Launch {
    /// Start this command (the program and its arguments) and speak ACP
    /// on its standard input and output.
    Command(Vec<String>),
    /// Replay a recorded transcript with rs-rich-acp's fake agent: tests,
    /// tapes and demonstrations, with no agent installed. Each step the
    /// client must send waits `timeout` for it, or for ever with `None`.
    Replay {
        /// The recording.
        transcript: Transcript,
        /// How long a client step waits.
        timeout: Option<Duration>,
    },
}

/// Builds the shell pane, on the app's thread.
pub type ShellPane = Box<dyn FnOnce() -> TerminalPane>;

/// What [`app`] runs with.
pub struct Options {
    /// The agent.
    pub launch: Launch,
    /// The workspace: the sessions' working directory and the root the
    /// agent's reads and writes are kept inside.
    pub cwd: PathBuf,
    /// Offer `fs/read_text_file` (answered without asking).
    pub read: bool,
    /// Offer `fs/write_text_file` (each write reviewed as a diff).
    pub write: bool,
    /// Offer `terminal/*` (each command asked about).
    pub terminal: bool,
    /// The shell pane, if there is one.
    pub shell: Option<ShellPane>,
    /// Run the agent's `terminal/*` commands on a PTY (colour, programs
    /// that want a terminal), falling back to pipes for a command whose
    /// PTY cannot be opened. Off: pipes.
    pub pty: bool,
    /// Where the sessions opened are kept, so one can be reopened
    /// (Ctrl+O). `None`: nothing is kept.
    pub store: Option<SessionStore>,
    /// Reopen this session (`session/load`) instead of opening a new one,
    /// when the agent offers it.
    pub resume: Option<String>,
}

impl Options {
    /// `launch` in `cwd`, reads answered, writes and terminals asking, no
    /// shell pane.
    pub fn new(launch: Launch, cwd: impl Into<PathBuf>) -> Options {
        Options {
            launch,
            cwd: cwd.into(),
            read: true,
            write: true,
            terminal: true,
            shell: None,
            pty: false,
            store: None,
            resume: None,
        }
    }

    /// Keep the sessions opened in `store`.
    #[must_use]
    pub fn session_store(mut self, store: SessionStore) -> Options {
        self.store = Some(store);
        self
    }

    /// Reopen session `id` at the start, instead of opening a new one.
    #[must_use]
    pub fn resume(mut self, id: impl Into<String>) -> Options {
        self.resume = Some(id.into());
        self
    }

    /// Run the agent's terminal commands on a PTY when `on`.
    #[must_use]
    pub fn pty(mut self, on: bool) -> Options {
        self.pty = on;
        self
    }

    /// A shell pane running `command` on a PTY ([`rich_embed::LocalPty`]),
    /// started in the workspace (`cwd`), as the agent is.
    #[must_use]
    pub fn shell_command(mut self, command: Vec<String>) -> Options {
        let cwd = self.cwd.clone();
        self.shell = Some(Box::new(move || {
            rich_embed::terminal(rich_embed::Command::from(command).cwd(cwd))
        }));
        self
    }

    /// A shell pane built by `pane` (a [`rich_embed::terminal_with`] over
    /// any host).
    #[must_use]
    pub fn shell_pane(mut self, pane: impl FnOnce() -> TerminalPane + 'static) -> Options {
        self.shell = Some(Box::new(pane));
        self
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities::none()
            .read_files(self.read)
            .write_files(self.write)
            .terminals(self.terminal)
    }
}

/// What the app and the thread that connects share.
pub(crate) struct Hub {
    connection: OnceLock<Connection>,
    fake: Mutex<Option<FakeAgent>>,
    cwd: PathBuf,
    /// The agent as the session store names it: its launch command.
    agent_key: String,
    store: Option<SessionStore>,
    /// Held while the store is read and written: the connecting thread,
    /// the prompts' and the reader's change it.
    store_lock: Mutex<()>,
    /// The sign-in methods `initialize` advertised.
    auth_methods: Mutex<Vec<AuthMethod>>,
    /// Whether the agent offers `session/load`.
    can_load: AtomicBool,
}

impl Hub {
    /// Notes in the store that session `id` was used now; `fallback` is
    /// its title when it has none yet.
    fn touch(&self, id: &SessionId, fallback: Option<&str>) {
        let Some(store) = &self.store else {
            return;
        };
        let _held = self.store_lock.lock().unwrap_or_else(|e| e.into_inner());
        let kept = store.get(&self.agent_key, id.as_str());
        let title = match kept {
            Some(kept) if !kept.title.is_empty() => kept.title,
            _ => fallback.map(title_from).unwrap_or_default(),
        };
        let _ = store.remember(StoredSession {
            id: id.as_str().to_owned(),
            agent: self.agent_key.clone(),
            cwd: self.cwd.clone(),
            title,
            last_used: now(),
        });
    }

    /// The agent gave session `id` a title.
    pub(crate) fn retitle(&self, id: &SessionId, title: &str) {
        let Some(store) = &self.store else {
            return;
        };
        let _held = self.store_lock.lock().unwrap_or_else(|e| e.into_inner());
        let title = title_from(title);
        let _ = match store.get(&self.agent_key, id.as_str()) {
            Some(_) => store.retitle(&self.agent_key, id.as_str(), &title),
            // Before `session/new`'s answer was noted.
            None => store.remember(StoredSession {
                id: id.as_str().to_owned(),
                agent: self.agent_key.clone(),
                cwd: self.cwd.clone(),
                title,
                last_used: now(),
            }),
        };
    }

    fn stored(&self) -> Vec<StoredSession> {
        self.store
            .as_ref()
            .map(|store| store.sessions_of(&self.agent_key, &self.cwd))
            .unwrap_or_default()
    }
}

/// A title for the store: the first line of `text`, at most 60
/// characters, cleaned.
fn title_from(text: &str) -> String {
    let line = clean(text.lines().find(|l| !l.trim().is_empty()).unwrap_or(""));
    let line = line.trim();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    title
}

/// A handle on a running [`app`]'s connection, for tests and embedders.
#[derive(Clone)]
pub struct Handle {
    hub: Arc<Hub>,
}

impl Handle {
    /// The connection, once the agent has started.
    pub fn connection(&self) -> Option<&Connection> {
        self.hub.connection.get()
    }

    /// For [`Launch::Replay`]: waits for the transcript to end and says
    /// whether every step replayed (once; `None` after, or for a command).
    pub fn finish_replay(&self) -> Option<Result<(), ReplayError>> {
        let fake = self.hub.fake.lock().ok()?.take()?;
        Some(fake.finish())
    }

    /// For [`Launch::Replay`]: whether the transcript has ended.
    pub fn replay_finished(&self) -> bool {
        self.hub
            .fake
            .lock()
            .ok()
            .and_then(|f| f.as_ref().map(FakeAgent::is_finished))
            .unwrap_or(true)
    }
}

/// The app's state: every field is a signal, so it is `Copy` and travels
/// to the connection's threads inside [`Proxy`] closures, which run back
/// on the app's thread.
#[derive(Clone, Copy)]
pub(crate) struct Ui {
    pub sessions: Signal<Vec<Session>>,
    pub active: Signal<usize>,
    pub agent: Signal<String>,
    pub logs: Signal<Vec<String>>,
    /// Updates for a session whose id has not come back yet.
    pub orphans: Signal<Vec<SessionNotification>>,
    pub shell_open: Signal<bool>,
    pub connected: Signal<bool>,
    pub next_key: Signal<u64>,
}

/// The most agent log lines kept.
const LOG_LINES: usize = 500;

impl Ui {
    fn new() -> Ui {
        Ui {
            sessions: signal(vec![Session::new(0)]),
            active: signal(0),
            agent: signal("starting the agent…".to_string()),
            logs: signal(Vec::new()),
            orphans: signal(Vec::new()),
            shell_open: signal(false),
            connected: signal(false),
            next_key: signal(1),
        }
    }

    fn session(&self, id: &SessionId) -> Option<Session> {
        self.sessions.with_untracked(|sessions| {
            sessions
                .iter()
                .find(|s| s.id.with_untracked(|i| i.as_ref() == Some(id)))
                .copied()
        })
    }

    fn current(&self) -> Session {
        let at = self.active.get_untracked();
        self.sessions
            .with_untracked(|sessions| sessions[at.min(sessions.len() - 1)])
    }

    /// Apply a `session/update` to its session.
    pub fn route(&self, notification: SessionNotification) {
        match self.session(&notification.session_id) {
            Some(session) => session.apply(notification.update),
            None => self.orphans.update(|o| o.push(notification)),
        }
    }

    pub fn log(&self, line: String) {
        self.logs.update(|logs| {
            logs.push(line);
            if logs.len() > LOG_LINES {
                logs.drain(..logs.len() - LOG_LINES);
            }
        });
    }

    pub fn notice(&self, session: &SessionId, markup: String) {
        if let Some(session) = self.session(session) {
            session.notice(markup);
        }
    }

    pub fn terminal_output(&self, session: &SessionId, terminal: &str, output: String) {
        if let Some(session) = self.session(session) {
            session
                .terminals
                .update(|t| drop(t.insert(terminal.to_string(), clean(&output))));
        }
    }

    /// `session/new` answered for `session`.
    fn session_ready(&self, session: Session, response: NewSessionResponse) {
        let id = response.session_id;
        session.id.set(Some(id.clone()));
        if let Some(modes) = response.modes {
            session.mode.set(Some(modes.current_mode_id.0));
        }
        let mine: Vec<SessionNotification> = self
            .orphans
            .with_untracked(|o| o.iter().filter(|n| n.session_id == id).cloned().collect());
        if !mine.is_empty() {
            self.orphans.update(|o| o.retain(|n| n.session_id != id));
            for notification in mine {
                session.apply(notification.update);
            }
        }
    }

    /// A permission prompt: one button per option the agent offers.
    pub fn ask_permission(
        &self,
        cx: &mut Ctx,
        request: RequestPermissionRequest,
        tx: Sender<RequestPermissionResponse>,
    ) {
        let session = self.session(&request.session_id);
        if let Some(session) = session {
            session.update_tool(&request.tool_call);
        }
        let call = session
            .and_then(|s| s.tool(&request.tool_call.tool_call_id.0))
            .map(|c| c.get_untracked())
            .unwrap_or_else(|| {
                let mut call = ToolCall::default();
                call.apply(&request.tool_call);
                call
            });
        let answer = Rc::new(RefCell::new(Some(tx)));
        let options = request.options.clone();
        let reject = options
            .iter()
            .find(|o| o.kind == PermissionOptionKind::RejectOnce)
            .or_else(|| options.iter().find(|o| !o.kind.allows()))
            .map(|o| o.option_id.clone());
        let cwd = self.cwd_of();
        let title = session.map(|s| s.title.get_untracked()).unwrap_or_default();
        cx.modal(Size::Percent(80), Size::Auto, move || {
            let pick = {
                let answer = answer.clone();
                move |cx: &mut Ctx, response: RequestPermissionResponse| {
                    if let Some(tx) = answer.borrow_mut().take() {
                        let _ = tx.send(response);
                    }
                    cx.pop();
                }
            };
            let buttons: Vec<Node> = options
                .iter()
                .enumerate()
                .map(|(i, option)| {
                    let id = option.option_id.clone();
                    let pick = pick.clone();
                    let face = format!("{} {}", i + 1, markup(&option.name));
                    let node = button(face, move |cx| {
                        pick(cx, RequestPermissionResponse::selected(id.clone()))
                    });
                    match option.kind {
                        PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways => {
                            node.variant(rich_intuituive::ButtonVariant::Primary)
                        }
                        _ => node.variant(rich_intuituive::ButtonVariant::Danger),
                    }
                })
                .collect();
            let mut root = column([
                label(format!(
                    "[b]The agent asks to {}[/] [dim]· {}[/]",
                    kind_word(&call.kind),
                    markup(&title)
                ))
                .auto(),
                label(format!(
                    "“{}” [dim](the agent's words)[/]",
                    markup(&call.title)
                ))
                .auto(),
                describe(&call, &cwd).auto(),
                row(buttons).fixed(3),
                label("[dim]1–9 choose · Esc rejects once[/]").fixed(1),
            ])
            .auto()
            .padding(0, 1)
            .panel("Permission")
            .on_key("escape", {
                let pick = pick.clone();
                let reject = reject.clone();
                move |cx| {
                    let response = match &reject {
                        Some(id) => RequestPermissionResponse::selected(id.clone()),
                        None => RequestPermissionResponse::cancelled(),
                    };
                    pick(cx, response)
                }
            });
            for (i, option) in options.iter().enumerate().take(9) {
                let id = option.option_id.clone();
                let pick = pick.clone();
                root = root.on_key(&(i + 1).to_string(), move |cx| {
                    pick(cx, RequestPermissionResponse::selected(id.clone()))
                });
            }
            root
        });
    }

    /// A write to review: the file's text and the agent's, as an
    /// interactive diff whose hunks are accepted or rejected.
    #[allow(clippy::too_many_arguments)]
    pub fn review_write(
        &self,
        cx: &mut Ctx,
        session: &SessionId,
        path: &Path,
        old: Option<String>,
        new: String,
        executes: bool,
        tx: Sender<Review>,
    ) {
        let title = self
            .session(session)
            .map(|s| s.title.get_untracked())
            .unwrap_or_default();
        let shown = markup(&display(path, &self.cwd_of()));
        let fresh = old.is_none();
        let old = old.unwrap_or_default();
        let answer = Rc::new(RefCell::new(Some(tx)));
        cx.modal(Size::Percent(90), Size::Percent(85), move || {
            let decisions = signal(Vec::<Decision>::new());
            let finish = {
                let (answer, old, new) = (answer.clone(), old.clone(), new.clone());
                move |cx: &mut Ctx, decided: Option<Vec<Decision>>| {
                    let review = match decided {
                        None => Review::Reject,
                        // No decision at all: only a diff with no hunks
                        // (the texts are the same) is written; otherwise
                        // nothing was accepted.
                        Some(d) if d.is_empty() && old == new => Review::Write {
                            text: new.clone(),
                            accepted: (0, 0),
                        },
                        Some(d) if d.is_empty() => Review::Reject,
                        Some(d) => {
                            let accepted = d.iter().filter(|d| **d == Decision::Accepted).count();
                            if accepted == 0 {
                                Review::Reject
                            } else {
                                Review::Write {
                                    text: accepted_text(&old, &new, &d),
                                    accepted: (accepted, d.len()),
                                }
                            }
                        }
                    };
                    if let Some(tx) = answer.borrow_mut().take() {
                        let _ = tx.send(review);
                    }
                    cx.pop();
                }
            };
            let options = DiffOptions::new().context(3).decide(decisions).on_done({
                let finish = finish.clone();
                move |decided, cx| finish(cx, Some(decided.to_vec()))
            });
            let warning = if executes {
                " [b red]⚠ this file controls what runs (hooks, settings)[/]"
            } else {
                ""
            };
            let what = if fresh { "create" } else { "change" };
            column([
                label(format!(
                    "[b]The agent wants to {what}[/] {shown}{warning} [dim]· {}[/]",
                    markup(&title)
                ))
                .auto(),
                diff_view_with(signal(old.clone()), signal(new.clone()), options)
                    .autofocus()
                    .flex(1),
                label(
                    "[dim]n/p next or previous hunk · a/r accept or reject it · A/R all of \
                     them · y write what is accepted · x or Esc reject the write[/]",
                )
                .auto(),
            ])
            .padding(0, 1)
            .panel("Review the edit")
            .on_key("y", {
                let finish = finish.clone();
                move |cx| finish(cx, Some(decisions.get_untracked()))
            })
            .on_key("x escape", {
                let finish = finish.clone();
                move |cx| finish(cx, None)
            })
        });
    }

    /// A terminal command the agent wants to run: shown in full, run only
    /// if the user says so.
    pub fn ask_command(&self, cx: &mut Ctx, request: CreateTerminalRequest, tx: Sender<bool>) {
        let cwd = self.cwd_of();
        let dir = request
            .cwd
            .as_deref()
            .map(|d| display(d, &cwd))
            .unwrap_or_else(|| ".".into());
        let line = std::iter::once(request.command.as_str())
            .chain(request.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let answer = Rc::new(RefCell::new(Some(tx)));
        cx.modal(Size::Percent(80), Size::Auto, move || {
            let say = {
                let answer = answer.clone();
                move |cx: &mut Ctx, yes: bool| {
                    if let Some(tx) = answer.borrow_mut().take() {
                        let _ = tx.send(yes);
                    }
                    cx.pop();
                }
            };
            column([
                label("[b]The agent wants to run a command[/]").auto(),
                label(format!("[b]$[/] {}", markup(&line))).auto(),
                label(format!("[dim]in[/] {}", markup(&dir))).auto(),
                row([
                    button("y Run", {
                        let say = say.clone();
                        move |cx| say(cx, true)
                    }),
                    button("n Refuse", {
                        let say = say.clone();
                        move |cx| say(cx, false)
                    }),
                ])
                .fixed(3),
            ])
            .auto()
            .padding(0, 1)
            .panel("Command")
            .on_key("y", {
                let say = say.clone();
                move |cx| say(cx, true)
            })
            .on_key("n escape", {
                let say = say.clone();
                move |cx| say(cx, false)
            })
        });
    }

    /// The workspace, as the sessions use it.
    fn cwd_of(&self) -> PathBuf {
        CWD.with(|cwd| cwd.borrow().clone())
    }
}

thread_local! {
    /// The workspace, on the app's thread (the views and prompts read it).
    static CWD: RefCell<PathBuf> = RefCell::new(PathBuf::new());
}

/// The words for a tool's kind, after "The agent asks to".
fn kind_word(kind: &ToolKind) -> &str {
    match kind {
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Delete => "delete",
        ToolKind::Move => "move",
        ToolKind::Search => "search",
        ToolKind::Execute => "run",
        ToolKind::Think => "think",
        ToolKind::Fetch => "fetch",
        ToolKind::SwitchMode => "switch mode",
        _ => "use a tool",
    }
}

/// What a tool call touches, for a prompt: its paths and command in full,
/// hidden characters revealed, and a diff's size.
fn describe(call: &ToolCall, cwd: &Path) -> Node {
    let mut lines = Vec::new();
    for location in &call.locations {
        let mut line = format!("[dim]file[/] {}", markup(&display(&location.path, cwd)));
        if let Some(n) = location.line {
            line.push_str(&format!("[dim]:{n}[/]"));
        }
        lines.push(line);
    }
    if let Some(command) = call
        .raw_input
        .as_ref()
        .and_then(|input| input.get("command"))
        .and_then(|c| c.as_str())
    {
        lines.push(format!("[b]$[/] {}", markup(command)));
    }
    for content in &call.content {
        if let ToolCallContent::Diff(diff) = content {
            let (old, new) = (
                diff.old_text.as_deref().map_or(0, |t| t.lines().count()),
                diff.new_text.lines().count(),
            );
            lines.push(format!(
                "[dim]diff[/] {} [dim]({old} → {new} lines)[/]",
                markup(&display(&diff.path, cwd))
            ));
        }
    }
    label(lines.join("\n"))
}

/// Builds Cornacai: the app to run (or drive), and a handle on its
/// connection. The agent starts on a thread of its own, which then
/// initialises and opens the first session; until it has, the prompt
/// waits.
pub fn app(options: Options) -> (App, Handle) {
    let agent_key = match &options.launch {
        Launch::Command(argv) => argv.join(" "),
        Launch::Replay { .. } => "replay".to_string(),
    };
    let hub = Arc::new(Hub {
        connection: OnceLock::new(),
        fake: Mutex::new(None),
        cwd: options.cwd.clone(),
        agent_key,
        store: options.store.clone(),
        store_lock: Mutex::new(()),
        auth_methods: Mutex::new(Vec::new()),
        can_load: AtomicBool::new(false),
    });
    CWD.with(|cwd| *cwd.borrow_mut() = options.cwd.clone());
    let capabilities = options.capabilities();
    let Options {
        launch,
        cwd,
        shell,
        pty,
        resume,
        ..
    } = options;
    let out = Rc::new(RefCell::new(None));
    let built = out.clone();
    let view_hub = hub.clone();
    let app = App::new(move || {
        let ui = Ui::new();
        *built.borrow_mut() = Some(ui);
        view(ui, view_hub, shell)
    });
    let ui = out.borrow_mut().take().expect("the view was built");
    let proxy = app.proxy();
    let thread_hub = hub.clone();
    thread::Builder::new()
        .name("cornacai-connect".into())
        .spawn(move || {
            let start = Start {
                launch,
                cwd,
                capabilities,
                pty,
                resume,
            };
            connect(thread_hub, start, proxy, ui)
        })
        .expect("a thread to start the agent");
    (app, Handle { hub })
}

/// What the thread that connects starts with.
struct Start {
    launch: Launch,
    cwd: PathBuf,
    capabilities: ClientCapabilities,
    pty: bool,
    resume: Option<String>,
}

/// Start the agent, initialise, and open the first session (or reopen
/// the one asked for).
fn connect(hub: Arc<Hub>, start: Start, proxy: Proxy, ui: Ui) {
    let Start {
        launch,
        cwd,
        capabilities,
        pty,
        resume,
    } = start;
    let fail = |proxy: &Proxy, what: String| {
        proxy.run(move || {
            ui.agent.set("no agent".into());
            ui.current().notice(format!("[red]{}[/]", markup(&what)));
        });
    };
    let workspace = match Workspace::new(&cwd) {
        Ok(workspace) => workspace,
        Err(e) => return fail(&proxy, format!("The workspace {}: {e}", cwd.display())),
    };
    let bridge = Bridge {
        proxy: proxy.clone(),
        ui,
        terminals: AgentTerminals::new(workspace.clone(), pty),
        workspace,
        hub: hub.clone(),
    };
    let connection = match launch {
        Launch::Command(argv) => {
            let Some((program, args)) = argv.split_first() else {
                return fail(&proxy, "No agent command.".into());
            };
            let mut command = std::process::Command::new(program);
            command.args(args).current_dir(&cwd);
            Connection::spawn(command, bridge)
                .map_err(|e| format!("Starting `{}` failed: {e}", argv.join(" ")))
        }
        Launch::Replay {
            transcript,
            timeout,
        } => {
            let transcript = transcript.var("$WORKSPACE", &cwd.display().to_string());
            rich_acp::replay::connect_with(transcript, bridge, timeout)
                .map(|(connection, fake)| {
                    if let Ok(mut slot) = hub.fake.lock() {
                        *slot = Some(fake);
                    }
                    connection
                })
                .map_err(|e| format!("The replay failed: {e}"))
        }
    };
    let connection = match connection {
        Ok(connection) => connection,
        Err(e) => return fail(&proxy, e),
    };
    let connection = hub.connection.get_or_init(|| connection);
    let init = match connection.initialize(InitializeRequest::new(
        capabilities,
        Implementation::new("cornacai", env!("CARGO_PKG_VERSION")),
    )) {
        Ok(init) => init,
        Err(e) => return fail(&proxy, format!("The agent did not initialise: {e}")),
    };
    // Self-reported, so shown, never trusted (the design note, rule 7).
    let name = init
        .agent_info
        .as_ref()
        .map(|info| {
            let name = clean(info.display_name());
            match info.version.as_str() {
                "" => name,
                version => format!("{name} {}", clean(version)),
            }
        })
        .unwrap_or_else(|| "the agent".into());
    *hub.auth_methods.lock().unwrap_or_else(|e| e.into_inner()) = init.auth_methods.clone();
    let can_load = init.agent_capabilities.load_session;
    hub.can_load.store(can_load, Ordering::SeqCst);
    let first = {
        let (tx, rx) = std::sync::mpsc::channel();
        proxy.run(move || {
            ui.agent.set(name);
            ui.connected.set(true);
            let _ = tx.send(ui.current());
        });
        match rx.recv() {
            Ok(session) => session,
            Err(_) => return,
        }
    };
    let open = match resume {
        Some(id) if can_load => Open::Load(SessionId::from(id.as_str())),
        Some(_) => {
            first.notice_from(
                &proxy,
                "[yellow]This agent cannot reopen a session[/] (it does not offer \
                 session/load): a new one is open instead.",
            );
            Open::New
        }
        None => Open::New,
    };
    open_session(&hub, &proxy, ui, first, open);
}

/// How a session tab is opened.
#[derive(Clone)]
enum Open {
    /// `session/new`.
    New,
    /// `session/load`: the agent replays the conversation.
    Load(SessionId),
}

/// What to do again once the user has signed in.
enum Retry {
    Open(Ui, Session, Open),
    Prompt(Session, PromptRequest),
}

impl Retry {
    fn session(&self) -> Session {
        match self {
            Retry::Open(_, session, _) | Retry::Prompt(session, _) => *session,
        }
    }

    fn run(self, hub: &Arc<Hub>, proxy: &Proxy) {
        match self {
            Retry::Open(ui, session, open) => open_session(hub, proxy, ui, session, open),
            Retry::Prompt(session, request) => run_prompt(hub, proxy, session, request),
        }
    }
}

impl Session {
    /// A notice, from another thread.
    fn notice_from(self, proxy: &Proxy, markup: impl Into<String>) {
        let markup = markup.into();
        proxy.run(move || self.notice(markup));
    }
}

/// Whether `error` is the agent's "authentication required".
fn auth_required(error: &rich_acp::Error) -> bool {
    matches!(error, rich_acp::Error::Rpc(e) if e.is_auth_required())
}

/// Runs `job` on the app's thread and waits for it.
fn on_app<T: Send + 'static>(proxy: &Proxy, job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    proxy.run(move || {
        let _ = tx.send(job());
    });
    rx.recv().ok()
}

/// Opens `session` as `open` says, on the calling thread: `session/new`,
/// or `session/load`, whose replayed history fills its conversation.
fn open_session(hub: &Arc<Hub>, proxy: &Proxy, ui: Ui, session: Session, open: Open) {
    let Some(connection) = hub.connection.get() else {
        return;
    };
    match open {
        Open::New => {
            let result = connection.new_session(NewSessionRequest::new(hub.cwd.clone()));
            match result {
                Err(e) if auth_required(&e) => {
                    sign_in(hub, proxy, Retry::Open(ui, session, Open::New));
                }
                Ok(response) => {
                    hub.touch(&response.session_id, None);
                    proxy.run(move || ui.session_ready(session, response));
                }
                Err(e) => session.notice_from(
                    proxy,
                    format!(
                        "[red]The session did not open:[/] {}",
                        markup(&e.to_string())
                    ),
                ),
            }
        }
        Open::Load(id) => {
            // The history arrives as updates before the answer: the tab
            // takes the id first, so they land in it.
            let title = hub
                .stored()
                .into_iter()
                .find(|s| s.id == id.as_str())
                .map(|s| s.title)
                .filter(|t| !t.is_empty());
            let shown = id.clone();
            on_app(proxy, move || {
                session.id.set(Some(shown));
                session.busy.set(true);
                if let Some(title) = title {
                    session.title.set(title);
                }
            });
            let request = LoadSessionRequest {
                session_id: id.clone(),
                cwd: hub.cwd.clone(),
                mcp_servers: Vec::new(),
            };
            match connection.load_session(request) {
                Err(e) if auth_required(&e) => {
                    on_app(proxy, move || session.id.set(None));
                    sign_in(hub, proxy, Retry::Open(ui, session, Open::Load(id)));
                }
                Ok(response) => {
                    hub.touch(&id, None);
                    proxy.run(move || {
                        session.busy.set(false);
                        if let Some(modes) = response.modes {
                            session.mode.set(Some(modes.current_mode_id.0));
                        }
                        session.notice(
                            "[dim]Reopened: the conversation so far, as the agent kept it.[/]",
                        );
                    });
                }
                Err(e) => {
                    let message = markup(&e.to_string());
                    proxy.run(move || {
                        session.busy.set(false);
                        session.id.set(None);
                        session.notice(format!(
                            "[red]The session did not reopen:[/] {message}. Ctrl+N opens a new one."
                        ));
                    });
                }
            }
        }
    }
}

/// The agent wants the user signed in: ask which of its methods, call
/// `authenticate` with it, then do `retry` again. Methods of the
/// `terminal` type are not offered (Cornacai does not advertise terminal
/// sign-in); with none left, the user is told how to sign in instead.
fn sign_in(hub: &Arc<Hub>, proxy: &Proxy, retry: Retry) {
    let session = retry.session();
    let methods = hub
        .auth_methods
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let usable: Vec<AuthMethod> = methods
        .iter()
        .filter(|m| m.kind.as_deref().is_none_or(|kind| kind == "agent"))
        .cloned()
        .collect();
    let Some(connection) = hub.connection.get() else {
        return;
    };
    if usable.is_empty() {
        let names: Vec<String> = methods.iter().map(|m| clean(&m.name)).collect();
        let names = if names.is_empty() {
            String::new()
        } else {
            format!(" ({})", markup(&names.join(", ")))
        };
        proxy.run(move || {
            session.busy.set(false);
            session.notice(format!(
                "[red]The agent needs you to sign in first[/]{names}. Sign in with the \
                 agent's own command, then start Cornacai again."
            ));
        });
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let shown = usable.clone();
    if !proxy.run_with(move |cx| ask_sign_in(cx, shown, tx)) {
        return;
    }
    let Ok(Some(method)) = rx.recv() else {
        proxy.run(move || {
            session.busy.set(false);
            session.notice("[yellow]Not signed in:[/] the agent will ask again.");
        });
        return;
    };
    let name = markup(&clean(&method.name));
    session.notice_from(proxy, format!("[dim]Signing in with {name}…[/]"));
    match connection.authenticate(method.id.clone()) {
        Ok(()) => {
            session.notice_from(proxy, format!("[green]Signed in[/] with {name}."));
            retry.run(hub, proxy);
        }
        Err(e) => {
            let message = markup(&e.to_string());
            proxy.run(move || {
                session.busy.set(false);
                session.notice(format!("[red]Signing in with {name} failed:[/] {message}"));
            });
        }
    }
}

/// The sign-in prompt: one button per method; Esc signs in with none.
fn ask_sign_in(cx: &mut Ctx, methods: Vec<AuthMethod>, tx: Sender<Option<AuthMethod>>) {
    let answer = Rc::new(RefCell::new(Some(tx)));
    cx.modal(Size::Percent(70), Size::Auto, move || {
        let pick = {
            let answer = answer.clone();
            move |cx: &mut Ctx, method: Option<AuthMethod>| {
                if let Some(tx) = answer.borrow_mut().take() {
                    let _ = tx.send(method);
                }
                cx.pop();
            }
        };
        let mut lines = Vec::new();
        for (i, method) in methods.iter().enumerate() {
            let mut line = format!("[b]{}[/] {}", i + 1, markup(&clean(&method.name)));
            if let Some(about) = &method.description {
                line.push_str(&format!(" [dim]· {}[/]", markup(&clean(about))));
            }
            lines.push(line);
        }
        let buttons: Vec<Node> = methods
            .iter()
            .enumerate()
            .map(|(i, method)| {
                let (pick, method) = (pick.clone(), method.clone());
                let face = format!("{} {}", i + 1, markup(&clean(&method.name)));
                button(face, move |cx| pick(cx, Some(method.clone())))
                    .variant(rich_intuituive::ButtonVariant::Primary)
            })
            .collect();
        let mut root = column([
            label("[b]The agent asks you to sign in[/]").auto(),
            label(lines.join("\n")).auto(),
            row(buttons).fixed(3),
            label("[dim]1–9 choose · Esc not now[/]").fixed(1),
        ])
        .auto()
        .padding(0, 1)
        .panel("Sign in")
        .on_key("escape", {
            let pick = pick.clone();
            move |cx| pick(cx, None)
        });
        for (i, method) in methods.iter().enumerate().take(9) {
            let (pick, method) = (pick.clone(), method.clone());
            root = root.on_key(&(i + 1).to_string(), move |cx| {
                pick(cx, Some(method.clone()))
            });
        }
        root
    });
}

/// The whole screen.
fn view(ui: Ui, hub: Arc<Hub>, shell: Option<ShellPane>) -> Node {
    let prompts: Rc<RefCell<HashMap<u64, u64>>> = Rc::default();
    let shell_id = Rc::new(RefCell::new(None::<u64>));
    let tabs = text(move || {
        let active = ui.active.get();
        let sessions = ui.sessions.get();
        let mut line = String::new();
        for (i, session) in sessions.iter().enumerate() {
            let busy = if session.busy.get() { " …" } else { "" };
            let title = markup(&session.title.get());
            if i == active {
                line.push_str(&format!("[reverse] {} {title}{busy} [/]", i + 1));
            } else {
                line.push_str(&format!(" {} {title}{busy} ", i + 1));
            }
        }
        format!("{line}  [dim]·[/] [b]{}[/]", markup(&ui.agent.get()))
    })
    .fixed(1);
    let views = {
        let (hub, prompts) = (hub.clone(), prompts.clone());
        switch(
            move || {
                let at = ui.active.get();
                ui.sessions.with(|s| s[at.min(s.len() - 1)].key)
            },
            move |key| {
                let session = ui
                    .sessions
                    .with_untracked(|s| s.iter().find(|s| s.key == key).copied())
                    .expect("a session for its key");
                session_view(hub.clone(), session, prompts.clone())
            },
        )
        .flex(1)
    };
    let hints = text(move || {
        let busy = ui.current_tracked().busy.get();
        let esc = if busy { "Esc cancel · " } else { "" };
        format!(
            "[dim]Enter send · Alt+Enter line · {esc}Ctrl+N new · Ctrl+O reopen · \
             Alt+N/P tab · Ctrl+T shell · F2 log · Ctrl+Q quit[/]"
        )
    })
    .fixed(1);
    let main = column([tabs, views, hints]).flex(2);
    let has_shell = shell.is_some();
    let screen = match shell {
        Some(build) => {
            let pane = build()
                .release_keys("ctrl+t ctrl+q alt+n alt+p")
                .node()
                .panel("Shell");
            *shell_id.borrow_mut() = Some(pane.id());
            row([main, pane.flex(1).shown_when(move || ui.shell_open.get())])
        }
        None => row([main]),
    };
    let focus_prompt = {
        let prompts = prompts.clone();
        move |cx: &mut Ctx| {
            let key = ui.current().key;
            if let Some(id) = prompts.borrow().get(&key) {
                cx.focus(*id);
            }
        }
    };
    let mut screen = screen;
    for n in 1..=9usize {
        let focus_prompt = focus_prompt.clone();
        screen = screen.on_key(&format!("alt+{n}"), move |cx| {
            if n <= ui.sessions.with_untracked(Vec::len) {
                ui.active.set(n - 1);
                let _ = cx.after(Duration::ZERO, focus_prompt.clone());
            }
        });
    }
    screen
        .on_key("ctrl+q", |cx| cx.quit())
        .on_key("ctrl+n", {
            let hub = hub.clone();
            let focus_prompt = focus_prompt.clone();
            move |cx| {
                if !ui.connected.get_untracked() {
                    return;
                }
                let key = ui.next_key.get_untracked();
                ui.next_key.set(key + 1);
                let session = Session::new(key);
                ui.sessions.update(|s| s.push(session));
                ui.active.set(ui.sessions.with_untracked(Vec::len) - 1);
                let (hub, proxy) = (hub.clone(), cx.proxy());
                thread::spawn(move || open_session(&hub, &proxy, ui, session, Open::New));
                let _ = cx.after(Duration::ZERO, focus_prompt.clone());
            }
        })
        .on_key("ctrl+o", {
            let hub = hub.clone();
            let focus_prompt = focus_prompt.clone();
            move |cx| {
                if ui.connected.get_untracked() {
                    reopen_list(cx, ui, &hub, focus_prompt.clone());
                }
            }
        })
        .on_key("alt+n", {
            let focus_prompt = focus_prompt.clone();
            move |cx| {
                let n = ui.sessions.with_untracked(Vec::len);
                ui.active.update(|a| *a = (*a + 1) % n);
                let _ = cx.after(Duration::ZERO, focus_prompt.clone());
            }
        })
        .on_key("alt+p", {
            let focus_prompt = focus_prompt.clone();
            move |cx| {
                let n = ui.sessions.with_untracked(Vec::len);
                ui.active.update(|a| *a = (*a + n - 1) % n);
                let _ = cx.after(Duration::ZERO, focus_prompt.clone());
            }
        })
        .on_key("ctrl+t", {
            let focus_prompt = focus_prompt.clone();
            move |cx| {
                if !has_shell {
                    ui.current()
                        .notice("[yellow]No shell pane:[/] no shell is configured.");
                    return;
                }
                let open = !ui.shell_open.get_untracked();
                ui.shell_open.set(open);
                match (*shell_id.borrow(), open) {
                    (Some(id), true) => {
                        let _ = cx.after(Duration::ZERO, move |cx| cx.focus_within(id));
                    }
                    _ => focus_prompt(cx),
                }
            }
        })
        .on_key("f2", move |cx| {
            cx.modal(Size::Percent(90), Size::Percent(80), move || {
                scroll(text(move || {
                    let logs = ui.logs.get();
                    if logs.is_empty() {
                        "[dim]The agent has written nothing to its log.[/]".to_string()
                    } else {
                        markup(&logs.join("\n"))
                    }
                }))
                .follow_end()
                .panel("Agent log")
                .on_key("escape f2 q", |cx| cx.pop())
            })
        })
}

impl Ui {
    /// The active session, subscribing to which it is.
    fn current_tracked(&self) -> Session {
        let at = self.active.get();
        self.sessions.with(|s| s[at.min(s.len() - 1)])
    }
}

/// One session's conversation, plan and prompt.
fn session_view(hub: Arc<Hub>, session: Session, prompts: Rc<RefCell<HashMap<u64, u64>>>) -> Node {
    let s = session;
    let cwd = hub.cwd.clone();
    let transcript = scroll(
        each(
            move || (0..s.entries.with(Vec::len)).collect::<Vec<usize>>(),
            move |i| entry_view(s, i).auto(),
        )
        .auto(),
    )
    .follow_end()
    .flex(1);
    let plan = text(move || plan_markup(&s.plan.get()))
        .auto()
        .shown_when(move || s.plan.with(|p| !p.is_empty()));
    let walk = rich_ext::listing::walk(&cwd)
        .skip_dir(".git")
        .skip_dir("target")
        .skip_dir("node_modules");
    let send_hub = hub.clone();
    let prompt = text_area(s.prompt)
        .grow(1, 6)
        .submit_on("enter")
        .placeholder("Ask the agent…  @ names a file")
        .complete_at('@', path_completer(walk))
        .history(history_session())
        .on_submit(move |text, cx| send(&send_hub, s, text, cx))
        .autofocus();
    prompts.borrow_mut().insert(s.key, prompt.id());
    let prompt = prompt.panel_with(move || {
        let status = match (s.id.get(), s.busy.get()) {
            (None, _) => "[dim]waiting for the agent…[/]".to_string(),
            (Some(_), true) => "[yellow]the agent is working[/] [dim](Esc cancels)[/]".to_string(),
            (Some(_), false) => "Prompt".to_string(),
        };
        match s.mode.get() {
            Some(mode) => format!("{status} [dim]· mode {}[/]", markup(&mode)),
            None => status,
        }
    });
    column([transcript, plan, prompt.auto()]).on_key("escape", move |_| {
        if !s.busy.get_untracked() {
            return;
        }
        if let (Some(connection), Some(id)) = (hub.connection.get(), s.id.get_untracked()) {
            let _ = connection.cancel(&id);
            s.notice("[dim]Cancelling…[/]");
        }
    })
}

/// Send `text` as a prompt turn of `s`.
fn send(hub: &Arc<Hub>, s: Session, text: &str, cx: &mut Ctx) {
    let text = text.trim_end().to_string();
    if text.trim().is_empty() {
        return;
    }
    if s.busy.get_untracked() {
        s.notice("[yellow]The agent is still working:[/] Esc cancels the turn.");
        return;
    }
    let Some(id) = s.id.get_untracked() else {
        s.notice("[yellow]The session has not opened yet.[/]");
        return;
    };
    let request = prompt_request(id, &text, &hub.cwd);
    let mentions = request
        .prompt
        .iter()
        .filter_map(|block| match block {
            rich_acp::ContentBlock::ResourceLink(link) => link.title.clone(),
            _ => None,
        })
        .collect();
    s.push(Entry::User {
        text: text.clone(),
        mentions,
    });
    s.prompt.set(String::new());
    s.busy.set(true);
    let (hub, proxy) = (hub.clone(), cx.proxy());
    thread::spawn(move || {
        hub.touch(&request.session_id, Some(&text));
        run_prompt(&hub, &proxy, s, request);
    });
}

/// A prompt turn of `s`, on the calling thread; signs in first when the
/// agent asks for it.
fn run_prompt(hub: &Arc<Hub>, proxy: &Proxy, s: Session, request: PromptRequest) {
    let result = match hub.connection.get() {
        Some(connection) => connection.prompt(request.clone()),
        None => Err(rich_acp::Error::Closed),
    };
    if let Err(e) = &result
        && auth_required(e)
    {
        return sign_in(hub, proxy, Retry::Prompt(s, request));
    }
    proxy.run(move || {
        s.busy.set(false);
        match result {
            Ok(done) if done.stop_reason == StopReason::EndTurn => {}
            Ok(done) if done.stop_reason == StopReason::Cancelled => {
                s.notice("[dim]The turn was cancelled.[/]")
            }
            Ok(done) => s.notice(format!(
                "[yellow]The turn ended:[/] {}",
                markup(done.stop_reason.as_str())
            )),
            Err(e) => s.notice(format!(
                "[red]The prompt failed:[/] {}",
                markup(&e.to_string())
            )),
        }
    });
}

/// The most sessions the reopen list shows.
const REOPEN_SHOWN: usize = 20;

/// Ctrl+O: the sessions of this agent in this workspace, most recent
/// first; the one picked opens in a tab of its own (or its tab, when it is
/// open already), with `session/load`.
fn reopen_list(
    cx: &mut Ctx,
    ui: Ui,
    hub: &Arc<Hub>,
    focus_prompt: impl Fn(&mut Ctx) + Clone + 'static,
) {
    if !hub.can_load.load(Ordering::SeqCst) {
        ui.current().notice(
            "[yellow]This agent cannot reopen sessions:[/] it does not offer session/load.",
        );
        return;
    }
    if hub.store.is_none() {
        ui.current()
            .notice("[yellow]No session store:[/] Cornacai keeps no sessions in this run.");
        return;
    }
    let mut stored = hub.stored();
    stored.truncate(REOPEN_SHOWN);
    if stored.is_empty() {
        ui.current()
            .notice("[dim]No earlier sessions of this agent in this workspace.[/]");
        return;
    }
    let now = now();
    let items: Vec<String> = stored
        .iter()
        .map(|s| {
            let title = if s.title.is_empty() {
                "untitled".to_string()
            } else {
                markup(&s.title)
            };
            format!(
                "{title} [dim]· {} · {}[/]",
                ago(s.last_used, now),
                markup(&s.id)
            )
        })
        .collect();
    let hub = hub.clone();
    cx.pick("Reopen a session", Origin::Center, items, move |i, cx| {
        let chosen = &stored[i];
        let id = SessionId::from(chosen.id.as_str());
        let open_at = ui.sessions.with_untracked(|sessions| {
            sessions
                .iter()
                .position(|s| s.id.with_untracked(|own| own.as_ref() == Some(&id)))
        });
        if let Some(at) = open_at {
            ui.active.set(at);
        } else {
            let key = ui.next_key.get_untracked();
            ui.next_key.set(key + 1);
            let session = Session::new(key);
            ui.sessions.update(|s| s.push(session));
            ui.active.set(ui.sessions.with_untracked(Vec::len) - 1);
            let proxy = cx.proxy();
            thread::spawn(move || open_session(&hub, &proxy, ui, session, Open::Load(id)));
        }
        let _ = cx.after(Duration::ZERO, focus_prompt.clone());
    });
}

/// The plan, one entry a line.
fn plan_markup(plan: &[rich_acp::PlanEntry]) -> String {
    let mut out = String::from("[b]Plan[/]");
    for entry in plan {
        let mark = match entry.status {
            PlanEntryStatus::Completed => "[green]✓[/]",
            PlanEntryStatus::InProgress => "[yellow]▸[/]",
            _ => "[dim]○[/]",
        };
        out.push_str(&format!("\n {mark} {}", markup(&entry.content)));
    }
    out
}

/// Entry `i` of `s`'s conversation.
fn entry_view(s: Session, i: usize) -> Node {
    let Some(entry) = s.entries.with_untracked(|e| e.get(i).cloned()) else {
        return label("");
    };
    match entry {
        Entry::User { text, mentions } => {
            let mut shown = format!("[b cyan]❯[/] {}", markup(&text));
            if !mentions.is_empty() {
                shown.push_str(&format!(
                    "\n  [dim]with {}[/]",
                    mentions
                        .iter()
                        .map(|m| format!("@{}", markup(m)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            label(shown).padding(1, 0)
        }
        Entry::Agent { doc, .. } => markdown(doc).padding(1, 0),
        Entry::Thought { doc, open, .. } => collapsible_with(
            || "[dim i]Thinking[/]".to_string(),
            open,
            markdown(doc).style("dim"),
        )
        .summary(move || doc.with(|d| last_line(d, 60))),
        Entry::Tool { call, open } => tool_view(s, call, open),
        Entry::Notice(markup) => label(markup).style("dim"),
    }
}

/// The last non-empty line of `text`, cut to `width` characters: plain
/// text, as a collapsible's summary is.
fn last_line(text: &str, width: usize) -> String {
    let line = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let mut cut: String = line.chars().take(width).collect();
    if line.chars().count() > width {
        cut.push('…');
    }
    reveal(&cut)
}

/// A tool call: a folded card, its title and status following updates.
fn tool_view(s: Session, call: Signal<ToolCall>, open: Signal<bool>) -> Node {
    let body = rich_intuituive::keyed(
        move || call.with(|c| serde_json::to_string(&c.content).unwrap_or_default()),
        move |_| tool_body(s, call),
    )
    .auto();
    collapsible_with(
        move || {
            call.with(|c| {
                format!(
                    "[dim]{}[/] {}",
                    markup(c.kind.as_str()),
                    markup(&c.title.replace('\n', " "))
                )
            })
        },
        open,
        body,
    )
    .status(move || match call.with(|c| c.status.clone()) {
        ToolCallStatus::Pending => Status::Markup("[dim]pending[/]".into()),
        ToolCallStatus::InProgress => Status::Spinner("dots"),
        ToolCallStatus::Completed => Status::Markup("[green]✓[/]".into()),
        ToolCallStatus::Failed => Status::Markup("[red]✗ failed[/]".into()),
        _ => Status::None,
    })
    .summary(move || call.with(summary))
}

/// A closed tool call's one line (plain text): its file, or its output's
/// last line.
fn summary(call: &ToolCall) -> String {
    let cwd = CWD.with(|c| c.borrow().clone());
    for content in call.content.iter().rev() {
        match content {
            ToolCallContent::Diff(diff) => {
                return format!("diff {}", reveal(&display(&diff.path, &cwd)));
            }
            ToolCallContent::Content(wrapped) => {
                if let Some(text) = wrapped.content.as_text() {
                    return last_line(text, 70);
                }
            }
            _ => {}
        }
    }
    match call.locations.first() {
        Some(location) => reveal(&display(&location.path, &cwd)),
        None => String::new(),
    }
}

/// The most lines of a tool's output shown.
const OUTPUT_LINES: usize = 40;

/// What an open tool call shows: its output, its diffs, its terminals.
fn tool_body(s: Session, call: Signal<ToolCall>) -> Node {
    let cwd = CWD.with(|c| c.borrow().clone());
    let content = call.with_untracked(|c| c.content.clone());
    let mut nodes = Vec::new();
    if let Some(command) = call.with_untracked(|c| {
        c.raw_input
            .as_ref()
            .and_then(|i| i.get("command"))
            .and_then(|c| c.as_str())
            .map(str::to_owned)
    }) {
        nodes.push(label(format!("[b]$[/] {}", markup(&command))).auto());
    }
    for item in content {
        match item {
            ToolCallContent::Content(wrapped) => {
                let text = crate::session::chunk_text(&wrapped.content);
                nodes.push(label(tail(&text)).auto());
            }
            ToolCallContent::Diff(diff) => {
                let old = diff.old_text.unwrap_or_default();
                let rows = (old.lines().count() + diff.new_text.lines().count()).clamp(3, 16) + 1;
                nodes.push(label(format!("[b]{}[/]", markup(&display(&diff.path, &cwd)))).auto());
                nodes.push(
                    diff_view_with(
                        signal(old),
                        signal(diff.new_text),
                        DiffOptions::new().context(2),
                    )
                    .fixed(rows as u16),
                );
            }
            ToolCallContent::Terminal(terminal) => {
                let id = terminal.terminal_id.0;
                nodes.push(
                    text(move || {
                        s.terminals.with(|t| match t.get(&id) {
                            Some(out) if !out.is_empty() => tail(out),
                            Some(_) => "[dim](no output)[/]".to_string(),
                            None => "[dim](running…)[/]".to_string(),
                        })
                    })
                    .auto(),
                );
            }
            other => nodes.push(label(format!("[dim][{}][/]", markup(other.kind()))).auto()),
        }
    }
    if nodes.is_empty() {
        nodes.push(label("[dim](nothing yet)[/]").auto());
    }
    column(nodes).auto().padding(0, 2)
}

/// `text`'s last [`OUTPUT_LINES`] lines, escaped, with a count of those
/// left out.
fn tail(text: &str) -> String {
    let text = reveal(text);
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= OUTPUT_LINES {
        return crate::clean::markup(&text);
    }
    let skipped = lines.len() - OUTPUT_LINES;
    format!(
        "[dim]… {skipped} lines above[/]\n{}",
        rich_intuituive::rich::markup::escape(&lines[skipped..].join("\n"))
    )
}

/// Waits for the screen of `driver` to hold `text`, settling it; panics
/// with the screen after `timeout`. For tests.
pub fn wait_for(driver: &mut Driver, text: &str, timeout: Duration) {
    let start = Instant::now();
    loop {
        driver.settle();
        let screen = driver.screen().plain().join("\n");
        if screen.contains(text) {
            return;
        }
        assert!(
            start.elapsed() < timeout,
            "{text:?} never showed:\n{screen}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
