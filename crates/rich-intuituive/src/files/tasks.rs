//! File jobs in the background, with progress: Yazi's task manager, as
//! file operations run on the core's generic [`Jobs`](crate::Jobs).

use rich_ext::cancel::CancelToken;
use rich_ext::file_ops::{self, Op, Progress, Report};

use crate::app::Ctx;
use crate::jobs::{jobs, jobs_panel, JobOutcome, JobProgress, Jobs};
use crate::node::Node;
use crate::reactive::Signal;

pub use crate::jobs::JobState;

/// A file job in a [`Tasks`] list: the core's [`Job`](crate::jobs::Job)
/// with a file operation's [`Progress`].
pub type Job = crate::jobs::Job<Progress>;

/// A file operation's progress, as a job panel shows it: by bytes, else
/// by items, with `items/total` when there are several.
impl JobProgress for Progress {
    fn fraction(&self) -> f64 {
        Progress::fraction(self)
    }

    fn detail(&self) -> String {
        if self.total_items > 1 {
            format!("{}/{}", self.items, self.total_items)
        } else {
            String::new()
        }
    }

    fn finish(&mut self) {
        self.current = None;
    }
}

impl JobOutcome for Report {
    fn errors(&self) -> Vec<String> {
        self.errors.clone()
    }

    fn cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Background file jobs (copies, moves, deletes, trashing, extraction),
/// each on a thread of its own, reporting progress into a signal a few
/// times a second and cancellable: Yazi's scheduler, on the core's
/// generic [`Jobs`]. A job that ends cleanly (or is cancelled) leaves the
/// list; one with errors stays, [`Failed`](JobState::Failed), until
/// [`clear_finished`](Self::clear_finished). `Copy`; made by [`tasks`],
/// shown by [`tasks_panel`].
#[derive(Clone, Copy)]
pub struct Tasks {
    jobs: Jobs<Progress>,
}

/// A task list. Call it while the app is built.
///
/// ```
/// use intuituive::files::{tasks, tasks_panel};
/// use intuituive::prelude::*;
/// use rich_ext::file_ops::{Conflict, Op};
///
/// let dir = std::env::temp_dir().join(format!("intuituive-tasks-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("to")).unwrap();
/// std::fs::write(dir.join("a.txt"), "a").unwrap();
/// let base = dir.clone();
/// let app = App::new(move || {
///     let jobs = tasks();
///     let copied = signal(String::new());
///     let base = base.clone();
///     column([tasks_panel(jobs), text!("{copied}")])
///         .on_key("p", move |_| {
///             let op = Op::Copy { from: vec![base.join("a.txt")], into: base.join("to"), conflict: Conflict::Rename };
///             jobs.run(op, move |report, _| copied.set(format!("made {}", report.made.len())));
///         })
///         .on_key("q", |cx| cx.quit())
/// })
/// .wait_for_tasks(true);
/// let screen = app.render_with(&["p", "q"], 30, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "made 1");
/// assert!(dir.join("to/a.txt").exists());
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn tasks() -> Tasks {
    Tasks { jobs: jobs() }
}

impl Tasks {
    /// The jobs in the list.
    pub fn jobs(&self) -> Signal<Vec<Job>> {
        self.jobs.list()
    }

    /// The core job list the file jobs run on.
    pub fn as_jobs(&self) -> Jobs<Progress> {
        self.jobs
    }

    /// How many jobs are running (subscribing).
    pub fn running(&self) -> usize {
        self.jobs.running()
    }

    /// Run a file operation ([`file_ops::run`]); `done` gets its report
    /// on the app's thread. Returns the job's id.
    pub fn run(&self, op: Op, done: impl FnOnce(Report, &mut Ctx) + Send + 'static) -> u64 {
        let title = op.title();
        self.run_with(
            &title,
            move |cancel, progress| file_ops::run(&op, cancel, progress),
            done,
        )
    }

    /// Extract the archive at `path` into `dest` as a job (with the
    /// `archive` feature): Yazi's `extract` opener. What was left out of
    /// it is in the report's errors; `made` is `dest`.
    #[cfg(feature = "archive")]
    pub fn extract(
        &self,
        path: std::path::PathBuf,
        dest: std::path::PathBuf,
        done: impl FnOnce(Report, &mut Ctx) + Send + 'static,
    ) -> u64 {
        let title = format!(
            "Extract {}",
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        );
        self.run_with(
            &title,
            move |cancel, progress| match rich_ext::archive::extract_with(
                &path, &dest, cancel, progress,
            ) {
                Ok(out) => Report {
                    made: vec![dest],
                    errors: out
                        .skipped
                        .into_iter()
                        .map(|(entry, why)| format!("{entry}: {why}"))
                        .collect(),
                    cancelled: out.cancelled,
                },
                Err(error) => Report {
                    errors: vec![error.to_string()],
                    ..Report::default()
                },
            },
            done,
        )
    }

    /// Run `work` as a job called `title`: it gets a cancel token to check
    /// and a function to report its progress with, and returns a report.
    pub fn run_with(
        &self,
        title: &str,
        work: impl FnOnce(&CancelToken, &mut dyn FnMut(&Progress)) -> Report + Send + 'static,
        done: impl FnOnce(Report, &mut Ctx) + Send + 'static,
    ) -> u64 {
        self.jobs.run(title, work, done)
    }

    /// Cancel the job `id`.
    pub fn cancel(&self, id: u64) {
        self.jobs.cancel(id);
    }

    /// Cancel every running job.
    pub fn cancel_all(&self) {
        self.jobs.cancel_all();
    }

    /// Take the failed jobs off the list.
    pub fn clear_finished(&self) {
        self.jobs.clear_finished();
    }
}

/// The jobs of `tasks`, one row each with a progress bar (Yazi's `w`
/// panel; the core's [`jobs_panel`]): `x` cancels the hovered job, `X` every job, `c` clears the
/// failed ones. `No tasks` when there are none.
pub fn tasks_panel(tasks: Tasks) -> Node {
    jobs_panel(tasks.jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::job_line;

    #[test]
    fn rows_show_progress_and_failures() {
        let progress = Progress {
            bytes: 50,
            total_bytes: 100,
            items: 1,
            total_items: 2,
            current: None,
        };
        let mut job = Job::new(1, "Copy a to b", progress, JobState::Running);
        let line = job_line(&job);
        assert!(line.contains(" 50% 1/2  Copy a to b"), "{line}");
        assert!(
            line.starts_with("[green]━━━━━━━━[/][bright_black]━━━━━━━━[/]"),
            "{line}"
        );
        job.state = JobState::Failed(vec!["a: denied".into(), "b: denied".into()]);
        assert_eq!(
            job_line(&job),
            "[red]failed[/]  Copy a to b: a: denied (and 1 more)"
        );
    }
}
