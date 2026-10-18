//! Lists: [`list`], [`list_optional`] and [`list_lazy`], markup items with one selected
//! (or none), and their options, as builders on the node.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rich::{Console, Segment, Style};
use rich_interact::{Button, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::{Runtime, Signal};
use crate::screen::Rect;
use crate::scrollbar::Bar;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// Items a turn of the mouse wheel moves a list's selection, as a table's
/// rows.
const WHEEL_STEP: isize = 3;

/// When a list's or a table's highlight symbol keeps its column: ratatui's
/// `HighlightSpacing`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HighlightSpacing {
    /// Always, even with nothing selected: the items never move.
    Always,
    /// While an item is selected (the default).
    #[default]
    WhenSelected,
    /// Never: the symbol is not drawn.
    Never,
}

impl HighlightSpacing {
    /// Whether the symbol's column is kept, with or without a selection.
    pub(crate) fn keeps(self, selected: bool) -> bool {
        match self {
            HighlightSpacing::Always => true,
            HighlightSpacing::WhenSelected => selected,
            HighlightSpacing::Never => false,
        }
    }
}

/// Which way a list's items run: ratatui's `ListDirection`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListDirection {
    /// The first item at the top (the default).
    #[default]
    TopToBottom,
    /// The first item at the bottom, the rest above it (a chat, a log).
    BottomToTop,
}

/// What a list's selection is: always an item, or an item or none.
#[derive(Clone, Copy)]
pub(crate) enum Selection {
    One(Signal<usize>),
    Maybe(Signal<Option<usize>>),
}

impl Selection {
    /// The selected item, subscribing the node that is drawing.
    pub(crate) fn get(self) -> Option<usize> {
        match self {
            Selection::One(s) => Some(s.get()),
            Selection::Maybe(s) => s.get(),
        }
    }

    pub(crate) fn get_untracked(self) -> Option<usize> {
        match self {
            Selection::One(s) => Some(s.get_untracked()),
            Selection::Maybe(s) => s.get_untracked(),
        }
    }

    pub(crate) fn set(self, index: usize) {
        match self {
            Selection::One(s) => s.set(index),
            Selection::Maybe(s) => s.set(Some(index)),
        }
    }

    /// Move `by` items in a list of `len`; with nothing selected, a move
    /// forward selects the first item and one back the last.
    pub(crate) fn step(self, len: usize, by: isize) {
        let last = len.saturating_sub(1) as isize;
        match self {
            Selection::One(s) => {
                s.update(|s| *s = (*s as isize + by).clamp(0, last.max(0)) as usize)
            }
            Selection::Maybe(s) => s.update(|s| {
                *s = match *s {
                    _ if len == 0 => None,
                    None if by > 0 => Some(0),
                    None => Some(len - 1),
                    Some(i) => Some((i as isize + by).clamp(0, last) as usize),
                }
            }),
        }
    }

    pub(crate) fn first(self, len: usize) {
        match self {
            Selection::One(s) => s.set(0),
            Selection::Maybe(s) => s.set((len > 0).then_some(0)),
        }
    }

    pub(crate) fn last(self, len: usize) {
        match self {
            Selection::One(s) => s.set(len.saturating_sub(1)),
            Selection::Maybe(s) => s.set(len.checked_sub(1)),
        }
    }
}

/// A list's row style: for item `i`, a style (or theme style name) for
/// its whole rows, or none.
type RowStyle = Box<dyn Fn(usize) -> Option<String>>;

/// What a list's builders set.
#[derive(Default)]
pub(crate) struct ListOptions {
    symbol: Option<String>,
    repeat_symbol: bool,
    spacing: HighlightSpacing,
    direction: ListDirection,
    padding: usize,
    multiline: bool,
    row_style: Option<RowStyle>,
    offset: Option<Signal<usize>>,
    /// Markup at the right of item `i`'s first line; empty for none.
    trailing: Option<Box<dyn Fn(usize) -> String>>,
    /// The style of item `i`'s cell in the marker column, or none (blank).
    marker: Option<RowStyle>,
    /// The selection's style when item `i` is selected, or none (the
    /// theme's `selected`).
    selected_style: Option<RowStyle>,
    /// Where the rows in view were drawn, on the screen.
    rects: Option<RowRects>,
}

/// Where a list's items come from: every item from one closure, or a count
/// and one item at a time ([`list_lazy`]).
#[derive(Clone)]
pub(crate) enum Items {
    All(Rc<dyn Fn() -> Vec<String>>),
    Lazy {
        len: Rc<dyn Fn() -> usize>,
        item: Rc<dyn Fn(usize) -> String>,
    },
}

impl Items {
    fn len(&self) -> usize {
        match self {
            Items::All(items) => items().len(),
            Items::Lazy { len, .. } => len(),
        }
    }

    /// The items one draw or measure looks at: all of them, once, or each
    /// as it is asked for.
    fn fetch(&self) -> Fetched<'_> {
        match self {
            Items::All(items) => Fetched::All(items()),
            Items::Lazy { len, item } => Fetched::Lazy {
                len: len(),
                item: &**item,
                cache: RefCell::new(HashMap::new()),
            },
        }
    }
}

/// A draw's items, from [`Items::fetch`].
enum Fetched<'a> {
    All(Vec<String>),
    Lazy {
        len: usize,
        item: &'a dyn Fn(usize) -> String,
        cache: RefCell<HashMap<usize, String>>,
    },
}

