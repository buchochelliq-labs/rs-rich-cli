//! File jobs in the background, with progress: Yazi's task manager.

use std::time::{Duration, Instant};

use rich::markup::escape;
use rich_ext::cancel::CancelToken;
use rich_ext::file_ops::{self, Op, Progress, Report};

use crate::app::Ctx;
use crate::layout::TextAlign;
use crate::list::list_lazy;
use crate::node::{switch, text, Node};
use crate::reactive::{signal, Signal};

/// Where a [`Job`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    /// Ended with these errors (a job that ends cleanly, or is cancelled,
    /// leaves the list).
    Failed(Vec<String>),
}

/// A job in a [`Tasks`] list.
#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    /// What it does: `Copy 3 items to docs`.
    pub title: String,
    pub progress: Progress,
    pub state: JobState,
    cancel: CancelToken,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        (self.id, &self.title, &self.progress, &self.state)
            == (other.id, &other.title, &other.progress, &other.state)
    }
}

impl Job {
    /// Ask it to stop at its next safe point.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

/// Background file jobs (copies, moves, deletes, trashing, extraction),
/// each on a thread of its own, reporting progress into a signal a few
/// times a second and cancellable: Yazi's scheduler. A job that ends
/// cleanly (or is cancelled) leaves the list; one with errors stays,
/// [`Failed`](JobState::Failed), until [`clear_finished`](Self::clear_finished).
/// `Copy`; made by [`tasks`], shown by [`tasks_panel`].
#[derive(Clone, Copy)]
pub struct Tasks {
    jobs: Signal<Vec<Job>>,
    next: Signal<u64>,
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
        jobs: signal(Vec::new()),
        next: signal(1),
    }
}

/// How often a job's progress reaches the app.
const EVERY: Duration = Duration::from_millis(50);

impl Tasks {
    /// The jobs in the list.
    pub fn jobs(&self) -> Signal<Vec<Job>> {
        self.jobs
    }

    /// How many jobs are running (subscribing).
    pub fn running(&self) -> usize {
        self.jobs
            .with(|jobs| jobs.iter().filter(|j| j.state == JobState::Running).count())
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
        let id = self.next.get_untracked();
        self.next.set(id + 1);
        let cancel = CancelToken::new();
        self.jobs.update(|jobs| {
            jobs.push(Job {
                id,
                title: title.to_string(),
                progress: Progress::default(),
                state: JobState::Running,
                cancel: cancel.clone(),
            })
        });
        let jobs = self.jobs;
        super::background("Tasks::run()", move |proxy| {
            let mut last: Option<Instant> = None;
            let mut report_progress = |progress: &Progress| {
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
            let report = work(&cancel, &mut report_progress);
            proxy.run_with(move |cx| {
                jobs.update(|all| {
                    if report.errors.is_empty() || report.cancelled {
                        all.retain(|j| j.id != id);
                    } else if let Some(job) = all.iter_mut().find(|j| j.id == id) {
                        job.state = JobState::Failed(report.errors.clone());
                        job.progress.current = None;
                    }
                });
                done(report, cx);
            });
        });
        id
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

/// A job's row: its title, a bar and the share done, or its errors.
fn job_line(job: &Job) -> String {
    let title = escape(&job.title);
    match &job.state {
        JobState::Running => {
            let p = &job.progress;
            let percent = (p.fraction() * 100.0).floor() as u32;
            let count = if p.total_items > 1 {
                format!(" {}/{}", p.items, p.total_items)
            } else {
                String::new()
            };
            format!("{} {percent:>3}%{count}  {title}", bar(p.fraction(), 16))
        }
        JobState::Failed(errors) => {
            let first = errors.first().map(String::as_str).unwrap_or_default();
            let more = match errors.len() {
                0 | 1 => String::new(),
                n => format!(" (and {} more)", n - 1),
            };
            format!("[red]failed[/]  {title}: {}{more}", escape(first))
        }
    }
}

/// The jobs of `tasks`, one row each with a progress bar (Yazi's `w`
/// panel): `x` cancels the hovered job, `X` every job, `c` clears the
/// failed ones. `No tasks` when there are none.
pub fn tasks_panel(tasks: Tasks) -> Node {
    let cursor = signal(0usize);
    let jobs = tasks.jobs;
    switch(
        move || jobs.with(Vec::is_empty),
        move |empty| {
            if empty {
                return text(|| "[bright_black]No tasks".into()).text_align(TextAlign::Center);
            }
            list_lazy(
                move || jobs.with(Vec::len),
                move |i| jobs.with(|all| all.get(i).map(job_line).unwrap_or_default()),
                cursor,
            )
        },
    )
    .bind("x", "Cancel the task", move |_| {
        let at = cursor.get_untracked();
        let id = jobs.with_untracked(|all| all.get(at).map(|j| j.id));
        if let Some(id) = id {
            tasks.cancel(id);
        }
    })
    .bind("X", "Cancel every task", move |_| tasks.cancel_all())
    .bind("c", "Clear failed tasks", move |_| tasks.clear_finished())
    .name("tasks")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_show_progress_and_failures() {
        let mut job = Job {
            id: 1,
            title: "Copy a to b".into(),
            progress: Progress {
                bytes: 50,
                total_bytes: 100,
                items: 1,
                total_items: 2,
                current: None,
            },
            state: JobState::Running,
            cancel: CancelToken::new(),
        };
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
