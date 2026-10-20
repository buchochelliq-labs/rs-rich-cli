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

type Options = Rc<dyn Fn() -> Vec<String>>;

/// What [`select_field`] makes.
struct SelectField {
    options: Options,
    selected: Signal<usize>,
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
/// the options and whether the menu is open.
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
    widget(SelectField {
        options: Rc::new(options),
        selected,
        open: Rc::new(Cell::new(false)),
        typeahead: Typeahead::default(),
    })
    .auto()
}

impl SelectField {
    /// Open the menu under the field.
    fn open(&mut self, cx: &mut EventCx) {
        let options = (self.options)();
        if options.is_empty() {
            return;
        }
        let items = options
            .iter()
            .enumerate()
            .map(|(i, option)| {
                let (selected, open) = (self.selected, self.open.clone());
                MenuItem::new(option, move |_| {
                    open.set(false);
                    selected.set(i);
                })
            })
            .collect();
        self.open.set(true);
        let at = cx.rect();
        let current = self.selected.get_untracked();
        crate::menu::open_choices(cx.app(), at, items, current, None);
    }
}

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
        (self.options)()
            .get(self.selected.get_untracked())
            .map(|option| plain(option))
    }

    fn access_state(&self) -> AccessState {
        let count = (self.options)().len();
        let at = self.selected.get_untracked();
        AccessState {
            expanded: Some(self.open.get()),
            position: (count > 0).then(|| (at.min(count - 1) + 1, count)),
            ..AccessState::default()
        }
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            // The longest option, a space and the arrow, and room so a
            // short one does not look cut.
            Axis::Horizontal => (self.options)()
                .iter()
                .map(|o| markup_width(o))
                .max()
                .unwrap_or(0)
                .max(8)
                .saturating_add(2),
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let options = (self.options)();
        let selected = self.selected.get();
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
        if let Some(option) = options.get(selected) {
            canvas.markup(cx.console(), 0, 0, room, option, Some(&face));
        }
        if width > 0 {
            canvas.print(width - 1, 0, arrow, Some(&face));
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let count = (self.options)().len();
        match event {
            WidgetEvent::Focus(true) => {
                // Back from the menu, however it closed.
                self.open.set(false);
                return Used::No;
            }
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.open(cx),
                KeyCode::Up | KeyCode::Down if count > 0 => {
                    let now = self.selected.get_untracked().min(count - 1);
                    let to = match key.code {
                        KeyCode::Up => now.saturating_sub(1),
                        _ => (now + 1).min(count - 1),
                    };
                    self.selected.set(to);
                }
                KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt && count > 0 => {
                    let labels: Vec<String> = (self.options)().iter().map(|o| plain(o)).collect();
                    match self
                        .typeahead
                        .key(c, &labels, self.selected.get_untracked())
                    {
                        Some(to) => self.selected.set(to),
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
