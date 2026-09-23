//! CLI boundary: `--watch` for one or more local files.
//!
//! Change detection is event-driven (the `notify` crate watches each file's
//! *parent directory*, so atomic rename-over saves and delete-and-recreate are
//! seen) with the original content-hash polling loop as the fallback. Events
//! pass through a per-resource [`Debouncer`] so an editor's burst of writes,
//! renames and attribute changes becomes one re-render. A single file keeps
//! the original clear-and-home viewport repaint; several files share the
//! terminal as one `rich_ext::live::LiveCoordinator` region each.
//!
//! URLs keep their polling-only loop in `main.rs` and never reach this module.
use super::{render_target, run_once, watch_fingerprint, Cli, ExitClass};
use rich::protocol::{RenderEnvironment, TargetCapabilities};
use rich::rule::Rule;
use rich::{Console, Segment, Style};
use rich_ext::live::{LiveCoordinator, RegionId};
use rich_ext::sanitize_terminal_controls;
use rich_ext::target::{RenderTarget, TargetKind, TargetOverrides};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// Default `--watch-debounce`: long enough to span an editor's
/// write/rename/chmod sequence, short enough to feel immediate.
pub(super) const DEFAULT_DEBOUNCE_SECONDS: f64 = 0.1;

/// A pending change is rendered at the latest this many debounce windows
/// after its first event, so a file rewritten faster than the window (a busy
/// log) still refreshes instead of waiting for a quiet period forever.
const MAX_WAIT_WINDOWS: u32 = 10;

/// How often live regions check the terminal size.
const RESIZE_CHECK: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------
// Frame capture
// ---------------------------------------------------------------------------

/// Output of one render captured for a live region instead of stdout.
#[derive(Default)]
pub(super) struct Frame {
    width: usize,
    segments: Vec<Segment>,
    error: Option<String>,
}

thread_local! {
    static CAPTURE: RefCell<Option<Frame>> = const { RefCell::new(None) };
}

/// Run `render` with stdout output and error messages captured into a frame.
fn capture(width: usize, render: impl FnOnce() -> ExitCode) -> (ExitCode, Frame) {
    CAPTURE.with(|slot| {
        *slot.borrow_mut() = Some(Frame {
            width,
            ..Frame::default()
        })
    });
    let status = render();
    let frame = CAPTURE
        .with(|slot| slot.borrow_mut().take())
        .unwrap_or_default();
    (status, frame)
}

/// The render width while a frame is being captured.
pub(super) fn capture_width() -> Option<usize> {
    CAPTURE.with(|slot| slot.borrow().as_ref().map(|frame| frame.width))
}

pub(super) fn capturing() -> bool {
    capture_width().is_some()
}

/// Store rendered segments for the current frame; `false` when not capturing.
pub(super) fn capture_segments(segments: Vec<Segment>) -> bool {
    CAPTURE.with(|slot| match slot.borrow_mut().as_mut() {
        Some(frame) => {
            frame.segments.extend(segments);
            true
        }
        None => false,
    })
}

/// Store an error for the current frame; `false` when not capturing.
pub(super) fn capture_error(message: &str) -> bool {
    CAPTURE.with(|slot| match slot.borrow_mut().as_mut() {
        Some(frame) => {
            frame.error.get_or_insert_with(|| message.to_string());
            true
        }
        None => false,
    })
}

// ---------------------------------------------------------------------------
// Debouncing
// ---------------------------------------------------------------------------

/// Per-resource trailing-edge debouncer with an injectable clock: callers pass
/// `now` explicitly, so tests drive it without sleeping.
#[derive(Debug)]
pub(super) struct Debouncer {
    window: Duration,
    max_wait: Duration,
    /// Resource index -> (first pending event, latest pending event).
    pending: BTreeMap<usize, (Instant, Instant)>,
}

impl Debouncer {
    pub(super) fn new(window: Duration) -> Self {
        Self {
            window,
            max_wait: window * MAX_WAIT_WINDOWS,
            pending: BTreeMap::new(),
        }
    }

    /// Record an event for `resource`, extending its quiet period.
    pub(super) fn note(&mut self, resource: usize, now: Instant) {
        self.pending
            .entry(resource)
            .and_modify(|(_, last)| *last = now)
            .or_insert((now, now));
    }

    fn deadline(&self, (first, last): (Instant, Instant)) -> Instant {
        (last + self.window).min(first + self.max_wait)
    }

