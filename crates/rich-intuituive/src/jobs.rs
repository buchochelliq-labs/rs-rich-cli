//! Cancellable background jobs with progress, and a panel listing them: a
//! task manager for any work (copies, downloads, builds, exports).

use std::time::Duration;

use crate::clock::Instant;

use rich::markup::escape;
pub use rich_ext::cancel::CancelToken;

use crate::app::Ctx;
use crate::layout::TextAlign;
use crate::list::list_lazy;
use crate::node::{switch, text, Node};
use crate::reactive::{signal, Signal};
use crate::task::spawn_stream;

/// How far a job has got, as a [`jobs_panel`] shows it.
pub trait JobProgress: Clone + PartialEq + Default + Send + 'static {
    /// The share done, from 0 to 1.
    fn fraction(&self) -> f64;

    /// A few words after the percentage (`3/10`), or nothing.
    fn detail(&self) -> String {
        String::new()
    }

    /// The job has ended with errors: forget what was under way (a file
    /// name, say). Nothing by default.
    fn finish(&mut self) {}
}

/// The share done as a bare number: `0.25` is a quarter.
impl JobProgress for f64 {
    fn fraction(&self) -> f64 {
        *self
    }
}

/// How a job ended, as a [`Jobs`] list takes it.
pub trait JobOutcome: Send + 'static {
    /// What went wrong; a job with errors (and not cancelled) stays on
    /// the list, [`Failed`](JobState::Failed).
    fn errors(&self) -> Vec<String> {
        Vec::new()
    }

    /// It stopped because it was cancelled.
    fn cancelled(&self) -> bool {
        false
    }
}

impl JobOutcome for () {}

/// A job's errors, or nothing.
impl<T: Send + 'static> JobOutcome for Result<T, String> {
    fn errors(&self) -> Vec<String> {
        match self {
            Ok(_) => Vec::new(),
            Err(why) => vec![why.clone()],
        }
    }
}

/// Where a [`Job`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    /// Ended with these errors (a job that ends cleanly, or is cancelled,
    /// leaves the list).
    Failed(Vec<String>),
}

/// A job in a [`Jobs`] list.
#[derive(Clone, Debug)]
pub struct Job<P> {
    pub id: u64,
    /// What it does: `Copy 3 items to docs`.
    pub title: String,
    pub progress: P,
    pub state: JobState,
    cancel: CancelToken,
}

impl<P: PartialEq> PartialEq for Job<P> {
    fn eq(&self, other: &Self) -> bool {
        (self.id, &self.title, &self.progress, &self.state)
            == (other.id, &other.title, &other.progress, &other.state)
    }
}

impl<P> Job<P> {
    /// A job as a list holds it, for tests and custom lists.
    pub fn new(id: u64, title: impl Into<String>, progress: P, state: JobState) -> Job<P> {
        Job {
            id,
            title: title.into(),
            progress,
            state,
            cancel: CancelToken::new(),
        }
    }

    /// Ask it to stop at its next safe point.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// What went wrong, for a [`Failed`](JobState::Failed) job; none
    /// while it runs.
    pub fn errors(&self) -> &[String] {
        match &self.state {
            JobState::Failed(errors) => errors,
            JobState::Running => &[],
        }
    }
}

/// What a process run by [`Jobs::run_process`] printed, and how it ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Its exit code (`None` when a signal ended it).
    pub code: Option<i32>,
    /// Its standard output's last lines (a thousand at most), in order.
    pub stdout: Vec<String>,
    /// Its standard error's last lines (a thousand at most), in order.
    pub stderr: Vec<String>,
}

/// How a process run by [`Jobs::run_process`] ended: it is the job's
/// [`JobOutcome`], so one that failed stays on the list with its error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessOutcome {
    /// It exited with 0.
    Done(ProcessOutput),
    /// It could not start, or exited otherwise: why (the last line of its
    /// standard error, else its exit status), and what it printed.
    Failed(String, ProcessOutput),
    /// It was cancelled, and killed.
    Cancelled(ProcessOutput),
}

