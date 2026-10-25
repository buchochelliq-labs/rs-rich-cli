//! Menus: a [`menu_bar`], the drop-down menus it opens, and context menus
//! ([`context_menu`]) at the mouse pointer. All are built on the public
//! [`Widget`] trait and [pop-ups](crate::Ctx::popup).
//!
//! A menu is a list of [`MenuItem`]s, each with a label, an optional key
//! hint and an action. ↑/↓ move, Enter or a click runs the item, the mouse
//! pointer moves the selection, and Esc (or a press outside) closes it.
//!
//! ```
//! use intuituive::menu::{menu_bar, Menu, MenuItem};
//! use intuituive::prelude::*;
//!
//! let app = App::new(|| {
//!     let saved = signal(0);
//!     column([
//!         menu_bar(vec![Menu::new(
//!             "File",
//!             vec![
//!                 MenuItem::new("Save", move |_| saved.update(|n| *n += 1)).hint("ctrl+s"),
//!                 MenuItem::separator(),
//!                 MenuItem::new("Quit", |cx| cx.quit()),
//!             ],
//!         )])
//!         .fixed(1),
//!         text!("saved {saved}"),
//!     ])
//!     .on_key("q", |cx| cx.quit())
//! });
//! // Open File and run Save, then open it again and move to Quit, past
//! // the separator, and close it with Esc.
//! let screen = app
//!     .render_with(&["enter", "enter", "enter", "down", "esc", "q"], 30, 6)
//!     .unwrap();
//! assert_eq!(screen[1].trim_end(), "saved 1");
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rich::Segment;
use rich_interact::{KeyCode, MouseKind};

use crate::app::{Anchor, Ctx, Placement};
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::screen::Rect;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

type Action = Rc<RefCell<dyn FnMut(&mut Ctx)>>;

/// One line of a menu: an action with a label and a key hint, or a
/// separator.
#[derive(Clone)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct MenuItem {
    label: String,
    hint: String,
    action: Option<Action>,
    /// Shown but not chosen: greyed, with why ([`disabled`](Self::disabled)).
    disabled: Option<String>,
}

impl MenuItem {
    /// An item labelled `label` (console markup) that runs `action`; the
    /// menu closes first, so the action may open a screen of its own.
    pub fn new(label: &str, action: impl FnMut(&mut Ctx) + 'static) -> MenuItem {
        MenuItem {
            label: label.to_string(),
            hint: String::new(),
            action: Some(Rc::new(RefCell::new(action))),
            disabled: None,
        }
    }

    /// A line between groups of items.
    pub fn separator() -> MenuItem {
        MenuItem {
            label: String::new(),
            hint: String::new(),
            action: None,
            disabled: None,
        }
    }

    /// The keys that do the same, shown at the right (`"ctrl+s"`).
    pub fn hint(mut self, keys: &str) -> MenuItem {
        self.hint = keys.to_string();
        self
    }

    /// Shown greyed, with `reason` (none when empty) where a hint would
    /// be: the arrows, letters and the mouse pass it by, and Enter or a
    /// click on it does nothing.
    pub fn disabled(mut self, reason: impl Into<String>) -> MenuItem {
        self.disabled = Some(reason.into());
        self
    }

    fn is_separator(&self) -> bool {
        self.action.is_none()
    }

    /// Whether the selection may rest on it: an item, not disabled.
    fn selectable(&self) -> bool {
        !self.is_separator() && self.disabled.is_none()
    }
}

/// A titled menu for a [`menu_bar`].
#[derive(Clone)]
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
}

impl Menu {
    pub fn new(title: &str, items: Vec<MenuItem>) -> Menu {
        Menu {
            title: title.to_string(),
            items,
        }
    }
}

/// The cells of `markup`.
fn width(markup: &str) -> u16 {
    rich::Text::from_markup(markup)
        .map(|t| t.cell_len())
        .unwrap_or_else(|_| rich::cells::cell_len(markup))
        .min(u16::MAX as usize) as u16
}

/// `markup` on one line, `width` wide.
fn line(cx: &DrawCx, markup: &str, width: u16) -> Vec<Segment> {
    let text =
        rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
    let mut options = cx.console().options().update_width(width.max(1) as usize);
    options.no_wrap = Some(true);
    options.overflow = Some(rich::Overflow::Ellipsis);
    cx.console()
        .render_lines(&text, &options, true)
        .into_iter()
        .next()
        .unwrap_or_default()
}

