//! A drop-down select field ([`select_field`]): the current choice, and a
//! menu of the others that opens under it, on [`open_menu`]'s list.
//!
//! [`open_menu`]: crate::menu::open_menu

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::{AccessState, Role};
use crate::controls::{look, plain};
use crate::menu::MenuItem;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::screen::Rect;
use crate::widget::{
    markup_width, widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent,
};

/// How long a pause ends what was typed: the next letter starts afresh.
const TYPEAHEAD_PAUSE: Duration = Duration::from_secs(1);

/// Letters typed to jump to an option: one letter goes to the next option
/// it starts (again and again, round), several typed together to the
/// first option they start.
#[derive(Default)]
pub(crate) struct Typeahead {
    typed: String,
    /// When the last letter came, on the app's clock.
    at: Option<Duration>,
}

impl Typeahead {
    /// The option `c` jumps to among `labels` (plain text), from option
    /// `current`; `None` when no option starts so.
    pub(crate) fn key(&mut self, c: char, labels: &[String], current: usize) -> Option<usize> {
        let now = crate::reactive::Runtime::current().now.get();
        if self
            .at
            .is_none_or(|at| now.saturating_sub(at) > TYPEAHEAD_PAUSE)
        {
            self.typed.clear();
        }
        self.at = Some(now);
        self.typed.extend(c.to_lowercase());
        let count = labels.len();
        let find = |prefix: &str, from: usize| {
            (0..count)
                .map(|k| (from + k) % count)
                .find(|&i| labels[i].to_lowercase().starts_with(prefix))
        };
        // Several letters: the option they start, this one if it does.
        if self.typed.chars().count() > 1 {
            if let Some(found) = find(&self.typed, current) {
                return Some(found);
            }
            // The same letter again (`b`, `b`): round the options it starts.
            self.typed = c.to_lowercase().collect();
        }
        find(&self.typed, current + 1)
    }
}

/// One option of a [`select_field_keyed`] or a
/// [`radio_keyed`](crate::radio_keyed): what identifies it (`key`), what
/// it says (`label`, console markup), and whether it may be chosen. A
/// disabled option is shown, greyed, with its `reason`, and keys, clicks
/// and typing pass it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption<K> {
    pub key: K,
    pub label: String,
    pub enabled: bool,
    /// Why it is disabled, shown beside it.
    pub reason: Option<String>,
}

impl<K> SelectOption<K> {
    /// An option that may be chosen.
    pub fn new(key: K, label: impl Into<String>) -> SelectOption<K> {
        SelectOption {
            key,
            label: label.into(),
            enabled: true,
            reason: None,
        }
    }

    /// Disabled, saying why (none when empty).
    #[must_use]
    pub fn disabled(mut self, reason: impl Into<String>) -> SelectOption<K> {
        let reason = reason.into();
        self.enabled = false;
        self.reason = (!reason.is_empty()).then_some(reason);
        self
    }
}

/// An option as a select field or a radio group shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Choice {
    pub label: String,
    pub enabled: bool,
    pub reason: Option<String>,
}

impl Choice {
    /// What it says, with why it is disabled.
    pub(crate) fn markup(&self) -> String {
        match &self.reason {
            Some(reason) if !self.enabled => format!("{} ({reason})", self.label),
            _ => self.label.clone(),
        }
    }
}

/// The options of a select field or a radio group, and which is chosen:
/// by index (`select_field`, `radio`) or by key (the keyed forms).
#[derive(Clone)]
pub(crate) struct Choices {
    options: Rc<dyn Fn() -> Vec<Choice>>,
    /// The chosen option's index, read tracked (`true`) or not; `None`
    /// when nothing is chosen.
    current: Rc<dyn Fn(bool) -> Option<usize>>,
    /// Choose option `index`.
    choose: Rc<dyn Fn(usize)>,
}

