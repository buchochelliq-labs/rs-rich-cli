//! Running a tape, and writing or checking what it produced.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::render::{cast, raster, svg, video};
use crate::screen::{Snapshot, Theme};
use crate::session::{Session, Timeline};
use crate::tape::{Pattern, Step, Tape, TapeError};

/// The prompt the recorded shell shows; `run` waits for it.
const PROMPT: &str = "❯";

/// How a tape is run.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Put first on `PATH`, so the binaries under test win.
    pub bin_dir: Option<PathBuf>,
    /// Exported as `$REPO` to the shell and to `Exec`, so tapes can copy
    /// fixtures from the repository.
    pub repo: Option<PathBuf>,
    pub theme: Theme,
}

/// What a tape produced.
#[derive(Clone, Debug)]
pub struct Recording {
    pub title: String,
    pub columns: u16,
    pub rows: u16,
    /// Screenshots in the order they were taken.
    pub shots: Vec<(String, Snapshot)>,
    pub timeline: Timeline,
}

/// A temporary directory, removed on drop.
struct Workspace(PathBuf);

impl Workspace {
    fn new(stem: &str) -> std::io::Result<Workspace> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path =
            std::env::temp_dir().join(format!("rich-record-{stem}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(path.join(".home"))?;
        std::fs::write(
            path.join(".home/.inputrc"),
            "set enable-bracketed-paste off\n",
        )?;
        Ok(Workspace(path))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The session's environment: pinned so recordings repeat.
fn environment(workspace: &Path, tape: &Tape, options: &Options) -> Vec<(String, String)> {
    let home = workspace.join(".home");
    let inherited = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
    let mut dirs: Vec<PathBuf> = options.bin_dir.iter().cloned().collect();
    dirs.extend(std::env::split_paths(&inherited));
    let path = std::env::join_paths(dirs).map_or_else(
        |_| inherited.to_string_lossy().into_owned(),
        |p| p.to_string_lossy().into_owned(),
    );
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), path),
        ("HOME".into(), home.display().to_string()),
        (
            "XDG_CONFIG_HOME".into(),
            home.join(".config").display().to_string(),
        ),
        (
            "INPUTRC".into(),
            home.join(".inputrc").display().to_string(),
        ),
        ("TERM".into(), "xterm-256color".into()),
        ("COLORTERM".into(), "truecolor".into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("LC_ALL".into(), "C.UTF-8".into()),
        ("TZ".into(), "UTC".into()),
        (
            "PS1".into(),
            format!("\\[\\e[1;35m\\]{PROMPT}\\[\\e[0m\\] "),
        ),
        ("PROMPT_COMMAND".into(), String::new()),
        ("HISTFILE".into(), "/dev/null".into()),
    ];
    if let Some(repo) = &options.repo {
        env.push(("REPO".into(), repo.display().to_string()));
    }
    env.extend(tape.env.iter().cloned());
    env
}

fn wait_for(
    session: &Session,
    pattern: &Pattern,
    limit: Duration,
    line: usize,
) -> Result<(), TapeError> {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if pattern.is_match(&session.contents()) {
            return Ok(());
        }
        if !session.alive() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(TapeError::new(
        line,
        format!(
            "timed out waiting for {pattern}; the screen shows:\n{}",
            session.contents()
        ),
    ))
}

fn exec(
    command: &str,
    workspace: &Path,
    env: &[(String, String)],
    line: usize,
) -> Result<(), TapeError> {
    let mut child = Command::new("sh")
        .args(["-c", command])
        .current_dir(workspace)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| TapeError::new(line, format!("Exec could not start: {e}")))?;
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
                }
                return Err(TapeError::new(
                    line,
                    format!("Exec failed ({status}): {}", stderr.trim()),
                ));
            }
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return Err(TapeError::new(line, "Exec timed out after 60s"));
            }
        }
    }
}