struct MenuList {
    items: Vec<MenuItem>,
    selected: Cell<usize>,
    /// Letters jump to the item they start (a select field's menu), rather
    /// than j and k moving.
    typeahead: Option<crate::select_field::Typeahead>,
}

impl MenuList {
    fn new(items: Vec<MenuItem>) -> MenuList {
        let first = items.iter().position(MenuItem::selectable).unwrap_or(0);
        MenuList {
            items,
            selected: Cell::new(first),
            typeahead: None,
        }
    }

    /// Move to the next item that is not a separator, by `step`.
    fn step(&self, step: isize) {
        let n = self.items.len() as isize;
        if n == 0 {
            return;
        }
        let mut at = self.selected.get() as isize;
        for _ in 0..n {
            at = (at + step).rem_euclid(n);
            if self.items[at as usize].selectable() {
                self.selected.set(at as usize);
                return;
            }
        }
    }

    /// A key that moves the selection or runs the selected item.
    fn arrows(&self, cx: &mut EventCx, code: KeyCode) -> Used {
        match code {
            KeyCode::Up => self.step(-1),
            KeyCode::Down => self.step(1),
            KeyCode::Char('k') if self.typeahead.is_none() => self.step(-1),
            KeyCode::Char('j') if self.typeahead.is_none() => self.step(1),
            KeyCode::Enter | KeyCode::Char(' ') => return self.run(cx, self.selected.get()),
            _ => return Used::No,
        }
        cx.redraw();
        Used::Yes
    }

    /// Close the menu and run item `index`.
    fn run(&self, cx: &mut EventCx, index: usize) -> Used {
        let Some(item) = self.items.get(index) else {
            return Used::No;
        };
        if item.disabled.is_some() {
            return Used::Yes;
        }
        let Some(action) = item.action.clone() else {
            return Used::No;
        };
        cx.app().pop();
        (action.borrow_mut())(cx.app());
        Used::Yes
    }
}

