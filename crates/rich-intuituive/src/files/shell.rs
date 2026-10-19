//! Commands on files from a prompt (Yazi's `;` and `:`), and renaming
//! many files at once in `$EDITOR` (Yazi's bulk rename).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rich::markup::escape;
use rich_ext::file_ops::{Progress, Report};

use super::tasks::Tasks;
use crate::app::Ctx;
use crate::dialog::PromptOptions;
use crate::notify::Level;
use crate::task::spawn;

/// What [`shell`] runs when the command has ended.
type Then = Box<dyn FnOnce(&mut Ctx)>;

/// How a [`shell`] prompt runs its command.
#[derive(Default)]
pub struct ShellOptions {
    block: bool,
    hovered: Option<PathBuf>,
    tasks: Option<Tasks>,
    prompt: Option<PromptOptions>,
    initial: String,
    done: Option<Then>,
}

impl ShellOptions {
    /// In the background, `$0` the shell's own name.
    pub fn new() -> ShellOptions {
        ShellOptions::default()
    }

    /// Give the command the terminal until it ends (Yazi's `:`), rather
    /// than running it in the background (`;`).
    pub fn block(mut self, on: bool) -> ShellOptions {
        self.block = on;
        self
    }

    /// The hovered file, the command's `$0` (`%0` on Windows), as Yazi
    /// gives it.
    pub fn hovered(mut self, path: Option<PathBuf>) -> ShellOptions {
        self.hovered = path;
        self
    }

    /// Run a background command as a job of `tasks`, shown in its panel
    /// (and its failure told to [`Tasks::on_fail`]), rather than on a
    /// thread of its own whose failure is a notification.
    pub fn tasks(mut self, tasks: Tasks) -> ShellOptions {
        self.tasks = Some(tasks);
        self
    }

    /// Where the prompt opens and how it looks (Yazi's 50 wide at the top
    /// centre by default).
    pub fn prompt(mut self, options: PromptOptions) -> ShellOptions {
        self.prompt = Some(options);
        self
    }

    /// What the prompt holds when it opens.
    pub fn initial(mut self, command: impl Into<String>) -> ShellOptions {
        self.initial = command.into();
        self
    }

    /// Run `done` once the command has ended (a blocking one) or started
    /// (in the background): read the directory again, say.
    pub fn done(mut self, done: impl FnOnce(&mut Ctx) + 'static) -> ShellOptions {
        self.done = Some(Box::new(done));
        self
    }
}

/// Ask for a shell command and run it on `paths` (the selection), as
/// Yazi's `;` (`Shell:`, in the background) and `:` (`Shell (block):`,
/// with the terminal: [`ShellOptions::block`]). The command runs in the
/// platform's shell (`sh -c`, `cmd /C`) with Yazi's arguments: `"$@"` is
/// every path, `$1`, `$2`… each, and `$0` the hovered file
/// ([`ShellOptions::hovered`]); on Windows `%*`, `%1`… and `%0`. A
/// background command's output is dropped; if it fails, its error output
/// is a notification titled `Shell` (or, with
/// [`tasks`](ShellOptions::tasks), a failed job). An empty command does
/// nothing.
///
/// ```
/// use intuituive::files::{shell, ShellOptions};
/// use intuituive::prelude::*;
///
/// let out = std::env::temp_dir().join(format!("intuituive-shell-doc-{}.txt", std::process::id()));
/// let file = out.clone();
/// let app = App::new(move || {
///     let file = file.clone();
///     label("files").on_key("semicolon", move |cx| {
///         shell(cx, vec![file.clone()], ShellOptions::new());
///     })
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(60, 6);
/// driver.press("semicolon");
/// assert!(driver.screen().plain()[2].contains("Shell:"));
/// # #[cfg(unix)]
/// # {
/// driver.type_text("echo made > \"$1\"");
/// driver.press("enter");
/// driver.settle();
/// assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), "made");
/// # }
/// # std::fs::remove_file(&out).ok();
/// ```
pub fn shell(cx: &mut Ctx, paths: Vec<PathBuf>, options: ShellOptions) {
    let ShellOptions {
        block,
        hovered,
        tasks,
        prompt,
        initial,
        done,
    } = options;
    let title = if block { "Shell (block):" } else { "Shell:" };
    let prompt = prompt.unwrap_or_default();
    cx.prompt_with(title, initial, prompt, move |run, cx| {
        if run.trim().is_empty() {
            return;
        }
        let command = shell_command(&run, hovered.as_deref(), &paths);
        if block {
            cx.handoff_then(command, move |ended, cx| {
                match ended {
                    Err(error) => cx.notify(Level::Error, "Shell", escape(&error.to_string())),
                    Ok(Some(code)) if code != 0 => {
                        cx.notify(Level::Warn, "Shell", format!("exited with {code}"))
                    }
                    _ => {}
                }
                if let Some(done) = done {
                    done(cx);
                }
            });
            return;
        }
        match tasks {
            Some(tasks) => {
                tasks.run_with(
                    &format!("Run {run}"),
                    move |_, progress| {
                        progress(&Progress {
                            total_items: 1,
                            ..Progress::default()
                        });
                        Report {
                            errors: run_quietly(command).err().into_iter().collect(),
                            ..Report::default()
                        }
                    },
                    |_, _| {},
                );
            }
            None => {
                spawn(
                    move || run_quietly(command),
                    |ran: Result<(), String>, cx| {
                        if let Err(error) = ran {
                            cx.notify(Level::Error, "Shell", escape(&error));
                        }
                    },
                );
            }
        }
        if let Some(done) = done {
            done(cx);
        }
    });
}

