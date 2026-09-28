//! Running a tape, and writing or checking what it produced.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::render::{cast, raster, svg, video};
use crate::screen::{Snapshot, Theme};
use crate::session::{Session, Timeline, FRAME_RATE, MAX_VIDEO};
use crate::tape::{Pattern, Shell, Step, Tape, TapeError};

/// The prompt the recorded shell shows; `run` waits for it.
const PROMPT: &str = "❯";
/// How long an `Exec` command may run.
const EXEC_TIMEOUT: Duration = Duration::from_secs(60);
/// How much of an `Exec` command's error output is kept for its report.
const EXEC_STDERR: u64 = 64 * 1024;

/// Whether `stem` may name a recording: its output directory, the prefix of
/// its cast, GIF and MP4, and its workspace. Not empty, `.` or `..`, and no
/// path separators, so everything written stays inside the output directory.
pub fn stem_allowed(stem: &str) -> bool {
    !stem.is_empty()
        && stem != "."
        && stem != ".."
        && !stem
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
}

fn stem_error(stem: &str) -> String {
    format!(
        "{stem:?} cannot name a recording: it must not be empty, . or .., \
         or contain / or \\"
    )
}

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
    /// The tape's `Mask` rewrites, applied to text grids.
    pub masks: Vec<(fancy_regex::Regex, String)>,
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
        // zsh reads only this, through ZDOTDIR, when started with -d.
        std::fs::write(
            path.join(".home/.zshrc"),
            format!(
                "PROMPT=$'%{{\\e[1;35m%}}{PROMPT}%{{\\e[0m%}} '\nRPROMPT=''\n\
                 unsetopt PROMPT_SP BEEP\nunset zle_bracketed_paste\nHISTFILE=/dev/null\n"
            ),
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
    // The shell runs in the workspace, so a relative directory is resolved
    // against where the recorder was started.
    let mut dirs: Vec<PathBuf> = options
        .bin_dir
        .iter()
        .map(|dir| std::path::absolute(dir).unwrap_or_else(|_| dir.clone()))
        .collect();
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
        ("PROMPT_COMMAND".into(), String::new()),
        ("HISTFILE".into(), "/dev/null".into()),
    ];
    match tape.shell {
        Shell::Bash => env.push((
            "PS1".into(),
            format!("\\[\\e[1;35m\\]{PROMPT}\\[\\e[0m\\] "),
        )),
        // POSIX sh has no escapes in PS1: the bytes themselves.
        Shell::Sh => env.push(("PS1".into(), format!("\x1b[1;35m{PROMPT}\x1b[0m "))),
        Shell::Zsh => env.push(("ZDOTDIR".into(), home.display().to_string())),
        Shell::Fish => {}
    }
    if let Some(repo) = &options.repo {
        let repo = std::path::absolute(repo).unwrap_or_else(|_| repo.clone());
        env.push(("REPO".into(), repo.display().to_string()));
    }
    env.extend(tape.env.iter().cloned());
    env
}

/// The command line that starts `shell` interactively, without the user's
/// profile or rc files.
fn shell_command(shell: Shell) -> Vec<String> {
    let args: &[&str] = match shell {
        Shell::Bash => &["--noprofile", "--norc", "-i"],
        // -d skips /etc/zsh*; ZDOTDIR points at the workspace's .zshrc.
        Shell::Zsh => &["-d", "-i"],
        Shell::Fish => &[
            "--no-config",
            "--private",
            "-i",
            "-C",
            "function fish_prompt; set_color -o magenta; echo -n '❯'; \
             set_color normal; echo -n ' '; end; set -g fish_greeting ''; \
             set -g fish_autosuggestion_enabled 0",
        ],
        Shell::Sh => &["-i"],
    };
    std::iter::once(shell.name())
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect()
}

