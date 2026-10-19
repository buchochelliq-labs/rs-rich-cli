//! Cancellable background jobs with progress, and a panel listing them: a
//! task manager for any work (copies, downloads, builds, exports).

use std::time::{Duration, Instant};

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
}

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
            format!("[red]failed[/]  {title}: {}{more}", escape(first))
        }
    }
}

/// The jobs of `jobs`, one row each with a progress bar ([`job_line`]):
/// `x` cancels the hovered job, `X` every job, `c` clears the failed ones.
/// `No tasks` when there are none.
pub fn jobs_panel<P: JobProgress>(jobs: Jobs<P>) -> Node {
    let cursor = signal(0usize);
    let list = jobs.jobs;
    switch(
        move || list.with(Vec::is_empty),
        move |empty| {
            if empty {
                return text(|| "[bright_black]No tasks".into()).text_align(TextAlign::Center);
            }
            list_lazy(
                move || list.with(Vec::len),
                move |i| list.with(|all| all.get(i).map(job_line).unwrap_or_default()),
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
