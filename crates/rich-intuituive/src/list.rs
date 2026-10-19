//! Lists: [`list`], [`list_optional`] and [`list_lazy`], markup items with one selected
//! (or none), and their options, as builders on the node.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rich::{Console, Segment, Style};
use rich_interact::{Button, KeyCode, Modifiers, MouseKind};

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

/// Where a list's highlight goes: over the item's markup (the default,
/// ratatui's), or under it, so markup that sets its own colours (Yazi's
/// rounded ends, `[not reverse blue]`) keeps them on the selected row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HighlightLayer {
    /// Over the markup and the row style: the selected row is all in the
    /// highlight (the default).
    #[default]
    Over,
    /// Over the row style, under the markup: what the markup styles keeps
    /// its own style.
    Under,
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

/// What [`Node::on_context`] runs: item `i` was right-pressed, or had the
/// focus when the menu key was.
type OnContext = Rc<RefCell<dyn FnMut(usize, &mut crate::app::Ctx)>>;

/// A marker cell's theme style for item `i`, with the style used when the
/// theme has none: a [`MultiSelect`](crate::MultiSelect)'s markers.
pub(crate) type Marks = Rc<dyn Fn(usize) -> Option<(&'static str, &'static str)>>;

/// A list option, kept so a node that wraps a list can give it on: given
/// a node, it sets itself if the node is a list (or a table, for an
/// option tables share) and says whether it was.
pub(crate) type ListOption = Rc<dyn Fn(&Node) -> bool>;

/// The list options given a [`switch`](crate::switch), for the lists it
/// builds now and later.
#[derive(Clone, Default)]
pub(crate) struct Forward(Rc<RefCell<Vec<ListOption>>>);

impl Forward {
    /// Give every option kept to `node`, a child just built.
    pub(crate) fn apply(&self, node: &Node) {
        let options = self.0.borrow().clone();
        for option in &options {
            give(node, option);
        }
    }
}

/// Give `option` to `node`: to the list (or table) it is, to the one a
/// wrapper with one child holds (a panel, padding, a reorderable list),
/// or, on a [`switch`](crate::switch), to each list it builds. Whether a
/// list took it, or a switch kept it.
pub(crate) fn give(node: &Node, option: &ListOption) -> bool {
    if option(node) {
        return true;
    }
    if let Some(forward) = &node.list_forward {
        forward.0.borrow_mut().push(option.clone());
        let body = node.body.borrow();
        for child in body
            .widget
            .children()
            .iter()
            .chain(body.widget.hidden_children())
        {
            give(child, option);
        }
        return true;
    }
    let body = node.body.borrow();
    match (
        body.widget.children(),
        body.widget.hidden_children().is_empty(),
    ) {
        ([child], true) => give(child, option),
        _ => false,
    }
}

/// In a debug build, say once on stderr that a list option met no list:
/// it was called on a node that is not one and wraps none.
fn unused(option: &'static str) {
    #[cfg(debug_assertions)]
    {
        thread_local! {
            static SAID: RefCell<std::collections::HashSet<&'static str>> =
                RefCell::new(std::collections::HashSet::new());
        }
        if SAID.with(|said| said.borrow_mut().insert(option)) {
            eprintln!(
                "intuituive: .{option}(..) met no list: call it on a list, or on a node that \
                 wraps one (a panel, a switch)"
            );
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = option;
}

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
    /// Whether the highlight goes over the markup or under it.
    layer: HighlightLayer,
    /// Items a turn of the wheel moves the selection, when not
    /// [`WHEEL_STEP`]; 0 leaves the wheel to the list's ancestors.
    wheel: Option<usize>,
    /// For [`list_rows`]: the rows one draw made, cleared at each.
    rows: Option<Rc<RowCache>>,
    /// Where the rows in view were drawn, on the screen.
    rects: Option<RowRects>,
    /// What a right press on an item, or the menu key, runs.
    context: Option<OnContext>,
    /// Marker cells by theme style, before [`marker`](Self::marker)'s.
    pub(crate) marks: Option<Marks>,
}

/// Where a list's items come from: every item from one closure, or a count
/// and one item at a time ([`list_lazy`]).
#[derive(Clone)]
pub(crate) enum Items {
    All(Rc<dyn Fn() -> Vec<String>>),
    Lazy {
        len: Rc<dyn Fn() -> usize>,
        item: Rc<dyn Fn(usize) -> String>,
        /// What forgets the rows the last draw made ([`list_rows`]).
        reset: Option<Rc<dyn Fn()>>,
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
            Items::Lazy { len, item, reset } => {
                if let Some(reset) = reset {
                    reset();
                }
                Fetched::Lazy {
                    len: len(),
                    item: &**item,
                    cache: RefCell::new(HashMap::new()),
                }
            }
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
        reset: None,
    };
    list_of(items, Selection::One(selected))
}

/// One row of a [`list_rows`] list: its markup, and what
/// [`trailing`](Node::trailing), [`marker`](Node::marker),
/// [`row_style`](Node::row_style) and
/// [`selected_style_for`](Node::selected_style_for) would give it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListRow {
    /// The item's console markup.
    pub text: String,
    /// Markup at the right of its first line; empty for none.
    pub trailing: String,
    /// Its marker cell's style (or theme style name), with
    /// [`marker_column`](Node::marker_column); `None` for a blank cell.
    pub marker: Option<String>,
    /// Its rows' style, under the markup; `None` for none.
    pub style: Option<String>,
    /// The highlight's style when it is selected; `None` for the theme's.
    pub selected_style: Option<String>,
}