    /// When the earliest pending resource becomes due, if any is pending.
    pub(super) fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|p| self.deadline(*p)).min()
    }

    /// Remove and return, in index order, every resource due at `now`.
    pub(super) fn take_due(&mut self, now: Instant) -> Vec<usize> {
        let due: Vec<usize> = self
            .pending
            .iter()
            .filter(|(_, pending)| self.deadline(**pending) <= now)
            .map(|(index, _)| *index)
            .collect();
        for index in &due {
            self.pending.remove(index);
        }
        due
    }
}

// ---------------------------------------------------------------------------
// Event sources
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Signal {
    /// A file event touched this resource.
    Changed(usize),
    /// Events may have been lost: re-check every resource.
    Rescan,
    /// Ctrl-C in live-region mode.
    Interrupt,
}

/// Maps paths reported by the watcher back to resource indices. Each resource
/// is matched by (canonical parent directory, file name), plus the symlink
/// target's location when the resource is a symlink.
#[derive(Clone, Debug)]
pub(super) struct Targets {
    entries: Vec<(usize, PathBuf, OsString)>,
}

impl Targets {
    pub(super) fn new(resources: &[String]) -> Self {
        let mut entries = Vec::new();
        for (index, resource) in resources.iter().enumerate() {
            let path = Path::new(resource);
            let mut locations = vec![path.to_path_buf()];
            if let Ok(real) = std::fs::canonicalize(path) {
                locations.push(real);
            }
            for location in locations {
                if let Some((dir, name)) = split(&location) {
                    if !entries
                        .iter()
                        .any(|(i, d, n)| *i == index && *d == dir && *n == name)
                    {
                        entries.push((index, dir, name));
                    }
                }
            }
        }
        Self { entries }
    }

    /// The distinct directories to watch.
    pub(super) fn directories(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for (_, dir, _) in &self.entries {
            if !dirs.contains(dir) {
                dirs.push(dir.clone());
            }
        }
        dirs
    }

    /// The resources a reported path refers to.
    pub(super) fn matches(&self, path: &Path) -> Vec<usize> {
        let Some((dir, name)) = split(path) else {
            return Vec::new();
        };
        let mut found: Vec<usize> = self
            .entries
            .iter()
            .filter(|(_, d, n)| *d == dir && *n == name)
            .map(|(index, _, _)| *index)
            .collect();
        found.dedup();
        found
    }
}

/// Canonical parent directory and file name of `path`.
fn split(path: &Path) -> Option<(PathBuf, OsString)> {
    let name = path.file_name()?.to_os_string();
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let dir = std::fs::canonicalize(&parent).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|cwd| cwd.join(&parent))
            .unwrap_or(parent)
    });
    Some((dir, name))
}

/// Whether an event can reflect a content change. Our own reads open and
/// close the file, so pure access notifications must not trigger a render.
pub(super) fn is_relevant(kind: &notify::EventKind) -> bool {
    use notify::event::{AccessKind, AccessMode};
    match kind {
        notify::EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        notify::EventKind::Access(_) => false,
        _ => true,
    }
}

