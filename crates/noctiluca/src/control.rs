//! Agent control (#816): scripts and coding agents of the same user drive
//! a running Noctiluca through its control socket, and read the page as
//! the projection's text, far fewer tokens than a screenshot.
//!
//! The socket is rs-rich-interact's generic [`control`] piece
//! (`rich_interact::control`): a Unix socket in a private directory
//! (`$XDG_RUNTIME_DIR/noctiluca`, mode 0700), the peer's user checked, no
//! symbolic links followed, bounded messages and timeouts, no network
//! listener. A session listens only when asked (`--control`, `[agent]
//! control = true`, or `--read-only`); with `--read-only` every verb that
//! changes anything is refused before Noctiluca sees it. This module is
//! Noctiluca's side of it: the [`VERBS`], what each does to the browser
//! ([`handle`]), the page as text with control ids ([`snapshot`]), and
//! the client the `noctiluca ls` and `noctiluca action …` commands are.
//! Each action an agent takes shows in the status bar as it happens.
//!
//! Unix only for now: on Windows the commands say so.
//!
//! [`control`]: https://docs.rs/rs-rich-interact/latest/rich_interact/control/

use std::time::Duration;

use rich_intuituive::prelude::*;
use serde_json::{Value, json};

use crate::adapt::Adapt;
use crate::layout::{self, Marks, Piece};
use crate::model::{Page, Role};

/// The control socket's verbs. `info`, `snapshot` and `find` only read;
/// every other verb changes something, and a read-only session refuses
/// it.
#[cfg(unix)]
pub const VERBS: &[rich_interact::control::Verb] = {
    use rich_interact::control::Verb;
    &[
        Verb::read("info", "the session: its pid, address, title and state"),
        Verb::read("snapshot", "the page as text, each control with its id"),
        Verb::read("find", "the rows of the page that hold some text"),
        Verb::write("open", "open an address, a keyword or a search"),
        Verb::write("click", "click a control, by its id or its name"),
        Verb::write("type", "type text, into a field by its id or name"),
        Verb::write("key", "press keys in the page"),
        Verb::write("scroll", "scroll: up, down, top or bottom"),
        Verb::write("back", "go back"),
        Verb::write("forward", "go forward"),
    ]
};

/// The socket files' prefix: `noctiluca-PID.sock`.
pub const PREFIX: &str = "noctiluca-";

/// How wide a snapshot is laid out when the request does not say.
pub const SNAPSHOT_WIDTH: u16 = 100;

/// The control socket of this session, while it listens. Cloning shares
/// it; the server is taken once, by the app that answers it.
#[derive(Clone, Debug, Default)]
pub struct ControlLink {
    #[cfg(unix)]
    server: std::sync::Arc<std::sync::Mutex<Option<rich_interact::control::ControlServer>>>,
    #[cfg(unix)]
    closer: Option<rich_interact::control::Closer>,
    path: Option<std::path::PathBuf>,
    read_only: bool,
}

impl ControlLink {
    /// Listen on this process's socket, in Noctiluca's private runtime
    /// directory.
    #[cfg(unix)]
    pub fn bind(read_only: bool) -> std::io::Result<ControlLink> {
        let dir = rich_interact::control::runtime_dir("noctiluca")?;
        let path = dir.join(format!("{PREFIX}{}.sock", std::process::id()));
        ControlLink::bind_at(&path, read_only)
    }

    /// Listen on the socket at `path` (its directory must be private).
    #[cfg(unix)]
    pub fn bind_at(path: &std::path::Path, read_only: bool) -> std::io::Result<ControlLink> {
        let server = rich_interact::control::ControlServer::bind(path, VERBS)?.read_only(read_only);
        Ok(ControlLink {
            closer: Some(server.closer()),
            server: std::sync::Arc::new(std::sync::Mutex::new(Some(server))),
            path: Some(path.to_path_buf()),
            read_only,
        })
    }

    /// The control socket is Unix only for now.
    #[cfg(not(unix))]
    pub fn bind(_read_only: bool) -> std::io::Result<ControlLink> {
        Err(unix_only())
    }

