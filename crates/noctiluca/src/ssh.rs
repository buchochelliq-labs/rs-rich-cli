//! `--ssh HOST`: Chrome runs on this machine and its network goes through
//! HOST (#817). A dev server on the remote's `localhost` then opens as if
//! it were local, with no browser on the remote and no frames over SSH.
//!
//! - **The tunnel** is `ssh -N -D 127.0.0.1:PORT … -- HOST`, a child of
//!   Noctiluca, with `ExitOnForwardFailure=yes` and keep-alives. It listens
//!   on the loopback address only: nothing on the network reaches it. The
//!   port is drawn at random from the system's randomness
//!   ([`random_port`]), not handed out in order. (Chrome takes only a TCP
//!   SOCKS proxy, so a Unix-socket forward is not an option.)
//! - **Chrome** gets `--proxy-server=socks5://127.0.0.1:PORT`, with names
//!   resolved on the far side (`--host-resolver-rules`), loopback sent
//!   through the proxy too (`<-loopback>`), and WebRTC and QUIC kept off
//!   the direct path ([`chrome_switches`]).
//! - **Ready** means the child is alive, the port answers a SOCKS5
//!   greeting, and (on Linux) the listener on it is this user's: another
//!   user's program on the port is refused ([`listener_uid`]).
//! - **Lifecycle:** dropping the [`Tunnel`] stops ssh; on Linux ssh is
//!   also stopped if Noctiluca dies (`PR_SET_PDEATHSIG`). A dropped tunnel
//!   is reconnected on the same port with `BatchMode=yes`, so it never
//!   prompts while Noctiluca owns the terminal.
//!
//! The threat model's Noctiluca section has what crosses the connection
//! and what it means for the profile.

use std::collections::VecDeque;
use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How the tunnel is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshOptions {
    /// `HOST` or `user@host`, or a `Host` of `~/.ssh/config`.
    pub host: String,
    /// The user's own ssh options (`--ssh-arg`), before the host.
    pub args: Vec<String>,
    /// The ssh client to run: `ssh` on `PATH` unless a test says
    /// otherwise.
    pub program: PathBuf,
}

impl SshOptions {
    pub fn new(host: impl Into<String>) -> SshOptions {
        SshOptions {
            host: host.into(),
            args: Vec::new(),
            program: PathBuf::from("ssh"),
        }
    }
}

/// Whether `host` can be given to ssh as its destination: not empty, not
/// an option (`-oProxyCommand=…` would run a command), no spaces or
/// control characters.
pub fn check_host(host: &str) -> Result<(), String> {
    if host.is_empty() {
        return Err("--ssh needs a host".into());
    }
    if host.starts_with('-') {
        return Err(format!(
            "--ssh takes a host, not an option ({host:?}); pass ssh's options with --ssh-arg"
        ));
    }
    if host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("--ssh: {host:?} is not a host name"));
    }
    Ok(())
}

/// ssh's arguments for a tunnel on `port`: a dynamic (SOCKS) forward on
/// the loopback address only, no command, no standard input, keep-alives,
/// failing if the port cannot be had; with `batch`, never prompting (a
/// reconnect, while Noctiluca owns the terminal). The user's own options
/// come after ours, so ssh's first-wins rule keeps ours; the host comes
/// after `--`.
pub fn tunnel_args(options: &SshOptions, port: u16, batch: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "-N",
        "-n",
        "-D",
        &format!("127.0.0.1:{port}"),
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if batch {
        args.extend(["-o".to_string(), "BatchMode=yes".to_string()]);
    }
    args.extend(options.args.iter().cloned());
    args.push("--".into());
    args.push(options.host.clone());
    args
}

/// Chrome's switches for a tunnel on `port`:
///
/// - every request through the SOCKS5 proxy, loopback too (`<-loopback>`
///   takes away Chrome's own bypass for it), so `localhost` is the far
///   side's;
/// - no name resolved here: every host maps to "not found" but the
///   proxy's own address, so Chrome hands names to the proxy (SOCKS5 with
///   remote DNS);
/// - WebRTC only over the proxy, so no UDP goes out directly from this
///   machine, and no QUIC, which a SOCKS proxy cannot carry.
pub fn chrome_switches(port: u16) -> Vec<String> {
    vec![
        format!("--proxy-server=socks5://127.0.0.1:{port}"),
        "--proxy-bypass-list=<-loopback>".into(),
        "--host-resolver-rules=MAP * ~NOTFOUND , EXCLUDE 127.0.0.1".into(),
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".into(),
        "--disable-quic".into(),
    ]
}

