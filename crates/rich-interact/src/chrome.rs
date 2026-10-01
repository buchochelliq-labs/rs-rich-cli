//! Chrome: a status bar (#481) and breadcrumbs (#482), the lines round a
//! view that say where you are and what you can do (0.0.14 workstream 2).
//!
//! Both are components that take no focus, so they sit in any container
//! beside the components they describe, and both are updated from outside
//! through a shared handle while the view runs:
//!
//! - a [`StatusBar`] is one line of [`StatusItem`]s, on the left or the
//!   right: console markup, key hints read from a [`Keymap`], spinners
//!   that are indexed by time as core's are, and badges;
//! - [`Breadcrumbs`] show a path (`project › src › main.rs`), eliding it
//!   from the left when it does not fit, and a click on a crumb goes back
//!   to it.
//!
//! [`Overlays`](crate::overlay::Overlays) puts them above and below any
//! component and keeps the status bar's hints in step with the component's
//! keymap.
//!
//! ```
//! use rich_interact::chrome::{StatusBar, StatusItem};
//! use rich_interact::keymap::{keys, Keymap};
//! use rich_interact::{headless, Flow, Outcome};
//!
//! let bar: StatusBar = StatusBar::new()
//!     .left("mode", StatusItem::badge("NORMAL", "bold reverse"))
//!     .left("file", StatusItem::text("[bold]main.rs[/]"))
//!     .right("keys", StatusItem::hints(2))
//!     .keymap(Keymap::new("demo").bind("save", keys("ctrl+s"), "save"));
//! let status = bar.handle();
//! status.set("file", StatusItem::text("lib.rs"));
//! assert!(bar.line_text(40).starts_with(" NORMAL  │ lib.rs"));
//! assert!(bar.line_text(40).ends_with("ctrl+s save"));
//! ```

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rich::cells::cell_len;
use rich::{Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::event::{Event, Key, KeyCode};
use crate::keymap::Keymap;
use crate::kit::{self, Theme};

/// Rendered markup, on one line and unwrapped, at no particular width.
fn inline(context: &Context<'_>, markup: &str) -> Vec<Segment> {
    let text = rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup));
    let options = context.console.options().update_width(4096);
    context
        .console
        .render_lines(&text, &options, false)
        .into_iter()
        .next()
        .unwrap_or_default()
}

fn style(definition: &str) -> Style {
    Style::parse(definition).unwrap_or_default()
}

/// A key as a hint shows it, shorter than its name: arrows as arrows,
/// `esc`, `pgup`, `pgdn`, `shift+tab`; anything else by its name.
pub fn key_hint(key: Key) -> String {
    let short = match key.code {
        KeyCode::Up => "↑",
        KeyCode::Down => "↓",
        KeyCode::Left => "←",
        KeyCode::Right => "→",
        KeyCode::Escape => "esc",
        KeyCode::PageUp => "pgup",
        KeyCode::PageDown => "pgdn",
        KeyCode::BackTab => "shift+tab",
        _ => return key.to_string(),
    };
    let mut hint = String::new();
    if key.modifiers.ctrl {
        hint.push_str("ctrl+");
    }
    if key.modifiers.alt {
        hint.push_str("alt+");
    }
    if key.modifiers.shift && key.code != KeyCode::BackTab {
        hint.push_str("shift+");
    }
    hint.push_str(short);
    hint
}

/// Keys as hints, joined by `/`: `↑/ctrl+p`.
pub fn keys_hint(keys: &[Key]) -> String {
    keys.iter()
        .map(|key| key_hint(*key))
        .collect::<Vec<_>>()
        .join("/")
}

// ---------------------------------------------------------------------------
// StatusBar.