impl ProcessOutcome {
    /// What it printed, however it ended.
    pub fn output(&self) -> &ProcessOutput {
        match self {
            ProcessOutcome::Done(out)
            | ProcessOutcome::Failed(_, out)
            | ProcessOutcome::Cancelled(out) => out,
        }
    }
}

impl JobOutcome for ProcessOutcome {
    fn errors(&self) -> Vec<String> {
        match self {
            ProcessOutcome::Failed(why, _) => vec![why.clone()],
            _ => Vec::new(),
        }
    }

    fn cancelled(&self) -> bool {
        matches!(self, ProcessOutcome::Cancelled(_))
    }
}

/// The most lines of each stream a [`ProcessOutput`] keeps.
const KEPT_LINES: usize = 1000;

/// Background jobs, each on a thread of its own, reporting progress `P`
/// into a signal a few times a second and cancellable. A job that ends
/// cleanly (or is cancelled) leaves the list; one with errors stays,
/// [`Failed`](JobState::Failed), until
/// [`clear_finished`](Self::clear_finished). `Copy`; made by [`jobs`],
/// shown by [`jobs_panel`]. It knows nothing of what the jobs do: the
/// `files` plugin's `tasks` runs file operations on it.
pub struct Jobs<P: 'static> {
    jobs: Signal<Vec<Job<P>>>,
    next: Signal<u64>,
}

impl<P> Clone for Jobs<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for Jobs<P> {}

/// A job list. Call it while the app is built.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{jobs, jobs_panel};
///
/// let app = App::new(|| {
///     let list = jobs::<f64>();
///     let said = signal(String::new());
///     column([jobs_panel(list).fixed(1), text!("{said}")])
///         .on_key("r", move |_| {
///             list.run(
///                 "Count to three",
///                 |cancel, progress| {
///                     for n in 1..=3 {
///                         if cancel.is_cancelled() {
///                             break;
///                         }
///                         progress(&(n as f64 / 3.0));
///                     }
///                     Ok::<_, String>(3)
///                 },
///                 move |counted, _| said.set(format!("counted {}", counted.unwrap())),
///             );
///         })
/// })
/// .wait_for_tasks(true);
/// let screen = app.render_with(&["r"], 30, 2).unwrap();
/// assert_eq!(screen[0].trim(), "No tasks");
/// assert_eq!(screen[1].trim_end(), "counted 3");
/// ```
pub fn jobs<P: JobProgress>() -> Jobs<P> {
    Jobs {
        jobs: signal(Vec::new()),
        next: signal(1),
    }
}

/// How often a job's progress reaches the app.
const EVERY: Duration = Duration::from_millis(50);

impl<P: JobProgress> Jobs<P> {
    /// The jobs in the list.
    pub fn list(&self) -> Signal<Vec<Job<P>>> {
        self.jobs
    }

    /// How many jobs are running (subscribing).
    pub fn running(&self) -> usize {
        self.jobs
            .with(|jobs| jobs.iter().filter(|j| j.state == JobState::Running).count())
    }