/// What the tunnel is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TunnelState {
    /// ssh runs and the port answers.
    Up,
    /// ssh was started again and the port does not answer yet.
    Connecting,
    /// ssh exited: why, as it said.
    Down(String),
}

/// The SSH tunnel: an `ssh -D` child. Dropping it stops ssh.
pub struct Tunnel {
    options: SshOptions,
    port: u16,
    child: Option<Child>,
    /// ssh's last lines on standard error, read on a thread of their own
    /// so ssh never blocks on them and nothing reaches the screen.
    said: Arc<Mutex<VecDeque<String>>>,
    /// The thread reading them, to let finish once ssh exits.
    reader: Option<std::thread::JoinHandle<()>>,
    state: TunnelState,
}

impl std::fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tunnel")
            .field("host", &self.options.host)
            .field("port", &self.port)
            .field("state", &self.state)
            .finish()
    }
}

/// How many times a port another program took first is given up for a
/// new one before giving up.
const PORT_TRIES: usize = 3;

impl Tunnel {
    /// Start ssh and wait up to `wait` for the tunnel (ssh may ask for a
    /// password or a host key on the terminal meanwhile). Fails with what
    /// ssh said: a missing `ssh`, a refused login, a host it cannot reach.
    pub fn open(options: SshOptions, wait: Duration) -> io::Result<Tunnel> {
        check_host(&options.host).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut last = None;
        for _ in 0..PORT_TRIES {
            let port = free_port()?;
            let mut tunnel = Tunnel {
                options: options.clone(),
                port,
                child: None,
                said: Arc::new(Mutex::new(VecDeque::new())),
                reader: None,
                state: TunnelState::Connecting,
            };
            tunnel.spawn(false)?;
            match tunnel.wait_up(wait) {
                Ok(()) => return Ok(tunnel),
                Err(error) if error.kind() == io::ErrorKind::AddrInUse => last = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::other("no port for the tunnel")))
    }

    /// The tunnel's port on `127.0.0.1`.
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn host(&self) -> &str {
        &self.options.host
    }

    /// Chrome's switches for this tunnel ([`chrome_switches`]).
    pub fn chrome_switches(&self) -> Vec<String> {
        chrome_switches(self.port)
    }

    /// Look at the child, without waiting: whether it is still up, has
    /// come up since [`reconnect`](Self::reconnect), or has exited.
    pub fn check(&mut self) -> TunnelState {
        let exited = match self.child.as_mut() {
            None => true,
            Some(child) => matches!(child.try_wait(), Ok(Some(_)) | Err(_)),
        };
        if exited {
            if !matches!(self.state, TunnelState::Down(_)) {
                self.child = None;
                self.finish_reading();
                self.state = TunnelState::Down(self.why_down());
            }
        } else if self.state == TunnelState::Connecting && self.ready() {
            self.state = match self.owned() {
                Ok(()) => TunnelState::Up,
                Err(error) => {
                    self.stop();
                    TunnelState::Down(error.to_string())
                }
            };
        }
        self.state.clone()
    }

    /// Start ssh again on the same port (Chrome's proxy switch names it),
    /// never prompting; [`check`](Self::check) says when it is up. Nothing
    /// if it is running.
    pub fn reconnect(&mut self) -> io::Result<()> {
        if self.child.is_some() {
            return Ok(());
        }
        self.said.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.spawn(true)?;
        self.state = TunnelState::Connecting;
        Ok(())
    }