impl Widget for MenuList {
    fn name(&self) -> &'static str {
        "menu"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Menu
    }

    fn cursor(&self) -> Option<crate::screen::Rect> {
        Some(crate::screen::Rect::new(
            0,
            self.selected.get() as u16,
            u16::MAX,
            1,
        ))
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        // Its place among the items, not counting separators.
        let items: Vec<usize> = (0..self.items.len())
            .filter(|&i| !self.items[i].is_separator())
            .collect();
        let at = items.iter().position(|&i| i == self.selected.get());
        crate::a11y::AccessState::item(at.unwrap_or(0), items.len())
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => self.items.len().min(u16::MAX as usize) as u16,
            Axis::Horizontal => self
                .items
                .iter()
                .map(|i| {
                    let hint = match &i.disabled {
                        Some(reason) if i.hint.is_empty() => reason,
                        _ => &i.hint,
                    };
                    width(&i.label) + if hint.is_empty() { 0 } else { width(hint) + 2 }
                })
                .max()
                .unwrap_or(0)
                .saturating_add(2),
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        // Asked so the pointer's movement reaches the menu.
        cx.hovered();
        let w = canvas.width();
        let selected = cx.style("selected", "reverse");
        let dim = cx.style("menu.separator", "bright_black");
        let hint_style = cx.style("menu.hint", "dim");
        for (row, item) in self.items.iter().enumerate() {
            let y = row as u16;
            if item.is_separator() {
                // Decoration: blank in text mode, so it is not read out.
                let rule = if crate::a11y::text_mode() { " " } else { "─" };
                canvas.print(0, y, &rule.repeat(w as usize), Some(&dim));
                continue;
            }
            // " label   hint ": a space each side, two before the hint; a
            // disabled item's reason where its hint would be.
            let hint = match &item.disabled {
                Some(reason) if item.hint.is_empty() => reason,
                _ => &item.hint,
            };
            let hint_w = width(hint);
            let right = if hint_w > 0 { hint_w + 2 } else { 0 };
            let label_w = w.saturating_sub(right + 2).max(1);
            let mut segments = vec![Segment::new(" ", None)];
            segments.extend(line(cx, &item.label, label_w));
            let used: usize = segments.iter().map(Segment::cell_length).sum();
            let gap = (w as usize).saturating_sub(used + right as usize);
            segments.push(Segment::new(" ".repeat(gap.max(1)), None));
            if hint_w > 0 {
                segments.push(Segment::new(hint.clone(), Some(hint_style.clone())));
                segments.push(Segment::new(" ", None));
            }
            if item.disabled.is_some() {
                let off = cx.style("menu.disabled", "dim");
                segments = segments
                    .into_iter()
                    .map(|s| {
                        let style = s
                            .style
                            .as_ref()
                            .map_or(off.clone(), |own| own.combine(&off));
                        Segment::new(s.text, Some(style))
                    })
                    .collect();
            }
            if row == self.selected.get() && item.selectable() {
                segments = segments
                    .into_iter()
                    .map(|s| {
                        let style = match &s.style {
                            Some(own) => own.combine(&selected),
                            None => selected.clone(),
                        };
                        Segment::new(s.text, Some(style))
                    })
                    .collect();
            }
            canvas.lines_at(0, y, w, 1, &[segments]);
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Key(key) if self.typeahead.is_some() && key.code != KeyCode::Char(' ') => {
                let KeyCode::Char(c) = key.code else {
                    return self.arrows(cx, key.code);
                };
                if key.modifiers.ctrl || key.modifiers.alt {
                    return Used::No;
                }
                // A disabled item says nothing letters could start.
                let labels: Vec<String> = self
                    .items
                    .iter()
                    .map(|i| match i.selectable() {
                        true => crate::controls::plain(&i.label),
                        false => String::new(),
                    })
                    .collect();
                let at = self.selected.get();
                let typeahead = self.typeahead.as_mut().expect("typeahead");
                if let Some(to) = typeahead.key(c, &labels, at) {
                    self.selected.set(to);
                }
            }
            WidgetEvent::Key(key) => return self.arrows(cx, key.code),
            WidgetEvent::Mouse(mouse) => {
                let row = mouse.row as usize;
                let on_item = self.items.get(row).is_some_and(MenuItem::selectable);
                let on_disabled = self.items.get(row).is_some_and(|i| i.disabled.is_some());
                match mouse.kind {
                    MouseKind::Moved if on_item && row != self.selected.get() => {
                        self.selected.set(row)
                    }
                    MouseKind::Down(_) if on_item => return self.run(cx, row),
                    MouseKind::Down(_) if on_disabled => return Used::Yes,
                    MouseKind::ScrollUp => self.step(-1),
                    MouseKind::ScrollDown => self.step(1),
                    _ => return Used::No,
                }
            }
            _ => return Used::No,
        }
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// Open a menu of `items` next to `at` (a node or a rectangle of the
/// screen), on the `placement` side.
pub fn open_menu(cx: &mut Ctx, at: impl Into<Anchor>, placement: Placement, items: Vec<MenuItem>) {
    cx.popup(at, placement, Size::Auto, Size::Auto, move || {
        widget(MenuList::new(items)).panel("")
    });
}

/// [`open_menu`] with item `selected` selected, letters jumping to the
/// item they start: a [`select_field`](crate::select_field)'s menu.
pub(crate) fn open_choices(
    cx: &mut Ctx,
    at: impl Into<Anchor>,
    items: Vec<MenuItem>,
    selected: usize,
    label: Option<String>,
) {
    cx.popup(at, Placement::Below, Size::Auto, Size::Auto, move || {
        let mut list = MenuList::new(items);
        if selected < list.items.len() && list.items[selected].selectable() {
            list.selected.set(selected);
        }
        list.typeahead = Some(crate::select_field::Typeahead::default());
        let menu = widget(list);
        match label {
            Some(label) => menu.label(label),
            None => menu,
        }
        .panel("")
    });
}

/// Open a menu of `items` at the mouse pointer, from a mouse handler (a
/// right click: `.on_mouse(|cx, m| …)`); elsewhere, in the middle of the
/// screen.
///
/// ```
/// use intuituive::interact::{Button, MouseKind};
/// use intuituive::menu::{context_menu, MenuItem};
/// use intuituive::prelude::*;
///
/// let node = label("right-click me").on_mouse(|cx, mouse| {
///     if mouse.kind != MouseKind::Down(Button::Right) {
///         return false;
///     }
///     context_menu(cx, vec![MenuItem::new("Copy", |_| {}), MenuItem::new("Paste", |_| {})]);
///     true
/// });
/// # let _ = node;
/// ```
pub fn context_menu(cx: &mut Ctx, items: Vec<MenuItem>) {
    match cx.pointer() {
        Some((x, y)) => open_menu(cx, Rect::new(x, y, 1, 1), Placement::Below, items),
        None => cx.modal(Size::Auto, Size::Auto, move || {
            widget(MenuList::new(items)).panel("")
        }),
    }
}

struct MenuBar {
    menus: Vec<Menu>,
    selected: Cell<usize>,
    /// Where each title was drawn: its first column and width.
    spans: RefCell<Vec<(u16, u16)>>,
}

impl MenuBar {
    fn open(&self, cx: &mut EventCx, index: usize) -> Used {
        let Some(menu) = self.menus.get(index) else {
            return Used::No;
        };
        let (x, w) = self.spans.borrow().get(index).copied().unwrap_or((0, 1));
        let bar = cx.rect();
        let at = Rect::new(bar.x + x, bar.y, w, 1);
        open_menu(cx.app(), at, Placement::Below, menu.items.clone());
        Used::Yes
    }
}

/// A row of menu titles. ←/→ move between them while it has the focus,
/// and Enter, ↓ or a click opens one below its title. Give it a key from
/// anywhere with `.on_key("f10", move |cx| cx.focus(bar_id))` on the root.
pub fn menu_bar(menus: Vec<Menu>) -> Node {
    widget(MenuBar {
        menus,
        selected: Cell::new(0),
        spans: RefCell::new(Vec::new()),
    })
}

impl Widget for MenuBar {
    fn name(&self) -> &'static str {
        "menu bar"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::MenuBar
    }

    fn cursor(&self) -> Option<crate::screen::Rect> {
        let spans = self.spans.borrow();
        spans
            .get(self.selected.get())
            .map(|&(x, w)| crate::screen::Rect::new(x, 0, w, 1))
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        crate::a11y::AccessState::item(self.selected.get(), self.menus.len())
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let bar = cx.style("menu.bar", "");
        let chosen = if cx.focused() {
            cx.style("selected", "reverse")
        } else {
            bar.clone()
        };
        canvas.fill(0, 0, canvas.width(), 1, Some(&bar));
        let mut spans = Vec::new();
        let mut x = 0u16;
        for (i, menu) in self.menus.iter().enumerate() {
            let w = width(&menu.title) + 2;
            let style = if i == self.selected.get() {
                &chosen
            } else {
                &bar
            };
            let mut segments = vec![Segment::new(" ", Some(style.clone()))];
            segments.extend(line(cx, &menu.title, w - 2).into_iter().map(|s| {
                let combined = match &s.style {
                    Some(own) => style.combine(own),
                    None => style.clone(),
                };
                Segment::new(s.text, Some(combined))
            }));
            segments.push(Segment::new(" ", Some(style.clone())));
            canvas.lines_at(x, 0, w, 1, &[segments]);
            spans.push((x, w));
            x = x.saturating_add(w);
        }
        *self.spans.borrow_mut() = spans;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let last = self.menus.len().saturating_sub(1);
        match event {
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Left => self
                    .selected
                    .set(self.selected.get().checked_sub(1).unwrap_or(last)),
                KeyCode::Right => self.selected.set(if self.selected.get() >= last {
                    0
                } else {
                    self.selected.get() + 1
                }),
                KeyCode::Enter | KeyCode::Down | KeyCode::Char(' ') => {
                    return self.open(cx, self.selected.get())
                }
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) => {
                if !matches!(mouse.kind, MouseKind::Down(_)) {
                    return Used::No;
                }
                let hit = self
                    .spans
                    .borrow()
                    .iter()
                    .position(|(x, w)| mouse.column >= *x && mouse.column < x + w);
                let Some(index) = hit else {
                    return Used::No;
                };
                self.selected.set(index);
                cx.redraw();
                return self.open(cx, index);
            }
            _ => return Used::No,
        }
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}