    /// Run `work` as a job called `title`, on a thread of its own: it gets
    /// a cancel token to check and a function to report its progress
    /// with (passed on at most every 50 ms), and returns its outcome,
    /// which `done` gets on the app's thread. Returns the job's id.
    pub fn run<R: JobOutcome>(
        &self,
        title: &str,
        work: impl FnOnce(&CancelToken, &mut dyn FnMut(&P)) -> R + Send + 'static,
        done: impl FnOnce(R, &mut Ctx) + Send + 'static,
    ) -> u64 {
        let id = self.next.get_untracked();
        self.next.set(id + 1);
        let cancel = CancelToken::new();
        self.jobs.update(|jobs| {
            jobs.push(Job {
                id,
                title: title.to_string(),
                progress: P::default(),
                state: JobState::Running,
                cancel: cancel.clone(),
            })
        });
        let jobs = self.jobs;
        spawn_stream(move |stream| {
            let proxy = stream.proxy();
            let mut last: Option<Instant> = None;
            let mut report_progress = |progress: &P| {
                if last.is_some_and(|l| l.elapsed() < EVERY) {
                    return;
                }
                last = Some(Instant::now());
                let progress = progress.clone();
                proxy.run(move || {
                    jobs.update(|all| {
                        if let Some(job) = all.iter_mut().find(|j| j.id == id) {
                            job.progress = progress;
                        }
                    })
                });
            };
            let outcome = work(&cancel, &mut report_progress);
            let (errors, cancelled) = (outcome.errors(), outcome.cancelled());
            proxy.run_with(move |cx| {
                jobs.update(|all| {
                    if errors.is_empty() || cancelled {
                        all.retain(|j| j.id != id);
                    } else if let Some(job) = all.iter_mut().find(|j| j.id == id) {
                        job.state = JobState::Failed(errors);
                        job.progress.finish();
                    }
                });
                done(outcome, cx);
            });
        });
        id
    }

    /// Run `command` as a job called `title`: its output read line by
    /// line (stdin closed), cancelling it kills it, and `done` gets how it
    /// ended ([`ProcessOutcome`]) on the app's thread. A process that
    /// exits otherwise than with 0 fails the job with the last line of its
    /// standard error (or its exit status), which stays on the list.
    /// Returns the job's id. [`run_process_with`](Self::run_process_with)
    /// also reads progress from its lines.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{jobs, ProcessOutcome};
    ///
    /// let app = App::new(|| {
    ///     let list = jobs::<f64>();
    ///     let said = signal(String::new());
    ///     text!("{said}").on_key("r", move |_| {
    ///         let mut command = std::process::Command::new("sh");
    ///         command.args(["-c", "echo hello; echo oops >&2; exit 3"]);
    ///         list.run_process("Greet", command, move |outcome, _| {
    ///             if let ProcessOutcome::Failed(why, out) = outcome {
    ///                 said.set(format!("{why} after {}", out.stdout.join(" ")));
    ///             }
    ///         });
    ///     })
    /// })
    /// .wait_for_tasks(true);
    /// # if cfg!(unix) {
    /// let screen = app.render_with(&["r"], 30, 1).unwrap();
    /// assert_eq!(screen[0].trim_end(), "oops after hello");
    /// # }
    /// ```
    pub fn run_process(
        &self,
        title: &str,
        command: std::process::Command,
        done: impl FnOnce(ProcessOutcome, &mut Ctx) + Send + 'static,
    ) -> u64 {
        self.run_process_with(title, command, |_| None, done)
    }