    fn spawn(&mut self, batch: bool) -> io::Result<()> {
        let mut command = Command::new(&self.options.program);
        command
            .args(tunnel_args(&self.options, self.port, batch))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        stop_with_parent(&mut command);
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    error.kind(),
                    format!(
                        "no {} found (install OpenSSH's client)",
                        self.options.program.display()
                    ),
                )
            } else {
                error
            }
        })?;
        if let Some(stderr) = child.stderr.take() {
            let said = self.said.clone();
            self.reader = Some(std::thread::spawn(move || {
                for line in io::BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut said = said.lock().unwrap_or_else(|e| e.into_inner());
                    said.push_back(crate::clean::clean(line.trim()));
                    while said.len() > 8 {
                        said.pop_front();
                    }
                }
            }));
        }
        self.child = Some(child);
        Ok(())
    }

    /// Wait for the tunnel to come up, or ssh to exit.
    fn wait_up(&mut self, wait: Duration) -> io::Result<()> {
        let end = Instant::now() + wait;
        loop {
            match self.check() {
                TunnelState::Up => return Ok(()),
                TunnelState::Down(why) => {
                    let kind = if port_taken(&why) {
                        io::ErrorKind::AddrInUse
                    } else {
                        io::ErrorKind::ConnectionRefused
                    };
                    return Err(io::Error::new(kind, why));
                }
                TunnelState::Connecting => {}
            }
            if Instant::now() >= end {
                self.stop();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "the tunnel to {} did not come up in {} s",
                        self.options.host,
                        wait.as_secs()
                    ),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Whether the port answers a SOCKS5 greeting (no authentication).
    fn ready(&self) -> bool {
        socks_greets(self.port, Duration::from_millis(300))
    }

    /// On Linux, refuses a listener on the port that is not this user's:
    /// another user's program that took the port would see every request.
    fn owned(&self) -> io::Result<()> {
        match listener_uid(self.port) {
            Some(uid) if Some(uid) != current_uid() => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "port {} is another user's (uid {uid}), not ssh's: refused",
                    self.port
                ),
            )),
            _ => Ok(()),
        }
    }

    /// Let the reader take ssh's last words, for half a second at most (a
    /// program ssh started may hold the pipe open).
    fn finish_reading(&mut self) {
        let Some(reader) = self.reader.take() else {
            return;
        };
        let end = Instant::now() + Duration::from_millis(500);
        while !reader.is_finished() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        if reader.is_finished() {
            let _ = reader.join();
        }
    }

    /// Why ssh exited, from what it said.
    fn why_down(&self) -> String {
        let said: Vec<String> = self
            .said
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|l| !l.is_empty() && !l.starts_with("Warning: Permanently added"))
            .cloned()
            .collect();
        explain(&self.options.host, &said)
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A [`Tunnel`] the app and `main` share: the app checks and reconnects
/// it, and `main` drops the last one, which stops ssh.
#[derive(Clone, Debug)]
pub struct TunnelLink(pub Arc<Mutex<Tunnel>>);

impl TunnelLink {
    pub fn new(tunnel: Tunnel) -> TunnelLink {
        TunnelLink(Arc::new(Mutex::new(tunnel)))
    }

    pub fn host(&self) -> String {
        self.lock().host().to_string()
    }

    pub fn check(&self) -> TunnelState {
        self.lock().check()
    }