impl Fetched<'_> {
    fn len(&self) -> usize {
        match self {
            Fetched::All(items) => items.len(),
            Fetched::Lazy { len, .. } => *len,
        }
    }

    fn get(&self, i: usize) -> String {
        match self {
            Fetched::All(items) => items.get(i).cloned().unwrap_or_default(),
            Fetched::Lazy { item, cache, .. } => cache
                .borrow_mut()
                .entry(i)
                .or_insert_with(|| item(i))
                .clone(),
        }
    }
}

/// Where a list's (or a table's) rows were drawn on the screen at its last
/// draw: give one to [`Node::row_rects`] and ask it in a handler, to place a
/// popup at a row.
///
/// Each row in view has its item's index and its rectangle in screen cells:
/// the cells its highlight takes, without a [marker](Node::marker) column
/// or a scroll bar. Rows out of view have none.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::RowRects;
///
/// let rects = RowRects::new();
/// let shown = rects.clone();
/// let app = App::new(move || {
///     let items = || (0..20).map(|n| format!("item {n}")).collect();
///     column([label("title").fixed(1), list(items, signal(4)).row_rects(&shown)])
/// });
/// let mut driver = app.driver(12, 4);
/// driver.settle();
/// // Item 4 is on the list's last row, the screen's fourth.
/// let rect = rects.get(4).unwrap();
/// assert_eq!((rect.x, rect.y, rect.width, rect.height), (0, 3, 12, 1));
/// assert_eq!(rects.get(0), None);
/// ```
#[derive(Clone, Default)]
pub struct RowRects(Rc<RefCell<Vec<(usize, Rect)>>>);

impl RowRects {
    /// A handle with no rows yet.
    pub fn new() -> RowRects {
        RowRects::default()
    }

    /// Where item `item`'s rows were drawn, if it was in view.
    pub fn get(&self, item: usize) -> Option<Rect> {
        self.0
            .borrow()
            .iter()
            .find(|(i, _)| *i == item)
            .map(|(_, rect)| *rect)
    }

    /// Every row in view: its item and where it was drawn, top to bottom.
    pub fn shown(&self) -> Vec<(usize, Rect)> {
        self.0.borrow().clone()
    }

    pub(crate) fn set(&self, rows: Vec<(usize, Rect)>) {
        *self.0.borrow_mut() = rows;
    }
}

/// A list of markup items: [`list`] and [`list_optional`].
pub(crate) struct List {
    items: Items,
    selection: Selection,
    options: Rc<RefCell<ListOptions>>,
    /// The first item shown, and the items on screen at the last draw (a
    /// page's size).
    first: Rc<Cell<usize>>,
    rows: Rc<Cell<usize>>,
    /// The item each row on screen shows, top to bottom.
    shown: Rc<RefCell<Vec<Option<usize>>>>,
    /// Whether the selected item is being carried, once it is reorderable.
    carried: Rc<Cell<Option<Signal<bool>>>>,
    /// Where the selected item is, for the cursor in accessible mode.
    cursor: Cell<Option<Rect>>,
    /// The [`offset`](Node::offset) signal's value as last read or set.
    seen: Cell<Option<usize>>,
    pub(crate) bar: Bar,
    /// Where the bar was drawn, and what it showed: the first item, the
    /// items and the items in view.
    bar_at: Cell<Option<(Rect, usize, usize, usize)>>,
}

/// A scrolling list of markup rows, one selected: ratatui's `List` and
/// `ListState` in one node. `items` may read signals; `selected` is the
/// index of the selected row, which you read and write like any signal.
///
/// The list keeps the selected row in view, scrolling only as far as it
/// must, and draws it in the theme's `selected` style, filled to the full
/// width. It is focusable, and while it has the focus ↑/k, ↓/j, Home/g,
/// End/G, PageUp and PageDown move the selection; bind Enter (or anything
/// else) yourself with [`on_key`](Node::on_key). A press on an item selects
/// it, and the wheel moves the selection three items, as on a table.
///
/// Its options are builders on the node: [`Node::symbol`] (a highlight
/// symbol, with [`Node::highlight_spacing`] and [`Node::repeat_symbol`]),
/// [`Node::direction`], [`Node::scroll_padding`], [`Node::multiline`],
/// [`Node::row_style`], [`Node::offset`],
/// [`Node::scrollbar`](crate::Node::scrollbar), and the row controls
/// [`Node::trailing`], [`Node::marker`], [`Node::selected_style_for`] and
/// [`Node::row_rects`]. Sized [`Size::Auto`](crate::Size::Auto) across, it
/// is as wide as its widest item with those columns. For a list with
/// nothing selected, see [`list_optional`]; for one that asks only for the
/// items it draws, [`list_lazy`].
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let selected = signal(0usize);
///     let items = || (1..=50).map(|n| format!("item {n}")).collect();
///     list(items, selected).on_key("q", |cx| cx.quit())
/// });
/// // Down 7 times in a 5-row list: rows 4-8 (items 4-8) show, item 8 selected.
/// let keys = ["j", "j", "j", "j", "j", "j", "j", "q"];
/// let screen = app.render_with(&keys, 20, 5).unwrap();
/// assert_eq!(screen[0].trim_end(), "item 4");
/// assert_eq!(screen[4].trim_end(), "item 8");
/// ```
pub fn list(items: impl Fn() -> Vec<String> + 'static, selected: Signal<usize>) -> Node {
    list_of(Items::All(Rc::new(items)), Selection::One(selected))
}

