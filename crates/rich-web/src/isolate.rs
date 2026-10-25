//! One process per app session ([`Isolation::Processes`], the default).
//!
//! The server keeps the browser's connection. For each session it starts
//! its own program again (the same executable and arguments, or
//! [`Server::session_command`](crate::Server::session_command)) with
//! [`SESSION_ENV`] set, and listens on a loopback port for that one
//! process. The child's call to [`Server::bind`](crate::Server::bind) (or
//! [`serve`](crate::serve)) sees the variable and, instead of listening,
//! connects back, proves it is the process the server started with a
//! secret from the variable (the server answers with a second one, so a
//! process whose server died serves nobody who took over its port), runs
//! the session there, and exits. The
//! server relays the WebSocket's bytes both ways, so the session loop is
//! the same one a thread runs, and the page cannot tell the difference.
//!
//! A crash, a hang or a runaway allocation then ends one process, not the
//! server: the server tells the page the session ended, kills the process
//! (on Unix with its process group, on Windows with its job object) when
//! the page goes or the server stops, and on Unix can cap each process's
//! memory and CPU time
//! ([`Server::session_memory`](crate::Server::session_memory),
//! [`Server::session_cpu`](crate::Server::session_cpu)).
//!
//! On Windows the server puts each session's process in a job object of
//! its own as soon as it has started, and holds the job's only handle. The
//! job kills its processes when that handle closes: when the session ends,
//! and when the server dies however it dies, since the system closes a
//! dead process's handles. What the session's process starts is in the job
//! too, and cannot leave it (the job allows no breakaway). Only what a
//! process starts in the moment between its start and the server's
//! assignment escapes: the session's own code runs after it (the server
//! answers the process's connection only once it is in the job), so only
//! a program's startup before `serve` can, and a `session_command` that
//! starts the program as a child rather than being it. A server that runs
//! in a job itself (a CI runner, a service) nests the session's job in its
//! own (Windows 8 and later); one whose job forbids that (UI limits) cannot
//! start sessions in processes, and the page is told why.

use std::ffi::OsString;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use intuituive::App;
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::WebSocket;

use crate::{session, Renderer};

/// The variable a session's process is started with: what it needs to
/// connect back and run its session. Not for setting by hand.
pub const SESSION_ENV: &str = "RS_RICH_WEB_SESSION";

/// Where each app session runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Isolation {
    /// A process of its own: this program started again, which runs the
    /// session when it reaches the same `serve` or `Server::bind`. A crash,
    /// a hang or a runaway allocation ends that session alone.
    #[default]
    Processes,
    /// A thread of the server (0.0.18's behaviour): for a program that
    /// cannot start itself again (tests, an app built inside another
    /// program, a REPL).
    Threads,
}

/// How long a session's process has to connect back before the page is
/// told it did not start.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long it has to send its secret once connected (in all, however
/// slowly it trickles in).
const HELLO_TIMEOUT: Duration = Duration::from_secs(2);
/// The most connections waiting to send a secret at once; more are turned
/// away.
const MAX_WAITING: usize = 64;
/// How long an ended session waits for its process to exit before it is
/// killed.
const EXIT_WAIT: Duration = Duration::from_secs(1);
/// How often the relay looks up from its reads (the server stopping, the
/// other side gone).
const TURN: Duration = Duration::from_millis(50);

/// How a session's process is started and limited.
#[derive(Clone, Debug, Default)]
pub(crate) struct Spawn {
    /// The program and its arguments; `None`: this one, with the same.
    pub command: Option<(PathBuf, Vec<OsString>)>,
    /// The most memory (address space) a process may take, in bytes.
    pub memory: Option<u64>,
    /// The most CPU time a process may use.
    pub cpu: Option<Duration>,
}

/// Which `bind` of this program a session belongs to: the parent numbers
/// its binds, and the child runs the one with the same number.
static BINDS: AtomicUsize = AtomicUsize::new(0);