    /// The server, once: the app answering it takes it.
    #[cfg(unix)]
    pub fn take(&self) -> Option<rich_interact::control::ControlServer> {
        self.server.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// Stop listening and remove the socket file: on every way out.
    pub fn close(&self) {
        #[cfg(unix)]
        if let Some(closer) = &self.closer {
            closer.close();
        }
        #[cfg(unix)]
        drop(self.take());
    }

    /// The socket's path, while it listens.
    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
}

/// The error the control commands give where there is no socket yet.
pub fn unix_only() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the control socket is Unix only for now",
    )
}

/// A control as a snapshot lists it.
fn control_json(index: usize, page: &Page) -> Value {
    let control = &page.controls[index];
    let mut out = json!({
        "id": index,
        "role": control.role.name(),
        "name": control.name,
    });
    if let Some(value) = &control.value {
        out["value"] = json!(value);
    }
    if let Some(href) = &control.href {
        out["href"] = json!(href);
    }
    if control.disabled {
        out["disabled"] = json!(true);
    }
    out
}

/// The marks of a snapshot: each usable control's id as its hint.
fn id_marks(page: &Page, adapt: Adapt) -> Marks {
    Marks {
        hints: page
            .controls
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.disabled)
            .map(|(i, _)| (i, i.to_string()))
            .collect(),
        adapt,
        ..Marks::default()
    }
}

/// The page as text an agent reads: the projection (or reader mode) laid
/// out `width` cells wide, each usable control marked `[ID]` where it is,
/// and the controls listed with their ids, roles, names, values and
/// links. `click 12` then means control 12. Untrusted text, cleaned as
/// everything the page says is.
pub fn snapshot(page: &Page, reader: bool, width: u16, adapt: Adapt) -> Value {
    let marks = id_marks(page, adapt);
    let mut lines: Vec<String> = Vec::new();
    if reader {
        for piece in layout::reader(page, width.saturating_sub(2), &marks) {
            match piece {
                Piece::Text(text) => lines.extend(text.iter().map(|l| l.plain())),
                Piece::Media { index, .. } => {
                    if let Some(media) = page.media.get(index) {
                        lines.push(format!("[{:?}: {}]", media.kind, media.alt).to_lowercase());
                    }
                }
                Piece::Controls(controls) => {
                    for c in controls {
                        let control = &page.controls[c];
                        let value = control
                            .value
                            .as_deref()
                            .map(|v| format!(" = {v:?}"))
                            .unwrap_or_default();
                        lines.push(format!(
                            "[{c}] {} {:?}{value}",
                            control.role.name(),
                            control.name
                        ));
                    }
                }
                Piece::Player(index) => {
                    if let Some(media) = page.media.get(index) {
                        lines.push(format!("[player: {}]", media.alt));
                    }
                }
            }
        }
    } else {
        let projection = layout::project(
            page,
            width,
            (crate::cdp::CELL.0 as f64, crate::cdp::CELL.1 as f64),
            &marks,
        );
        lines.extend(projection.lines.iter().map(|l| l.plain()));
    }
    // Trailing spaces off, and no more than one blank row in a row.
    let mut text = String::new();
    let mut blank = false;
    for line in lines {
        let line = line.trim_end();
        if line.is_empty() {
            if blank || text.is_empty() {
                continue;
            }
            blank = true;
        } else {
            blank = false;
        }
        text.push_str(line);
        text.push('\n');
    }
    json!({
        "url": page.url,
        "title": page.title,
        "view": if reader { "reader" } else { "projection" },
        "text": text.trim_end(),
        "controls": (0..page.controls.len()).map(|i| control_json(i, page)).collect::<Vec<_>>(),
    })
}