/// The major version of the `bash` on `PATH`, if it runs.
fn bash_major() -> Option<u32> {
    let output = Command::new("bash")
        .args(["-c", "echo ${BASH_VERSINFO[0]}"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Warnings about the machine a tape is about to be recorded on: a shell
/// that is missing, or a bash old enough (macOS ships 3.2) that its line
/// editing may differ from recordings made elsewhere.
pub fn warnings(tape: &Tape) -> Vec<String> {
    let mut out = Vec::new();
    if tape.shell == Shell::Bash {
        match bash_major() {
            Some(major) if major < 4 => out.push(format!(
                "bash {major} is older than 4 (macOS ships 3.2): line editing may \
                 differ from recordings made with a newer bash; install one (for \
                 example `brew install bash`) and put it first on PATH"
            )),
            Some(_) => {}
            None => out.push("bash was not found on PATH".into()),
        }
    }
    out
}

/// A failure of the terminal emulator, reported at `line`.
fn failed(session: &Session, line: usize) -> Result<(), TapeError> {
    match session.error() {
        Some(error) => Err(TapeError::new(
            line,
            format!("the terminal emulator failed: {error}"),
        )),
        None => Ok(()),
    }
}

fn wait_for(
    session: &Session,
    pattern: &Pattern,
    limit: Duration,
    line: usize,
) -> Result<(), TapeError> {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if session.seen(|screen| pattern.is_match(screen)) {
            return Ok(());
        }
        failed(session, line)?;
        if !session.alive() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    failed(session, line)?;
    Err(TapeError::new(
        line,
        format!(
            "timed out waiting for {pattern}; the screen shows:\n{}",
            session.contents()
        ),
    ))
}

/// Run `command` with `sh -c` in `workspace`, for at most `limit`. Its error
/// output is read as it is written, so a chatty command cannot fill the pipe
/// and hang, and the command is killed and reaped when it runs too long.
fn exec(
    command: &str,
    workspace: &Path,
    env: &[(String, String)],
    limit: Duration,
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
    let (sender, stderr) = std::sync::mpsc::channel();
    if let Some(pipe) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut pipe = pipe;
            let mut kept = Vec::new();
            let _ = (&mut pipe).take(EXEC_STDERR).read_to_end(&mut kept);
            // Drain the rest, so the command never blocks on a full pipe.
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            let _ = sender.send(kept);
        });
    }
    // A command that leaves a background process holding the pipe open does
    // not delay the report for long.
    let stderr = || {
        stderr
            .recv_timeout(Duration::from_millis(500))
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
            .unwrap_or_default()
    };
    let end = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(TapeError::new(
                    line,
                    format!("Exec failed ({status}): {}", stderr()),
                ));
            }
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(TapeError::new(
                    line,
                    format!("Exec timed out after {}s", limit.as_secs_f64()),
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(TapeError::new(line, format!("Exec failed: {error}")));
            }
        }
    }
}

/// Run `tape` (named `stem`) and record it.
pub fn record(tape: &Tape, stem: &str, options: &Options) -> Result<Recording, TapeError> {
    if !stem_allowed(stem) {
        return Err(TapeError::new(0, stem_error(stem)));
    }
    let io = |e: std::io::Error| TapeError::new(0, e.to_string());
    let workspace = Workspace::new(stem).map_err(io)?;
    let env = environment(&workspace.0, tape, options);
    let shell = shell_command(tape.shell);
    let mut session = Session::start(
        &shell,
        &workspace.0,
        tape.columns,
        tape.rows,
        &env,
        options.theme.clone(),
    )
    .map_err(|e| TapeError::new(0, format!("cannot start {}: {e}", tape.shell.name())))?;
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
        // A `Wait` matches anything shown since the step before it began, so
        // fast output that scrolls past between polls is not missed.
        if !matches!(step, Step::Wait { .. }) {
            session.mark();
        }
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
                // `parse` checks this too; a tape built in code may not.
                if !crate::tape::write_path_allowed(path) {
                    return Err(TapeError::new(
                        *line,
                        format!("Write path {path:?} must be relative and stay in the workspace"),
                    ));
                }
                let target = workspace.0.join(path);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                std::fs::write(target, content).map_err(io)?;
            }
            Step::Exec(command) => exec(command, &workspace.0, &env, EXEC_TIMEOUT, *line)?,
        }
        failed(&session, *line)?;
    }
    std::thread::sleep(Duration::from_millis(300));
    failed(&session, 0)?;
    let timeline = session.finish();
    if shots.is_empty() {
        return Err(TapeError::new(0, "the tape takes no Screenshot"));
    }
    Ok(Recording {
        title: tape.title.clone().unwrap_or_else(|| stem.to_string()),
        columns: tape.columns,
        rows: tape.rows,
        shots,
        masks: tape.masks.clone(),
        timeline,
    })
}