/// The number of the next app server this program binds.
pub(crate) fn next_bind() -> usize {
    BINDS.fetch_add(1, Ordering::SeqCst)
}

/// What a session's process was started with.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    port: u16,
    /// The server's process id: only its own child is a session.
    parent: u32,
    /// What the process sends to prove it is the one started.
    secret: String,
    /// What the server answers to prove it is the server, so a process
    /// whose server died serves nobody who took over its port.
    reply: String,
    bind: usize,
    columns: u16,
    rows: u16,
    renderer: Renderer,
    memory: Option<u64>,
    cpu: Option<u64>,
    /// Compress frames from this size.
    compress: Option<u64>,
}

impl Spec {
    fn encode(&self) -> String {
        let limit = |n: Option<u64>| n.map_or("-".to_string(), |n| n.to_string());
        format!(
            "3 {} {} {} {} {} {} {} {} {} {} {}",
            self.port,
            self.parent,
            self.secret,
            self.reply,
            self.bind,
            self.columns,
            self.rows,
            self.renderer.name(),
            limit(self.memory),
            limit(self.cpu),
            limit(self.compress)
        )
    }

    fn decode(text: &str) -> Option<Spec> {
        let mut parts = text.split(' ');
        if parts.next()? != "3" {
            return None;
        }
        let mut next = || parts.next();
        let limit = |text: &str| match text {
            "-" => Some(None),
            n => n.parse().ok().map(Some),
        };
        let spec = Spec {
            port: next()?.parse().ok()?,
            parent: next()?.parse().ok()?,
            secret: next()?.to_string(),
            reply: next()?.to_string(),
            bind: next()?.parse().ok()?,
            columns: next()?.parse().ok()?,
            rows: next()?.parse().ok()?,
            renderer: Renderer::parse(next()?)?,
            memory: limit(next()?)?,
            cpu: limit(next()?)?,
            compress: limit(next()?)?,
        };
        next().is_none().then_some(spec)
    }
}

/// The session this process was started for, if it was, and this `bind`
/// is the one it belongs to.
pub(crate) fn session_for(bind: usize) -> Option<Spec> {
    let spec = own_spec()?;
    (spec.bind == bind).then_some(spec)
}

/// Whether this process was started for a session (of any `bind`).
pub(crate) fn is_session_process() -> bool {
    own_spec().is_some()
}

/// The spec, when this process is the one the server started. A program
/// the app runs inherits the variable and is not a session: it serves as
/// it would anywhere. On Unix the server starts each session's process
/// leading a process group of its own, which what it runs joins; so a group
/// leader whose parent is no longer the server is a session whose server
/// has died, and exits. A `session_command` must therefore be the program,
/// or `exec` it. (On Windows a session whose server has died is ended by
/// its job object, and the session's process takes the variable out of
/// its environment before the app runs, so what the app starts does not
/// see it.)
fn own_spec() -> Option<Spec> {
    let spec = Spec::decode(&std::env::var(SESSION_ENV).ok()?)?;
    #[cfg(unix)]
    {
        use rustix::process::{getpgrp, getpid, getppid, Pid};
        if Pid::as_raw(getppid()) as u32 != spec.parent {
            if getpgrp() == getpid() {
                eprintln!("rich-web: the session's server has gone");
                std::process::exit(1);
            }
            return None;
        }
    }
    Some(spec)
}

