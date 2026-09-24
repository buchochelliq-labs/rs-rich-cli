//! Transient notifications for command-line apps: small toasts that appear
//! beside live output and vanish when they expire, without a full-screen UI.
//!
//! A [`Notification`] has a level (an a11y [`Status`]), an optional title,
//! a message and an optional time to live. It renders as one line (or, with
//! [`ToastStyle::Panel`], a small panel) that starts with the status symbol,
//! so the level reads without colour:
//!
//! ```text
//! ✔ ok Saved: 3 files written
//! ⚠ warning Disk: 92% full
//! ```
//!
//! [`Notifications`] is the stack: [`push`](Notifications::push) posts at a
//! time you give, [`expire`](Notifications::expire) drops the ones past their
//! time, and rendering shows the newest few. To show them under the live
//! regions of a [`LiveCoordinator`], call
//! [`present`](Notifications::present) on each tick: it adds a region below
//! the others while there is something to show and removes it when the last
//! toast expires, so no blank row is left behind. (To place toasts above
//! other content, render the stack into your own first region instead.) On
//! a non-interactive target ([`Notifications::for_target`]) nothing is
//! stacked: each notification is printed once as an ordinary line, so logs
//! keep every one.
//!
//! ```
//! use std::time::Duration;
//! use rich::Console;
//! use rich_ext::notify::{Notification, Notifications};
//!
//! let secs = Duration::from_secs;
//! let mut stack = Notifications::new().default_ttl(Some(secs(5)));
//! stack.push(Notification::ok("3 files written").title("Saved"), secs(0));
//! stack.push(Notification::warning("92% full").title("Disk").ttl(secs(60)), secs(1));
//!
//! let console = Console::builder().width(40).build();
//! assert_eq!(
//!     console.render_to_string(&stack),
//!     "✔ ok Saved: 3 files written\n⚠ warning Disk: 92% full"
//! );
//! stack.expire(secs(6)); // the first has lived its 5 seconds
//! assert_eq!(console.render_to_string(&stack), "⚠ warning Disk: 92% full");
//! ```

use std::io::Write;
use std::time::Duration;

use rich::{Console, ConsoleOptions, Panel, Renderable, Segment, Style, Text};

use crate::a11y::{Status, SymbolSet};
use crate::live::{LiveCoordinator, LiveError, RegionId};
use crate::target::RenderTarget;
use crate::transfer::{effective_symbols, finish_line, join_lines, keyed_style};

/// The default styles for notification keys. [`extended_theme`] includes
/// them; renderers fall back to them when a theme lacks a key.
///
/// [`extended_theme`]: crate::theme::extended_theme
pub const STYLES: &[(&str, &str)] = &[
    ("notify.title", "bold"),
    ("notify.message", "none"),
    ("notify.more", "dim"),
    ("notify.ok", "green"),
    ("notify.warning", "yellow"),
    ("notify.error", "bold red"),
    ("notify.info", "cyan"),
    ("notify.pending", "magenta"),
    ("notify.skipped", "dim"),
];

fn style(console: &Console, key: &str) -> Style {
    keyed_style(console, STYLES, key)
}

fn level_style(console: &Console, status: Status) -> Style {
    style(console, &format!("notify.{}", status.word()))
}

/// How a notification is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastStyle {
    /// One line: `✔ ok Saved: 3 files written`.
    #[default]
    Line,
    /// A small panel titled with the symbol and title, bordered in the
    /// level's colour.
    Panel,
}

/// One message to show for a while. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    status: Status,
    title: Option<String>,
    message: String,
    ttl: Option<Duration>,
    symbols: SymbolSet,
    toast: ToastStyle,
}

impl Notification {
    /// A notification at `status` level.
    pub fn new(status: Status, message: impl Into<String>) -> Self {
        Notification {
            status,
            title: None,
            message: message.into(),
            ttl: None,
            symbols: SymbolSet::Unicode,
            toast: ToastStyle::Line,
        }
    }

    /// A success.
    pub fn ok(message: impl Into<String>) -> Self {
        Notification::new(Status::Ok, message)
    }