/// Run `command` with no input, its output kept only to say why it failed.
fn run_quietly(mut command: Command) -> Result<(), String> {
    let out = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr);
    let last = said.lines().rev().find(|l| !l.trim().is_empty());
    Err(last.map_or_else(|| out.status.to_string(), str::to_string))
}

/// The command [`shell`] runs: `run` in the platform's shell, with the
/// hovered file as `$0` and `paths` as `$1`… (`"$@"`); on Windows `%0`,
/// `%1`… and `%*` replaced by them, quoted.
///
/// ```
/// # #[cfg(unix)]
/// # {
/// use intuituive::files::shell_command;
/// use std::path::{Path, PathBuf};
///
/// let paths = [PathBuf::from("a b.txt"), PathBuf::from("c.txt")];
/// let mut command = shell_command("echo \"$0\" $# \"$@\"", Some(Path::new("here.md")), &paths);
/// let out = command.output().unwrap();
/// assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "here.md 2 a b.txt c.txt");
/// # }
/// ```
pub fn shell_command(run: &str, hovered: Option<&Path>, paths: &[PathBuf]) -> Command {
    if cfg!(windows) {
        let quote = |p: &Path| format!("\"{}\"", p.display());
        let all: Vec<String> = paths.iter().map(|p| quote(p)).collect();
        let mut line = run.replace("%*", &all.join(" "));
        // `%10` before `%1`.
        for (i, path) in paths.iter().enumerate().take(9).rev() {
            line = line.replace(&format!("%{}", i + 1), &quote(path));
        }
        if let Some(hovered) = hovered {
            line = line.replace("%0", &quote(hovered));
        }
        let mut command = Command::new("cmd");
        command.arg("/C").arg(line);
        command
    } else {
        let mut command = Command::new("sh");
        command.arg("-c").arg(run);
        match hovered {
            Some(hovered) => command.arg(hovered),
            None => command.arg("sh"),
        };
        command.args(paths);
        command
    }
}

/// Rename `paths` in `$EDITOR`, as Yazi's bulk rename: their names are
/// edited as text ([`Ctx::edit_text`]), a line each (just the names when
/// they share a directory, else whole paths), and once the editor ends
/// each changed line renames its file. Nothing is renamed if
/// the file no longer has a line for each path; a new name taken by a
/// file not being renamed, or given twice, is left out; the files can
/// swap names. What could not be done is a notification titled `Bulk
/// rename`; `done` gets what was renamed, old path and new.
///
/// ```
/// use intuituive::files::bulk_rename;
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-bulk-doc-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// std::fs::write(dir.join("a.txt"), "").unwrap();
/// let base = dir.clone();
/// let app = App::new(move || {
///     let base = base.clone();
///     let renamed = signal(0usize);
///     text!("renamed {renamed}").on_key("r", move |cx| {
///         bulk_rename(cx, vec![base.join("a.txt")], move |done, _| renamed.set(done.len()));
///     })
/// });
/// let mut driver = app.driver(20, 1);
/// driver.press("r");
/// let handoff = driver.take_handoff().expect("the editor takes the terminal");
/// // The editor's last argument is the file of names: edit it as a user would.
/// let names = handoff.command.get_args().last().unwrap().to_owned();
/// assert_eq!(std::fs::read_to_string(&names).unwrap(), "a.txt\n");
/// std::fs::write(&names, "b.txt\n").unwrap();
/// driver.handed_back(handoff, Ok(Some(0)));
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "renamed 1");
/// assert!(dir.join("b.txt").exists() && !dir.join("a.txt").exists());
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn bulk_rename(
    cx: &mut Ctx,
    paths: Vec<PathBuf>,
    done: impl FnOnce(Vec<(PathBuf, PathBuf)>, &mut Ctx) + 'static,
) {
    if paths.is_empty() {
        return;
    }
    let base = shared_dir(&paths);
    let lines: Vec<String> = paths
        .iter()
        .map(|p| match &base {
            Some(_) => p
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            None => p.display().to_string(),
        })
        .collect();
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    cx.edit_text(&body, move |edited, cx| {
        let edited = match edited {
            Ok(edited) => edited,
            Err(error) => {
                cx.notify(Level::Error, "Bulk rename", escape(&error.to_string()));
                return done(Vec::new(), cx);
            }
        };
        let (renames, mut problems) = match plan_renames(&paths, &lines, &edited, base.as_deref()) {
            Ok(plan) => plan,
            Err(why) => (Vec::new(), vec![why]),
        };
        let (made, failed) = apply_renames(&renames);
        problems.extend(failed);
        if !problems.is_empty() {
            cx.notify(Level::Error, "Bulk rename", escape(&problems.join("\n")));
        }
        done(made, cx);
    });
}