/// One part of a [`StatusBar`].
#[derive(Clone, Debug, PartialEq)]
pub enum StatusItem {
    /// Console markup: `[bold]main.rs[/] · 12 lines`.
    Text(String),
    /// Up to this many key hints (`ctrl+s save · esc cancel`), read from
    /// the keymap the bar is given, or the component's under
    /// [`Overlays`](crate::overlay::Overlays).
    Hints(usize),
    /// A spinner (one of core's, by name: `dots`, `line`, ...) and markup
    /// after it. Its frame is picked by the time since the bar was made, as
    /// core's `Spinner` picks it, and the bar ticks while one shows.
    Spinner { name: String, text: String },
    /// A short label in a style of its own, padded by a space each side:
    /// a mode, a count, a state.
    Badge { text: String, style: Style },
    /// An icon, then markup: a micro asset's badge (`✅ deployed`). The
    /// icon is one line of cells ([`kit::icon`]) with its style metadata,
    /// so a micro asset's placeholder is drawn over by the painter's
    /// graphics where the terminal can show images, and shows its emoji or
    /// text fallback anywhere else.
    Icon { icon: Vec<Segment>, text: String },
}

impl StatusItem {
    pub fn text(markup: impl Into<String>) -> StatusItem {
        StatusItem::Text(markup.into())
    }

    pub fn hints(max: usize) -> StatusItem {
        StatusItem::Hints(max)
    }

    pub fn spinner(name: impl Into<String>, markup: impl Into<String>) -> StatusItem {
        StatusItem::Spinner {
            name: name.into(),
            text: markup.into(),
        }
    }

    /// `icon` (one line of text, such as a micro asset's placeholder),
    /// then `markup`.
    pub fn icon(icon: &rich::Text, markup: impl Into<String>) -> StatusItem {
        StatusItem::Icon {
            icon: kit::icon(icon),
            text: markup.into(),
        }
    }

    /// A micro asset (its placeholder, drawn by the painter's graphics
    /// where the terminal can, its emoji or text elsewhere), then `markup`.
    /// Needs the `micro` feature.
    #[cfg(feature = "micro")]
    pub fn micro(asset: &rich_micro::MicroAsset, markup: impl Into<String>) -> StatusItem {
        StatusItem::icon(
            &rich_micro::placeholder(asset, rich_micro::FallbackPreference::Emoji),
            markup,
        )
    }

    /// A badge in `style`, a style definition (`bold white on blue`); one
    /// that does not parse leaves the badge unstyled.
    pub fn badge(text: impl Into<String>, style_definition: &str) -> StatusItem {
        StatusItem::Badge {
            text: text.into(),
            style: style(style_definition),
        }
    }
}

#[derive(Clone, Debug)]
struct Entry {
    id: String,
    item: StatusItem,
    right: bool,
}

#[derive(Default)]
struct Shared {
    entries: Vec<Entry>,
    keymap: Option<Keymap>,
}

/// Changes a [`StatusBar`]'s items from anywhere while it shows: a
/// component's handler, a timer, another thread's result as it arrives.
#[derive(Clone, Default)]
pub struct StatusHandle {
    shared: Rc<RefCell<Shared>>,
}

impl StatusHandle {
    /// Set item `id` to `item`, where it is; a new id goes last on the
    /// left.
    pub fn set(&self, id: &str, item: StatusItem) {
        self.put(id, item, None);
    }

    /// Set item `id` on the left or (`right`) the right, moving it there.
    pub fn put(&self, id: &str, item: StatusItem, right: Option<bool>) {
        let mut shared = self.shared.borrow_mut();
        match shared.entries.iter().position(|entry| entry.id == id) {
            Some(index) if right.is_none_or(|right| right == shared.entries[index].right) => {
                shared.entries[index].item = item;
            }
            found => {
                if let Some(index) = found {
                    shared.entries.remove(index);
                }
                shared.entries.push(Entry {
                    id: id.to_string(),
                    item,
                    right: right.unwrap_or(false),
                });
            }
        }
    }

    /// Remove item `id`; `false` when there was none.
    pub fn remove(&self, id: &str) -> bool {
        let mut shared = self.shared.borrow_mut();
        let before = shared.entries.len();
        shared.entries.retain(|entry| entry.id != id);
        shared.entries.len() != before
    }