    /// Information.
    pub fn info(message: impl Into<String>) -> Self {
        Notification::new(Status::Info, message)
    }

    /// A warning.
    pub fn warning(message: impl Into<String>) -> Self {
        Notification::new(Status::Warning, message)
    }

    /// An error.
    pub fn error(message: impl Into<String>) -> Self {
        Notification::new(Status::Error, message)
    }

    /// A short title shown before the message.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// How long to show it, overriding the stack's default.
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// How the level is marked (default Unicode; ASCII on an ASCII-only
    /// console).
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.symbols = set;
        self
    }

    /// Draw as a line (default) or a panel.
    pub fn toast_style(mut self, toast: ToastStyle) -> Self {
        self.toast = toast;
        self
    }

    /// The level.
    pub fn status(&self) -> Status {
        self.status
    }

    /// The title, if any.
    pub fn get_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Its own time to live, if set.
    pub fn get_ttl(&self) -> Option<Duration> {
        self.ttl
    }

    fn render_as(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        set: SymbolSet,
        toast: ToastStyle,
    ) -> Vec<Segment> {
        let set = effective_symbols(console, set);
        let level = level_style(console, self.status);
        match toast {
            ToastStyle::Line => {
                let mut line = vec![
                    Segment::new(self.status.symbol(set), Some(level)),
                    Segment::new(" ", None),
                ];
                if let Some(title) = &self.title {
                    line.push(Segment::new(
                        title.clone(),
                        Some(style(console, "notify.title")),
                    ));
                    line.push(Segment::new(": ", None));
                }
                line.push(Segment::new(
                    self.message.clone(),
                    Some(style(console, "notify.message")),
                ));
                finish_line(line, options.max_width)
            }
            ToastStyle::Panel => {
                let heading = match &self.title {
                    Some(title) => format!("{} {title}", self.status.symbol(set)),
                    None => self.status.symbol(set).to_string(),
                };
                let body = Text::styled(self.message.clone(), style(console, "notify.message"));
                Panel::fit(Box::new(body))
                    .title(rich::markup::escape(&heading))
                    .title_align(rich::HorizontalAlign::Left)
                    .border_style(level)
                    .rich_render(console, options)
            }
        }
    }
}

impl Renderable for Notification {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.render_as(console, options, self.symbols, self.toast)
    }
}

/// Identifies a posted notification, to [`dismiss`](Notifications::dismiss)
/// it early.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NotificationId(u64);

#[derive(Clone, Debug)]
struct Entry {
    id: NotificationId,
    notification: Notification,
    expires: Option<Duration>,
}

/// A stack of notifications with expiry. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Notifications {
    entries: Vec<Entry>,
    log: Vec<Notification>,
    next: u64,
    default_ttl: Option<Duration>,
    max_visible: usize,
    transient: bool,
    symbols: Option<SymbolSet>,
    toast: Option<ToastStyle>,
}

impl Default for Notifications {
    fn default() -> Self {
        Notifications {
            entries: Vec::new(),
            log: Vec::new(),
            next: 0,
            default_ttl: Some(Duration::from_secs(5)),
            max_visible: 3,
            transient: true,
            symbols: None,
            toast: None,
        }
    }
}

impl Notifications {
    /// An empty, transient stack: five-second default lifetime, three shown.
    pub fn new() -> Self {
        Notifications::default()
    }

    /// A stack for `target`: transient when it is interactive, otherwise
    /// printing each notification once (see [`transient`](Self::transient)).
    pub fn for_target(target: &RenderTarget) -> Self {
        use rich::protocol::RenderEnvironment;
        Notifications::new().transient(target.capabilities().interactive)
    }

    /// How long notifications without their own TTL stay; `None` keeps them
    /// until dismissed.
    pub fn default_ttl(mut self, ttl: Option<Duration>) -> Self {
        self.default_ttl = ttl;
        self
    }

    /// Show at most `count` (the newest), with a `+N more` line for the rest.
    pub fn max_visible(mut self, count: usize) -> Self {
        self.max_visible = count.max(1);
        self
    }