/// In a session's process: limit it, connect back, run the session, and
/// give the exit code.
pub(crate) fn run_child(spec: Spec, app: impl FnOnce() -> App) -> i32 {
    // What this process starts from here on is not the session, and on
    // Windows no process group tells them apart: it does not inherit the
    // variable. (Windows guards its environment, so this is safe beside
    // other threads; on Unix, `own_spec`'s group check does it.)
    #[cfg(windows)]
    std::env::remove_var(SESSION_ENV);
    // Should the server die without ending it, this process ends its whole
    // group. (Not Linux's parent-death signal: that would kill this process
    // alone, before it could end the rest.) On Windows the job object the
    // server holds does that.
    #[cfg(unix)]
    watch_server(spec.parent);
    limit(spec.memory, spec.cpu);
    let stream = match TcpStream::connect(("127.0.0.1", spec.port)) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("rich-web: the session could not reach its server: {error}");
            return 1;
        }
    };
    let _ = stream.set_nodelay(true);
    let mut hello = stream.try_clone().expect("a socket clones");
    if hello
        .write_all(format!("{}\n", spec.secret).as_bytes())
        .is_err()
    {
        return 1;
    }
    // The server answers with the reply; anyone else who took the port
    // when the server died cannot.
    let _ = stream.set_read_timeout(Some(CONNECT_TIMEOUT));
    let mut answer = vec![0u8; spec.reply.len() + 1];
    if hello.read_exact(&mut answer).is_err()
        || !crate::http::same_token(
            std::str::from_utf8(&answer[..spec.reply.len()]).unwrap_or(""),
            &spec.reply,
        )
        || answer[spec.reply.len()] != b'\n'
    {
        eprintln!("rich-web: the session's server did not answer");
        return 1;
    }
    let _ = stream.set_read_timeout(None);
    let config = WebSocketConfig::default()
        .max_message_size(Some(crate::MAX_MESSAGE))
        .max_frame_size(Some(crate::MAX_MESSAGE));
    let mut socket = WebSocket::from_raw_socket(stream, Role::Server, Some(config));
    // The server ends the session by closing the connection, or by killing
    // this process.
    let stopping = AtomicBool::new(false);
    let code = match session::run(
        &mut socket,
        app(),
        spec.columns,
        spec.rows,
        &stopping,
        spec.renderer,
        session::Compress(spec.compress.map(|n| n as usize)),
    ) {
        Ok(()) => 0,
        Err(_) => 1,
    };
    // A server that died closed the connection a moment before it let go
    // of this process: a session ended by that is the watcher's to end, so
    // what the app started does not outlive it. A live server ends the
    // group itself.
    #[cfg(unix)]
    thread::sleep(ENDING);
    code
}

/// How long a session's process waits, once its session ends, for the
/// watcher to see a server that died.
#[cfg(unix)]
const ENDING: Duration = Duration::from_millis(250);

/// How often a session's process looks for its server.
#[cfg(unix)]
const WATCH: Duration = Duration::from_millis(50);

/// Should the server go (killed, so it could not end this process), end
/// this process's whole group: what the app started goes with it. A
/// process that left the group (`setsid`, a daemon) is not reached.
#[cfg(unix)]
fn watch_server(server: u32) {
    use rustix::process::{getpid, getppid, kill_process_group, Pid, Signal};
    thread::spawn(move || loop {
        let parent = Pid::as_raw(getppid()) as u32;
        if parent != server {
            let _ = kill_process_group(getpid(), Signal::KILL);
            std::process::exit(1);
        }
        thread::sleep(WATCH);
    });
}

/// The limits to set: address space in bytes, CPU time in seconds.
#[cfg(unix)]
fn limits(
    memory: Option<u64>,
    cpu: Option<u64>,
) -> impl Iterator<Item = (rustix::process::Resource, rustix::process::Rlimit)> {
    use rustix::process::{Resource, Rlimit};
    // macOS refuses an address-space limit (`EINVAL`): there it has none.
    let memory = memory.filter(|_| !cfg!(target_vendor = "apple"));
    [(Resource::As, memory), (Resource::Cpu, cpu)]
        .into_iter()
        .filter_map(|(resource, value)| {
            let value = value?;
            let limit = Rlimit {
                current: Some(value),
                maximum: Some(value),
            };
            Some((resource, limit))
        })
}

