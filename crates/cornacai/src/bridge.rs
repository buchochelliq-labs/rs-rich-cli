//! The [`Client`] Cornacai gives rs-rich-acp: it hands everything to the
//! app's loop through intuiTUIve's [`Proxy`], and nothing in the UI
//! touches the connection's threads.
//!
//! - A `session/update` is applied on the app's thread, in arrival order.
//! - A permission request opens a prompt there and blocks its own thread
//!   (rs-rich-acp gives each request one) until the user answers.
//! - A write is shown as a diff to review, hunk by hunk; what the user
//!   accepted is written inside the workspace, and a write rejected whole
//!   is refused.
//! - A terminal command is asked about, then runs on [`AgentTerminals`]
//!   (pipes, or a PTY when the setting is on); its output, as the agent
//!   reads it, is shown in its tool call.
//! - Reads are answered inside the workspace, without asking.

use std::path::Path;
use std::sync::{Arc, mpsc};

use rich_acp::{
    CreateTerminalRequest, CreateTerminalResponse, Error, ReadTextFileRequest,
    ReadTextFileResponse, RequestPermissionRequest, RequestPermissionResponse, RpcError,
    SessionNotification, SessionUpdate, TerminalExitStatus, TerminalOutputResponse,
    TerminalRequest, Workspace, WriteTextFileRequest, WriteTextFileResponse, controls_execution,
    fs,
};
use rich_intuituive::Proxy;

use crate::app::{Hub, Ui};
use crate::clean::markup;
use crate::terminals::AgentTerminals;

/// What the user decided about a write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Review {
    /// Write this text: the new text, or the hunks accepted of it.
    Write {
        /// The text to write.
        text: String,
        /// Hunks accepted, of how many.
        accepted: (usize, usize),
    },
    /// Write nothing.
    Reject,
}

/// The client end Cornacai hands to its connection.
pub(crate) struct Bridge {
    pub proxy: Proxy,
    pub ui: Ui,
    pub workspace: Workspace,
    pub terminals: AgentTerminals,
    pub hub: Arc<Hub>,
}

/// The error a refused write or command answers with.
fn refused(what: &str) -> RpcError {
    RpcError::new(
        RpcError::REQUEST_CANCELLED,
        format!("The user refused {what}."),
    )
}

impl Bridge {
    fn log(&self, line: String) {
        let ui = self.ui;
        self.proxy.run(move || ui.log(line));
    }

    fn notice(&self, session: &rich_acp::SessionId, markup: String) {
        let (ui, session) = (self.ui, session.clone());
        self.proxy.run(move || ui.notice(&session, markup));
    }

    fn show_output(&self, request: &TerminalRequest) {
        if let Ok(out) = self.terminals.output(request) {
            let (ui, request) = (self.ui, request.clone());
            self.proxy.run(move || {
                ui.terminal_output(&request.session_id, &request.terminal_id.0, out.output)
            });
        }
    }
}

impl rich_acp::Client for Bridge {
    fn session_update(&self, notification: SessionNotification) {
        if let SessionUpdate::Unknown { kind, value } = &notification.update
            && kind == "session_info_update"
            && let Some(title) = value.get("title").and_then(|t| t.as_str())
        {
            self.hub.retitle(&notification.session_id, title);
        }
        let ui = self.ui;
        self.proxy.run(move || ui.route(notification));
    }

    fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, RpcError> {
        let (tx, rx) = mpsc::channel();
        let ui = self.ui;
        if !self
            .proxy
            .run_with(move |cx| ui.ask_permission(cx, request, tx))
        {
            return Ok(RequestPermissionResponse::cancelled());
        }
        // The app gone, or the prompt closed without an answer: the turn
        // is over for this request.
        Ok(rx
            .recv()
            .unwrap_or_else(|_| RequestPermissionResponse::cancelled()))
    }

    fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> Result<ReadTextFileResponse, RpcError> {
        fs::read_text_file(&self.workspace, &request)
    }

    fn write_text_file(
        &self,
        request: WriteTextFileRequest,
    ) -> Result<WriteTextFileResponse, RpcError> {
        let path = self.workspace.check(&request.path)?;
        let old = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let (tx, rx) = mpsc::channel();
        let ui = self.ui;
        let shown = request.clone();
        let executes = controls_execution(&path);
        if !self.proxy.run_with(move |cx| {
            ui.review_write(
                cx,
                &shown.session_id,
                &shown.path,
                old,
                shown.content,
                executes,
                tx,
            )
        }) {
            return Err(refused("the write"));
        }
        let shown_path = markup(&display(&request.path, self.workspace.cwd()));
        match rx.recv() {
            Ok(Review::Write { text, accepted }) => {
                let written = fs::write_text_file(
                    &self.workspace,
                    &WriteTextFileRequest {
                        content: text,
                        ..request.clone()
                    },
                );
                let what = match (&written, accepted) {
                    (Err(e), _) => format!(
                        "[red]Writing {shown_path} failed:[/] {}",
                        markup(&e.message)
                    ),
                    (Ok(_), (a, n)) if a == n => format!("[green]Wrote[/] {shown_path}"),
                    (Ok(_), (a, n)) => format!("[green]Wrote[/] {shown_path}: {a} of {n} hunks"),
                };
                self.notice(&request.session_id, what);
                written
            }
            Ok(Review::Reject) | Err(_) => {
                self.notice(
                    &request.session_id,
                    format!("[yellow]Rejected[/] the write to {shown_path}"),
                );
                Err(refused("the write"))
            }
        }
    }

    fn create_terminal(
        &self,
        request: CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, RpcError> {
        if let Some(cwd) = &request.cwd {
            self.workspace.check(cwd)?;
        }
        let (tx, rx) = mpsc::channel();
        let ui = self.ui;
        let shown = request.clone();
        if !self.proxy.run_with(move |cx| ui.ask_command(cx, shown, tx)) {
            return Err(refused("the command"));
        }
        if rx.recv().unwrap_or(false) {
            self.terminals.create(&request)
        } else {
            let line = std::iter::once(request.command.as_str())
                .chain(request.args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            self.notice(
                &request.session_id,
                format!("[yellow]Refused[/] the command {}", markup(&line)),
            );
            Err(refused("the command"))
        }
    }

    fn terminal_output(
        &self,
        request: TerminalRequest,
    ) -> Result<TerminalOutputResponse, RpcError> {
        let out = self.terminals.output(&request)?;
        let (ui, shown) = (self.ui, request.clone());
        let text = out.output.clone();
        self.proxy
            .run(move || ui.terminal_output(&shown.session_id, &shown.terminal_id.0, text));
        Ok(out)
    }

    fn wait_for_terminal_exit(
        &self,
        request: TerminalRequest,
    ) -> Result<TerminalExitStatus, RpcError> {
        let status = self.terminals.wait_for_exit(&request)?;
        self.show_output(&request);
        Ok(status)
    }

    fn kill_terminal(&self, request: TerminalRequest) -> Result<(), RpcError> {
        self.terminals.kill(&request)?;
        self.show_output(&request);
        Ok(())
    }

    fn release_terminal(&self, request: TerminalRequest) -> Result<(), RpcError> {
        self.show_output(&request);
        self.terminals.release(&request)
    }

    fn agent_log(&self, line: &str) {
        self.log(crate::clean::clean(line));
    }

    fn protocol_error(&self, error: &Error) {
        self.log(format!("protocol error: {error}"));
    }
}

/// `path` relative to `cwd` when it is under it.
pub(crate) fn display(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}