/// The control `target` names: its id (`12`), or its accessible name,
/// exact first (ignoring case), then the first that contains it. Disabled
/// controls are not targets.
pub fn resolve(page: &Page, target: &str) -> Result<usize, String> {
    let target = target.trim();
    if let Ok(id) = target.parse::<usize>() {
        return match page.controls.get(id) {
            None => Err(format!(
                "no control {id} on this page (`snapshot` lists them)"
            )),
            Some(c) if c.disabled => Err(format!("control {id} is disabled")),
            Some(_) => Ok(id),
        };
    }
    let wanted = target.to_lowercase();
    let usable = || {
        page.controls
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.disabled)
    };
    usable()
        .find(|(_, c)| c.name.to_lowercase() == wanted)
        .or_else(|| usable().find(|(_, c)| c.name.to_lowercase().contains(&wanted)))
        .map(|(i, _)| i)
        .ok_or_else(|| format!("no control named {target:?} on this page"))
}

/// Whether a role is clicked (a link, a button, a box) rather than
/// focused for typing.
fn clicks(role: &Role) -> bool {
    matches!(
        role,
        Role::Link
            | Role::Button
            | Role::CheckBox
            | Role::Switch
            | Role::Radio
            | Role::Tab
            | Role::MenuItem
    )
}

/// Whether a role takes typed text.
fn types(role: &Role) -> bool {
    matches!(role, Role::TextBox | Role::SearchBox | Role::Password)
}

/// The rows of `lines` that hold `query` (ignoring case), at most 50.
pub fn find_rows(lines: &[String], query: &str) -> Vec<Value> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !layout::matches(line, query).is_empty())
        .take(50)
        .map(|(row, line)| json!({"row": row, "text": line.trim()}))
        .collect()
}

/// A string argument.
fn arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("needs {name:?}"))
}

