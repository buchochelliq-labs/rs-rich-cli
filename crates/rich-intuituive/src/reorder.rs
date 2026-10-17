//! Reorderable lists (#720): [`Node::reorderable`] on a [`list`].
//!
//! [`list`]: crate::list

use std::cell::Cell;
use std::rc::Rc;

use rich_interact::{Button, Key, KeyCode, Modifiers, MouseKind};

use crate::app::Ctx;
use crate::node::{Axis, Node};
use crate::reactive::{signal, Signal};
use crate::screen::Rect;
use crate::widget::{EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// One move of a reorderable list: the item at `from` now sits at `to`,
/// and those between shift a place towards `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: usize,
    pub to: usize,
}

impl Move {
    /// Make the same move in `items`, the app's own copy of the list.
    ///
    /// ```
    /// use intuituive::widgets::Move;
    ///
    /// let mut items = vec!["a", "b", "c"];
    /// Move { from: 2, to: 0 }.apply(&mut items);
    /// assert_eq!(items, ["c", "a", "b"]);
    /// ```
    pub fn apply<T>(&self, items: &mut [T]) {
        if self.from < items.len() && self.to < items.len() {
            rich_interact::components::move_item(items, self.from, self.to);
        }
    }
}

/// What a [`list`](crate::list) shares with its reorder: the selection, the
/// item count, the first row shown and the rows on screen, and where it
/// reads whether the selected row is being carried.
pub(crate) struct ListModel {
    pub selected: Signal<usize>,
    pub len: Rc<dyn Fn() -> usize>,
    pub first: Rc<Cell<usize>>,
    pub rows: Rc<Cell<usize>>,
    pub carried: Rc<Cell<Option<Signal<bool>>>>,
}

type OnReorder = Box<dyn FnMut(Move, &mut Ctx)>;

/// Round a reorderable list: it sees the keys before the list does, and the
/// mouse the list does not use.
struct Reorderable {
    child: Node,
    model: Rc<ListModel>,
    on_reorder: OnReorder,
    /// Whether the selected row is being carried (grabbed with Space, or
    /// dragged), for the list to mark it.
    carried: Signal<bool>,
    /// Where the item grabbed with Space was, for Esc to put it back.
    grabbed: Option<usize>,
    dragging: bool,
}

fn key(code: KeyCode, alt: bool, ctrl: bool) -> Key {
    Key::with(
        code,
        Modifiers {
            alt,
            ctrl,
            shift: false,
        },
    )
}

impl Reorderable {
    fn len(&self) -> usize {
        (self.model.len)()
    }

    /// Carry the selected item to `to` (clamped to the list), telling the
    /// app, and keep it selected.
    fn carry(&mut self, cx: &mut EventCx, to: usize) {
        let len = self.len();
        if len == 0 {
            return;
        }
        let from = self.model.selected.get_untracked().min(len - 1);
        let to = to.min(len - 1);
        if from != to {
            (self.on_reorder)(Move { from, to }, cx.app());
            self.model.selected.set(to);
        }
    }

    fn set_carried(&self, on: bool) {
        if self.carried.get_untracked() != on {
            self.carried.set(on);
        }
    }

    /// A key, before the list sees it.
    fn key(&mut self, cx: &mut EventCx, pressed: Key) -> Used {
        let selected = self.model.selected.get_untracked();
        let page = self.model.rows.get().max(1);
        let is = |code: KeyCode, alt: bool, ctrl: bool| pressed.matches(&key(code, alt, ctrl));
        let char_is = |c: char| pressed.matches(&Key::char(c));
        // Moves that carry the item, grabbed or not: Alt or Ctrl with an
        // arrow, Home or End, and K and J for any terminal.
        let target = if is(KeyCode::Up, true, false) || is(KeyCode::Up, false, true) || char_is('K')
        {
            Some(selected.saturating_sub(1))
        } else if is(KeyCode::Down, true, false) || is(KeyCode::Down, false, true) || char_is('J') {
            Some(selected + 1)
        } else if is(KeyCode::Home, true, false) || is(KeyCode::Home, false, true) {
            Some(0)
        } else if is(KeyCode::End, true, false) || is(KeyCode::End, false, true) {
            Some(usize::MAX)
        } else {
            None
        };
        if let Some(to) = target {
            self.carry(cx, to);
            return Used::Yes;
        }
        if char_is(' ') {
            self.grabbed = match self.grabbed {
                Some(_) => None,
                None if self.len() > 0 => Some(selected),
                None => None,
            };
            self.set_carried(self.grabbed.is_some());
            return Used::Yes;
        }
        let Some(origin) = self.grabbed else {
            return Used::No;
        };
        // Grabbed: the list's own moves carry the item.
        let to = if is(KeyCode::Up, false, false) || char_is('k') {
            selected.saturating_sub(1)
        } else if is(KeyCode::Down, false, false) || char_is('j') {
            selected + 1
        } else if is(KeyCode::PageUp, false, false) {
            selected.saturating_sub(page)
        } else if is(KeyCode::PageDown, false, false) {
            selected + page
        } else if is(KeyCode::Home, false, false) || char_is('g') {
            0
        } else if is(KeyCode::End, false, false) || char_is('G') {
            usize::MAX
        } else if is(KeyCode::Escape, false, false) {
            // Put it back where it was grabbed.
            self.carry(cx, origin);
            self.grabbed = None;
            self.set_carried(false);
            return Used::Yes;
        } else {
            // Any other key drops it, and goes on as usual (Enter to the
            // app's binding).
            self.grabbed = None;
            self.set_carried(false);
            return Used::No;
        };
        self.carry(cx, to);
        Used::Yes
    }