/// The active change-detection backend.
pub(super) enum Backend {
    /// Kept alive for as long as events should flow.
    Events(#[allow(dead_code)] notify::RecommendedWatcher),
    Poll,
}

/// Start the event backend unless polling is forced; on any watcher error,
/// return the polling backend with the reason.
pub(super) fn start_backend(
    targets: &Targets,
    force_poll: bool,
    sender: Sender<Signal>,
) -> (Backend, Option<String>) {
    if force_poll {
        return (Backend::Poll, None);
    }
    let lookup = targets.clone();
    let handler = move |result: notify::Result<notify::Event>| {
        let signals: Vec<Signal> = match result {
            Ok(event) if event.need_rescan() => vec![Signal::Rescan],
            Ok(event) if is_relevant(&event.kind) => {
                let mut indices: Vec<usize> =
                    event.paths.iter().flat_map(|p| lookup.matches(p)).collect();
                indices.sort_unstable();
                indices.dedup();
                indices.into_iter().map(Signal::Changed).collect()
            }
            Ok(_) => Vec::new(),
            Err(_) => vec![Signal::Rescan],
        };
        for signal in signals {
            let _ = sender.send(signal);
        }
    };
    let result = (|| {
        use notify::Watcher;
        let mut watcher = notify::recommended_watcher(handler)?;
        for dir in targets.directories() {
            watcher.watch(&dir, notify::RecursiveMode::NonRecursive)?;
        }
        Ok::<_, notify::Error>(watcher)
    })();
    match result {
        Ok(watcher) => (Backend::Events(watcher), None),
        Err(error) => (Backend::Poll, Some(error.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Presentation
// ---------------------------------------------------------------------------

/// Outcome of rendering one resource.
enum Rendered {
    Ok,
    /// Failed; the message is already visible (stderr or its region).
    Failed(ExitCode, String),
}

trait Presenter {
    fn render(&mut self, cli: &Cli, index: usize) -> Rendered;
    /// Periodic housekeeping; returns resources that must re-render.
    fn tick(&mut self) -> Vec<usize> {
        Vec::new()
    }
    fn tick_interval(&self) -> Option<Duration> {
        None
    }
    /// Stop presenting, leaving the last frame on screen.
    fn finish(&mut self) {}
}

fn resource_cli(cli: &Cli, resource: &str) -> Cli {
    let mut iteration = cli.clone();
    iteration.watch = false;
    iteration.resource = Some(resource.to_string());
    iteration.resources = vec![resource.to_string()];
    iteration
}

/// The original single-resource repaint: clear, home, render to stdout.
struct Viewport;

impl Presenter for Viewport {
    fn render(&mut self, cli: &Cli, index: usize) -> Rendered {
        let resource = &cli.resources[index];
        let console = Console::new();
        console.clear();
        console.control(&rich::control::Control::home());
        let _ = std::io::stdout().flush();
        let status = run_once(resource_cli(cli, resource));
        if status == ExitClass::Success.exit_code() {
            return Rendered::Ok;
        }
        if !cli.watch_exit_on_error {
            // A temporary disappearance or parse failure is a frame error,
            // not a reason to abandon a watch that may recover.
            eprintln!("rich: watch will retry after the next change");
        }
        Rendered::Failed(status, String::new())
    }
}

/// One `LiveCoordinator` region per resource.
struct Regions {
    live: LiveCoordinator<std::io::Stdout>,
    target: RenderTarget,
    ids: Vec<RegionId>,
    content: Vec<Vec<Segment>>,
    observed: Option<(usize, usize)>,
    last_tick: Instant,
}

impl Regions {
    fn new(cli: &Cli) -> Self {
        let mut builder = Console::builder().no_color(cli.no_color);
        if !cli.theme_styles.is_empty() {
            builder = builder.theme(super::cli_theme(cli));
        }
        let console = builder.build();
        let mut caps = render_target::observe(&console, TargetOverrides::default()).capabilities;
        if cli.no_color {
            caps.color_system = None;
        }
        let target = RenderTarget::new(TargetKind::Terminal, caps, console.theme().clone());
        Self {
            live: LiveCoordinator::new(std::io::stdout(), target.clone()),
            target,
            ids: Vec::new(),
            content: Vec::new(),
            observed: terminal_dimensions(),
            last_tick: Instant::now(),
        }
    }

    fn caps(&self) -> TargetCapabilities {
        self.target.capabilities()
    }

    /// Rows one region may use: an equal share of what the Live display
    /// paints (one row is reserved for the cursor), never less than two.
    fn budget(&self, count: usize) -> usize {
        (self.caps().height.saturating_sub(1) / count.max(1)).max(2)
    }

    fn header(&self, resource: &str) -> Vec<Segment> {
        let mut caps = self.caps();
        caps.width = caps.width.saturating_sub(1).max(1);
        let target = RenderTarget::new(TargetKind::Terminal, caps, self.target_theme());
        let title = rich::markup::escape(&sanitize_terminal_controls(resource));
        let rule = Rule::new(title).align(rich::align::HorizontalAlign::Left);
        let mut segments = target.segments(&rule);
        segments.retain(|segment| !segment.control);
        segments
    }

    fn target_theme(&self) -> rich::Theme {
        self.target.console().theme().clone()
    }

    fn compose(&self, resource: &str, frame: Frame, count: usize) -> Vec<Segment> {
        let body: Vec<Segment> = match &frame.error {
            Some(message) => vec![
                Segment::new(
                    format!("rich: {}", sanitize_terminal_controls(message)),
                    Style::parse("bold red").ok(),
                ),
                Segment::line(),
                Segment::new(
                    "watch will retry after the next change",
                    Style::parse("dim").ok(),
                ),
                Segment::line(),
            ],
            None => frame
                .segments
                .into_iter()
                .filter(|segment| !segment.control)
                .map(|mut segment| {
                    segment.text = sanitize_terminal_controls(&segment.text);
                    segment
                })
                .collect(),
        };
        let mut lines = Segment::split_lines(&body);
        // Drop trailing blank lines so regions stay compact.
        while lines
            .last()
            .is_some_and(|line| line.iter().all(|s| s.text.trim().is_empty()))
        {
            lines.pop();
        }
        let room = self.budget(count).saturating_sub(1);
        let shown = if lines.len() > room {
            room.saturating_sub(1)
        } else {
            lines.len()
        };
        // Rows are joined, not terminated: a trailing newline would paint an
        // extra blank row under every region.
        let mut content = self.header(resource);
        for line in lines.iter().take(shown) {
            content.push(Segment::line());
            content.extend(line.iter().cloned());
        }
        if shown < lines.len() {
            content.push(Segment::line());
            content.push(Segment::new(
                format!("… {} more lines", lines.len() - shown),
                Style::parse("dim").ok(),
            ));
        }
        content
    }
}

fn terminal_dimensions() -> Option<(usize, usize)> {
    terminal_size::terminal_size()
        .map(|(w, h)| (w.0 as usize, h.0 as usize))
        .filter(|(w, h)| *w > 0 && *h > 0)
}

impl Presenter for Regions {
    fn render(&mut self, cli: &Cli, index: usize) -> Rendered {
        let resource = &cli.resources[index];
        let width = self.caps().width.saturating_sub(1).max(1);
        let (status, frame) = capture(width, || run_once(resource_cli(cli, resource)));
        let message = frame.error.clone();
        let failed = status != ExitClass::Success.exit_code() || message.is_some();
        let content = self.compose(resource, frame, cli.resources.len());
        if index < self.ids.len() {
            let _ = self.live.update(self.ids[index].clone(), content.clone());
            self.content[index] = content;
            let _ = self.live.refresh();
        } else if let Ok(id) = self.live.add(content.clone()) {
            self.ids.push(id);
            self.content.push(content);
            if self.ids.len() == cli.resources.len() {
                let _ = self.live.refresh();
            }
        }
        if failed {
            let code = if status == ExitClass::Success.exit_code() {
                ExitClass::Input.exit_code()
            } else {
                status
            };
            Rendered::Failed(code, message.unwrap_or_default())
        } else {
            Rendered::Ok
        }
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(RESIZE_CHECK)
    }

    fn tick(&mut self) -> Vec<usize> {
        if self.last_tick.elapsed() < RESIZE_CHECK {
            return Vec::new();
        }
        self.last_tick = Instant::now();
        let now = terminal_dimensions();
        if now.is_none() || now == self.observed {
            return Vec::new();
        }
        self.observed = now;
        let (width, height) = now.expect("checked above");
        let mut caps = self.caps();
        caps.width = width;
        caps.height = height;
        self.target = RenderTarget::new(TargetKind::Terminal, caps, self.target_theme());
        let _ = self.live.resize(width, height);
        (0..self.ids.len()).collect()
    }

    fn finish(&mut self) {
        // Leave the final frame as ordinary output, not a vanishing region.
        for id in self.ids.drain(..) {
            let _ = self.live.remove(id);
        }
        let all: Vec<Segment> = self.content.join(&Segment::line());
        let _ = self.live.print(&all);
        let _ = self.live.finish();
    }
}

// ---------------------------------------------------------------------------
// The watch loop
// ---------------------------------------------------------------------------

/// Watch local files until interrupted (or until a render fails with
/// `--watch-exit-on-error`). The caller has validated every resource.
pub(super) fn watch_files(cli: Cli) -> ExitCode {
    let resources = cli.resources.clone();
    let (sender, receiver) = mpsc::channel();
    let targets = Targets::new(&resources);
    let (backend, fallback) = start_backend(&targets, cli.watch_poll, sender.clone());
    if let Some(reason) = fallback {
        eprintln!(
            "rich: file events unavailable ({reason}); polling every {}s",
            cli.watch_interval
        );
    }
    if resources.len() == 1 {
        return run_loop(&cli, &backend, &receiver, &mut Viewport);
    }
    let interrupt = sender.clone();
    let _ = ctrlc::set_handler(move || {
        let _ = interrupt.send(Signal::Interrupt);
    });
    let mut regions = Regions::new(&cli);
    let status = run_loop(&cli, &backend, &receiver, &mut regions);
    drop(sender);
    status
}

fn run_loop(
    cli: &Cli,
    backend: &Backend,
    receiver: &Receiver<Signal>,
    presenter: &mut dyn Presenter,
) -> ExitCode {
    let count = cli.resources.len();
    let fingerprint =
        |index: usize| watch_fingerprint(&cli.resources[index], false, cli.extensions.encoding);
    let polling = matches!(backend, Backend::Poll);
    let interval = Duration::from_secs_f64(cli.watch_interval);
    let mut debouncer = Debouncer::new(Duration::from_secs_f64(cli.watch_debounce));
    let mut previous: Vec<String> = Vec::with_capacity(count);

    let stop = |presenter: &mut dyn Presenter, status: ExitCode, message: &str| {
        presenter.finish();
        if !message.is_empty() {
            eprintln!("rich: {message}");
        }
        status
    };

    for index in 0..count {
        previous.push(fingerprint(index));
        if let Rendered::Failed(status, message) = presenter.render(cli, index) {
            if cli.watch_exit_on_error {
                return stop(presenter, status, &message);
            }
        }
    }

    let mut next_poll = Instant::now() + interval;
    loop {
        let now = Instant::now();
        let mut deadline = debouncer.next_deadline();
        if polling {
            deadline = Some(deadline.map_or(next_poll, |d| d.min(next_poll)));
        }
        if let Some(tick) = presenter.tick_interval() {
            let at = now + tick;
            deadline = Some(deadline.map_or(at, |d| d.min(at)));
        }
        let first = match deadline {
            Some(deadline) => receiver.recv_timeout(deadline.saturating_duration_since(now)),
            None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let mut signals = Vec::new();
        match first {
            Ok(signal) => signals.push(signal),
            Err(RecvTimeoutError::Timeout) => {}
            // Every sender is gone: nothing can wake this loop again.
            Err(RecvTimeoutError::Disconnected) => return ExitClass::Success.exit_code(),
        }
        signals.extend(receiver.try_iter());
        let now = Instant::now();
        let mut changed: Vec<usize> = Vec::new();
        for signal in signals {
            match signal {
                Signal::Changed(index) if index < count => debouncer.note(index, now),
                Signal::Changed(_) => {}
                Signal::Rescan => (0..count).for_each(|index| debouncer.note(index, now)),
                Signal::Interrupt => return stop(presenter, ExitCode::from(130), ""),
            }
        }
        if polling && now >= next_poll {
            changed.extend(0..count);
            next_poll = now + interval;
        }
        changed.extend(debouncer.take_due(now));
        changed.sort_unstable();
        changed.dedup();
        let mut forced = presenter.tick();
        forced.sort_unstable();
        for (index, last) in previous.iter_mut().enumerate() {
            let force = forced.binary_search(&index).is_ok();
            if !force && changed.binary_search(&index).is_err() {
                continue;
            }
            let current = fingerprint(index);
            if !force && current == *last {
                continue;
            }
            *last = current;
            if let Rendered::Failed(status, message) = presenter.render(cli, index) {
                if cli.watch_exit_on_error {
                    return stop(presenter, status, &message);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, millis: u64) -> Instant {
        base + Duration::from_millis(millis)
    }

    #[test]
    fn a_burst_of_events_becomes_one_render_after_the_quiet_window() {
        let base = Instant::now();
        let mut debouncer = Debouncer::new(Duration::from_millis(100));
        for millis in [0, 20, 40, 60] {
            debouncer.note(0, at(base, millis));
            assert!(debouncer.take_due(at(base, millis)).is_empty());
        }
        assert_eq!(debouncer.next_deadline(), Some(at(base, 160)));
        assert!(debouncer.take_due(at(base, 159)).is_empty());
        assert_eq!(debouncer.take_due(at(base, 160)), vec![0]);
        assert!(debouncer.take_due(at(base, 10_000)).is_empty());
        assert_eq!(debouncer.next_deadline(), None);
    }

    #[test]
    fn resources_debounce_independently() {
        let base = Instant::now();
        let mut debouncer = Debouncer::new(Duration::from_millis(100));
        debouncer.note(0, at(base, 0));
        debouncer.note(1, at(base, 50));
        assert_eq!(debouncer.take_due(at(base, 100)), vec![0]);
        debouncer.note(1, at(base, 120));
        assert!(debouncer.take_due(at(base, 150)).is_empty());
        assert_eq!(debouncer.take_due(at(base, 220)), vec![1]);
    }

    #[test]
    fn continuous_events_still_render_after_the_maximum_wait() {
        let base = Instant::now();
        let mut debouncer = Debouncer::new(Duration::from_millis(100));
        let mut renders = 0;
        for millis in (0..=2_100).step_by(50) {
            debouncer.note(0, at(base, millis));
            renders += debouncer.take_due(at(base, millis)).len();
        }
        // One forced render per 10 windows of uninterrupted writes.
        assert_eq!(renders, 2);
    }

    #[test]
    fn zero_window_renders_immediately() {
        let base = Instant::now();
        let mut debouncer = Debouncer::new(Duration::ZERO);
        debouncer.note(3, base);
        assert_eq!(debouncer.take_due(base), vec![3]);
    }

    #[test]
    fn access_notifications_do_not_count_as_changes() {
        use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind};
        use notify::EventKind;
        assert!(!is_relevant(&EventKind::Access(AccessKind::Open(
            AccessMode::Read
        ))));
        assert!(!is_relevant(&EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));
        assert!(is_relevant(&EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(is_relevant(&EventKind::Modify(ModifyKind::Any)));
        assert!(is_relevant(&EventKind::Create(CreateKind::File)));
        assert!(is_relevant(&EventKind::Remove(RemoveKind::File)));
    }

    #[test]
    fn targets_match_by_parent_directory_and_name() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.md");
        let b = root.path().join("b.json");
        std::fs::write(&a, "a").unwrap();
        let resources = vec![
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned(),
        ];
        let targets = Targets::new(&resources);
        assert_eq!(targets.directories().len(), 1);
        let dir = std::fs::canonicalize(root.path()).unwrap();
        assert_eq!(targets.matches(&dir.join("a.md")), vec![0]);
        // A file that does not exist yet is still matched when it appears.
        assert_eq!(targets.matches(&dir.join("b.json")), vec![1]);
        assert!(targets.matches(&dir.join("save.tmp")).is_empty());
    }

    #[test]
    fn forced_polling_never_starts_the_event_backend() {
        let (sender, _receiver) = mpsc::channel();
        let targets = Targets::new(&["missing-dir/x.md".to_string()]);
        let (backend, reason) = start_backend(&targets, true, sender);
        assert!(matches!(backend, Backend::Poll));
        assert!(reason.is_none());
    }

    #[test]
    fn an_unwatchable_directory_falls_back_to_polling() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("no-such-dir").join("x.md");
        let (sender, _receiver) = mpsc::channel();
        let targets = Targets::new(&[missing.to_string_lossy().into_owned()]);
        let (backend, reason) = start_backend(&targets, false, sender);
        assert!(matches!(backend, Backend::Poll));
        assert!(reason.is_some());
    }

    /// Wait (bounded) for the watcher to report a change to `index`.
    fn expect_change(receiver: &Receiver<Signal>, index: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match receiver.recv_timeout(left) {
                Ok(Signal::Changed(i)) if i == index => return,
                Ok(Signal::Rescan) => return,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        panic!("no change event for resource {index}");
    }

    #[test]
    fn events_report_atomic_rename_and_delete_then_recreate() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.json");
        let b = root.path().join("b.json");
        std::fs::write(&a, "{}").unwrap();
        std::fs::write(&b, "{}").unwrap();
        let resources = vec![
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned(),
        ];
        let (sender, receiver) = mpsc::channel();
        let (backend, reason) = start_backend(&Targets::new(&resources), false, sender);
        if let Some(reason) = reason {
            // Some sandboxes forbid inotify; the polling fallback covers them.
            eprintln!("skipping: file events unavailable: {reason}");
            return;
        }
        assert!(matches!(backend, Backend::Events(_)));

        let temporary = root.path().join("a.json.tmp");
        std::fs::write(&temporary, "[1]").unwrap();
        std::fs::rename(&temporary, &a).unwrap();
        expect_change(&receiver, 0);

        std::fs::remove_file(&b).unwrap();
        expect_change(&receiver, 1);
        std::fs::write(&b, "[2]").unwrap();
        expect_change(&receiver, 1);
    }

    #[test]
    fn captured_frames_hold_segments_and_errors() {
        assert!(!capturing());
        let (status, frame) = capture(40, || {
            assert_eq!(capture_width(), Some(40));
            assert!(capture_segments(vec![Segment::new("x", None)]));
            assert!(capture_error("first"));
            assert!(capture_error("second"));
            ExitCode::SUCCESS
        });
        assert_eq!(status, ExitCode::SUCCESS);
        assert_eq!(frame.segments.len(), 1);
        assert_eq!(frame.error.as_deref(), Some("first"));
        assert!(!capturing());
        assert!(!capture_segments(Vec::new()));
    }
}