/// Answer one request on the app's thread: what `verb` does to the
/// browser `b`, with `args`. The socket already refused unknown verbs, and
/// mutating ones in a read-only session.
pub fn handle(
    b: crate::app::Browser,
    verb: &str,
    args: &Value,
    cx: &mut Ctx,
) -> Result<Value, String> {
    use crate::app::{availability, run};
    use crate::engine::HistoryStep;
    use crate::tab::{Command, Intent};
    let page = b.page.get_untracked();
    let reads = b.reads.get_untracked();
    let say = |what: String| b.say(format!("[cyan]agent:[/] {}", crate::clean::markup(&what)));
    let sent = || Ok(json!({"sent": true, "reads": reads}));
    let need_page = || page.clone().ok_or_else(|| "no page yet".to_string());
    match verb {
        "info" => {
            let state = b.state.get_untracked();
            let ready = !state.loading
                && page
                    .as_ref()
                    .is_some_and(|p| p.url == state.url || state.url.is_empty());
            Ok(json!({
                "pid": std::process::id(),
                "url": state.url,
                "title": page.as_ref().map(|p| p.title.clone()).unwrap_or(state.title),
                "loading": state.loading,
                "ready": ready,
                "reads": reads,
                "view": b.view.get_untracked().name(),
                "read_only": b.read_only.get_untracked(),
            }))
        }
        "snapshot" => {
            let page = need_page()?;
            let reader = args.get("reader").and_then(Value::as_bool).unwrap_or(false);
            let width = args
                .get("width")
                .and_then(Value::as_u64)
                .map_or(SNAPSHOT_WIDTH, |w| w.clamp(20, 400) as u16);
            Ok(snapshot(&page, reader, width, b.effective_adapt()))
        }
        "find" => {
            let query = arg(args, "text")?;
            let page = need_page()?;
            let shot = snapshot(&page, false, SNAPSHOT_WIDTH, b.effective_adapt());
            let lines: Vec<String> = shot["text"]
                .as_str()
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect();
            Ok(json!({"matches": find_rows(&lines, query)}))
        }
        "open" => {
            let typed = arg(args, "url")?;
            let here = b.url();
            let destination = b.config.with_untracked(|c| {
                b.bookmarks
                    .with_untracked(|m| c.destination(typed, &here, &m.items))
            });
            let url = destination.url().to_string();
            say(format!("open {url}"));
            b.send(Command::Open(url));
            sent()
        }
        "click" => {
            let page = need_page()?;
            let index = resolve(&page, arg(args, "target")?)?;
            let control = page.controls[index].clone();
            let intent = if clicks(&control.role) {
                Intent::Click
            } else {
                Intent::Focus
            };
            say(format!("click {}", crate::tab::describe(&control)));
            b.act(control.binding(&page), intent, |result, cx| {
                if let Err(refusal) = result {
                    cx.toast(format!(
                        "[red]{}[/]",
                        crate::clean::markup(&refusal.to_string())
                    ));
                }
            });
            sent()
        }
        "type" => {
            let text = arg(args, "text")?.to_string();
            match args.get("into").and_then(Value::as_str) {
                Some(target) => {
                    let page = need_page()?;
                    let index = resolve(&page, target)?;
                    let control = page.controls[index].clone();
                    if !types(&control.role) {
                        return Err(format!(
                            "control {index} is a {}, not a field",
                            control.role.name()
                        ));
                    }
                    say(format!("type into {}", crate::tab::describe(&control)));
                    let intent = Intent::SetValue {
                        value: text,
                        base: control.value.clone(),
                    };
                    b.act(control.binding(&page), intent, |result, cx| {
                        if let Err(refusal) = result {
                            cx.toast(format!(
                                "[red]{}[/]",
                                crate::clean::markup(&refusal.to_string())
                            ));
                        }
                    });
                }
                None => {
                    say("type".into());
                    b.send(Command::Type(text));
                }
            }
            sent()
        }
        "key" => {
            let names = arg(args, "keys")?;
            let keys = rich_intuituive::interact::keymap::try_keys(names)
                .map_err(|name| format!("{name:?} is not a key"))?;
            say(format!("key {names}"));
            for key in keys {
                b.send(Command::Key(key));
            }
            sent()
        }
        "scroll" => {
            let id = match arg(args, "to")? {
                "down" => "page.down",
                "up" => "page.up",
                "top" => "top",
                "bottom" => "bottom",
                other => {
                    return Err(format!(
                        "scroll takes up, down, top or bottom, not {other:?}"
                    ));
                }
            };
            say(format!("scroll {}", arg(args, "to")?));
            run(b, id, cx);
            sent()
        }
        "back" | "forward" => {
            availability(b, verb)?;
            say(verb.to_string());
            b.send(Command::History(if verb == "back" {
                HistoryStep::Back
            } else {
                HistoryStep::Forward
            }));
            sent()
        }
        other => Err(format!("unknown verb {other:?}")),
    }
}

// --- The client: `noctiluca ls` and `noctiluca action …` ---------------

/// `noctiluca action …`, parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionArgs {
    /// `--session PID`; else the only session running.
    pub session: Option<String>,
    pub verb: String,
    pub args: Value,
    /// `--json`: print the reply as JSON.
    pub json: bool,
    /// How long to wait, after an action, for the page to be read again
    /// (`--no-wait`: not at all).
    pub wait: Duration,
}