    /// [`run_process`](Self::run_process), its progress read from each
    /// line it prints (either stream): `progress` returns the progress a
    /// line says, or `None` (a line `37%`, `3/10`, a download's).
    pub fn run_process_with(
        &self,
        title: &str,
        mut command: std::process::Command,
        mut progress: impl FnMut(&str) -> Option<P> + Send + 'static,
        done: impl FnOnce(ProcessOutcome, &mut Ctx) + Send + 'static,
    ) -> u64 {
        use std::io::BufRead;
        use std::process::Stdio;
        use std::sync::mpsc;

        self.run(
            title,
            move |cancel, report| {
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut child = match command.spawn() {
                    Ok(child) => child,
                    Err(error) => {
                        let program = command.get_program().to_string_lossy().into_owned();
                        return ProcessOutcome::Failed(
                            format!("{program}: {error}"),
                            ProcessOutput::default(),
                        );
                    }
                };
                // Each stream's lines, read on threads of their own, to
                // this one: `true` for standard error.
                let (tx, rx) = mpsc::channel::<(bool, String)>();
                let mut readers = Vec::new();
                let streams: [(bool, Option<Box<dyn std::io::Read + Send>>); 2] = [
                    (false, child.stdout.take().map(|s| Box::new(s) as _)),
                    (true, child.stderr.take().map(|s| Box::new(s) as _)),
                ];
                for (is_err, stream) in streams {
                    let Some(stream) = stream else { continue };
                    let tx = tx.clone();
                    readers.push(std::thread::spawn(move || {
                        for line in std::io::BufReader::new(stream).lines() {
                            let Ok(line) = line else { break };
                            if tx.send((is_err, line)).is_err() {
                                break;
                            }
                        }
                    }));
                }
                drop(tx);
                let mut out = ProcessOutput::default();
                let mut keep = |out: &mut ProcessOutput, is_err: bool, line: String| {
                    if let Some(p) = progress(&line) {
                        report(&p);
                    }
                    let lines = if is_err {
                        &mut out.stderr
                    } else {
                        &mut out.stdout
                    };
                    lines.push(line);
                    if lines.len() > KEPT_LINES {
                        lines.remove(0);
                    }
                };
                let mut cancelled = false;
                loop {
                    match rx.recv_timeout(Duration::from_millis(20)) {
                        Ok((is_err, line)) => keep(&mut out, is_err, line),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if cancel.is_cancelled() {
                        // Killed; a process it started may still hold its
                        // output open, so its readers are left to end
                        // with it.
                        cancelled = true;
                        let _ = child.kill();
                        readers.clear();
                        break;
                    }
                }
                for reader in readers {
                    let _ = reader.join();
                }
                let status = child.wait();
                out.code = status.as_ref().ok().and_then(|s| s.code());
                if cancelled {
                    return ProcessOutcome::Cancelled(out);
                }
                match status {
                    Ok(status) if status.success() => ProcessOutcome::Done(out),
                    Ok(status) => {
                        let why = out
                            .stderr
                            .iter()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .cloned()
                            .unwrap_or_else(|| format!("exited with {status}"));
                        ProcessOutcome::Failed(why, out)
                    }
                    Err(error) => ProcessOutcome::Failed(error.to_string(), out),
                }
            },
            done,
        )
    }

    /// Cancel the job `id`.
    pub fn cancel(&self, id: u64) {
        self.jobs.with_untracked(|jobs| {
            if let Some(job) = jobs.iter().find(|j| j.id == id) {
                job.cancel();
            }
        });
    }

    /// Cancel every running job.
    pub fn cancel_all(&self) {
        self.jobs
            .with_untracked(|jobs| jobs.iter().for_each(Job::cancel));
    }

    /// Take the failed jobs off the list.
    pub fn clear_finished(&self) {
        self.jobs
            .update(|jobs| jobs.retain(|j| j.state == JobState::Running));
    }
}

/// A bar of `width` cells, `fraction` of it filled.
fn bar(fraction: f64, width: usize) -> String {
    let filled = ((fraction.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    format!(
        "[green]{}[/][bright_black]{}[/]",
        "━".repeat(filled),
        "━".repeat(width - filled)
    )
}

/// A job's row: a bar, the share done and its detail, and its title; or
/// its errors.
pub fn job_line<P: JobProgress>(job: &Job<P>) -> String {
    job_line_with(job, &JobsPanelOptions::new())
}

/// A job's row as `options` says ([`job_line`] with the default ones).
pub fn job_line_with<P: JobProgress>(job: &Job<P>, options: &JobsPanelOptions) -> String {
    let title = escape(&job.title);
    match &job.state {
        JobState::Running => {
            let p = &job.progress;
            let percent = (p.fraction().clamp(0.0, 1.0) * 100.0).floor() as u32;
            let detail = p.detail();
            let detail = if detail.is_empty() {
                detail
            } else {
                format!(" {}", escape(&detail))
            };
            format!("{} {percent:>3}%{detail}  {title}", bar(p.fraction(), 16))
        }
        JobState::Failed(errors) => {
            let first = errors.first().map(String::as_str).unwrap_or_default();
            let more = match errors.len() {
                0 | 1 => String::new(),
                n => format!(" (and {} more)", n - 1),
            };
            if options.failed_title {
                format!("[red]failed[/]  {title}: {}{more}", escape(first))
            } else {
                format!("[red]failed[/]  {}{more}", escape(first))
            }
        }
    }
}

/// How a [`jobs_panel_with`] shows its jobs.
#[derive(Clone, Debug)]
pub struct JobsPanelOptions {
    failed_title: bool,
}

impl Default for JobsPanelOptions {
    fn default() -> JobsPanelOptions {
        JobsPanelOptions { failed_title: true }
    }
}

impl JobsPanelOptions {
    /// Each row as [`job_line`] draws it.
    pub fn new() -> JobsPanelOptions {
        JobsPanelOptions::default()
    }

    /// Whether a failed job's row starts its error with the job's title
    /// (it does by default). Off when the error already names what failed
    /// (`src: can't go inside itself`), or when the app reported the job
    /// with its title already.
    pub fn failed_title(mut self, on: bool) -> JobsPanelOptions {
        self.failed_title = on;
        self
    }
}

/// The jobs of `jobs`, one row each with a progress bar ([`job_line`]):
/// `x` cancels the hovered job, `X` every job, `c` clears the failed ones.
/// `No tasks` when there are none.
pub fn jobs_panel<P: JobProgress>(jobs: Jobs<P>) -> Node {
    jobs_panel_with(jobs, JobsPanelOptions::new())
}

/// A [`jobs_panel`] whose rows are as `options` says.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{jobs, jobs_panel_with, JobsPanelOptions};
///
/// let app = App::new(|| {
///     let list = jobs::<f64>();
///     jobs_panel_with(list, JobsPanelOptions::new().failed_title(false)).on_key("r", move |_| {
///         list.run("Export", |_, _| Err::<(), _>("disk full".to_string()), |_, _| {});
///     })
/// })
/// .wait_for_tasks(true);
/// let screen = app.render_with(&["r"], 30, 1).unwrap();
/// assert_eq!(screen[0].trim_end(), "failed  disk full");
/// ```
pub fn jobs_panel_with<P: JobProgress>(jobs: Jobs<P>, options: JobsPanelOptions) -> Node {
    let cursor = signal(0usize);
    let list = jobs.jobs;
    switch(
        move || list.with(Vec::is_empty),
        move |empty| {
            if empty {
                return text(|| "[bright_black]No tasks".into()).text_align(TextAlign::Center);
            }
            let options = options.clone();
            list_lazy(
                move || list.with(Vec::len),
                move |i| {
                    list.with(|all| {
                        all.get(i)
                            .map(|job| job_line_with(job, &options))
                            .unwrap_or_default()
                    })
                },
                cursor,
            )
        },
    )
    .bind("x", "Cancel the task", move |_| {
        let at = cursor.get_untracked();
        let id = list.with_untracked(|all| all.get(at).map(|j| j.id));
        if let Some(id) = id {
            jobs.cancel(id);
        }
    })
    .bind("X", "Cancel every task", move |_| jobs.cancel_all())
    .bind("c", "Clear failed tasks", move |_| jobs.clear_finished())
    .name("tasks")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_show_progress_and_failures() {
        let mut job = Job::new(1, "Export", 0.5f64, JobState::Running);
        let line = job_line(&job);
        assert!(line.contains(" 50%  Export"), "{line}");
        assert!(
            line.starts_with("[green]━━━━━━━━[/][bright_black]━━━━━━━━[/]"),
            "{line}"
        );
        job.state = JobState::Failed(vec!["a: denied".into(), "b: denied".into()]);
        assert_eq!(
            job_line(&job),
            "[red]failed[/]  Export: a: denied (and 1 more)"
        );
    }
}