impl Recording {
    /// A screenshot's text grid, with the tape's masks applied.
    pub fn text_grid(&self, snapshot: &Snapshot) -> String {
        crate::tape::apply_masks(&self.masks, &snapshot.text_grid())
    }
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

/// The screenshots [`write`] last wrote into `dir`, from the `screenshots`
/// list in its `provenance.json`.
fn manifest(dir: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(dir.join("provenance.json")) else {
        return BTreeSet::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return BTreeSet::new();
    };
    json.get("screenshots")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        // A name that could leave `dir` is never ours.
        .filter(|name| crate::tape::screenshot_name_allowed(name))
        .map(str::to_string)
        .collect()
}

/// Names of committed screenshots the recording no longer takes: those the
/// last [`write`] listed in `provenance.json` and this recording does not
/// take. Other files in the directory are never counted, or removed, however
/// they are named.
pub fn orphans(recording: &Recording, dir: &Path) -> Vec<String> {
    let taken: BTreeSet<&str> = recording
        .shots
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    manifest(dir)
        .into_iter()
        .filter(|name| !taken.contains(name.as_str()))
        .collect()
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
        let got = recording.text_grid(snapshot);
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

fn invalid(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}

/// An error unless an image of `columns` x `rows` cells, drawn with
/// `options`, fits in [`raster::MAX_PIXELS`].
fn check_pixels(
    columns: usize,
    rows: usize,
    fonts: &raster::Fonts,
    options: &raster::Frame<'_>,
) -> std::io::Result<()> {
    let (width, height) = raster::size(columns, rows, fonts, options);
    if width.saturating_mul(height) > raster::MAX_PIXELS {
        return Err(invalid(format!(
            "an image of {columns}x{rows} cells would be {width}x{height} pixels, more than \
             {} million; use a smaller Size or font",
            raster::MAX_PIXELS / 1_000_000
        )));
    }
    Ok(())
}

/// Write the recording's files into `dir`, removing screenshots the last
/// write listed that this recording no longer takes. With `provenance`, the
/// written `provenance.json` lists the screenshots for the next write.
/// Returns the paths written.
///
/// Refused before anything is written: a `stem` that [`stem_allowed`]
/// refuses, a GIF or MP4 of a recording longer than
/// [`crate::session::MAX_VIDEO`], and images too large to draw.
pub fn write(
    recording: &Recording,
    dir: &Path,
    stem: &str,
    formats: Formats,
    fonts: &raster::Fonts,
    theme: &Theme,
    provenance: Option<(&Path, &[u8])>,
) -> std::io::Result<Vec<PathBuf>> {
    if !stem_allowed(stem) {
        return Err(invalid(stem_error(stem)));
    }
    let video = formats.gif || formats.mp4;
    if video && recording.timeline.truncated {
        return Err(invalid(format!(
            "the recording is longer than {}s, too long for video; record it without \
             GIF or MP4 (--no-video), or shorten the tape",
            MAX_VIDEO.as_secs()
        )));
    }
    let still = raster::Frame {
        title: &recording.title,
        key: None,
        size: 28.0,
        window: true,
    };
    if formats.png {
        for (_, snapshot) in &recording.shots {
            check_pixels(snapshot.columns(), snapshot.rows.len(), fonts, &still)?;
        }
    }
    let samples = if video {
        video::sample(&recording.timeline, FRAME_RATE)
    } else {
        Vec::new()
    };
    let (video_width, video_height) = video::size(&recording.timeline, &samples, fonts);
    if video_width.saturating_mul(video_height) > raster::MAX_PIXELS {
        return Err(invalid(format!(
            "video frames would be {video_width}x{video_height} pixels, more than {} million; \
             use a smaller Size or font",
            raster::MAX_PIXELS / 1_000_000
        )));
    }
    std::fs::create_dir_all(dir)?;
    for name in orphans(recording, dir) {
        for suffix in ["txt", "png", "svg"] {
            let _ = std::fs::remove_file(dir.join(format!("{name}.{suffix}")));
        }
    }
    let mut written = Vec::new();
    let save = |written: &mut Vec<PathBuf>, name: String, bytes: &[u8]| -> std::io::Result<()> {
        let path = dir.join(name);
        std::fs::write(&path, bytes)?;
        written.push(path);
        Ok(())
    };
    for (name, snapshot) in &recording.shots {
        save(
            &mut written,
            format!("{name}.txt"),
            recording.text_grid(snapshot).as_bytes(),
        )?;
        if formats.svg {
            save(
                &mut written,
                format!("{name}.svg"),
                svg::svg(snapshot, theme, &recording.title).as_bytes(),
            )?;
        }
        if formats.png {
            save(
                &mut written,
                format!("{name}.png"),
                &raster::render(snapshot, theme, fonts, &still).png(),
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
        save(&mut written, format!("{stem}.cast"), cast.as_bytes())?;
    }
    let mp4_path = dir.join(format!("{stem}.mp4"));
    let mut mp4 = if formats.mp4 && video::ffmpeg_available() {
        Some(video::Mp4::start(&mp4_path, video_width, video_height)?)
    } else {
        None
    };
    let timeline = &recording.timeline;
    let title = recording.title.as_str();
    if formats.gif {
        let gif_path = dir.join(format!("{stem}.gif"));
        let mut file = std::io::BufWriter::new(std::fs::File::create(&gif_path)?);
        // The GIF draws every frame twice; the MP4 takes them on the first.
        let mut pass = 0;
        raster::gif_streamed(
            video_width,
            video_height,
            |sink| {
                pass += 1;
                video::render_each(timeline, &samples, theme, fonts, title, &mut |canvas, s| {
                    if pass == 1 {
                        if let Some(mp4) = mp4.as_mut() {
                            mp4.write(&canvas, s)?;
                        }
                    }
                    sink(canvas, s)
                })
            },
            &mut file,
        )?;
        std::io::Write::flush(&mut file)?;
        written.push(gif_path);
    } else if let Some(mp4) = mp4.as_mut() {
        video::render_each(timeline, &samples, theme, fonts, title, &mut |canvas, s| {
            mp4.write(&canvas, s)
        })?;
    }
    let mp4_written = mp4.is_some();
    if let Some(mp4) = mp4 {
        mp4.finish()?;
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
        let screenshots: Vec<&str> = recording
            .shots
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        let json = json!({
            "tape": tape_path.display().to_string(),
            "tape_fnv1a64": fingerprint(tape_bytes),
            "recorder": format!("rs-rich-record {}", env!("CARGO_PKG_VERSION")),
            "rich": output("rich", &["--version"]),
            "commit": output("git", &["rev-parse", "HEAD"]),
            "screenshots": screenshots,
            "note": "Every frame is real output of the program under test on a PTY.",
        });
        save(
            &mut written,
            "provenance.json".into(),
            format!("{:#}\n", json).as_bytes(),
        )?;
    }
    if mp4_written {
        written.push(mp4_path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording() -> Recording {
        let mut parser = vt100::Parser::new(3, 10, 0);
        parser.process(b"hi");
        let snapshot = Snapshot::from_screen(parser.screen(), &Theme::default());
        Recording {
            title: "t".into(),
            columns: 10,
            rows: 3,
            shots: vec![("shot".into(), snapshot.clone())],
            masks: Vec::new(),
            timeline: Timeline {
                frames: vec![(0.0, snapshot)],
                ..Timeline::default()
            },
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rich-record-unit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const TEXT_ONLY: Formats = Formats {
        png: false,
        svg: false,
        cast: false,
        gif: false,
        mp4: false,
    };

    #[test]
    fn stems_cannot_leave_the_output_directory() {
        for stem in ["", ".", "..", "a/b", "a\\b", "../x", "x\u{0}"] {
            assert!(!stem_allowed(stem), "{stem:?}");
        }
        for stem in ["demo", "my demo", ".hidden", "a..b", "v1.2"] {
            assert!(stem_allowed(stem), "{stem:?}");
        }
        let dir = scratch("stem");
        let fonts = raster::Fonts::embedded();
        let error = write(
            &recording(),
            &dir,
            "..",
            TEXT_ONLY,
            &fonts,
            &Theme::default(),
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        // Refused before anything was written.
        assert!(!dir.exists());
        let tape = crate::tape::parse("Screenshot x").unwrap();
        let error = record(&tape, "a/b", &Options::default()).unwrap_err();
        assert!(error.message.contains("cannot name a recording"), "{error}");
    }

    #[test]
    fn a_recording_too_long_for_video_says_so() {
        let mut recording = recording();
        recording.timeline.truncated = true;
        let dir = scratch("long");
        let fonts = raster::Fonts::embedded();
        let gif = Formats {
            gif: true,
            ..TEXT_ONLY
        };
        let error = write(
            &recording,
            &dir,
            "long",
            gif,
            &fonts,
            &Theme::default(),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("too long for video"), "{error}");
        assert!(!dir.exists());
        // Without video it is written.
        write(
            &recording,
            &dir,
            "long",
            TEXT_ONLY,
            &fonts,
            &Theme::default(),
            None,
        )
        .unwrap();
        assert!(dir.join("shot.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gif_is_written_by_streaming() {
        let dir = scratch("gif");
        let fonts = raster::Fonts::embedded();
        let gif = Formats {
            gif: true,
            ..TEXT_ONLY
        };
        let written = write(
            &recording(),
            &dir,
            "g",
            gif,
            &fonts,
            &Theme::default(),
            None,
        )
        .unwrap();
        assert!(written.iter().any(|p| p.ends_with("g.gif")), "{written:?}");
        let bytes = std::fs::read(dir.join("g.gif")).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn exec_reports_stderr_and_times_out() {
        let dir = std::env::temp_dir();
        let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
        let limit = Duration::from_secs(10);
        assert!(exec("true", &dir, &env, limit, 3).is_ok());
        let error = exec("echo oops >&2; exit 2", &dir, &env, limit, 3).unwrap_err();
        assert_eq!(error.line, 3);
        assert!(error.message.contains("oops"), "{error}");
        // More error output than a pipe holds does not hang the command.
        let error = exec(
            "head -c 1000000 /dev/zero | tr '\\0' x >&2; exit 1",
            &dir,
            &env,
            limit,
            4,
        )
        .unwrap_err();
        assert!(
            error.message.starts_with("Exec failed"),
            "{}",
            error.message
        );
        assert!(error.message.len() < 70 * 1024);
        let started = Instant::now();
        let error = exec("sleep 30", &dir, &env, Duration::from_millis(300), 5).unwrap_err();
        assert!(error.message.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