/// The usage of `noctiluca action`.
pub const ACTION_USAGE: &str = "\
usage: noctiluca ls [--json]
       noctiluca action [--session PID] [--json] [--no-wait] VERB [ARGS…]

  info                         the session: pid, address, title, state
  snapshot [--reader]          the page as text; [ID] marks each control
  find TEXT…                   the rows that hold TEXT
  open URL|KEYWORD WORDS…      open it (as the address bar would)
  click ID|NAME                click a control, by its id or its name
  type [--into ID|NAME] TEXT…  type, into a field or where the focus is
  key KEYS                     press keys in the page (\"enter\", \"ctrl+a tab\")
  scroll up|down|top|bottom    scroll the page
  back | forward               go through the history

The session must run with --control (or --read-only, which refuses every
verb that changes anything).";

/// `noctiluca action ARGS` (the words after `action`).
pub fn parse_action(words: &[String]) -> Result<ActionArgs, String> {
    let mut session = None;
    let mut json_out = false;
    let mut wait = true;
    let mut rest = words.iter().peekable();
    while let Some(word) = rest.peek() {
        match word.as_str() {
            "--session" => {
                rest.next();
                session = Some(rest.next().ok_or("--session needs a pid")?.clone());
            }
            w if w.starts_with("--session=") => {
                session = Some(w["--session=".len()..].to_string());
                rest.next();
            }
            "--json" => {
                json_out = true;
                rest.next();
            }
            "--no-wait" => {
                wait = false;
                rest.next();
            }
            _ => break,
        }
    }
    let verb = rest
        .next()
        .ok_or("action needs a verb (see --help)")?
        .clone();
    let mut words: Vec<String> = rest.cloned().collect();
    let mut flag = |name: &str| {
        let before = words.len();
        words.retain(|w| w != name);
        before != words.len()
    };
    let reader = flag("--reader");
    json_out |= flag("--json");
    let joined = |words: &[String]| words.join(" ");
    let args = match verb.as_str() {
        "info" | "back" | "forward" => json!({}),
        "snapshot" => json!({"reader": reader}),
        "find" | "open" | "click" | "key" if words.is_empty() => {
            return Err(format!("{verb} needs something to {verb}"));
        }
        "find" => json!({"text": joined(&words)}),
        "open" => json!({"url": joined(&words)}),
        "click" => json!({"target": joined(&words)}),
        "key" => json!({"keys": joined(&words)}),
        "scroll" => match words.as_slice() {
            [to] if matches!(to.as_str(), "up" | "down" | "top" | "bottom") => json!({"to": to}),
            _ => return Err("scroll takes up, down, top or bottom".into()),
        },
        "type" => {
            let into = match words.iter().position(|w| w == "--into") {
                Some(at) if at + 1 < words.len() => {
                    let target = words.remove(at + 1);
                    words.remove(at);
                    Some(target)
                }
                Some(_) => return Err("--into needs a control's id or name".into()),
                None => None,
            };
            if words.is_empty() {
                return Err("type needs text".into());
            }
            let mut args = json!({"text": joined(&words)});
            if let Some(into) = into {
                args["into"] = json!(into);
            }
            args
        }
        other => return Err(format!("{other:?} is not a verb (see --help)")),
    };
    // Going somewhere waits for the page to be read; a click may go, so it
    // waits a little.
    let wait = match verb.as_str() {
        _ if !wait => Duration::ZERO,
        "open" | "back" | "forward" => Duration::from_secs(30),
        "click" | "key" | "type" | "scroll" => Duration::from_secs(2),
        _ => Duration::ZERO,
    };
    Ok(ActionArgs {
        session,
        verb,
        args,
        json: json_out,
        wait,
    })
}

/// The running sessions' sockets.
#[cfg(unix)]
pub fn sessions() -> std::io::Result<Vec<std::path::PathBuf>> {
    let dir = rich_interact::control::runtime_dir("noctiluca")?;
    rich_interact::control::sessions(&dir, PREFIX)
}

/// The socket `--session` names, or the only one running.
#[cfg(unix)]
fn session_path(session: Option<&str>) -> std::io::Result<std::path::PathBuf> {
    use std::io::{Error, ErrorKind};
    match session {
        Some(pid) => {
            if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
                return Err(Error::new(ErrorKind::InvalidInput, "--session takes a pid"));
            }
            let dir = rich_interact::control::runtime_dir("noctiluca")?;
            Ok(dir.join(format!("{PREFIX}{pid}.sock")))
        }
        None => {
            let live: Vec<_> = sessions()?
                .into_iter()
                .filter(|p| call(p, "info", json!({})).is_ok())
                .collect();
            match live.as_slice() {
                [one] => Ok(one.clone()),
                [] => Err(Error::new(
                    ErrorKind::NotFound,
                    "no Noctiluca is listening: start one with --control",
                )),
                more => Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "{} sessions are listening: pick one with --session PID (noctiluca ls)",
                        more.len()
                    ),
                )),
            }
        }
    }
}