/// Run `tape` (named `stem`) and record it.
pub fn record(tape: &Tape, stem: &str, options: &Options) -> Result<Recording, TapeError> {
    let io = |e: std::io::Error| TapeError::new(0, e.to_string());
    let workspace = Workspace::new(stem).map_err(io)?;
    let env = environment(&workspace.0, tape, options);
    let mut session = Session::start(
        &workspace.0,
        tape.columns,
        tape.rows,
        &env,
        options.theme.clone(),
    )
    .map_err(io)?;
    let mut typing = Duration::from_millis(40);
    let mut timeout = Duration::from_secs(15);
    let prompt = Pattern::Text(PROMPT.into());
    let mut shots = Vec::new();
    // Start hidden, on a cleared screen at the prompt.
    wait_for(&session, &prompt, timeout, 0)?;
    session.send("clear\r", None).map_err(io)?;
    std::thread::sleep(Duration::from_millis(200));
    wait_for(&session, &prompt, timeout, 0)?;
    session.show();
    for (line, step) in &tape.steps {
        let io = |e: std::io::Error| TapeError::new(*line, e.to_string());
        match step {
            Step::TypingDelay(delay) => typing = *delay,
            Step::Timeout(limit) => timeout = *limit,
            Step::Type(text) => {
                for c in text.chars() {
                    session.send(&c.to_string(), None).map_err(io)?;
                    std::thread::sleep(typing);
                }
            }
            Step::Key { key, count } => {
                for _ in 0..*count {
                    session.send(&key.bytes(), Some(key.label())).map_err(io)?;
                    std::thread::sleep(typing.max(Duration::from_millis(120)));
                }
            }
            Step::Sleep(delay) => std::thread::sleep(*delay),
            Step::Wait {
                pattern,
                timeout: limit,
            } => wait_for(&session, pattern, limit.unwrap_or(timeout), *line)?,
            Step::Screenshot(name) => {
                // Let a repaint in flight land.
                std::thread::sleep(Duration::from_millis(150));
                shots.push((name.clone(), session.snapshot()));
            }
            Step::Hide => session.hide(),
            Step::Show => session.show(),
            Step::Resize { columns, rows } => session.resize(*columns, *rows).map_err(io)?,
            Step::Write { path, content } => {
                let target = workspace.0.join(path);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                std::fs::write(target, content).map_err(io)?;
            }
            Step::Exec(command) => exec(command, &workspace.0, &env, *line)?,
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    let timeline = session.finish();
    if shots.is_empty() {
        return Err(TapeError::new(0, "the tape takes no Screenshot"));
    }
    Ok(Recording {
        title: tape.title.clone().unwrap_or_else(|| stem.to_string()),
        columns: tape.columns,
        rows: tape.rows,
        shots,
        timeline,
    })
}

/// Which files [`write`] produces. Text grids are always written: `--check`
/// needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Formats {
    pub png: bool,
    pub svg: bool,
    pub cast: bool,
    pub gif: bool,
    /// Only when FFmpeg is installed.
    pub mp4: bool,
}

impl Formats {
    pub const ALL: Formats = Formats {
        png: true,
        svg: true,
        cast: true,
        gif: true,
        mp4: true,
    };
    /// Stills, text and the cast: no video encoding.
    pub const NO_VIDEO: Formats = Formats {
        png: true,
        svg: true,
        cast: true,
        gif: false,
        mp4: false,
    };
}

/// Names of committed screenshots (`<name>.txt`) the recording no longer takes.
pub fn orphans(recording: &Recording, dir: &Path) -> Vec<String> {
    let taken: BTreeSet<&str> = recording
        .shots
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension()? == "txt").then(|| path.file_stem()?.to_str().map(str::to_string))?
        })
        .filter(|name| !taken.contains(name.as_str()))
        .collect();
    names.sort();
    names
}

