//! Render with Mermaid's own CLI, `mmdc` (`@mermaid-js/mermaid-cli`).
//!
//! `mmdc` runs Node and a headless Chromium, so it is only used where the
//! caller asked for it. The source goes through a file in a private temporary
//! directory, never the shell or a command line; the process gets no network
//! address to fetch, a timeout, and size caps on what goes in and comes out.
//!
//! `mmdc` stays in the caller's process group, so Ctrl-C reaches it (and
//! Puppeteer then closes Chromium). On Unix a small watchdog removes the
//! temporary directory even when the caller is killed before it can.

use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How to run `mmdc`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MmdcOptions {
    /// The program to run: `mmdc` on `PATH` by default.
    pub program: PathBuf,
    /// Kill `mmdc` after this long (default 20 s).
    pub timeout: Duration,
    /// Refuse sources larger than this many bytes (default 64 KiB).
    pub max_input: usize,
    /// Refuse images larger than this many bytes (default 16 MiB).
    pub max_output: usize,
    /// A Puppeteer configuration file passed with `-p`, for example to set
    /// Chromium's path or `--no-sandbox` where the sandbox is unavailable.
    pub puppeteer_config: Option<PathBuf>,
    /// The image background (`-b`); default `white`, which keeps dark lines
    /// readable on a dark terminal.
    pub background: String,
}

impl Default for MmdcOptions {
    fn default() -> Self {
        MmdcOptions {
            program: PathBuf::from("mmdc"),
            timeout: Duration::from_secs(20),
            max_input: 64 * 1024,
            max_output: 16 * 1024 * 1024,
            puppeteer_config: None,
            background: "white".into(),
        }
    }
}

/// Why `mmdc` produced no image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MmdcError {
    /// The program is not installed (or not on `PATH`).
    NotFound(String),
    /// The source is larger than [`MmdcOptions::max_input`].
    TooLarge { bytes: usize, limit: usize },
    /// The image is larger than [`MmdcOptions::max_output`].
    OutputTooLarge { bytes: u64, limit: usize },
    /// It ran longer than [`MmdcOptions::timeout`] and was stopped.
    Timeout(Duration),
    /// It exited unsuccessfully; the first line of what it printed.
    Failed(String),
    /// Anything else: the temporary directory, reading the image.
    Io(String),
}

impl fmt::Display for MmdcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MmdcError::NotFound(program) => write!(f, "`{program}` is not installed"),
            MmdcError::TooLarge { bytes, limit } => {
                write!(f, "the diagram is {bytes} bytes, more than mmdc's {limit}")
            }
            MmdcError::OutputTooLarge { bytes, limit } => {
                write!(f, "mmdc drew a {bytes} byte image, more than {limit}")
            }
            MmdcError::Timeout(after) => {
                write!(f, "mmdc did not finish within {} s", after.as_secs_f32())
            }
            MmdcError::Failed(message) => write!(f, "mmdc failed: {message}"),
            MmdcError::Io(message) => write!(f, "mmdc could not run: {message}"),
        }
    }
}

impl std::error::Error for MmdcError {}