    /// Item `id`, if the bar has it.
    pub fn get(&self, id: &str) -> Option<StatusItem> {
        let shared = self.shared.borrow();
        shared
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.item.clone())
    }

    /// The keymap [`StatusItem::Hints`] read.
    pub fn set_keymap(&self, keymap: Keymap) {
        self.shared.borrow_mut().keymap = Some(keymap);
    }
}

/// A status line (#481): [`StatusItem`]s on the left and on the right,
/// separated by `│`. It takes no focus and uses no events but ticks, so it
/// sits under anything in a container, or under any component with
/// [`Overlays::status_bar`](crate::overlay::Overlays::status_bar), which
/// also feeds its hints the component's keymap. Change it while it shows
/// through its [`handle`](Self::handle).
pub struct StatusBar<M = ()> {
    handle: StatusHandle,
    style: Style,
    separator: String,
    theme: Theme,
    clock: Rc<dyn Fn() -> Duration>,
    /// Whether spinners turn; off, each shows its first frame.
    animate: bool,
    _output: PhantomData<fn() -> M>,
}

impl<M> Default for StatusBar<M> {
    fn default() -> Self {
        StatusBar::new()
    }
}

impl<M> StatusBar<M> {
    pub fn new() -> StatusBar<M> {
        let started = Instant::now();
        StatusBar {
            handle: StatusHandle::default(),
            style: Style::default(),
            separator: "│".to_string(),
            theme: Theme::default(),
            clock: Rc::new(move || started.elapsed()),
            animate: animation_allowed(&rich_ext::capabilities::SystemEnvironment),
            _output: PhantomData,
        }
    }

    /// Add `item` as `id` on the left.
    pub fn left(self, id: &str, item: StatusItem) -> Self {
        self.handle.put(id, item, Some(false));
        self
    }

    /// Add `item` as `id` on the right.
    pub fn right(self, id: &str, item: StatusItem) -> Self {
        self.handle.put(id, item, Some(true));
        self
    }

    /// The keymap hints read (see [`StatusHandle::set_keymap`]).
    pub fn keymap(self, keymap: Keymap) -> Self {
        self.handle.set_keymap(keymap);
        self
    }