impl ListRow {
    /// A row of console `markup`, with nothing else.
    pub fn new(markup: impl Into<String>) -> ListRow {
        ListRow {
            text: markup.into(),
            ..ListRow::default()
        }
    }

    /// Markup at the right of the row.
    pub fn trailing(mut self, markup: impl Into<String>) -> ListRow {
        self.trailing = markup.into();
        self
    }

    /// The marker cell's style.
    pub fn marker(mut self, style: Option<String>) -> ListRow {
        self.marker = style;
        self
    }

    /// The row's style.
    pub fn style(mut self, style: Option<String>) -> ListRow {
        self.style = style;
        self
    }

    /// The highlight's style when the row is selected.
    pub fn selected_style(mut self, style: Option<String>) -> ListRow {
        self.selected_style = style;
        self
    }
}

/// The rows a [`list_rows`] list made in one draw (or measure): each made
/// once, however many of its parts are asked for.
pub(crate) struct RowCache {
    row: Box<dyn Fn(usize) -> ListRow>,
    made: RefCell<HashMap<usize, ListRow>>,
}

impl RowCache {
    /// Row `i`'s part `part`, the row made if this draw has not yet.
    fn get<R>(&self, i: usize, part: impl FnOnce(&ListRow) -> R) -> R {
        if let Some(row) = self.made.borrow().get(&i) {
            return part(row);
        }
        let row = (self.row)(i);
        let out = part(&row);
        self.made.borrow_mut().insert(i, row);
        out
    }
}