/// Render `source` to PNG bytes with `mmdc`.
pub fn render_png(source: &str, options: &MmdcOptions) -> Result<Vec<u8>, MmdcError> {
    if source.len() > options.max_input {
        return Err(MmdcError::TooLarge {
            bytes: source.len(),
            limit: options.max_input,
        });
    }
    let dir = TempDir::new().map_err(|e| MmdcError::Io(e.to_string()))?;
    let input = dir.path.join("diagram.mmd");
    let output = dir.path.join("diagram.png");
    let log = dir.path.join("mmdc.log");
    std::fs::write(&input, source).map_err(|e| MmdcError::Io(e.to_string()))?;
    let log_file = std::fs::File::create(&log).map_err(|e| MmdcError::Io(e.to_string()))?;
    let log_err = log_file
        .try_clone()
        .map_err(|e| MmdcError::Io(e.to_string()))?;

    let mut command = Command::new(&options.program);
    command
        .arg("--quiet")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--backgroundColor")
        .arg(&options.background)
        .stdin(Stdio::null())
        .stdout(log_file)
        .stderr(log_err)
        .current_dir(&dir.path);
    if let Some(config) = &options.puppeteer_config {
        command.arg("--puppeteerConfigFile").arg(config);
    }
    // mmdc stays in our process group: Ctrl-C at the terminal must reach it,
    // and Puppeteer closes Chromium when it does.
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MmdcError::NotFound(options.program.display().to_string())
        } else {
            MmdcError::Io(e.to_string())
        }
    })?;
    // Dropped (removing the directory) on every return below; the watchdog
    // covers the case where this process dies first.
    let _watchdog = Watchdog::start(&dir, child.id());

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= options.timeout => {
                stop(&mut child);
                return Err(MmdcError::Timeout(options.timeout));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                stop(&mut child);
                return Err(MmdcError::Io(e.to_string()));
            }
        }
    };
    if !status.success() {
        let printed = read_start(&log, 4096);
        let first = printed
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("no output");
        let first: String = first
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        return Err(MmdcError::Failed(format!("{first} ({status})")));
    }
    let image = std::fs::File::open(&output)
        .map_err(|e| MmdcError::Io(format!("no image was written: {e}")))?;
    let bytes = image
        .metadata()
        .map_err(|e| MmdcError::Io(e.to_string()))?
        .len();
    let too_large = MmdcError::OutputTooLarge {
        bytes,
        limit: options.max_output,
    };
    if bytes > options.max_output as u64 {
        return Err(too_large);
    }
    // The file cannot grow past the limit unnoticed, whatever its size said.
    let mut png = Vec::with_capacity(bytes as usize);
    image
        .take(options.max_output as u64 + 1)
        .read_to_end(&mut png)
        .map_err(|e| MmdcError::Io(e.to_string()))?;
    if png.len() > options.max_output {
        return Err(too_large);
    }
    Ok(png)
}

/// At most `limit` bytes from the start of `path`, as text.
fn read_start(path: &std::path::Path, limit: u64) -> String {
    let mut bytes = Vec::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(limit).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Stop the child. On Unix, ask it to terminate first: Puppeteer closes
/// Chromium (which it starts in a group of its own) on SIGTERM, but cannot on
/// SIGKILL.
fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(child.id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let asked = Instant::now();
        while asked.elapsed() < Duration::from_secs(2) {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// On Unix, a `sh` in a process group of its own that removes the temporary
/// directory once this process is gone, however it went: it waits for the end
/// of a pipe only this process holds, then for `mmdc` to exit (30 s at most),
/// then removes the directory. Terminal signals do not reach it. The
/// directory's path is an argument, never part of the script.
///
/// On drop (the normal and error paths) the watchdog is killed and reaped,
/// and [`TempDir`]'s own drop removes the directory.
struct Watchdog {
    #[cfg(unix)]
    child: Option<(std::process::Child, std::process::ChildStdin)>,
}

impl Watchdog {
    #[cfg_attr(not(unix), allow(unused_variables))]
    fn start(dir: &TempDir, pid: u32) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // A zombie (an orphan nobody has reaped yet) has exited.
            const SCRIPT: &str = "read _ignored\n\
                running() {\n\
                  kill -0 \"$1\" 2>/dev/null || return 1\n\
                  case $(ps -o stat= -p \"$1\" 2>/dev/null) in Z*) return 1;; esac\n\
                }\n\
                n=0\n\
                while [ $n -lt 30 ] && running \"$2\"; do sleep 1; n=$((n+1)); done\n\
                rm -rf -- \"$1\"\n";
            let child = Command::new("/bin/sh")
                .arg("-c")
                .arg(SCRIPT)
                .arg("rich-mermaid-watchdog")
                .arg(&dir.path)
                .arg(pid.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .ok()
                .and_then(|mut child| match child.stdin.take() {
                    Some(stdin) => Some((child, stdin)),
                    None => {
                        let _ = child.kill();
                        let _ = child.wait();
                        None
                    }
                });
            Watchdog { child }
        }
        #[cfg(not(unix))]
        Watchdog {}
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some((mut child, stdin)) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            drop(stdin);
        }
    }
}

/// A private temporary directory, removed on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        for _ in 0..16 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let path = base.join(format!(
                "rich-mermaid-{}-{}-{nanos}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(TempDir { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::other(
            "could not create a temporary directory",
        ))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