/// One request to the socket at `path`; the verb's refusal is an error.
#[cfg(unix)]
pub fn call(path: &std::path::Path, verb: &str, args: Value) -> std::io::Result<Value> {
    let request = rich_interact::control::Request::new(verb, args);
    rich_interact::control::call(path, &request, Duration::from_secs(30))?
        .map_err(std::io::Error::other)
}

/// `noctiluca ls`: each session's pid, address and title (one a line, or
/// JSON).
#[cfg(unix)]
pub fn ls(json_out: bool) -> std::io::Result<String> {
    let mut found = Vec::new();
    for path in sessions()? {
        if let Ok(info) = call(&path, "info", json!({})) {
            found.push(info);
        }
    }
    if json_out {
        return Ok(Value::Array(found).to_string());
    }
    if found.is_empty() {
        return Ok("No Noctiluca is listening (start one with --control).".into());
    }
    let lines: Vec<String> = found
        .iter()
        .map(|info| {
            format!(
                "{:>8}  {}{}  {}",
                info["pid"],
                info["url"].as_str().unwrap_or_default(),
                if info["read_only"] == true {
                    " (read-only)"
                } else {
                    ""
                },
                info["title"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    Ok(lines.join("\n"))
}

/// `noctiluca action …`: the reply, as text for people or as JSON. After
/// an action it waits (up to [`ActionArgs::wait`]) for the page to be
/// read again and ready, so a `snapshot` next sees what it did.
#[cfg(unix)]
pub fn action(args: &ActionArgs) -> std::io::Result<String> {
    let path = session_path(args.session.as_deref())?;
    let reply = call(&path, &args.verb, args.args.clone())?;
    if !args.wait.is_zero() {
        let before = reply["reads"].as_u64().unwrap_or(0);
        let start = std::time::Instant::now();
        while start.elapsed() < args.wait {
            std::thread::sleep(Duration::from_millis(50));
            let info = call(&path, "info", json!({}))?;
            if info["ready"] == true && info["reads"].as_u64().unwrap_or(0) > before {
                break;
            }
        }
    }
    if args.json {
        return Ok(reply.to_string());
    }
    Ok(match args.verb.as_str() {
        "snapshot" => format!(
            "# {}\n# {}\n\n{}",
            reply["title"].as_str().unwrap_or_default(),
            reply["url"].as_str().unwrap_or_default(),
            reply["text"].as_str().unwrap_or_default()
        ),
        "find" => reply["matches"]
            .as_array()
            .map(|m| {
                m.iter()
                    .map(|m| {
                        format!(
                            "{:>5}  {}",
                            m["row"].as_u64().unwrap_or_default(),
                            m["text"].as_str().unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "info" => serde_json::to_string_pretty(&reply).unwrap_or_default(),
        _ => "ok".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn action_lines_become_requests() {
        let a = parse_action(&words("--session 42 click Sign up")).unwrap();
        assert_eq!(a.session.as_deref(), Some("42"));
        assert_eq!(
            (a.verb.as_str(), &a.args),
            ("click", &json!({"target": "Sign up"}))
        );
        let a = parse_action(&words("type --into 3 Ada Lovelace")).unwrap();
        assert_eq!(a.args, json!({"text": "Ada Lovelace", "into": "3"}));
        let a = parse_action(&words("snapshot --reader --json")).unwrap();
        assert_eq!(a.args, json!({"reader": true}));
        assert!(a.json && a.wait.is_zero());
        assert_eq!(
            parse_action(&words("open docs.rs")).unwrap().wait,
            Duration::from_secs(30)
        );
        assert!(
            parse_action(&words("--no-wait back"))
                .unwrap()
                .wait
                .is_zero()
        );
        assert!(parse_action(&words("scroll sideways")).is_err());
        assert!(
            parse_action(&words("rm -rf"))
                .unwrap_err()
                .contains("not a verb")
        );
        assert!(parse_action(&[]).is_err());
        assert!(parse_action(&words("type --into")).is_err());
    }
}