/// A [`list`] whose selection may be empty, as ratatui's
/// `ListState::select(None)`: `None` shows no highlight, ↓ (or Home) then
/// selects the first item and ↑ (or End) the last. Setting the signal to
/// `None` clears the selection.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let selected = signal(None);
///     let items = || vec!["alpha".to_string(), "beta".into()];
///     column([
///         list_optional(items, selected).symbol("> ").fixed(2),
///         text(move || format!("{:?}", selected.get())),
///     ])
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 12, 3).unwrap();
/// assert_eq!(screen[0].trim_end(), "alpha");
/// assert_eq!(screen[2].trim_end(), "None");
/// let screen = app_with_down().render_with(&["down", "q"], 12, 3).unwrap();
/// assert_eq!(screen[0].trim_end(), "> alpha");
/// assert_eq!(screen[2].trim_end(), "Some(0)");
/// # fn app_with_down() -> App {
/// #     App::new(|| {
/// #         let selected = signal(None);
/// #         let items = || vec!["alpha".to_string(), "beta".into()];
/// #         column([
/// #             list_optional(items, selected).symbol("> ").fixed(2),
/// #             text(move || format!("{:?}", selected.get())),
/// #         ])
/// #         .on_key("q", |cx| cx.quit())
/// #     })
/// # }
/// ```
pub fn list_optional(
    items: impl Fn() -> Vec<String> + 'static,
    selected: Signal<Option<usize>>,
) -> Node {
    list_of(Items::All(Rc::new(items)), Selection::Maybe(selected))
}

/// A [`list`] that asks only for the items it draws: `len` is how many
/// there are and `item` makes one, so a list of a million items costs a
/// screenful each draw, where [`list`]'s closure builds every item. It
/// takes every option a list does; with [`multiline`](Node::multiline),
/// or sized [`Size::Auto`](crate::Size::Auto) across, it asks for every
/// item to measure them.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::list_lazy;
///
/// let app = App::new(|| {
///     let selected = signal(0usize);
///     list_lazy(|| 1_000_000, |i| format!("item {i}"), selected)
///         .scroll_padding(1)
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["end", "q"], 16, 3).unwrap();
/// assert_eq!(screen[0].trim_end(), "item 999997");
/// assert_eq!(screen[2].trim_end(), "item 999999");
/// ```
pub fn list_lazy(
    len: impl Fn() -> usize + 'static,
    item: impl Fn(usize) -> String + 'static,
    selected: Signal<usize>,
) -> Node {
    let items = Items::Lazy {
        len: Rc::new(len),
        item: Rc::new(item),
    };
    list_of(items, Selection::One(selected))
}

fn list_of(items: Items, selection: Selection) -> Node {
    let options = Rc::new(RefCell::new(ListOptions::default()));
    let first = Rc::new(Cell::new(0usize));
    let rows = Rc::new(Cell::new(1usize));
    let shown = Rc::new(RefCell::new(Vec::new()));
    let carried: Rc<Cell<Option<Signal<bool>>>> = Rc::new(Cell::new(None));
    let reversed = Rc::new(Cell::new(false));
    let len: Rc<dyn Fn() -> usize> = {
        let items = items.clone();
        Rc::new(move || items.len())
    };
    // The item a press on a row selects; and the item a drag on a row
    // carries to, a row past the far end of the items reaching one more.
    let item_at: Rc<dyn Fn(u16) -> Option<usize>> = {
        let shown = shown.clone();
        Rc::new(move |row| shown.borrow().get(row as usize).copied().flatten())
    };
    let drag_to: Rc<dyn Fn(u16) -> usize> = {
        let (shown, first, rows, reversed) =
            (shown.clone(), first.clone(), rows.clone(), reversed.clone());
        Rc::new(move |row| {
            let shown = shown.borrow();
            if let Some(Some(item)) = shown.get(row as usize) {
                return *item;
            }
            let beyond = first.get() + rows.get();
            // Below the rows going down, or above them going up: one more.
            let below = row as usize >= shown.len();
            if below != reversed.get() {
                beyond
            } else {
                first.get().saturating_sub(1)
            }
        })
    };
    let model = Rc::new(crate::reorder::ListModel {
        selection,
        len: len.clone(),
        rows: rows.clone(),
        carried: carried.clone(),
        reversed: reversed.clone(),
        item_at,
        drag_to,
    });
    let widget = List {
        items: items.clone(),
        selection,
        options: options.clone(),
        first,
        rows: rows.clone(),
        shown,
        carried,
        cursor: Cell::new(None),
        seen: Cell::new(None),
        bar: Bar::default(),
        bar_at: Cell::new(None),
    };
    // Up and down the screen: with the items bottom to top, ↑ goes on to
    // the next item.
    let along = {
        let options = options.clone();
        move |by: isize| match options.borrow().direction {
            ListDirection::TopToBottom => by,
            ListDirection::BottomToTop => -by,
        }
    };
    let step = {
        let len = len.clone();
        move |by: isize| selection.step(len(), along(by))
    };
    let (rows_up, rows_down) = (rows.clone(), rows);
    let (up, down, pgup, pgdn) = (step.clone(), step.clone(), step.clone(), step);
    let (home_len, end_len) = (len.clone(), len);
    let mut node = Node::from_widget(Box::new(widget), "list");
    node.list_model = Some(model);
    node.focusable()
        .on_key("up k", move |_| up(-1))
        .on_key("down j", move |_| down(1))
        .on_key("pageup", move |_| pgup(-(rows_up.get() as isize)))
        .on_key("pagedown", move |_| pgdn(rows_down.get() as isize))
        .on_key("home g", move |_| selection.first(home_len()))
        .on_key("end G", move |_| selection.last(end_len()))
}