impl Choices {
    /// Options by index, all enabled; the chosen one `selected` holds.
    pub(crate) fn indexed(
        options: impl Fn() -> Vec<String> + 'static,
        selected: Signal<usize>,
    ) -> Choices {
        Choices {
            options: Rc::new(move || {
                options()
                    .into_iter()
                    .map(|label| Choice {
                        label,
                        enabled: true,
                        reason: None,
                    })
                    .collect()
            }),
            current: Rc::new(move |tracked| {
                Some(match tracked {
                    true => selected.get(),
                    false => selected.get_untracked(),
                })
            }),
            choose: Rc::new(move |index| selected.set(index)),
        }
    }

    /// Options by key; the chosen one's key `selected` holds. When it
    /// holds a key no option has, nothing is chosen, and (in a running
    /// app) it is set to `None`.
    pub(crate) fn keyed<K: Clone + PartialEq + 'static>(
        options: impl Fn() -> Vec<SelectOption<K>> + 'static,
        selected: Signal<Option<K>>,
    ) -> Choices {
        let options = Rc::new(options);
        let read = options.clone();
        let pick = options.clone();
        if crate::reactive::Runtime::try_current().is_some() {
            // A removed key goes: the field says nothing is chosen.
            let all = options.clone();
            crate::reactive::watch(
                move || {
                    selected.with(|key| {
                        key.as_ref()
                            .is_some_and(|key| !all().iter().any(|o| o.key == *key))
                    })
                },
                move |gone, _| {
                    if gone {
                        selected.set(None);
                    }
                },
            );
        }
        Choices {
            options: Rc::new(move || {
                read()
                    .into_iter()
                    .map(|option| Choice {
                        label: option.label,
                        enabled: option.enabled,
                        reason: option.reason,
                    })
                    .collect()
            }),
            current: Rc::new(move |tracked| {
                let find = |key: &Option<K>| {
                    let key = key.as_ref()?;
                    options().iter().position(|o| o.key == *key)
                };
                match tracked {
                    true => selected.with(find),
                    false => selected.with_untracked(find),
                }
            }),
            choose: Rc::new(move |index| {
                let chosen = pick().into_iter().nth(index).filter(|o| o.enabled);
                if let Some(option) = chosen {
                    selected.set(Some(option.key));
                }
            }),
        }
    }

    pub(crate) fn options(&self) -> Vec<Choice> {
        (self.options)()
    }

    pub(crate) fn current(&self, tracked: bool) -> Option<usize> {
        (self.current)(tracked)
    }

    /// Choose option `index`, if it is there and enabled.
    pub(crate) fn choose(&self, index: usize) {
        let enabled = self.options().get(index).is_some_and(|o| o.enabled);
        if enabled {
            (self.choose)(index);
        }
    }

    /// The enabled option `step` places from `from` (wrapping round when
    /// `wrap`), or `None` when there is none that way.
    pub(crate) fn step(
        options: &[Choice],
        from: Option<usize>,
        step: isize,
        wrap: bool,
    ) -> Option<usize> {
        let n = options.len() as isize;
        if n == 0 {
            return None;
        }
        let start = match from {
            Some(at) => at.min(n as usize - 1) as isize,
            // Nothing chosen: down from before the first, up from after
            // the last.
            None if step > 0 => -1,
            None => n,
        };
        let mut at = start;
        for _ in 0..n {
            at += step;
            if wrap {
                at = at.rem_euclid(n);
            } else if !(0..n).contains(&at) {
                return None;
            }
            if options[at as usize].enabled {
                return Some(at as usize);
            }
        }
        None
    }

    /// The first enabled option (`last`: the last).
    pub(crate) fn end(options: &[Choice], last: bool) -> Option<usize> {
        match last {
            false => options.iter().position(|o| o.enabled),
            true => options.iter().rposition(|o| o.enabled),
        }
    }

    /// Plain labels for typing to jump to; a disabled option's is empty,
    /// so no letters start it.
    pub(crate) fn typeahead_labels(options: &[Choice]) -> Vec<String> {
        options
            .iter()
            .map(|o| match o.enabled {
                true => plain(&o.label),
                false => String::new(),
            })
            .collect()
    }
}