    /// The style of the whole line, padded to the width (default: none;
    /// `on grey23` for a bar with a background).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// What goes between items (default `│`).
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Where spinners read the time from (default: the time since the bar
    /// was made): a fixed clock makes a test's frames exact.
    pub fn clock(mut self, clock: impl Fn() -> Duration + 'static) -> Self {
        self.clock = Rc::new(clock);
        self
    }

    /// Whether spinners turn (default: unless `RICH_ANIMATION=0`, or
    /// `RICH_A11Y` asks for reduced motion, no animation or a screen
    /// reader, as for every other animation). Off, a spinner shows its
    /// first frame and the bar never ticks.
    pub fn animate(mut self, on: bool) -> Self {
        self.animate = on;
        self
    }

    /// A handle that changes this bar's items while it shows.
    pub fn handle(&self) -> StatusHandle {
        self.handle.clone()
    }

    /// The line at `context`'s width, with hints read from `keymap` when
    /// given, else from the bar's own.
    pub fn line(&self, context: &Context<'_>, keymap: Option<&Keymap>) -> Vec<Segment> {
        let shared = self.handle.shared.borrow();
        let keymap = keymap.or(shared.keymap.as_ref());
        let now = if self.animate {
            (self.clock)()
        } else {
            Duration::ZERO
        };
        let part = |entry: &Entry| -> Vec<Segment> {
            match &entry.item {
                StatusItem::Text(markup) => inline(context, markup),
                StatusItem::Hints(max) => self.hints(keymap, *max),
                StatusItem::Spinner { name, text } => {
                    let mut line = vec![kit::text(spinner_frame(name, now), &style("green"))];
                    if !text.is_empty() {
                        line.push(kit::plain(" "));
                        line.extend(inline(context, text));
                    }
                    line
                }
                StatusItem::Badge { text, style } => vec![kit::text(format!(" {text} "), style)],
                StatusItem::Icon { icon, text } => {
                    let mut line = icon.clone();
                    if !text.is_empty() {
                        line.push(kit::plain(" "));
                        line.extend(inline(context, text));
                    }
                    line
                }
            }
        };
        let join = |right: bool| -> Vec<Segment> {
            let mut line = Vec::new();
            for entry in shared.entries.iter().filter(|entry| entry.right == right) {
                let part = part(entry);
                if part.is_empty() {
                    continue;
                }
                if !line.is_empty() {
                    line.push(kit::text(format!(" {} ", self.separator), &self.theme.hint));
                }
                line.extend(part);
            }
            line
        };
        let (left, right) = (join(false), join(true));
        let width = context.width;
        let right_width = kit::width(&right);
        let mut line = if right_width + 1 < width {
            kit::pad(kit::fit(left, width - right_width - 1), width - right_width)
        } else {
            kit::fit(left, width)
        };
        if right_width + 1 < width {
            line.extend(right);
        }
        let line = kit::pad(line, width);
        if self.style == Style::default() {
            line
        } else {
            kit::restyle(&line, &self.style)
        }
    }

    /// The line at `width`, as plain text, for tests and logs.
    pub fn line_text(&self, width: usize) -> String {
        let console = rich::Console::new();
        let context = Context {
            console: &console,
            width,
            height: 1,
        };
        self.line(&context, None)
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn hints(&self, keymap: Option<&Keymap>, max: usize) -> Vec<Segment> {
        let Some(keymap) = keymap else {
            return Vec::new();
        };
        let mut line = Vec::new();
        for binding in keymap
            .bindings()
            .into_iter()
            .filter(|binding| !binding.keys.is_empty())
            .take(max)
        {
            if !line.is_empty() {
                line.push(kit::text(" · ", &self.theme.hint));
            }
            line.push(kit::text(key_hint(binding.keys[0]), &style("bold")));
            line.push(kit::text(
                format!(" {}", binding.description),
                &self.theme.hint,
            ));
        }
        line
    }

    /// The fastest spinner's interval, while one shows and turns.
    fn spinner_interval(&self) -> Option<Duration> {
        if !self.animate {
            return None;
        }
        let shared = self.handle.shared.borrow();
        shared
            .entries
            .iter()
            .filter_map(|entry| match &entry.item {
                StatusItem::Spinner { name, .. } => Some(spinner(name).0),
                _ => None,
            })
            .min()
    }
}

/// Whether animation may run in `env`: `RICH_ANIMATION` when set, else not
/// when `RICH_A11Y` lists `reduced-motion`, `no-animation` or
/// `screen-reader` (the same reading as the capability report's).
fn animation_allowed(env: &dyn rich_ext::capabilities::Environment) -> bool {
    if let Some(on) = env
        .var("RICH_ANIMATION")
        .and_then(|value| rich_ext::capabilities::parse_bool(&value))
    {
        return on;
    }
    let policy = rich_ext::a11y::AccessibilityPolicy::from_env(env);
    !(policy.no_animation || policy.reduced_motion)
}

/// A spinner's interval and frames, by name; `dots` for one unknown.
fn spinner(name: &str) -> (Duration, &'static [&'static str]) {
    let (interval, frames) = rich::spinner::spinner_frames(name)
        .or_else(|| rich::spinner::spinner_frames("dots"))
        .expect("the dots spinner exists");
    (Duration::from_secs_f64(interval / 1000.0), frames)
}

/// The frame of spinner `name` at `time`: the one core's `Spinner`
/// renders at that time since it started.
pub fn spinner_frame(name: &str, time: Duration) -> &'static str {
    let (interval, frames) = spinner(name);
    let index = time.as_nanos() / interval.as_nanos().max(1);
    frames[(index % frames.len() as u128) as usize]
}