    /// Stack and expire (`true`, the default), or queue every notification
    /// to be printed once and never stack (`false`), for logs and pipes.
    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    /// Mark every notification with `set`, overriding their own.
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.symbols = Some(set);
        self
    }

    /// Draw every notification in `toast` style, overriding their own.
    pub fn toast_style(mut self, toast: ToastStyle) -> Self {
        self.toast = Some(toast);
        self
    }

    /// Post `notification` at `now`. When not transient it is queued for
    /// [`take_log`](Self::take_log) instead of stacked.
    pub fn push(&mut self, notification: Notification, now: Duration) -> NotificationId {
        let id = NotificationId(self.next);
        self.next += 1;
        if !self.transient {
            self.log.push(notification);
            return id;
        }
        let expires = notification
            .ttl
            .or(self.default_ttl)
            .map(|ttl| now.saturating_add(ttl));
        self.entries.push(Entry {
            id,
            notification,
            expires,
        });
        id
    }

    /// Remove a notification now; `false` if it was already gone.
    pub fn dismiss(&mut self, id: NotificationId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        self.entries.len() != before
    }

    /// Drop every notification whose time is up at `now`; returns how many.
    pub fn expire(&mut self, now: Duration) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.expires.is_none_or(|at| at > now));
        before - self.entries.len()
    }

    /// When the next notification expires, to schedule the next redraw.
    pub fn next_expiry(&self) -> Option<Duration> {
        self.entries.iter().filter_map(|entry| entry.expires).min()
    }

    /// How many notifications are stacked.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether none are stacked.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The stacked notifications, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Notification> {
        self.entries.iter().map(|entry| &entry.notification)
    }

    /// Take the notifications queued for printing (non-transient mode).
    pub fn take_log(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.log)
    }

    /// Show the stack through `live` at `now`: print queued notifications
    /// as ordinary lines, drop expired ones, and redraw the toasts in
    /// `region`, then refresh.
    ///
    /// `region` starts as `None`. While toasts are stacked it holds a region
    /// added after (below) the coordinator's other regions; once the stack
    /// empties the region is removed and set back to `None`, because an
    /// empty region would still take a blank row.
    pub fn present<W: Write>(
        &mut self,
        live: &mut LiveCoordinator<W>,
        target: &RenderTarget,
        region: &mut Option<RegionId>,
        now: Duration,
    ) -> Result<(), LiveError> {
        for notification in self.take_log() {
            live.print(&target.segments(&self.styled(&notification)))?;
        }
        self.expire(now);
        match (region.take(), self.is_empty()) {
            (Some(id), true) => live.remove(id)?,
            (None, true) => {}
            (Some(id), false) => {
                live.update(id.clone(), target.segments(self))?;
                *region = Some(id);
            }
            (None, false) => *region = Some(live.add(target.segments(self))?),
        }
        live.refresh()
    }

    fn styled(&self, notification: &Notification) -> Notification {
        let mut out = notification.clone();
        if let Some(set) = self.symbols {
            out.symbols = set;
        }
        if let Some(toast) = self.toast {
            out.toast = toast;
        }
        out
    }
}

impl Renderable for Notifications {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let hidden = self.entries.len().saturating_sub(self.max_visible);
        let mut lines = Vec::new();
        if hidden > 0 {
            lines.push(finish_line(
                vec![Segment::new(
                    format!("+{hidden} more"),
                    Some(style(console, "notify.more")),
                )],
                options.max_width,
            ));
        }
        for entry in &self.entries[hidden..] {
            lines.push(
                self.styled(&entry.notification)
                    .rich_render(console, options),
            );
        }
        join_lines(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismiss_and_next_expiry() {
        let secs = Duration::from_secs;
        let mut stack = Notifications::new().default_ttl(None);
        let a = stack.push(Notification::info("a").ttl(secs(3)), secs(1));
        stack.push(Notification::info("b"), secs(2));
        assert_eq!(stack.next_expiry(), Some(secs(4)));
        assert!(stack.dismiss(a));
        assert!(!stack.dismiss(a));
        assert_eq!(stack.next_expiry(), None);
        assert_eq!(stack.expire(secs(1000)), 0, "no TTL, kept");
    }
}