/// The directory every path is in, if they share one.
fn shared_dir(paths: &[PathBuf]) -> Option<PathBuf> {
    let first = paths.first()?.parent()?;
    paths
        .iter()
        .all(|p| p.parent() == Some(first))
        .then(|| first.to_path_buf())
}

/// Renames to make: each path, and its new one.
type Renames = Vec<(PathBuf, PathBuf)>;

/// The renames the edited names ask for, and the ones left out with why;
/// an error if the lines no longer match the files.
fn plan_renames(
    paths: &[PathBuf],
    lines: &[String],
    edited: &str,
    base: Option<&Path>,
) -> Result<(Renames, Vec<String>), String> {
    let mut new: Vec<&str> = edited.lines().map(|l| l.trim_end_matches('\r')).collect();
    while new.len() > paths.len() && new.last().is_some_and(|l| l.trim().is_empty()) {
        new.pop();
    }
    if new.len() != paths.len() {
        return Err(format!(
            "{} names for {} files: nothing renamed",
            new.len(),
            paths.len()
        ));
    }
    let renaming: HashSet<&PathBuf> = paths
        .iter()
        .zip(lines.iter().zip(&new))
        .filter(|(_, (old, new))| old.as_str() != **new)
        .map(|(p, _)| p)
        .collect();
    let (mut renames, mut problems) = (Vec::new(), Vec::new());
    let mut taken: HashSet<PathBuf> = HashSet::new();
    for (path, (old, line)) in paths.iter().zip(lines.iter().zip(new)) {
        if old == line {
            continue;
        }
        if line.trim().is_empty() {
            problems.push(format!("{old}: an empty name"));
            continue;
        }
        let to = match base {
            Some(dir) => dir.join(line),
            None => PathBuf::from(line),
        };
        let in_use = std::fs::symlink_metadata(&to).is_ok() && !renaming.contains(&to);
        if in_use || !taken.insert(to.clone()) {
            problems.push(format!("{old}: {line} is taken"));
            continue;
        }
        renames.push((path.clone(), to));
    }
    Ok((renames, problems))
}

/// Make `renames`, through a free name each first, so files can swap
/// names; what was renamed, and what failed.
fn apply_renames(renames: &[(PathBuf, PathBuf)]) -> (Renames, Vec<String>) {
    let (mut staged, mut failed) = (Vec::new(), Vec::new());
    for (n, (from, to)) in renames.iter().enumerate() {
        let name = from
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let temp = rich_ext::file_ops::unique(
            &from.with_file_name(format!(".{name}.renaming-{}-{n}", std::process::id())),
        );
        match std::fs::rename(from, &temp) {
            Ok(()) => staged.push((from, temp, to)),
            Err(error) => failed.push(format!("{}: {error}", from.display())),
        }
    }
    let mut made = Vec::new();
    for (from, temp, to) in staged {
        match std::fs::rename(&temp, to) {
            Ok(()) => made.push((from.clone(), to.clone())),
            Err(error) => {
                // Back where it was.
                let _ = std::fs::rename(&temp, from);
                failed.push(format!("{}: {error}", to.display()));
            }
        }
    }
    (made, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("intuituive-bulk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_can_swap_and_taken_names_are_left_out() {
        let dir = dir("swap");
        for (name, body) in [("a", "A"), ("b", "B"), ("c", "C"), ("keep", "K")] {
            std::fs::write(dir.join(name), body).unwrap();
        }
        let paths: Vec<PathBuf> = ["a", "b", "c"].iter().map(|n| dir.join(n)).collect();
        let lines: Vec<String> = ["a", "b", "c"].iter().map(|n| n.to_string()).collect();
        let (plan, problems) = plan_renames(&paths, &lines, "b\na\nkeep\n\n", Some(&dir)).unwrap();
        assert_eq!(problems, ["c: keep is taken"]);
        let (made, failed) = apply_renames(&plan);
        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(made.len(), 2);
        assert_eq!(std::fs::read_to_string(dir.join("a")).unwrap(), "B");
        assert_eq!(std::fs::read_to_string(dir.join("b")).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(dir.join("keep")).unwrap(), "K");
        // A line lost: nothing is renamed.
        let lost = plan_renames(&paths, &lines, "x\ny\n", Some(&dir)).unwrap_err();
        assert_eq!(lost, "2 names for 3 files: nothing renamed");
        // The same new name twice: the second is left out.
        let (plan, problems) = plan_renames(&paths, &lines, "z\nz\nc\n", Some(&dir)).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(problems, ["b: z is taken"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