impl<M> Component for StatusBar<M> {
    type Output = M;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<M> {
        match event {
            // Repainted with the spinner's next frame.
            Event::Tick => Flow::Continue,
            _ => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        View::new(vec![self.line(context, None)])
    }

    fn tick(&self) -> Option<Duration> {
        self.spinner_interval()
    }

    fn focusable(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Breadcrumbs.

/// Changes a [`Breadcrumbs`]' path from anywhere while it shows.
#[derive(Clone, Default, Debug)]
pub struct Crumbs {
    path: Rc<RefCell<Vec<String>>>,
}

impl Crumbs {
    /// Go into `crumb`.
    pub fn push(&self, crumb: impl Into<String>) {
        self.path.borrow_mut().push(crumb.into());
    }

    /// Go back out of the last crumb.
    pub fn pop(&self) -> Option<String> {
        self.path.borrow_mut().pop()
    }

    /// Keep the first `len` crumbs.
    pub fn truncate(&self, len: usize) {
        self.path.borrow_mut().truncate(len);
    }

    /// Replace the whole path.
    pub fn set<I, S>(&self, path: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        *self.path.borrow_mut() = path.into_iter().map(Into::into).collect();
    }

    pub fn get(&self) -> Vec<String> {
        self.path.borrow().clone()
    }
}

type PickHandler<'a, M> = Box<dyn FnMut(usize, &Crumbs) -> Flow<M> + 'a>;
type IconFor<'a> = Box<dyn Fn(usize, &str) -> Option<rich::Text> + 'a>;

/// Where you are, as a path of crumbs (#482): `project › src › main.rs`,
/// the last in bold. A path wider than the line loses crumbs from the
/// left, behind `…`. With the mouse on, a click on a crumb goes back to it
/// (the path is cut after it), or does what [`on_pick`](Self::on_pick)
/// says. It takes no focus and no keys.
pub struct Breadcrumbs<'a, M = ()> {
    crumbs: Crumbs,
    icons: Option<IconFor<'a>>,
    separator: String,
    theme: Theme,
    current: Style,
    mouse: bool,
    on_pick: Option<PickHandler<'a, M>>,
}

impl<'a, M> Breadcrumbs<'a, M> {
    pub fn new<I, S>(path: I) -> Breadcrumbs<'a, M>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let crumbs = Crumbs::default();
        crumbs.set(path);
        Breadcrumbs {
            crumbs,
            icons: None,
            separator: "›".to_string(),
            theme: Theme::default(),
            current: style("bold"),
            mouse: false,
            on_pick: None,
        }
    }

    /// An icon before a crumb: `icon(index, crumb)` returns one line of
    /// text (a micro asset's placeholder, say) or `None`. It counts in the
    /// crumb's width and clicks on it pick the crumb.
    pub fn icons(mut self, icon: impl Fn(usize, &str) -> Option<rich::Text> + 'a) -> Self {
        self.icons = Some(Box::new(icon));
        self
    }

    /// What goes between crumbs (default `›`).
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Clicks go back to a crumb (default: off).
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    /// What a click on crumb `index` does, instead of cutting the path
    /// after it: given the index and the path's handle.
    pub fn on_pick(mut self, handler: impl FnMut(usize, &Crumbs) -> Flow<M> + 'a) -> Self {
        self.on_pick = Some(Box::new(handler));
        self.mouse = true;
        self
    }

    /// A handle that changes the path while it shows.
    pub fn crumbs(&self) -> Crumbs {
        self.crumbs.clone()
    }

    /// The line at `width`, and each crumb shown: its index and the cells
    /// it takes.
    pub fn layout(&self, width: usize) -> (Vec<Segment>, Vec<(usize, std::ops::Range<usize>)>) {
        // As painted: a control in a crumb shows as a one-cell picture, so
        // the spans clicks are matched against count it.
        let path: Vec<String> = self
            .crumbs
            .get()
            .iter()
            .map(|crumb| kit::shown(crumb))
            .collect();
        let separator = format!(" {} ", self.separator);
        let gap = cell_len(&separator);
        let icons: Vec<Option<Vec<Segment>>> = path
            .iter()
            .enumerate()
            .map(|(index, crumb)| {
                self.icons
                    .as_ref()
                    .and_then(|icon| icon(index, crumb))
                    .map(|text| kit::icon(&text))
                    .filter(|icon| !icon.is_empty())
            })
            .collect();
        let widths: Vec<usize> = path
            .iter()
            .zip(&icons)
            .map(|(crumb, icon)| cell_len(crumb) + icon.as_ref().map_or(0, |i| kit::width(i) + 1))
            .collect();
        // Drop crumbs from the left until the rest fits after `… › `.
        let mut first = 0;
        let total = |from: usize| -> usize {
            let shown: usize = widths[from..].iter().sum::<usize>() + gap * (path.len() - from - 1);
            if from > 0 {
                shown + 1 + gap
            } else {
                shown
            }
        };
        while first + 1 < path.len() && total(first) > width {
            first += 1;
        }
        let mut line = Vec::new();
        let mut spans = Vec::new();
        let mut column = 0;
        if first > 0 {
            line.push(kit::text("…", &self.theme.hint));
            line.push(kit::text(separator.clone(), &self.theme.hint));
            column += 1 + gap;
        }
        for (index, crumb) in path.iter().enumerate().skip(first) {
            if index > first {
                line.push(kit::text(separator.clone(), &self.theme.hint));
                column += gap;
            }
            let last = index + 1 == path.len();
            if let Some(icon) = &icons[index] {
                line.extend(icon.iter().cloned());
                line.push(kit::plain(" "));
            }
            line.push(if last {
                kit::text(crumb.clone(), &self.current)
            } else {
                kit::plain(crumb.clone())
            });
            spans.push((index, column..column + widths[index]));
            column += widths[index];
        }
        (kit::fit(line, width), spans)
    }
}

impl<M> Component for Breadcrumbs<'_, M> {
    type Output = M;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        let Some(mouse) = event
            .mouse()
            .filter(|mouse| mouse.is_click() && mouse.row == 0)
        else {
            return Flow::Ignored;
        };
        let (_, spans) = self.layout(context.width);
        let column = mouse.column as usize;
        let Some((index, _)) = spans.into_iter().find(|(_, span)| span.contains(&column)) else {
            return Flow::Ignored;
        };
        match &mut self.on_pick {
            Some(handler) => handler(index, &self.crumbs),
            None => {
                self.crumbs.truncate(index + 1);
                Flow::Continue
            }
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        View::new(vec![self.layout(context.width).0])
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    fn focusable(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &[Segment]) -> String {
        line.iter().map(|segment| segment.text.as_str()).collect()
    }

    #[test]
    fn hints_shorten_keys() {
        let keys = crate::keymap::keys("up ctrl+p escape pagedown shift+tab alt+left f1");
        assert_eq!(keys_hint(&keys), "↑/ctrl+p/esc/pgdn/shift+tab/alt+←/f1");
    }

    #[test]
    fn spinners_are_indexed_by_time() {
        // `line` is `-\|/` at 130ms a frame.
        assert_eq!(spinner_frame("line", Duration::ZERO), "-");
        assert_eq!(spinner_frame("line", Duration::from_millis(130)), "\\");
        assert_eq!(spinner_frame("line", Duration::from_millis(4 * 130)), "-");
        assert_eq!(spinner_frame("no-such", Duration::ZERO), "⠋");
        let bar: StatusBar = StatusBar::new()
            .left("work", StatusItem::spinner("line", "building"))
            .clock(|| Duration::from_millis(270))
            .animate(true);
        assert_eq!(bar.line_text(30), "| building");
        assert_eq!(Component::tick(&bar), Some(Duration::from_millis(130)));
    }

    /// Reduced motion stills a spinner on its first frame, and the bar
    /// stops ticking, like every other animation (0.0.14 release-test
    /// audit B).
    #[test]
    fn spinners_hold_still_under_reduced_motion() {
        use rich_ext::capabilities::MapEnvironment;
        let env = |pairs: &[(&str, &str)]| {
            let mut env = MapEnvironment::tty();
            for (name, value) in pairs {
                env.vars.insert(name.to_string(), value.to_string());
            }
            env
        };
        assert!(animation_allowed(&env(&[])));
        assert!(!animation_allowed(&env(&[("RICH_A11Y", "reduced-motion")])));
        assert!(!animation_allowed(&env(&[("RICH_A11Y", "screen-reader")])));
        assert!(!animation_allowed(&env(&[("RICH_ANIMATION", "0")])));
        assert!(animation_allowed(&env(&[
            ("RICH_A11Y", "reduced-motion"),
            ("RICH_ANIMATION", "1")
        ])));
        let bar: StatusBar = StatusBar::new()
            .left("work", StatusItem::spinner("line", "building"))
            .clock(|| Duration::from_millis(270))
            .animate(false);
        assert_eq!(bar.line_text(30), "- building");
        assert_eq!(Component::tick(&bar), None);
    }

    #[test]
    fn items_update_through_the_handle_and_the_right_side_fits() {
        let bar: StatusBar = StatusBar::new()
            .left("a", StatusItem::text("one"))
            .right("b", StatusItem::badge("OK", "green"));
        let handle = bar.handle();
        handle.set("a", StatusItem::text("[bold]two[/]"));
        handle.set("c", StatusItem::text("three"));
        assert_eq!(bar.line_text(24), "two │ three          OK");
        assert!(handle.remove("c"));
        assert_eq!(bar.line_text(12), "two      OK");
        // Too narrow for both: the left side only, cut.
        assert_eq!(bar.line_text(3), "two");
    }

    #[test]
    fn breadcrumbs_elide_from_the_left() {
        let crumbs: Breadcrumbs = Breadcrumbs::new(["home", "project", "src", "main.rs"]);
        assert_eq!(text(&crumbs.layout(40).0), "home › project › src › main.rs");
        assert_eq!(text(&crumbs.layout(18).0), "… › src › main.rs");
        let (_, spans) = crumbs.layout(40);
        assert_eq!(spans[1], (1, 7..14));
        let crumbs: Breadcrumbs = Breadcrumbs::new(["home", "src"])
            .icons(|index, _| (index == 1).then(|| rich::Text::new("[]")));
        let (line, spans) = crumbs.layout(40);
        assert_eq!(text(&line), "home › [] src");
        assert_eq!(spans[1], (1, 7..13));
    }

    /// A crumb holding controls is measured as painted (each control a
    /// one-cell picture), so a click lands on the crumb under it (0.0.14
    /// release-test audit B).
    #[test]
    fn breadcrumbs_measure_controls_as_painted() {
        let crumbs: Breadcrumbs = Breadcrumbs::new(["a\u{1b}\u{1b}\u{1b}", "bb"]);
        let (line, spans) = crumbs.layout(40);
        assert_eq!(text(&line), "a␛␛␛ › bb");
        assert_eq!(spans, [(0, 0..4), (1, 7..9)]);
    }

    #[test]
    fn icons_keep_their_metadata() {
        let mut meta = rich::style::Meta::new();
        meta.insert("rich.micro", rich::style::MetaValue::Str("x".into()));
        let mut icon = rich::Text::new("");
        icon.append("ok", Some(Style::from_meta(meta).into()));
        let bar: StatusBar = StatusBar::new().left("a", StatusItem::icon(&icon, "deployed"));
        assert_eq!(bar.line_text(20), "ok deployed");
        let StatusItem::Icon { icon, .. } = StatusItem::icon(&icon, "") else {
            unreachable!()
        };
        assert!(icon[0].style.as_ref().unwrap().meta_ref().is_some());
    }
}