/// Cap this process's address space and CPU time (Unix; elsewhere there is
/// no `setrlimit`, and the limits are not applied). On Linux the server has
/// set them already, from the moment the process started.
#[cfg(unix)]
fn limit(memory: Option<u64>, cpu: Option<u64>) {
    for (resource, limit) in limits(memory, cpu) {
        if let Err(error) = rustix::process::setrlimit(resource, limit) {
            eprintln!("rich-web: the session's limit could not be set: {error}");
        }
    }
}

/// Cap a session's process from the server, as soon as it has started, so
/// the limits hold for its startup too (before it reaches `serve`).
#[cfg(target_os = "linux")]
fn limit_child(pid: u32, memory: Option<u64>, cpu: Option<u64>) -> io::Result<()> {
    let pid = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| io::Error::other("no such process"))?;
    for (resource, limit) in limits(memory, cpu) {
        rustix::process::prlimit(Some(pid), resource, limit)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn limit(_memory: Option<u64>, _cpu: Option<u64>) {}

/// Counts a session's process as running until dropped, so that stopping
/// the server waits for it.
struct Running<'a>(&'a AtomicUsize);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What the parent needs to run one session in a process.
pub(crate) struct Session<'a> {
    pub spawn: &'a Spawn,
    pub bind: usize,
    pub columns: u16,
    pub rows: u16,
    pub renderer: Renderer,
    pub compress: Option<usize>,
    pub stopping: &'a AtomicBool,
    pub running: &'a AtomicUsize,
}

/// Run a session in a process of its own, relaying `browser`'s WebSocket
/// (upgraded already) to it, until either side ends or the server stops.
pub(crate) fn run_parent(browser: TcpStream, session: Session<'_>) {
    session.running.fetch_add(1, Ordering::SeqCst);
    let _counted = Running(session.running);
    if session.stopping.load(Ordering::SeqCst) {
        return goodbye(browser, "The server stopped.");
    }
    let started = match start(&session) {
        Ok(started) => started,
        Err(error) => {
            return goodbye(
                browser,
                &format!("The session's process could not start: {error}."),
            );
        }
    };
    let (mut child, tree, listener, secrets) = started;
    let child_stream = match connected(&listener, &secrets, &mut child, session.stopping) {
        Ok(stream) => stream,
        Err(why) => {
            end(&mut child, tree);
            return goodbye(browser, &why);
        }
    };
    drop(listener);
    let relayed = relay(&browser, &child_stream, session.stopping);
    // The page went, the process ended, or the server is stopping: the
    // process's connection closes, and its session ends. It gets a moment
    // to exit, and is killed if it has not.
    let _ = child_stream.shutdown(Shutdown::Both);
    let status = end(&mut child, tree);
    // The page is told why, when it is still there, between frames, and
    // was not sent a close by the session itself.
    let reason = match relayed.ended {
        _ if !relayed.can_close => None,
        _ if session.stopping.load(Ordering::SeqCst) => Some("The server stopped.".to_string()),
        Ended::Page => None,
        Ended::Child => Some(match status {
            Some(status) if status.success() => "The session ended.".to_string(),
            Some(status) => format!("The session's process ended unexpectedly ({status})."),
            None => "The session's process ended unexpectedly.".to_string(),
        }),
    };
    match reason {
        Some(reason) => goodbye(browser, &reason),
        None => {
            let _ = browser.shutdown(Shutdown::Both);
        }
    }
}