/// Run `f` on `node`'s list: the node itself, or the list a
/// [`reorderable`](Node::reorderable) wraps. `None` for any other node.
pub(crate) fn with_list<R>(node: &Node, f: impl FnOnce(&mut List) -> R) -> Option<R> {
    let mut f = Some(f);
    if let Some(r) = node.widget_mut(|list: &mut List| (f.take().expect("once"))(list)) {
        return Some(r);
    }
    node.widget_mut(|wrapper: &mut crate::reorder::Reorderable| {
        wrapper
            .child
            .widget_mut(|list: &mut List| (f.take().expect("once"))(list))
    })
    .flatten()
}

/// The first item in view of `len` items in a view `view` lines tall, item
/// `i` being `height(i)` lines: `first` moved only as far as it must so the
/// `selected` item, and `padding` items either side where they fit, are in
/// view, and never so far down that the view ends with room to spare.
/// Walks only the items near the selection and the end, so a million items
/// cost a screenful.
pub(crate) fn first_in_view(
    len: usize,
    view: usize,
    first: usize,
    selected: Option<usize>,
    padding: usize,
    height: &mut dyn FnMut(usize) -> usize,
) -> usize {
    if len == 0 {
        return 0;
    }
    let view = view.max(1);
    // The smallest first that shows `to` and the items before it: back
    // from `to`, as many as fit.
    let from_end = |to: usize, height: &mut dyn FnMut(usize) -> usize| {
        let (mut at, mut used) = (to + 1, 0);
        while at > 0 {
            let h = height(at - 1).max(1);
            if used + h > view {
                break;
            }
            used += h;
            at -= 1;
        }
        at
    };
    let mut first = first.min(from_end(len - 1, height)).min(len - 1);
    if let Some(selected) = selected {
        let selected = selected.min(len - 1);
        first = first.min(selected.saturating_sub(padding));
        let far = (selected + padding).min(len - 1);
        let need = from_end(far, height).min(selected);
        first = first.max(need);
    }
    first
}

/// Rendered `markup` at `width`: one line cut with an ellipsis, or every
/// line it wraps to.
fn item_lines(console: &Console, markup: &str, width: u16, wrap: bool) -> Vec<Vec<Segment>> {
    let text =
        rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
    let mut options = console.options().update_width(width.max(1) as usize);
    if wrap {
        let lines = console.render_lines(&text, &options, false);
        if lines.is_empty() {
            return vec![Vec::new()];
        }
        return lines;
    }
    options.no_wrap = Some(true);
    options.overflow = Some(rich::Overflow::Ellipsis);
    vec![console
        .render_lines(&text, &options, false)
        .into_iter()
        .next()
        .unwrap_or_default()]
}

/// `line` with `style` under its own styles, filled to `width` cells in it.
fn under(line: Vec<Segment>, style: &Style, width: u16) -> Vec<Segment> {
    let mut used = 0;
    let mut out: Vec<Segment> = line
        .into_iter()
        .map(|segment| {
            used += segment.cell_length();
            let combined = match &segment.style {
                Some(own) => style.combine(own),
                None => style.clone(),
            };
            Segment::new(segment.text, Some(combined))
        })
        .collect();
    if used < width as usize {
        out.push(Segment::new(
            " ".repeat(width as usize - used),
            Some(style.clone()),
        ));
    }
    out
}

/// `line` with `style` over its own styles, filled to `width` cells in it:
/// the selection's highlight.
pub(crate) fn over(line: Vec<Segment>, style: &Style, width: u16) -> Vec<Segment> {
    let used: usize = line.iter().map(Segment::cell_length).sum();
    let mut out = crate::widgets::over(line, style);
    if used < width as usize {
        out.push(Segment::new(
            " ".repeat(width as usize - used),
            Some(style.clone()),
        ));
    }
    out
}

/// Item `markup`'s lines at `width`, with `trailing` markup (when it is not
/// empty) at the right of the first line: the item is cut, or wraps, to the
/// room the trailing text and a blank cell before it leave.
fn row_lines(
    console: &Console,
    markup: &str,
    trailing: Option<&str>,
    width: u16,
    wrap: bool,
) -> Vec<Vec<Segment>> {
    let Some(trailing) = trailing.filter(|t| !t.is_empty()) else {
        return item_lines(console, markup, width, wrap);
    };
    let wide = crate::widget::markup_width(trailing).min(width);
    let tail = crate::widget::markup_line(console, trailing, wide);
    let room = width.saturating_sub(wide + 1);
    if room == 0 {
        // No room for the item: the trailing text, at the right.
        let mut line = vec![Segment::new(" ".repeat((width - wide) as usize), None)];
        line.extend(tail);
        return vec![line];
    }
    let mut lines = item_lines(console, markup, room, wrap);
    let first = &mut lines[0];
    let used: usize = first.iter().map(Segment::cell_length).sum();
    first.push(Segment::new(
        " ".repeat(room as usize + 1 - used.min(room as usize)),
        None,
    ));
    first.extend(tail);
    lines
}