/// What [`select_field`] and [`select_field_keyed`] make.
struct SelectField {
    choices: Choices,
    /// Its menu is open.
    open: Rc<Cell<bool>>,
    typeahead: Typeahead,
}

/// A drop-down field showing the option `selected` holds, of `options`
/// (console markup; may read signals): `Dark      ▾`. Enter, Space or a
/// click opens a menu of the options under it, the chosen one selected:
/// ↑/↓ move, Enter or a click chooses, Esc closes it. Closed, ↑/↓ choose
/// the option before or after; in either, typing letters jumps to the
/// option they start. The field is drawn in the theme's `select` style,
/// with `control.focus` over it while it has the focus; the menu is a
/// [menu](crate::menu)'s. Its role is
/// [`ComboBox`](crate::a11y::Role::ComboBox): name it with
/// [`label`](Node::label); its value is the choice, with its place among
/// the options and whether the menu is open. [`select_field_keyed`] knows
/// its options by key, and may disable some.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::select_field;
///
/// let app = App::new(|| {
///     let theme = signal(0usize);
///     let names = || vec!["Dark".into(), "Light".into(), "Mono".into()];
///     column([select_field(names, theme).label("Theme"), text!("{theme}")])
/// });
/// let mut driver = app.driver(12, 6);
/// // Open the menu, move down and choose.
/// driver.press("enter down enter");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Light      ▾");
/// // Closed, a letter jumps to the option it starts.
/// driver.press("m");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "2");
/// assert_eq!(
///     driver.accessibility()[0].describe(),
///     "Theme, combo box, 3 of 3: Mono, collapsed"
/// );
/// ```
pub fn select_field(options: impl Fn() -> Vec<String> + 'static, selected: Signal<usize>) -> Node {
    select_of(Choices::indexed(options, selected))
}

/// A [`select_field`] whose options are known by key: `selected` holds
/// the chosen option's key, so the choice stays with its option when the
/// options are reordered or relabelled. When the chosen key goes from the
/// options, `selected` becomes `None` and the field shows `(none)`. A
/// disabled option ([`SelectOption::disabled`]) is shown greyed in the
/// menu with its reason, and cannot be chosen by the arrows, a click or
/// typing.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{select_field_keyed, SelectOption};
///
/// let app = App::new(|| {
///     let device = signal(Some("hdmi"));
///     let options = || {
///         vec![
///             SelectOption::new("usb", "USB headset").disabled("unplugged"),
///             SelectOption::new("hdmi", "HDMI"),
///             SelectOption::new("jack", "Headphones"),
///         ]
///     };
///     select_field_keyed(options, device).label("Output")
/// });
/// let mut driver = app.driver(20, 6);
/// // Up passes the disabled option by: HDMI is the first that may be chosen.
/// driver.press("up");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "HDMI               ▾");
/// driver.press("down");
/// assert_eq!(driver.accessibility()[0].describe(), "Output, combo box, 3 of 3: Headphones, collapsed");
/// ```
pub fn select_field_keyed<K: Clone + PartialEq + 'static>(
    options: impl Fn() -> Vec<SelectOption<K>> + 'static,
    selected: Signal<Option<K>>,
) -> Node {
    select_of(Choices::keyed(options, selected))
}

fn select_of(choices: Choices) -> Node {
    widget(SelectField {
        choices,
        open: Rc::new(Cell::new(false)),
        typeahead: Typeahead::default(),
    })
    .auto()
}