/// Start the process, with what ends it and what it starts, a loopback
/// listener for it to connect to, and the secret it must send.
fn start(session: &Session<'_>) -> io::Result<(Child, Tree, TcpListener, Secrets)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let secrets = Secrets {
        secret: crate::random_token()?,
        reply: crate::random_token()?,
    };
    let spec = Spec {
        port: listener.local_addr()?.port(),
        parent: std::process::id(),
        secret: secrets.secret.clone(),
        reply: secrets.reply.clone(),
        bind: session.bind,
        columns: session.columns,
        rows: session.rows,
        renderer: session.renderer,
        memory: session.spawn.memory,
        cpu: session.spawn.cpu.map(|cpu| cpu.as_secs().max(1)),
        compress: session.compress.map(|n| n as u64),
    };
    let (program, args) = match &session.spawn.command {
        Some((program, args)) => (program.clone(), args.clone()),
        None => (
            std::env::current_exe()?,
            std::env::args_os().skip(1).collect(),
        ),
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .env(SESSION_ENV, spec.encode())
        .stdin(Stdio::null());
    // Its own process group, so that what it starts is ended with it.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn()?;
    #[cfg(target_os = "linux")]
    if let Err(error) = limit_child(child.id(), spec.memory, spec.cpu)
        .and_then(|()| within_memory(child.id(), spec.memory))
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other(format!(
            "its limits could not be set: {error}"
        )));
    }
    // Before the process is answered (it runs nothing of the session's
    // until then), so all the session starts is in the job.
    let tree = match Tree::new(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(format!(
                "it could not be put in a job object: {error}"
            )));
        }
    };
    listener.set_nonblocking(true)?;
    Ok((child, tree, listener, secrets))
}

/// What ends a session's process and everything it started: on Unix the
/// process group it leads, on Windows a job object whose only handle the
/// server holds (dropped, or closed by the system when the server dies, it
/// kills every process in the job).
struct Tree {
    #[cfg(unix)]
    leader: u32,
    #[cfg(windows)]
    _job: win32job::Job,
}

impl Tree {
    /// The tree of `child`, just started.
    #[cfg(unix)]
    fn new(child: &Child) -> io::Result<Tree> {
        Ok(Tree { leader: child.id() })
    }

    /// The tree of `child`, just started: a job object that kills its
    /// processes when its last handle closes, which `child` is put in.
    /// Without breakaway, what it starts stays in the job.
    #[cfg(windows)]
    fn new(child: &Child) -> io::Result<Tree> {
        use std::os::windows::io::AsRawHandle;
        let mut limits = win32job::ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        let job = win32job::Job::create_with_limit_info(&limits)?;
        job.assign_process(child.as_raw_handle() as isize)?;
        Ok(Tree { _job: job })
    }

    #[cfg(not(any(unix, windows)))]
    fn new(_child: &Child) -> io::Result<Tree> {
        Ok(Tree {})
    }

    /// Kill what is left of it.
    fn kill(self) {
        #[cfg(unix)]
        kill_group(self.leader);
        // On Windows, dropping the job closes its handle, which kills it.
    }
}

/// The secret a session's process sends, and the reply the server answers.
struct Secrets {
    secret: String,
    reply: String,
}