/// The cells markup `markup` takes on its widest line (its first, unless
/// `all` lines count).
fn natural_width(markup: &str, all: bool) -> u16 {
    let plain = rich::Text::from_markup(markup)
        .map(|text| text.plain().to_string())
        .unwrap_or_else(|_| markup.to_string());
    let mut lines = plain.split('\n').map(rich::cells::cell_len);
    let widest = if all {
        lines.max().unwrap_or(0)
    } else {
        lines.next().unwrap_or(0)
    };
    widest.min(u16::MAX as usize) as u16
}

/// One draw's layout of a list: the items in view and their lines.
struct Laid {
    first: usize,
    /// Each item in view, from `first` on, with its lines.
    items: Vec<(usize, Vec<Vec<Segment>>)>,
}

impl List {
    /// Item `i`'s lines at `width`, its trailing text included.
    fn lines_of(
        &self,
        console: &Console,
        items: &Fetched,
        i: usize,
        width: u16,
    ) -> Vec<Vec<Segment>> {
        let options = self.options.borrow();
        let trailing = options.trailing.as_ref().map(|trailing| trailing(i));
        row_lines(
            console,
            &items.get(i),
            trailing.as_deref(),
            width,
            options.multiline,
        )
    }

    /// Lay the list out `width` x `height`: its items' lines, rendered
    /// only for the items it looks at.
    fn lay(
        &self,
        console: &Console,
        items: &Fetched,
        width: u16,
        height: u16,
        first: usize,
        selected: Option<usize>,
    ) -> Laid {
        let len = items.len();
        let (multiline, padding) = {
            let options = self.options.borrow();
            (options.multiline, options.padding)
        };
        let mut cache: HashMap<usize, Vec<Vec<Segment>>> = HashMap::new();
        let mut lines = |i: usize| -> Vec<Vec<Segment>> {
            cache
                .entry(i)
                .or_insert_with(|| self.lines_of(console, items, i, width))
                .clone()
        };
        let first = {
            let mut height_of = |i: usize| if multiline { lines(i).len() } else { 1 };
            first_in_view(
                len,
                height as usize,
                first,
                selected,
                padding,
                &mut height_of,
            )
        };
        let mut used = 0usize;
        let mut laid = Vec::new();
        for i in first..len {
            let item = lines(i);
            if used > 0 && used + item.len() > height as usize {
                break;
            }
            used += item.len();
            laid.push((i, item));
            if used >= height as usize {
                break;
            }
        }
        Laid { first, items: laid }
    }

    /// Lines all `items` take at `width`.
    fn total_lines(&self, console: &Console, items: &Fetched, width: u16) -> usize {
        if self.options.borrow().multiline {
            (0..items.len())
                .map(|i| self.lines_of(console, items, i, width).len())
                .sum()
        } else {
            items.len()
        }
    }

    /// The cells before an item's text: the marker column (with
    /// [`marker`](Node::marker)), the text mode's marker (or a carried
    /// item's `↕`), and the highlight symbol's column.
    fn indents(&self, selected: bool) -> (u16, u16, u16) {
        let carrying = self.carried.get().is_some_and(|carried| carried.get());
        let marker = if crate::a11y::text_mode() || carrying {
            2
        } else {
            0
        };
        let options = self.options.borrow();
        let symbol = match &options.symbol {
            Some(symbol) if options.spacing.keeps(selected) => crate::widget::markup_width(symbol),
            _ => 0,
        };
        let gutter = u16::from(options.marker.is_some());
        (gutter, marker, symbol)
    }
}