    pub fn reconnect(&self) -> io::Result<()> {
        self.lock().reconnect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tunnel> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What ssh's last lines `said` mean, in one line: a refused login, a host
/// it cannot reach, a port in use, or its own last line.
pub fn explain(host: &str, said: &[String]) -> String {
    let has = |text: &str| said.iter().any(|l| l.contains(text));
    let last = said.last().cloned().unwrap_or_default();
    if has("Permission denied") || has("Too many authentication failures") {
        format!("SSH refused the login to {host}: {last}")
    } else if has("Host key verification failed") {
        format!("SSH could not verify {host}'s host key: {last}")
    } else if said.iter().any(|l| port_taken(l)) {
        format!("the tunnel's port was taken: {last}")
    } else if last.is_empty() {
        format!("ssh to {host} exited")
    } else {
        format!("ssh to {host}: {last}")
    }
}

/// Whether ssh's `line` says the local port could not be had.
fn port_taken(line: &str) -> bool {
    line.contains("Address already in use")
        || line.contains("cannot listen to port")
        || line.contains("Could not request local forwarding")
        || line.contains("port was taken")
}

/// The ports the tunnel's is drawn from: unprivileged, and above the ones
/// local services most often use.
pub const TUNNEL_PORTS: std::ops::RangeInclusive<u16> = 10_000..=65_535;

/// A port of `127.0.0.1` free now, drawn at random ([`random_port`]); the
/// system's own choice if none of a few draws is free.
fn free_port() -> io::Result<u16> {
    for _ in 0..32 {
        let port = random_port();
        if TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_ok() {
            return Ok(port);
        }
    }
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

/// A port in [`TUNNEL_PORTS`] from the system's randomness (bound and let
/// go by the caller: ssh's `ExitOnForwardFailure` catches one taken in
/// between). The draw is uniform, not the next port in the kernel's order.
pub fn random_port() -> u16 {
    let (low, high) = (*TUNNEL_PORTS.start() as u64, *TUNNEL_PORTS.end() as u64);
    (low + random_u64() % (high - low + 1)) as u16
}

/// 64 random bits: `/dev/urandom` on Unix; elsewhere std's per-process
/// hash keys (seeded from the system's randomness), over the time.
fn random_u64() -> u64 {
    #[cfg(unix)]
    {
        let mut bytes = [0u8; 8];
        if std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut bytes))
            .is_ok()
        {
            return u64::from_ne_bytes(bytes);
        }
    }
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.write_u32(std::process::id());
    hasher.finish()
}

/// Whether a SOCKS5 server on `127.0.0.1:port` answers the greeting "no
/// authentication" with "accepted".
pub fn socks_greets(port: u16, timeout: Duration) -> bool {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into();
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let mut answer = [0u8; 2];
    stream.write_all(&[5, 1, 0]).is_ok()
        && stream.read_exact(&mut answer).is_ok()
        && answer == [5, 0]
}

/// The uid that owns the listening TCP socket on `127.0.0.1:port`, from
/// `/proc/net/tcp`; `None` elsewhere, or with no such listener.
pub fn listener_uid(port: u16) -> Option<u32> {
    let table = std::fs::read_to_string("/proc/net/tcp").ok()?;
    listener_uid_in(&table, port)
}

/// [`listener_uid`] over the text of `/proc/net/tcp`: the line in state
/// `0A` (listening) whose local address is a loopback or any address with
/// this port.
pub fn listener_uid_in(table: &str, port: u16) -> Option<u32> {
    table.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (local, state, uid) = (fields.get(1)?, fields.get(3)?, fields.get(7)?);
        let (address, at) = local.split_once(':')?;
        let at = u16::from_str_radix(at, 16).ok()?;
        // The address in the kernel's byte order: loopback (127.x.x.x) or
        // any (0.0.0.0), whichever the machine's endianness.
        let raw = u32::from_str_radix(address, 16).ok()?;
        let ours = raw == 0 || raw.to_le_bytes()[0] == 127 || raw.to_be_bytes()[0] == 127;
        (*state == "0A" && at == port && ours).then(|| uid.parse().ok())?
    })
}

/// This process's user: the owner of `/proc/self` (Linux, where
/// [`listener_uid`] reads anything at all).
#[cfg(unix)]
fn current_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

#[cfg(not(unix))]
fn current_uid() -> Option<u32> {
    None
}