/// A difference `check` found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The screenshot's text differs from `<name>.txt`: a unified-style diff.
    Differs { name: String, diff: String },
    /// A committed screenshot the tape no longer takes.
    Orphaned { name: String },
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::Differs { name, diff } => write!(f, "screenshot {name} differs\n{diff}"),
            Problem::Orphaned { name } => {
                write!(
                    f,
                    "committed screenshot {name} is no longer taken; regenerate to remove it"
                )
            }
        }
    }
}

/// Compare each screenshot's text grid with `dir/<name>.txt`.
pub fn check(recording: &Recording, dir: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (name, snapshot) in &recording.shots {
        let got = snapshot.text_grid();
        let want = std::fs::read_to_string(dir.join(format!("{name}.txt"))).unwrap_or_default();
        if got != want {
            let diff = rich_ext::diff::TextDiff::new(&want, &got).unified("committed", "this run");
            problems.push(Problem::Differs {
                name: name.clone(),
                diff,
            });
        }
    }
    problems.extend(
        orphans(recording, dir)
            .into_iter()
            .map(|name| Problem::Orphaned { name }),
    );
    problems
}

/// FNV-1a, 64-bit: a stable fingerprint of the tape for provenance.
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Write the recording's files into `dir`, removing orphaned screenshots.
/// Returns the paths written.
pub fn write(
    recording: &Recording,
    dir: &Path,
    stem: &str,
    formats: Formats,
    fonts: &raster::Fonts,
    theme: &Theme,
    provenance: Option<(&Path, &[u8])>,
) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    for name in orphans(recording, dir) {
        for suffix in ["txt", "png", "svg"] {
            let _ = std::fs::remove_file(dir.join(format!("{name}.{suffix}")));
        }
    }
    let mut written = Vec::new();
    let mut mp4_written = false;
    let mut save = |name: String, bytes: &[u8]| -> std::io::Result<()> {
        let path = dir.join(name);
        std::fs::write(&path, bytes)?;
        written.push(path);
        Ok(())
    };
    for (name, snapshot) in &recording.shots {
        save(format!("{name}.txt"), snapshot.text_grid().as_bytes())?;
        if formats.svg {
            save(
                format!("{name}.svg"),
                svg::svg(snapshot, theme, &recording.title).as_bytes(),
            )?;
        }
        if formats.png {
            let options = raster::Frame {
                title: &recording.title,
                key: None,
                size: 28.0,
                window: true,
            };
            save(
                format!("{name}.png"),
                &raster::render(snapshot, theme, fonts, &options).png(),
            )?;
        }
    }
    if formats.cast {
        let cast = cast::cast(
            &recording.timeline,
            recording.columns,
            recording.rows,
            &recording.title,
            theme,
        );
        save(format!("{stem}.cast"), cast.as_bytes())?;
    }
    if formats.gif || formats.mp4 {
        let frames = video::render(
            &video::sample(&recording.timeline, 12.0),
            theme,
            fonts,
            &recording.title,
        );
        if formats.gif {
            let mut bytes = Vec::new();
            raster::gif(&frames, &mut bytes).map_err(std::io::Error::other)?;
            save(format!("{stem}.gif"), &bytes)?;
        }
        if formats.mp4 && video::ffmpeg_available() {
            video::mp4(&frames, &dir.join(format!("{stem}.mp4")))?;
            mp4_written = true;
        }
    }
    if let Some((tape_path, tape_bytes)) = provenance {
        let output = |program: &str, args: &[&str]| {
            Command::new(program)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let json = json!({
            "tape": tape_path.display().to_string(),
            "tape_fnv1a64": fingerprint(tape_bytes),
            "recorder": format!("rs-rich-record {}", env!("CARGO_PKG_VERSION")),
            "rich": output("rich", &["--version"]),
            "commit": output("git", &["rev-parse", "HEAD"]),
            "note": "Every frame is real output of the program under test on a PTY.",
        });
        save("provenance.json".into(), format!("{:#}\n", json).as_bytes())?;
    }
    if mp4_written {
        written.push(dir.join(format!("{stem}.mp4")));
    }
    Ok(written)
}