impl Widget for List {
    fn name(&self) -> &'static str {
        "list"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::List
    }

    fn cursor(&self) -> Option<Rect> {
        self.cursor.get()
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        match self.selection.get_untracked() {
            Some(selected) => crate::a11y::AccessState::item(selected, self.items.len()),
            None => crate::a11y::AccessState::default(),
        }
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let source = self.items.clone();
        let items = source.fetch();
        let len = items.len();
        let selected = self.selection.get_untracked().is_some();
        let (gutter, marker, symbol) = self.indents(selected);
        let indent = gutter + marker + symbol;
        match axis {
            // As wide as its widest row, with the columns before the text,
            // the trailing text and a scroll bar's column: every item is
            // looked at.
            Axis::Horizontal => {
                if len == 0 {
                    return 0;
                }
                let options = self.options.borrow();
                let widest = (0..len)
                    .map(|i| {
                        let text = natural_width(&items.get(i), options.multiline);
                        let trailing = options
                            .trailing
                            .as_ref()
                            .map(|trailing| trailing(i))
                            .filter(|t| !t.is_empty())
                            .map_or(0, |t| crate::widget::markup_width(&t).saturating_add(1));
                        text.saturating_add(trailing)
                    })
                    .max()
                    .unwrap_or(0);
                let view = if height == 0 {
                    usize::MAX
                } else {
                    height as usize
                };
                let bar = self
                    .bar
                    .look
                    .as_ref()
                    .is_some_and(|look| look.is_vertical() && look.wanted(len, view));
                let wanted = widest
                    .saturating_add(indent)
                    .saturating_add(u16::from(bar))
                    .max(1);
                if width == 0 {
                    wanted
                } else {
                    wanted.min(width)
                }
            }
            Axis::Vertical => {
                let room = width.max(1).saturating_sub(indent).max(1);
                let total = self.total_lines(cx.console(), &items, room);
                let total = if height == 0 {
                    total
                } else {
                    total.min(height as usize)
                };
                total.min(u16::MAX as usize) as u16
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let console = cx.console();
        let source = self.items.clone();
        let items = source.fetch();
        let len = items.len();
        let (width, height) = (canvas.width(), canvas.height());
        let selected = self
            .selection
            .get()
            .filter(|_| len > 0)
            .map(|s| s.min(len - 1));
        // The offset signal, set from outside, moves the view.
        let offset = self.options.borrow().offset;
        if let Some(offset) = offset {
            let value = offset.get();
            if self.seen.get() != Some(value) {
                self.first.set(value);
                self.seen.set(Some(value));
            }
        }
        let (gutter, marker, symbol) = self.indents(selected.is_some());
        let indent = gutter + marker + symbol;
        let first = self.first.get();
        // The bar, when there is one, keeps a column (or row) once the
        // items do not all fit.
        let bar = self.bar.look.clone();
        let mut area = Rect::new(0, 0, width, height);
        let text_width = |area: Rect| area.width.saturating_sub(indent).max(1);
        let mut laid = self.lay(
            console,
            &items,
            text_width(area),
            area.height,
            first,
            selected,
        );
        let mut bar_rect = None;
        if let Some(look) = &bar {
            if look.wanted(len, laid.items.len()) {
                let (rest, at) = look.split(width, height);
                area = rest;
                bar_rect = Some(at);
                laid = self.lay(
                    console,
                    &items,
                    text_width(area),
                    area.height,
                    first,
                    selected,
                );
            }
        }
        self.first.set(laid.first);
        self.rows.set(laid.items.len().max(1));
        if let Some(offset) = offset {
            if self.seen.get() != Some(laid.first) {
                self.seen.set(Some(laid.first));
                Runtime::current().untracked(|| offset.set(laid.first));
            }
        }
        let options = self.options.borrow();
        let carrying = self.carried.get().is_some_and(|carried| carried.get());
        let marked = crate::a11y::text_mode();
        let highlight = cx.style("selected", "reverse");
        let symbol_lines = options
            .symbol
            .as_ref()
            .filter(|_| symbol > 0)
            .map(|markup| {
                let line = crate::widget::markup_line(console, markup, symbol);
                let used: usize = line.iter().map(Segment::cell_length).sum();
                (line, " ".repeat((symbol as usize).saturating_sub(used)))
            });
        let blank_symbol = " ".repeat(symbol as usize);
        // The row's cells after the marker column: its style and the
        // highlight fill them.
        let row_width = area.width.saturating_sub(gutter);
        let style_of = |name: String| crate::widget::theme_style(console, &name, &name);
        let mut rows: Vec<(Option<usize>, Vec<Segment>)> = Vec::new();
        let mut cursor = None;
        for (i, lines) in &laid.items {
            let i = *i;
            let is_selected = selected == Some(i);
            let row_style = options
                .row_style
                .as_ref()
                .and_then(|style| style(i))
                .map(style_of);
            let row_highlight = is_selected.then(|| {
                options
                    .selected_style
                    .as_ref()
                    .and_then(|style| style(i))
                    .map_or_else(|| highlight.clone(), style_of)
            });
            let mark = (gutter > 0).then(|| {
                let style = options
                    .marker
                    .as_ref()
                    .and_then(|marker| marker(i))
                    .map(style_of);
                Segment::new(" ", style)
            });
            let top = rows.len();
            for (n, line) in lines.iter().enumerate() {
                let mut out = Vec::new();
                if carrying && is_selected {
                    out.push(Segment::new("↕ ", None));
                } else if marked {
                    out.push(Segment::new(if is_selected { "> " } else { "  " }, None));
                }
                if symbol > 0 {
                    match &symbol_lines {
                        Some((symbol, pad)) if is_selected && (n == 0 || options.repeat_symbol) => {
                            out.extend(symbol.iter().cloned());
                            if !pad.is_empty() {
                                out.push(Segment::new(pad.clone(), None));
                            }
                        }
                        _ => out.push(Segment::new(blank_symbol.clone(), None)),
                    }
                }
                out.extend(line.iter().cloned());
                if let Some(style) = &row_style {
                    out = under(out, style, row_width);
                }
                if let Some(style) = &row_highlight {
                    out = over(out, style, row_width);
                }
                if let Some(mark) = &mark {
                    out.insert(0, mark.clone());
                }
                rows.push((Some(i), out));
            }
            if is_selected {
                cursor = Some((top, lines.len()));
            }
        }
        // Bottom to top: the first item at the bottom, each item's own
        // lines still top to bottom.
        let reversed = options.direction == ListDirection::BottomToTop;
        if reversed {
            let mut flipped: Vec<(Option<usize>, Vec<Segment>)> = Vec::new();
            for (i, lines) in laid.items.iter().rev() {
                let start = rows.iter().position(|(item, _)| *item == Some(*i));
                let count = lines.len();
                if let Some(start) = start {
                    flipped.extend(rows[start..start + count].iter().cloned());
                }
            }
            let blank = (area.height as usize).saturating_sub(flipped.len());
            rows = std::iter::repeat_n((None, Vec::new()), blank)
                .chain(flipped)
                .collect();
            if let Some(i) = selected {
                cursor = rows
                    .iter()
                    .position(|(item, _)| *item == Some(i))
                    .map(|top| {
                        (
                            top,
                            rows.iter().filter(|(item, _)| *item == Some(i)).count(),
                        )
                    });
            }
        }
        rows.truncate(area.height as usize);
        // The cursor: the selected item's rows, after the symbol (with
        // one).
        let skip = gutter + if symbol > 0 { marker + symbol } else { 0 };
        self.cursor.set(cursor.and_then(|(top, count)| {
            (top < rows.len()).then(|| {
                Rect::new(
                    area.x + skip,
                    area.y + top as u16,
                    area.width.saturating_sub(skip),
                    count.min(rows.len() - top) as u16,
                )
            })
        }));
        // Where each item's rows are on the screen, for a popup at one.
        if let Some(rects) = &options.rects {
            let mut placed: Vec<(usize, Rect)> = Vec::new();
            for (y, (item, _)) in rows.iter().enumerate() {
                let Some(item) = *item else { continue };
                match placed.last_mut() {
                    Some((last, rect)) if *last == item => rect.height += 1,
                    _ => placed.push((
                        item,
                        Rect::new(area.x + gutter, area.y + y as u16, row_width, 1),
                    )),
                }
            }
            let shift = (cx.rect.x as i32 - cx.shift.0, cx.rect.y as i32 - cx.shift.1);
            rects.set(
                placed
                    .into_iter()
                    .map(|(item, rect)| (item, crate::node::translate(rect, shift)))
                    .collect(),
            );
        }
        let mut map: Vec<Option<usize>> = vec![None; area.y as usize];
        map.extend(rows.iter().map(|(item, _)| *item));
        *self.shown.borrow_mut() = map;
        let lines: Vec<Vec<Segment>> = rows.into_iter().map(|(_, line)| line).collect();
        match bar_rect {
            None => canvas.lines(&lines),
            Some(at) => {
                canvas.lines_at(area.x, area.y, area.width, area.height, &lines);
                let look = bar.as_ref().expect("a bar was laid out");
                let view = laid.items.len().max(1);
                look.paint(console, canvas, at, laid.first, len, view);
                self.bar_at.set(Some((at, laid.first, len, view)));
            }
        }
        if bar_rect.is_none() {
            self.bar_at.set(None);
        }
        drop(options);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let WidgetEvent::Mouse(mouse) = event else {
            return Used::No;
        };
        if let Some((rect, first, len, view)) = self.bar_at.get() {
            match self.bar.mouse(cx, mouse, rect, first, len, view) {
                Some(Some(to)) => {
                    // The view moves, and the selection with it, into view.
                    self.first.set(to);
                    if let Some(selected) = self.selection.get_untracked() {
                        let last = (to + view).saturating_sub(1);
                        let inside = selected.clamp(to, last.max(to));
                        if inside != selected {
                            self.selection.set(inside);
                        }
                    }
                    cx.redraw();
                    return Used::Yes;
                }
                Some(None) => return Used::Yes,
                None => {}
            }
        }
        // A press on an item selects it, as on a table's row; the wheel
        // moves the selection up and down the screen.
        match mouse.kind {
            MouseKind::Down(button) => {
                // A reorderable list's wrapper takes left presses: it
                // selects, and drags.
                if button == Button::Left && self.carried.get().is_some() {
                    return Used::No;
                }
                let item = self
                    .shown
                    .borrow()
                    .get(mouse.row as usize)
                    .copied()
                    .flatten();
                let Some(item) = item else {
                    return Used::No;
                };
                // The press goes on to the node's own handler (a right
                // click's menu is about the item it selected), as it did
                // before a press selected.
                self.selection.set(item);
                Used::No
            }
            MouseKind::ScrollUp | MouseKind::ScrollDown => {
                let down = if mouse.kind == MouseKind::ScrollDown {
                    WHEEL_STEP
                } else {
                    -WHEEL_STEP
                };
                let by = match self.options.borrow().direction {
                    ListDirection::TopToBottom => down,
                    ListDirection::BottomToTop => -down,
                };
                self.selection.step(self.items.len(), by);
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// A [`list`]'s options, as builders. On any other node they do nothing;
/// call them on the list itself (or a reorderable list), before
/// `.panel(..)` or anything else that wraps it.
impl Node {
    /// Draw console `markup` (`"[red]>>[/] "`) before the selected item:
    /// ratatui's `highlight_symbol`. Other items get as many blank cells
    /// while the column is kept (see [`highlight_spacing`](Self::highlight_spacing)).
    pub fn symbol(self, markup: &str) -> Node {
        self.list_options(|options| options.symbol = Some(markup.to_string()))
    }

    /// Draw the highlight symbol on every line of a
    /// [multi-line](Self::multiline) selected item, not only its first.
    pub fn repeat_symbol(self) -> Node {
        self.list_options(|options| options.repeat_symbol = true)
    }

    /// When the highlight symbol keeps its column.
    pub fn highlight_spacing(self, spacing: HighlightSpacing) -> Node {
        self.list_options(|options| options.spacing = spacing)
    }

    /// Which way the items run. Bottom to top, ↑ and ↓ still move up and
    /// down the screen, and Home and End go to the first and the last item.
    pub fn direction(self, direction: ListDirection) -> Node {
        if let Some(model) = &self.list_model {
            model.reversed.set(direction == ListDirection::BottomToTop);
        }
        self.list_options(|options| options.direction = direction)
    }

    /// Keep `items` items in view either side of the selected one, where
    /// they fit: ratatui's `scroll_padding`, Vim's `scrolloff`. On a
    /// [`virtual_list`](crate::widgets::virtual_list) or a table too.
    pub fn scroll_padding(self, items: usize) -> Node {
        self.table_options(|options| options.scroll_padding = items);
        self.list_options(|options| options.padding = items)
    }

    /// Items keep their lines and wrap to the width, each as tall as it
    /// needs, rather than showing their first line cut with `…`.
    pub fn multiline(self) -> Node {
        self.list_options(|options| options.multiline = true)
    }

    /// A style (or theme style name) for item `i`'s whole rows, under the
    /// item's own markup and the selection's: zebra stripes,
    /// `.row_style(|i| (i % 2 == 1).then(|| "on grey15".into()))`. On a
    /// [`virtual_list`](crate::widgets::virtual_list) or a table too.
    pub fn row_style(self, style: impl Fn(usize) -> Option<String> + 'static) -> Node {
        let style: Rc<dyn Fn(usize) -> Option<String>> = Rc::new(style);
        let for_table = style.clone();
        self.table_options(move |options| {
            options.row_style = Some(Box::new(move |i| for_table(i)));
        });
        self.list_options(|options| options.row_style = Some(Box::new(move |i| style(i))))
    }

    /// The first item in view, in `first`: read it, or set it to scroll.
    /// The selected item is still kept in view. On a
    /// [`virtual_list`](crate::widgets::virtual_list) or a table too.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::virtual_list;
    ///
    /// let app = App::new(|| {
    ///     let first = signal(0usize);
    ///     column([
    ///         virtual_list(|| 100, |i| format!("row {i}"), signal(10)).offset(first),
    ///         text!("first {first}").fixed(1),
    ///     ])
    /// });
    /// let mut driver = app.driver(10, 5);
    /// driver.settle();
    /// // Row 10 is selected: the view scrolled to show it at the bottom.
    /// assert_eq!(driver.screen().plain()[4].trim_end(), "first 7");
    /// ```
    pub fn offset(self, first: Signal<usize>) -> Node {
        self.table_options(|options| options.offset = Some(first));
        self.list_options(|options| options.offset = Some(first))
    }

    /// Console markup at the right of item `i`'s row, right-aligned, its
    /// text cut (or wrapped) short of it with a blank cell between: Yazi's
    /// line mode (sizes, times) beside the names, in one list. The trailing
    /// text is inside the row, so its style and the highlight cover it. An
    /// empty string shows none. Asked only for the items drawn (and, for a
    /// list sized [`Size::Auto`](crate::Size::Auto) across, for every item
    /// when it is measured).
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let names = || vec!["notes.txt".to_string(), "src".into()];
    ///     let sizes = ["12K", "4"];
    ///     list(names, signal(0)).trailing(move |i| sizes[i].to_string())
    /// });
    /// let mut driver = app.driver(14, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "notes.txt  12K");
    /// assert_eq!(driver.screen().plain()[1], "src          4");
    /// ```
    pub fn trailing(self, markup: impl Fn(usize) -> String + 'static) -> Node {
        self.list_options(|options| options.trailing = Some(Box::new(markup)))
    }

    /// A one-cell marker column at the left of the rows, before the item's
    /// text, outside its row style and the highlight: a blank cell in the
    /// style (or theme style name) `style(i)` returns for item `i`, or an
    /// unstyled one for `None`. Yazi's selected, visual-range and yanked
    /// markers. The column is kept with no item marked, so the text never
    /// moves.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let items = || vec!["a.txt".to_string(), "b.txt".into()];
    ///     list(items, signal(0)).marker(|i| (i == 1).then(|| "on yellow".into()))
    /// });
    /// let mut driver = app.driver(8, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[1].trim_end(), " b.txt");
    /// ```
    pub fn marker(self, style: impl Fn(usize) -> Option<String> + 'static) -> Node {
        self.list_options(|options| options.marker = Some(Box::new(style)))
    }

    /// The selection's style (or theme style name) when item `i` is
    /// selected, over its row style and its markup, instead of the theme's
    /// `selected`; `None` keeps the theme's. Yazi reverses the hovered row
    /// in the entry's own colour, so a hovered directory is a blue bar:
    /// `.selected_style_for(|i| Some(format!("reverse {}", colour(i))))`.
    pub fn selected_style_for(self, style: impl Fn(usize) -> Option<String> + 'static) -> Node {
        self.list_options(|options| options.selected_style = Some(Box::new(style)))
    }

    /// Record where the rows in view are drawn on the screen in `rects`, at
    /// every draw: to place a popup at a row (`cx.popup(rect, ..)`). On a
    /// [`virtual_list`](crate::widgets::virtual_list) or a table too. See
    /// [`RowRects`].
    pub fn row_rects(self, rects: &RowRects) -> Node {
        let for_table = rects.clone();
        self.widget_mut(|table: &mut crate::widgets::Table| table.rects = Some(for_table));
        self.list_options(|options| options.rects = Some(rects.clone()))
    }

    /// Run `f` on the options of a table or a
    /// [`virtual_list`](crate::widgets::virtual_list) this node is.
    fn table_options(&self, f: impl FnOnce(&mut crate::widgets::TableOptions)) {
        self.widget_mut(|table: &mut crate::widgets::Table| f(table.options_mut()));
    }

    fn list_options(self, f: impl FnOnce(&mut ListOptions)) -> Node {
        with_list(&self, |list| f(&mut list.options.borrow_mut()));
        self
    }
}
