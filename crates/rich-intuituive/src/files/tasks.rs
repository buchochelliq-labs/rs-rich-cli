//! File jobs in the background, with progress: Yazi's task manager, as
//! file operations run on the core's generic [`Jobs`](crate::Jobs).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rich_ext::cancel::CancelToken;
use rich_ext::file_ops::{self, Op, Progress, Report};

use crate::app::Ctx;
use crate::jobs::{jobs, jobs_panel_with, JobOutcome, JobProgress, Jobs};
use crate::node::Node;
use crate::reactive::{signal, Signal};

pub use crate::jobs::{JobState, JobsPanelOptions};

/// What [`Tasks::on_fail`] runs.
type OnFail = Rc<RefCell<dyn FnMut(&Job, &mut Ctx)>>;

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
    on_fail: Signal<Option<OnFail>>,
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
    Tasks {
        jobs: jobs(),
        on_fail: signal(None),
    }
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

    /// The share of the running jobs done (their mean; 0 with none), how
    /// many run and how many failed, subscribing: Yazi's progress in the
    /// status line, as a [`seg_gauge`](crate::widgets::seg_gauge).
    ///
    /// ```
    /// use intuituive::files::tasks;
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{seg_gauge, status_bar};
    ///
    /// let app = App::new(|| {
    ///     let jobs = tasks();
    ///     status_bar(
    ///         [],
    ///         [seg_gauge(move || jobs.summary().0, move || {
    ///             let (_, running, failed) = jobs.summary();
    ///             format!("{running} running, {failed} failed")
    ///         })
    ///         .width(20)],
    ///     )
    /// });
    /// let screen = app.render_with(&[], 20, 1).unwrap();
    /// assert_eq!(screen[0].trim(), "0 running, 0 failed");
    /// ```
    pub fn summary(&self) -> (f64, usize, usize) {
        self.jobs.list().with(|jobs| {
            let (mut done, mut running, mut failed) = (0.0, 0, 0);
            for job in jobs {
                match job.state {
                    JobState::Running => {
                        done += job.progress.fraction().clamp(0.0, 1.0);
                        running += 1;
                    }
                    JobState::Failed(_) => failed += 1,
                }
            }
            let share = if running == 0 {
                0.0
            } else {
                done / running as f64
            };
            (share, running, failed)
        })
    }

    /// Run `handler` with each job that fails (it stays on the list,
    /// [`Failed`](JobState::Failed)), for every job of the list, before
    /// that job's own `done`: one place to report failures (a
    /// notification), not each call. A [`tasks_panel_with`] showing the
    /// list can leave the title out of failed rows, so a failure is not
    /// told twice.
    ///
    /// ```
    /// use intuituive::files::tasks;
    /// use intuituive::prelude::*;
    /// use rich_ext::file_ops::Op;
    ///
    /// let app = App::new(|| {
    ///     let said = signal(String::new());
    ///     let jobs = tasks().on_fail(move |job, _| said.set(format!("{} failed", job.title)));
    ///     text!("{said}").on_key("d", move |_| {
    ///         jobs.run(Op::Delete(vec!["/no/such/file".into()]), |_, _| {});
    ///     })
    /// })
    /// .wait_for_tasks(true);
    /// let screen = app.render_with(&["d"], 40, 1).unwrap();
    /// assert!(screen[0].trim_end().ends_with("failed"), "{screen:?}");
    /// ```
    #[must_use]
    pub fn on_fail(self, handler: impl FnMut(&Job, &mut Ctx) + 'static) -> Tasks {
        let handler: OnFail = Rc::new(RefCell::new(handler));
        self.on_fail.replace(Some(handler));
        self
    }

    /// Job `id` failed: tell the [`on_fail`](Self::on_fail) handler.
    fn failed(&self, id: u64, cx: &mut Ctx) {
        let Some(handler) = self.on_fail.with_untracked(Clone::clone) else {
            return;
        };
        let job = self
            .jobs
            .list()
            .with_untracked(|all| all.iter().find(|j| j.id == id).cloned());
        if let Some(job) = job {
            (handler.borrow_mut())(&job, cx);
        }
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
        let (tasks, id) = (*self, Arc::new(AtomicU64::new(0)));
        let mine = id.clone();
        let started = self.jobs.run(title, work, move |report: Report, cx| {
            if !report.errors.is_empty() && !report.cancelled {
                tasks.failed(mine.load(Ordering::SeqCst), cx);
            }
            done(report, cx);
        });
        // Set before the job can end: its end comes on this thread.
        id.store(started, Ordering::SeqCst);
        started
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
/// panel; the core's [`jobs_panel`](crate::jobs_panel)): `x` cancels the
/// hovered job, `X` every job, `c` clears the failed ones. `No tasks`
/// when there are none.
pub fn tasks_panel(tasks: Tasks) -> Node {
    tasks_panel_with(tasks, JobsPanelOptions::new())
}

/// A [`tasks_panel`] whose rows are as `options` says:
/// `JobsPanelOptions::new().failed_title(false)` shows a failed job's
/// error without its title (`src: can't go inside itself`), when an
/// [`on_fail`](Tasks::on_fail) handler has told it with the title already.
pub fn tasks_panel_with(tasks: Tasks, options: JobsPanelOptions) -> Node {
    jobs_panel_with(tasks.jobs, options)
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