/// A [`list_lazy`] whose items come from one closure: `row(i)` gives item
/// `i`'s markup with its trailing text, marker, row style and highlight
/// style as one [`ListRow`], made once a draw, where `list_lazy`'s
/// closures (and [`trailing`](Node::trailing), [`marker`](Node::marker),
/// [`row_style`](Node::row_style)) each read the app's data again. Only the
/// rows drawn are made. [`marker_column`](Node::marker_column) keeps the
/// marker column.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{list_rows, ListRow};
///
/// let app = App::new(|| {
///     let files = signal(vec![("notes.txt", 12), ("src", 4)]);
///     list_rows(
///         move || files.with(Vec::len),
///         move |i| {
///             let (name, size) = files.with(|f| f[i]);
///             ListRow::new(name)
///                 .trailing(size.to_string())
///                 .marker((i == 1).then(|| "on yellow".into()))
///         },
///         signal(0),
///     )
///     .marker_column()
/// });
/// let mut driver = app.driver(14, 2);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0], " notes.txt  12");
/// assert_eq!(driver.screen().plain()[1], " src         4");
/// ```
pub fn list_rows(
    len: impl Fn() -> usize + 'static,
    row: impl Fn(usize) -> ListRow + 'static,
    selected: Signal<usize>,
) -> Node {
    let rows = Rc::new(RowCache {
        row: Box::new(row),
        made: RefCell::new(HashMap::new()),
    });
    let (text, reset) = (rows.clone(), rows.clone());
    let items = Items::Lazy {
        len: Rc::new(len),
        item: Rc::new(move |i| text.get(i, |row| row.text.clone())),
        reset: Some(Rc::new(move || reset.made.borrow_mut().clear())),
    };
    let (trailing, style, selected_style) = (rows.clone(), rows.clone(), rows.clone());
    list_of(items, Selection::One(selected))
        .trailing(move |i| trailing.get(i, |row| row.trailing.clone()))
        .row_style(move |i| style.get(i, |row| row.style.clone()))
        .selected_style_for(move |i| selected_style.get(i, |row| row.selected_style.clone()))
        .list_options("list_rows", move |options| {
            options.rows = Some(rows.clone())
        })
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
        let gutter = u16::from(options.marker.is_some() || options.marks.is_some());
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
            let mark =
                (gutter > 0).then(|| {
                    let marked = options.marks.as_ref().and_then(|marks| marks(i)).map(
                        |(name, fallback)| crate::widget::theme_style(console, name, fallback),
                    );
                    let style = marked.or_else(|| {
                        options
                            .marker
                            .as_ref()
                            .and_then(|marker| marker(i))
                            .map(style_of)
                    });
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
                match (&row_style, &row_highlight, options.layer) {
                    // Under the markup: the row style, then the highlight
                    // over it, as one style under the item's own.
                    (row_style, Some(highlight), HighlightLayer::Under) => {
                        let style = match row_style {
                            Some(row) => row.combine(highlight),
                            None => highlight.clone(),
                        };
                        out = under(out, &style, row_width);
                    }
                    (row_style, highlight, _) => {
                        if let Some(style) = row_style {
                            out = under(out, style, row_width);
                        }
                        if let Some(style) = highlight {
                            out = over(out, style, row_width);
                        }
                    }
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
        let context = self.options.borrow().context.clone();
        let mouse = match event {
            WidgetEvent::Mouse(mouse) => mouse,
            // The menu key (Shift+F10) on the selected item.
            WidgetEvent::Key(key) => {
                let (Some(context), Some(at)) = (context, self.selection.get_untracked()) else {
                    return Used::No;
                };
                if *key
                    != rich_interact::Key::with(
                        KeyCode::F(10),
                        Modifiers {
                            shift: true,
                            ..Modifiers::NONE
                        },
                    )
                    || at >= self.items.len()
                {
                    return Used::No;
                }
                (context.borrow_mut())(at, cx.app());
                return Used::Yes;
            }
            _ => return Used::No,
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
                // before a press selected; unless it is a right press and
                // the list has a context handler.
                self.selection.set(item);
                match context {
                    Some(context) if button == Button::Right => {
                        (context.borrow_mut())(item, cx.app());
                        Used::Yes
                    }
                    _ => Used::No,
                }
            }
            MouseKind::ScrollUp | MouseKind::ScrollDown => {
                let step = self
                    .options
                    .borrow()
                    .wheel
                    .map_or(WHEEL_STEP, |step| step.min(isize::MAX as usize) as isize);
                if step == 0 {
                    return Used::No;
                }
                let down = if mouse.kind == MouseKind::ScrollDown {
                    step
                } else {
                    -step
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

/// A [`list`]'s options, as builders. On a node that wraps a list (a
/// panel, padding, a reorderable list: a node with one child; or a
/// [`switch`](crate::switch), whose lists get them as they are built) they
/// reach the list inside. On any other node they do nothing, and a debug
/// build says so on stderr, once for each option.
impl Node {
    /// Draw console `markup` (`"[red]>>[/] "`) before the selected item:
    /// ratatui's `highlight_symbol`. Other items get as many blank cells
    /// while the column is kept (see [`highlight_spacing`](Self::highlight_spacing)).
    pub fn symbol(self, markup: &str) -> Node {
        let markup = markup.to_string();
        self.list_options("symbol", move |options| {
            options.symbol = Some(markup.clone())
        })
    }

    /// Draw the highlight symbol on every line of a
    /// [multi-line](Self::multiline) selected item, not only its first.
    pub fn repeat_symbol(self) -> Node {
        self.list_options("repeat_symbol", |options| options.repeat_symbol = true)
    }

    /// When the highlight symbol keeps its column.
    pub fn highlight_spacing(self, spacing: HighlightSpacing) -> Node {
        self.list_options("highlight_spacing", move |options| {
            options.spacing = spacing
        })
    }

    /// Which way the items run. Bottom to top, ↑ and ↓ still move up and
    /// down the screen, and Home and End go to the first and the last item.
    pub fn direction(self, direction: ListDirection) -> Node {
        self.list_option("direction", move |node| {
            if let Some(model) = &node.list_model {
                model.reversed.set(direction == ListDirection::BottomToTop);
            }
            set_list(node, |options| options.direction = direction)
        })
    }

    /// Keep `items` items in view either side of the selected one, where
    /// they fit: ratatui's `scroll_padding`, Vim's `scrolloff`. On a
    /// [`virtual_list`](crate::widgets::virtual_list) or a table too.
    pub fn scroll_padding(self, items: usize) -> Node {
        self.list_option("scroll_padding", move |node| {
            let table = table_options(node, |options| options.scroll_padding = items);
            set_list(node, |options| options.padding = items) || table
        })
    }

    /// Items keep their lines and wrap to the width, each as tall as it
    /// needs, rather than showing their first line cut with `…`.
    pub fn multiline(self) -> Node {
        self.list_options("multiline", |options| options.multiline = true)
    }

    /// A style (or theme style name) for item `i`'s whole rows, under the
    /// item's own markup and the selection's: zebra stripes,
    /// `.row_style(|i| (i % 2 == 1).then(|| "on grey15".into()))`. On a
    /// [`virtual_list`](crate::widgets::virtual_list) or a table too.
    pub fn row_style(self, style: impl Fn(usize) -> Option<String> + 'static) -> Node {
        let style: Rc<dyn Fn(usize) -> Option<String>> = Rc::new(style);
        self.list_option("row_style", move |node| {
            let table = table_options(node, |options| options.row_style = Some(boxed(&style)));
            set_list(node, |options| options.row_style = Some(boxed(&style))) || table
        })
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
        self.list_option("offset", move |node| {
            let table = table_options(node, |options| options.offset = Some(first));
            set_list(node, |options| options.offset = Some(first)) || table
        })
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
        let markup = Rc::new(markup);
        self.list_options("trailing", move |options| {
            let markup = markup.clone();
            options.trailing = Some(Box::new(move |i| markup(i)));
        })
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
        let style: Rc<dyn Fn(usize) -> Option<String>> = Rc::new(style);
        self.list_options("marker", move |options| {
            options.marker = Some(boxed(&style))
        })
    }

    /// The selection's style (or theme style name) when item `i` is
    /// selected, over its row style and its markup, instead of the theme's
    /// `selected`; `None` keeps the theme's. For a highlight that differs
    /// from row to row in more than colour: Yazi's preview underlines its
    /// hovered row, `.selected_style_for(|_| Some("underline".into()))`.
    ///
    /// A bar in each row's own colour needs no closure: a
    /// [`row_style`](Self::row_style) of the colour and a `reverse`
    /// `selected` (`.theme_styles([("selected", "reverse")])`) reverse it
    /// already, a hovered directory a blue bar.
    pub fn selected_style_for(self, style: impl Fn(usize) -> Option<String> + 'static) -> Node {
        let style: Rc<dyn Fn(usize) -> Option<String>> = Rc::new(style);
        self.list_options("selected_style_for", move |options| {
            options.selected_style = Some(boxed(&style))
        })
    }

    /// Where the highlight goes: [`HighlightLayer::Over`] the item's markup
    /// (the default) or [`HighlightLayer::Under`] it, so a selected row's
    /// markup keeps the colours it sets. Yazi's hovered row has rounded
    /// ends in the entry's colour, outside the reversed bar:
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::HighlightLayer;
    ///
    /// let app = App::new(|| {
    ///     let items = || vec!["[not reverse blue]<[/]docs[not reverse blue]>".to_string()];
    ///     list(items, signal(0))
    ///         .row_style(|_| Some("blue".into()))
    ///         .highlight_layer(HighlightLayer::Under)
    /// });
    /// let mut driver = app.driver(8, 1);
    /// driver.settle();
    /// // The ends keep their own colour, not reversed; the name is.
    /// let styled = intuituive::testing::styled(&driver);
    /// assert!(styled.starts_with("[not reverse blue]<[/][reverse blue]docs[/][not reverse blue]>"));
    /// ```
    pub fn highlight_layer(self, layer: HighlightLayer) -> Node {
        self.list_options("highlight_layer", move |options| options.layer = layer)
    }

    /// Move the selection `items` items a turn of the mouse wheel, instead
    /// of three. `0` leaves the wheel alone: it goes on to the list's
    /// ancestors, whose [`on_mouse`](Self::on_mouse) sees it (a list
    /// otherwise uses every turn, and nothing above it hears the wheel).
    ///
    /// ```
    /// use intuituive::interact::{Event, Mouse, MouseKind};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let turns = signal(0);
    ///     let items = || (0..9).map(|i| i.to_string()).collect();
    ///     column([list(items, signal(0)).wheel_step(0).fixed(3), text!("{turns}")])
    ///         .on_mouse(move |_, mouse| {
    ///             turns.update(|t| *t += 1);
    ///             mouse.kind == MouseKind::ScrollDown
    ///         })
    /// });
    /// let mut driver = app.driver(5, 4);
    /// driver.settle();
    /// driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 0, 0)));
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[3].trim_end(), "1");
    /// ```
    pub fn wheel_step(self, items: usize) -> Node {
        self.list_options("wheel_step", move |options| options.wheel = Some(items))
    }

    /// On a [`list_rows`] list: keep the one-cell marker column of
    /// [`marker`](Self::marker), each cell in its row's
    /// [`ListRow::marker`] style. On any other list it does nothing.
    pub fn marker_column(self) -> Node {
        self.list_options("marker_column", |options| {
            if let Some(rows) = options.rows.clone() {
                options.marker = Some(Box::new(move |i| rows.get(i, |row| row.marker.clone())));
            }
        })
    }

    /// Record where the rows in view are drawn on the screen in `rects`, at
    /// every draw: to place a popup at a row (`cx.popup(rect, ..)`). On a
    /// [`virtual_list`](crate::widgets::virtual_list) or a table too. See
    /// [`RowRects`].
    pub fn row_rects(self, rects: &RowRects) -> Node {
        let rects = rects.clone();
        self.list_option("row_rects", move |node| {
            let table = node
                .widget_mut(|table: &mut crate::widgets::Table| table.rects = Some(rects.clone()))
                .is_some();
            set_list(node, |options| options.rects = Some(rects.clone())) || table
        })
    }

    /// Run `handler` with an item's index when it is pressed with the
    /// right button (it is selected first) or, while the list has the
    /// focus, when the menu key (Shift+F10) is pressed on the selected
    /// item: a context menu on a row, opened with `cx.pointer()` (`None`
    /// from the keyboard) or the row's [`row_rects`](Self::row_rects).
    /// The right press goes no further.
    ///
    /// ```
    /// use intuituive::interact::{Button, Event, Mouse, MouseKind};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let menu = signal(String::from("none"));
    ///     let items = || vec!["alpha".to_string(), "beta".into()];
    ///     column([
    ///         list(items, signal(0))
    ///             .on_context(move |i, _| menu.set(format!("menu on {i}")))
    ///             .fixed(2),
    ///         text!("{menu}"),
    ///     ])
    /// });
    /// let mut driver = app.driver(12, 3);
    /// driver.settle();
    /// driver.event(Event::Mouse(Mouse::new(MouseKind::Down(Button::Right), 0, 1)));
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[2].trim_end(), "menu on 1");
    /// ```
    pub fn on_context(self, handler: impl FnMut(usize, &mut crate::app::Ctx) + 'static) -> Node {
        let handler: OnContext = Rc::new(RefCell::new(handler));
        self.list_options("on_context", move |options| {
            options.context = Some(handler.clone())
        })
    }

    /// Keep the cursor on the same item when the items change (a re-sort,
    /// a filter, items read again): `key(i)` names item `i` (a file's
    /// name, a row's id), read from the app's data as
    /// [`trailing`](Self::trailing) is. When the item the cursor was on
    /// moves, the selection moves with it; when it is gone, the cursor
    /// stays at its index and keeps the item now there (while the list is
    /// empty it waits for the old one). Moving the cursor is unchanged.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let names = signal(vec!["cherry", "apple", "banana"]);
    ///     let cursor = signal(0usize);
    ///     list(move || names.get().iter().map(|n| n.to_string()).collect(), cursor)
    ///         .cursor_by(move |i| names.with(|n| n.get(i).copied()))
    ///         .on_key("s", move |_| names.update(|n| n.sort()))
    /// });
    /// let mut driver = app.driver(10, 3);
    /// driver.settle();
    /// driver.press("s");
    /// // `cherry` was hovered at the top; sorted, it is last, still hovered.
    /// assert_eq!(driver.screen().plain()[2].trim_end(), "cherry");
    /// assert!(intuituive::testing::styled(&driver).contains("[reverse]cherry"));
    /// ```
    pub fn cursor_by<K: PartialEq + Clone + 'static>(
        self,
        key: impl Fn(usize) -> K + 'static,
    ) -> Node {
        let key: Rc<dyn Fn(usize) -> K> = Rc::new(key);
        self.list_option("cursor_by", move |node| {
            with_list(node, |list| (list.selection, list.items.clone()))
                .map(|(selection, items)| keep_by_key(selection, items, key.clone()))
                .is_some()
        })
    }

    /// Give `option` to the list (or table) this node is or wraps; in a
    /// debug build, say so once if it met none.
    pub(crate) fn list_option(
        self,
        name: &'static str,
        option: impl Fn(&Node) -> bool + 'static,
    ) -> Node {
        let option: ListOption = Rc::new(option);
        if !give(&self, &option) {
            unused(name);
        }
        self
    }

    /// Set the options of the list this node is or wraps with `f`.
    pub(crate) fn list_options(
        self,
        name: &'static str,
        f: impl Fn(&mut ListOptions) + 'static,
    ) -> Node {
        self.list_option(name, move |node| set_list(node, &f))
    }
}

/// Run `f` on the options of a table or a
/// [`virtual_list`](crate::widgets::virtual_list) `node` is; whether it is
/// one.
fn table_options(node: &Node, f: impl FnOnce(&mut crate::widgets::TableOptions)) -> bool {
    node.widget_mut(|table: &mut crate::widgets::Table| f(table.options_mut()))
        .is_some()
}

/// Run `f` on the options of the list `node` is; whether it is one.
fn set_list(node: &Node, f: impl FnOnce(&mut ListOptions)) -> bool {
    with_list(node, |list| f(&mut list.options.borrow_mut())).is_some()
}

/// Keep `selection` on the item `key` names across changes to `items`:
/// [`Node::cursor_by`]. The item last under the cursor is remembered with
/// its index; when the key now at that index differs, the items changed
/// under it, and the selection goes to wherever its key is now.
fn keep_by_key<K: PartialEq + Clone + 'static>(
    selection: Selection,
    items: Items,
    key: Rc<dyn Fn(usize) -> K>,
) {
    let last: Rc<RefCell<Option<(usize, K)>>> = Rc::new(RefCell::new(None));
    let (seen, read, len) = (last.clone(), key.clone(), items.clone());
    crate::reactive::watch(
        move || {
            let len = items.len();
            let at = selection.get().filter(|at| *at < len);
            let was = seen.borrow().as_ref().map(|(i, _)| *i);
            (
                at,
                at.map(|at| read(at)),
                was.filter(|was| *was < len).map(|was| read(was)),
            )
        },
        move |(at, now, there), _| {
            let was = last.borrow().clone();
            if let Some((_, was)) = was.filter(|(_, was)| there.as_ref() != Some(was)) {
                // The items changed under the remembered item: find it.
                if let Some(to) = (0..len.len()).find(|i| key(*i) == was) {
                    *last.borrow_mut() = Some((to, was));
                    if at != Some(to) {
                        selection.set(to);
                    }
                    return;
                }
            }
            // A move, or the item is gone: remember what is hovered now
            // (nothing hovered keeps the item, for when it comes back).
            if let (Some(at), Some(now)) = (at, now) {
                *last.borrow_mut() = Some((at, now));
            }
        },
    );
}

/// A shared row style as the box an option keeps.
fn boxed(style: &Rc<dyn Fn(usize) -> Option<String>>) -> RowStyle {
    let style = style.clone();
    Box::new(move |i| style(i))
}