/// Refuse a process that took more memory than its limit before the limit
/// was set (it raced ahead of `prlimit`): the limit holds from then on, so
/// its peak until then is all there is to check.
#[cfg(target_os = "linux")]
fn within_memory(pid: u32, memory: Option<u64>) -> io::Result<()> {
    let Some(memory) = memory else {
        return Ok(());
    };
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
    let peak = status
        .lines()
        .find_map(|line| line.strip_prefix("VmPeak:"))
        .and_then(|kb| kb.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .unwrap_or(0)
        * 1024;
    if peak > memory {
        return Err(io::Error::other(format!(
            "it took {peak} bytes before they were set"
        )));
    }
    Ok(())
}

/// Wait for the process to connect and send its secret, then answer with
/// the reply. Anything else that connects meanwhile is turned away; every
/// connection is read at once, each for at most [`HELLO_TIMEOUT`], so none
/// holds up the process or the server stopping.
fn connected(
    listener: &TcpListener,
    secrets: &Secrets,
    child: &mut Child,
    stopping: &AtomicBool,
) -> Result<TcpStream, String> {
    let until = Instant::now() + CONNECT_TIMEOUT;
    let mut waiting: Vec<(TcpStream, Vec<u8>, Instant)> = Vec::new();
    loop {
        if stopping.load(Ordering::SeqCst) {
            return Err("The server stopped.".into());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "The session's process ended before it started the app ({status}): \
                 it must reach the same serve() call as the server."
            ));
        }
        let now = Instant::now();
        if now > until {
            return Err("The session's process did not start the app in time.".into());
        }
        while let Ok((stream, _)) = listener.accept() {
            if waiting.len() < MAX_WAITING && stream.set_nonblocking(true).is_ok() {
                waiting.push((stream, Vec::new(), now + HELLO_TIMEOUT));
            }
        }
        let mut found = None;
        waiting.retain_mut(|(stream, line, by)| {
            if found.is_some() {
                return true;
            }
            match hello(stream, line, &secrets.secret) {
                Some(true) => {
                    found = stream.try_clone().ok();
                    false
                }
                Some(false) => false,
                None => now < *by,
            }
        });
        if let Some(mut stream) = found {
            let answered = stream
                .set_nonblocking(false)
                .and_then(|()| stream.write_all(format!("{}\n", secrets.reply).as_bytes()));
            if answered.is_ok() {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            continue;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Read what `stream` (non-blocking) has sent of its secret into `line`:
/// `Some(true)` once it is the secret and a newline, `Some(false)` once it
/// cannot be, `None` until there is more. Only the secret's bytes are read,
/// one at a time: what follows is the session's.
fn hello(stream: &TcpStream, line: &mut Vec<u8>, secret: &str) -> Option<bool> {
    let mut byte = [0u8];
    loop {
        match (&*stream).read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => {
                return Some(
                    std::str::from_utf8(line)
                        .is_ok_and(|line| crate::http::same_token(line, secret)),
                )
            }
            Ok(1) if line.len() < secret.len() => line.push(byte[0]),
            Err(e) if e.kind() == ErrorKind::WouldBlock => return None,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            _ => return Some(false),
        }
    }
}

/// Which side a relay ended on.
#[derive(Debug, PartialEq, Eq)]
enum Ended {
    /// The page went (or the server stopped).
    Page,
    /// The process's side ended.
    Child,
}

/// How a relay ended.
struct Relayed {
    ended: Ended,
    /// What reached the page stopped between WebSocket frames, and held no
    /// close: the server can still send one.
    can_close: bool,
}

/// Copy bytes both ways until one side ends or the server stops.
fn relay(browser: &TcpStream, child: &TcpStream, stopping: &AtomicBool) -> Relayed {
    let failed = Relayed {
        ended: Ended::Page,
        can_close: true,
    };
    let done = Arc::new(AtomicBool::new(false));
    let (Ok(page_in), Ok(child_out)) = (browser.try_clone(), child.try_clone()) else {
        return failed;
    };
    let _ = page_in.set_read_timeout(Some(TURN));
    let _ = child.set_read_timeout(Some(TURN));
    // A process that has stopped reading (a hang) must not hold the relay
    // in a write: writes to it give up each turn, and try again unless the
    // relay is ending.
    let _ = child.set_write_timeout(Some(TURN));
    // The page's input, to the process, on a thread of its own.
    let upstream = {
        let done = done.clone();
        thread::Builder::new()
            .name("rich-web relay".into())
            .spawn(move || {
                pump(page_in, child_out, &done, &mut |_| {}, true);
                done.store(true, Ordering::SeqCst);
            })
    };
    // What the process draws, to the page, on this one, keeping track of
    // where its frames begin.
    let mut frames = Frames::default();
    let (Ok(child_in), Ok(page_out)) = (child.try_clone(), browser.try_clone()) else {
        done.store(true, Ordering::SeqCst);
        return failed;
    };
    let child_ended = {
        let stop = StopOr(&done, stopping);
        pump_until(
            child_in,
            page_out,
            &stop,
            &mut |bytes| frames.feed(bytes),
            false,
        )
    };
    done.store(true, Ordering::SeqCst);
    if let Ok(upstream) = upstream {
        let _ = upstream.join();
    }
    Relayed {
        ended: if child_ended {
            Ended::Child
        } else {
            Ended::Page
        },
        can_close: frames.at_boundary() && !frames.closed,
    }
}

/// Either of two flags.
struct StopOr<'a>(&'a AtomicBool, &'a AtomicBool);

impl StopOr<'_> {
    fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst) || self.1.load(Ordering::SeqCst)
    }
}