impl SelectField {
    /// Open the menu under the field.
    fn open(&mut self, cx: &mut EventCx) {
        let options = self.choices.options();
        if options.is_empty() {
            return;
        }
        let items = options
            .iter()
            .enumerate()
            .map(|(i, option)| {
                let (choices, open) = (self.choices.clone(), self.open.clone());
                let item = MenuItem::new(&option.label, move |_| {
                    open.set(false);
                    choices.choose(i);
                });
                match option.enabled {
                    true => item,
                    false => item.disabled(option.reason.clone().unwrap_or_default()),
                }
            })
            .collect();
        self.open.set(true);
        let at = cx.rect();
        let current = self
            .choices
            .current(false)
            .or_else(|| Choices::end(&options, false))
            .unwrap_or(0);
        crate::menu::open_choices(cx.app(), at, items, current, None);
    }
}

/// What a field says when nothing is chosen.
const NONE: &str = "(none)";

impl Widget for SelectField {
    fn name(&self) -> &'static str {
        "select"
    }

    fn describe(&self) -> Option<String> {
        self.access_value()
    }

    fn role(&self) -> Role {
        Role::ComboBox
    }

    fn cursor(&self) -> Option<Rect> {
        Some(Rect::new(0, 0, u16::MAX, 1))
    }

    fn access_value(&self) -> Option<String> {
        let options = self.choices.options();
        match self.choices.current(false) {
            Some(at) => options.get(at).map(|option| plain(&option.label)),
            None if !options.is_empty() => Some("none".to_string()),
            None => None,
        }
    }

    fn access_state(&self) -> AccessState {
        let count = self.choices.options().len();
        let at = self.choices.current(false);
        AccessState {
            expanded: Some(self.open.get()),
            position: at
                .filter(|_| count > 0)
                .map(|at| (at.min(count - 1) + 1, count)),
            ..AccessState::default()
        }
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            // The longest option, a space and the arrow, and room so a
            // short one does not look cut.
            Axis::Horizontal => self
                .choices
                .options()
                .iter()
                .map(|o| markup_width(&o.label))
                .max()
                .unwrap_or(0)
                .max(8)
                .saturating_add(2),
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let options = self.choices.options();
        let selected = self.choices.current(true);
        let width = canvas.width();
        let face = look(cx, &["select"], "");
        let face = match (cx.disabled(), cx.focused()) {
            (true, _) => face.combine(&cx.style("control.disabled", "dim")),
            (false, true) => face.combine(&cx.style("control.focus", "reverse")),
            (false, false) => face,
        };
        canvas.fill(0, 0, width, 1, Some(&face));
        let arrow = if crate::a11y::text_mode() { "v" } else { "▾" };
        let room = width.saturating_sub(2);
        match selected.and_then(|at| options.get(at)) {
            Some(option) => {
                canvas.markup(cx.console(), 0, 0, room, &option.label, Some(&face));
            }
            None if !options.is_empty() => {
                let hint = face.combine(&cx.style("control.placeholder", "dim"));
                canvas.print(0, 0, NONE, Some(&hint));
            }
            None => {}
        }
        if width > 0 {
            canvas.print(width - 1, 0, arrow, Some(&face));
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let options = self.choices.options();
        let count = options.len();
        match event {
            WidgetEvent::Focus(true) => {
                // Back from the menu, however it closed.
                self.open.set(false);
                return Used::No;
            }
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.open(cx),
                KeyCode::Up | KeyCode::Down if count > 0 => {
                    let step = if key.code == KeyCode::Up { -1 } else { 1 };
                    let now = self.choices.current(false);
                    // At the end, it stays.
                    if let Some(to) = Choices::step(&options, now, step, false) {
                        self.choices.choose(to);
                    }
                }
                KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt && count > 0 => {
                    let labels = Choices::typeahead_labels(&options);
                    let now = self.choices.current(false).unwrap_or(count - 1);
                    match self.typeahead.key(c, &labels, now) {
                        Some(to) => self.choices.choose(to),
                        None => return Used::No,
                    }
                }
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                self.open(cx)
            }
            _ => return Used::No,
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}