/// On Linux, ssh gets SIGTERM when Noctiluca dies, so a killed Noctiluca
/// leaves no tunnel behind. Only for a child spawned from a thread that
/// lives as long as the process (the death signal follows the spawning
/// thread): `main`'s, and the app's, which is the same.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)] // pre_exec is unsafe: the closure only calls prctl.
fn stop_with_parent(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: prctl is async-signal-safe, and nothing else runs between
    // fork and exec.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn stop_with_parent(_: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_never_an_option_or_two_words() {
        assert!(check_host("me@dev.example").is_ok());
        assert!(check_host("devbox").is_ok());
        assert!(check_host("").is_err());
        assert!(
            check_host("-oProxyCommand=sh")
                .unwrap_err()
                .contains("--ssh-arg")
        );
        assert!(check_host("dev box").is_err());
        assert!(check_host("dev\x1b[2Jbox").is_err());
    }

    #[test]
    fn the_tunnel_listens_on_loopback_and_the_host_comes_last() {
        let mut options = SshOptions::new("me@dev");
        options.args = vec!["-p".into(), "2222".into()];
        let args = tunnel_args(&options, 41000, false);
        assert_eq!(
            args,
            [
                "-N",
                "-n",
                "-D",
                "127.0.0.1:41000",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-p",
                "2222",
                "--",
                "me@dev"
            ]
        );
        let batch = tunnel_args(&options, 41000, true);
        assert!(batch.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert_eq!(batch.last().map(String::as_str), Some("me@dev"));
    }

    #[test]
    fn chrome_resolves_nothing_itself_and_sends_loopback_through_the_proxy() {
        let switches = chrome_switches(41000);
        assert!(switches.contains(&"--proxy-server=socks5://127.0.0.1:41000".to_string()));
        assert!(switches.contains(&"--proxy-bypass-list=<-loopback>".to_string()));
        assert!(
            switches
                .iter()
                .any(|s| s.starts_with("--host-resolver-rules=MAP * ~NOTFOUND"))
        );
        assert!(
            switches
                .iter()
                .any(|s| s.contains("disable_non_proxied_udp"))
        );
        // Nothing that weakens the sandbox or opens DevTools.
        assert!(
            !switches
                .iter()
                .any(|s| s.contains("sandbox") || s.contains("debugging"))
        );
    }

    #[test]
    fn what_ssh_said_is_explained_in_one_line() {
        let said = |lines: &[&str]| lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        assert!(
            explain("dev", &said(&["me@dev: Permission denied (publickey)."]))
                .starts_with("SSH refused the login to dev")
        );
        assert!(
            explain(
                "dev",
                &said(&["bind [127.0.0.1]:41000: Address already in use"])
            )
            .contains("port was taken")
        );
        assert_eq!(
            explain(
                "nowhere",
                &said(&["ssh: Could not resolve hostname nowhere: Name or service not known"])
            ),
            "ssh to nowhere: ssh: Could not resolve hostname nowhere: Name or service not known"
        );
        assert_eq!(explain("dev", &[]), "ssh to dev exited");
    }

    #[test]
    fn the_listener_s_owner_is_read_from_the_kernel_s_table() {
        // Port 0xA028 (41000) on 127.0.0.1, listening, uid 1000; another on
        // 0.0.0.0:22 by root; a connection (state 01) on 41000 by uid 7.
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:A028 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 1 1 0 100 0 0 10 0
   1: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 2 1 0 100 0 0 10 0
   2: 0100007F:A028 0100007F:9C40 01 00000000:00000000 00:00000000 00000000     7        0 3 1 0 100 0 0 10 0";
        assert_eq!(listener_uid_in(table, 41000), Some(1000));
        assert_eq!(listener_uid_in(table, 22), Some(0));
        assert_eq!(listener_uid_in(table, 80), None);
        // A listener on another address (10.0.0.1) is not the tunnel's.
        let other = "header\n   0: 0100000A:A028 00000000:0000 0A 0:0 0:0 0 5 0 1";
        assert_eq!(listener_uid_in(other, 41000), None);
    }

    #[test]
    fn the_tunnels_port_is_drawn_at_random_and_free() {
        let drawn: Vec<u16> = (0..16).map(|_| random_port()).collect();
        assert!(drawn.iter().all(|port| TUNNEL_PORTS.contains(port)));
        let mut distinct = drawn.clone();
        distinct.sort_unstable();
        distinct.dedup();
        // Sixteen draws from 55 536 ports: all alike only if not random.
        assert!(distinct.len() > 8, "{drawn:?}");
        // Free when drawn: it binds on loopback. The port is let go before
        // it is checked, so another socket (a parallel test's, or one the
        // kernel hands out) can take it in between; a port free when drawn
        // binds again on one of a few draws.
        let rebinds = (0..4).any(|_| {
            let port = free_port().unwrap();
            assert!(TUNNEL_PORTS.contains(&port));
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_ok()
        });
        assert!(rebinds, "no drawn port bound again on loopback");
    }

    #[test]
    fn a_missing_ssh_says_so() {
        let mut options = SshOptions::new("dev");
        options.program = PathBuf::from("/nonexistent/noctiluca-test-ssh");
        let error = Tunnel::open(options, Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("install OpenSSH"), "{error}");
    }

    #[test]
    fn an_ssh_that_exits_is_a_clear_error() {
        // A stand-in for ssh that says what a refused login says, and
        // exits.
        let dir = std::env::temp_dir().join(format!("noctiluca-ssh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("ssh");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho 'me@dev: Permission denied (publickey).' >&2\nexit 255\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut options = SshOptions::new("me@dev");
        options.program = fake;
        let error = Tunnel::open(options, Duration::from_secs(10)).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("SSH refused the login to me@dev"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