/// Copy from `from` to `to` until either ends or `done` is set.
fn pump(
    from: TcpStream,
    to: TcpStream,
    done: &AtomicBool,
    seen: &mut dyn FnMut(&[u8]),
    patient: bool,
) {
    let never = AtomicBool::new(false);
    pump_until(from, to, &StopOr(done, &never), seen, patient);
}

/// Copy from `from` to `to` until either ends or `stop` is set; whether
/// `from` ended (rather than `to`, or `stop`). `patient`: a write that
/// times out is tried again (until `stop`) rather than ending the copy.
fn pump_until(
    mut from: TcpStream,
    mut to: TcpStream,
    stop: &StopOr<'_>,
    seen: &mut dyn FnMut(&[u8]),
    patient: bool,
) -> bool {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        if stop.is_set() {
            return false;
        }
        match from.read(&mut buffer) {
            Ok(0) => return true,
            Ok(n) => {
                seen(&buffer[..n]);
                if !write_until(&mut to, &buffer[..n], stop, patient) {
                    return false;
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return true,
        }
    }
}

/// Write all of `bytes`; whether it was. With `patient`, a write that times
/// out is tried again until `stop` is set; without, it ends the copy.
fn write_until(to: &mut TcpStream, mut bytes: &[u8], stop: &StopOr<'_>, patient: bool) -> bool {
    while !bytes.is_empty() {
        match to.write(bytes) {
            Ok(0) => return false,
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e)
                if patient && matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                if stop.is_set() {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// Where a stream of WebSocket frames is: inside a frame, or between two,
/// and whether a close went by.
#[derive(Default)]
struct Frames {
    /// The header read so far of the next frame.
    header: Vec<u8>,
    /// Payload bytes left in the current frame.
    left: u64,
    closed: bool,
}

impl Frames {
    fn at_boundary(&self) -> bool {
        self.header.is_empty() && self.left == 0
    }

    fn feed(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.left > 0 {
                let n = (self.left.min(bytes.len() as u64)) as usize;
                self.left -= n as u64;
                bytes = &bytes[n..];
                continue;
            }
            self.header.push(bytes[0]);
            bytes = &bytes[1..];
            if let Some((length, opcode)) = header_done(&self.header) {
                if opcode == 0x8 {
                    self.closed = true;
                }
                self.header.clear();
                self.left = length;
            }
        }
    }
}

/// When `header` is a whole frame header: the payload's length and the
/// opcode.
fn header_done(header: &[u8]) -> Option<(u64, u8)> {
    if header.len() < 2 {
        return None;
    }
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    let short = u64::from(header[1] & 0x7f);
    let (extended, length) = match short {
        126 => (2, None),
        127 => (8, None),
        n => (0, Some(n)),
    };
    let mask = if masked { 4 } else { 0 };
    if header.len() < 2 + extended + mask {
        return None;
    }
    let length = length.unwrap_or_else(|| {
        header[2..2 + extended]
            .iter()
            .fold(0u64, |n, b| (n << 8) | u64::from(*b))
    });
    Some((length, opcode))
}

/// End the session's process: a moment to exit, then killed (on Unix with
/// its process group, on Windows with its job). Its exit status, when it
/// exited on its own.
fn end(child: &mut Child, tree: Tree) -> Option<std::process::ExitStatus> {
    let until = Instant::now() + EXIT_WAIT;
    while Instant::now() < until {
        match child.try_wait() {
            Ok(Some(status)) => {
                tree.kill();
                return Some(status);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }
    tree.kill();
    let _ = child.kill();
    let _ = child.wait();
    None
}

/// Kill what is left of the process group the session's process led.
#[cfg(unix)]
fn kill_group(leader: u32) {
    use rustix::process::{kill_process_group, Pid, Signal};
    if let Some(pid) = i32::try_from(leader).ok().and_then(Pid::from_raw) {
        let _ = kill_process_group(pid, Signal::KILL);
    }
}

/// Close the page's WebSocket with `reason`, which it shows, then let the
/// connection go.
fn goodbye(mut browser: TcpStream, reason: &str) {
    let mut end = reason.len().min(123);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    let mut payload = 1000u16.to_be_bytes().to_vec();
    payload.extend_from_slice(&reason.as_bytes()[..end]);
    let mut frame = vec![0x88, payload.len() as u8];
    frame.extend_from_slice(&payload);
    let _ = browser.write_all(&frame);
    let _ = browser.flush();
    // A moment for the page to answer, so the close is not cut off by a
    // reset.
    let _ = browser.set_read_timeout(Some(Duration::from_millis(200)));
    let _ = browser.read(&mut [0u8; 256]);
    let _ = browser.shutdown(Shutdown::Both);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_round_trip() {
        let spec = Spec {
            port: 4321,
            parent: 99,
            secret: "abc".into(),
            reply: "def".into(),
            bind: 2,
            columns: 80,
            rows: 24,
            renderer: Renderer::Dom,
            memory: Some(1 << 28),
            cpu: None,
            compress: Some(1024),
        };
        assert_eq!(Spec::decode(&spec.encode()), Some(spec));
        // 0.0.19's first form, without the parent and the reply.
        assert_eq!(Spec::decode("1 1 a 0 80 24 xterm - -"), None);
        assert_eq!(Spec::decode("3 1 9 a b 0 80 24 xterm - -"), None);
        assert_eq!(Spec::decode("2 1 9 a b 0 80 24 xterm - -"), None);
        assert_eq!(Spec::decode("3 1 9 a b 0 80 24 canvas - - -"), None);
    }

    #[test]
    fn frames_are_followed_across_reads() {
        let mut frames = Frames::default();
        // A text frame of 3 bytes, split across reads.
        frames.feed(&[0x81, 0x03, b'a']);
        assert!(!frames.at_boundary());
        frames.feed(b"bc");
        assert!(frames.at_boundary());
        // A 300-byte binary frame: a 16-bit length.
        let mut big = vec![0x82, 126, 0x01, 0x2c];
        big.extend(vec![0u8; 300]);
        frames.feed(&big[..3]);
        assert!(!frames.at_boundary());
        frames.feed(&big[3..]);
        assert!(frames.at_boundary());
        assert!(!frames.closed);
        frames.feed(&[0x88, 0x02, 0x03, 0xe8]);
        assert!(frames.closed && frames.at_boundary());
    }

    #[test]
    fn a_write_to_a_process_that_stopped_reading_gives_up_when_told() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut writer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        // The other end never reads: the socket's buffers fill.
        let (_reader, _) = listener.accept().unwrap();
        writer
            .set_write_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let stopping = Arc::new(AtomicBool::new(false));
        let never = AtomicBool::new(false);
        let stopper = {
            let stopping = stopping.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(300));
                stopping.store(true, Ordering::SeqCst);
            })
        };
        let started = Instant::now();
        let big = vec![0u8; 64 << 20];
        let stop = StopOr(&never, &stopping);
        assert!(!write_until(&mut writer, &big, &stop, true));
        assert!(started.elapsed() < Duration::from_secs(5));
        stopper.join().unwrap();
    }

    #[test]
    fn a_masked_header_counts_its_key() {
        assert_eq!(header_done(&[0x81, 0x85]), None);
        assert_eq!(header_done(&[0x81, 0x85, 1, 2, 3]), None);
        assert_eq!(header_done(&[0x81, 0x85, 1, 2, 3, 4]), Some((5, 1)));
    }
}