    fn mouse(&mut self, cx: &mut EventCx, kind: MouseKind, row: u16) -> Used {
        let first = self.model.first.get();
        let len = self.len();
        match kind {
            MouseKind::Down(Button::Left) => {
                let at = first + row as usize;
                if at >= len {
                    return Used::No;
                }
                self.grabbed = None;
                self.model.selected.set(at);
                self.dragging = true;
                cx.capture_mouse();
                Used::Yes
            }
            MouseKind::Drag(Button::Left) if self.dragging => {
                self.set_carried(true);
                // Below the last row shown, a row further: the list
                // scrolls to follow.
                let shown = self.model.rows.get().max(1);
                let to = if (row as usize) >= shown {
                    first + shown
                } else {
                    first + row as usize
                };
                self.carry(cx, to);
                Used::Yes
            }
            MouseKind::Up(_) if self.dragging => {
                self.dragging = false;
                self.set_carried(false);
                cx.release_mouse();
                Used::Yes
            }
            _ => Used::No,
        }
    }
}

impl Widget for Reorderable {
    fn name(&self) -> &'static str {
        "reorderable"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        cx.measure(&self.child, axis, width, height)
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![rect]
    }

    fn draw(&mut self, _cx: &mut crate::widget::DrawCx, _canvas: &mut crate::widget::Canvas) {}

    fn previews_keys(&self) -> bool {
        true
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Preview(pressed) => self.key(cx, *pressed),
            WidgetEvent::Mouse(mouse) => self.mouse(cx, mouse.kind, mouse.row),
            _ => Used::No,
        }
    }
}

impl Node {
    /// Let the user put this [`list`](crate::list)'s items in order (#720),
    /// calling `on_reorder` with each [`Move`] for the app to make in its
    /// own items ([`Move::apply`]); the selection follows the moved item.
    ///
    /// - Alt+↑/↓ or Ctrl+↑/↓ move the selected item a place, and Alt or
    ///   Ctrl with Home/End to the top or the bottom. These need a
    ///   terminal that reports modifiers with those keys (most do; any
    ///   with the kitty keyboard protocol).
    /// - `K` and `J` move it a place in any terminal.
    /// - Space grabs it: ↑/↓, `k`/`j`, PgUp/PgDn and Home/End then carry
    ///   it, Space drops it, Esc puts it back where it was, and any other
    ///   key drops it and does what it does.
    /// - With the mouse, a press selects a row and dragging carries it;
    ///   below the last row shown the list scrolls.
    ///
    /// The carried row is marked `↕`. On a node that is not a `list`, this
    /// does nothing.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::Move;
    ///
    /// let app = App::new(|| {
    ///     let items = signal(vec!["alpha".to_string(), "beta".into(), "gamma".into()]);
    ///     let selected = signal(0usize);
    ///     list(move || items.get(), selected)
    ///         .reorderable(move |step: Move, _| items.update(|items| step.apply(items)))
    ///         .on_key("q", |cx| cx.quit())
    /// });
    /// // alpha down two places.
    /// let screen = app.render_with(&["alt+down", "J", "q"], 12, 3).unwrap();
    /// assert_eq!(screen[0].trim_end(), "beta");
    /// assert_eq!(screen[2].trim_end(), "alpha");
    /// ```
    pub fn reorderable(self, on_reorder: impl FnMut(Move, &mut Ctx) + 'static) -> Node {
        let Some(model) = self.list_model.clone() else {
            return self;
        };
        let carried = signal(false);
        model.carried.set(Some(carried));
        self.wrap_in(
            move |child| {
                Box::new(Reorderable {
                    child,
                    model,
                    on_reorder: Box::new(on_reorder),
                    carried,
                    grabbed: None,
                    dragging: false,
                })
            },
            "reorderable",
        )
    }
}
