//! The ready-made widgets from Python: tables, virtual lists, tabs, trees,
//! the calendar, split panes, and menus.

use std::cell::RefCell;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyString, PyTuple};

use rich_intuituive as intuituive;
use rich_intuituive::menu::{Menu as CoreMenu, MenuItem as CoreMenuItem};
use rich_intuituive::widgets::{self as core, LazyItem as CoreLazy, TreeItem as CoreTreeItem};
use rich_intuituive::Signal;

use super::node::{string_source, strings, take_node, PyNode};
use super::reactive::{call_with_ctx, date_arg, signal_arg, PySignal};
use super::{axis_arg, placement_arg, size_arg, Callback, PySize};

// ---------------------------------------------------------------------------
// Tables

/// `Column(title, size)`: a table column, sized like a child in a row
/// (`Size.Auto` fits its title and the cells in view).
#[pyclass(name = "Column", module = "rs_rich.tui", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct PyColumn(core::Column);

#[pymethods]
impl PyColumn {
    #[new]
    #[pyo3(signature = (title, size=None))]
    fn new(title: &str, size: Option<&Bound<'_, PyAny>>) -> PyResult<PyColumn> {
        let size = match size {
            Some(size) => size_arg(size)?,
            None => intuituive::Size::Flex(1),
        };
        Ok(PyColumn(core::Column::new(title, size)))
    }

    #[getter]
    fn title(&self) -> &str {
        &self.0.title
    }

    #[getter]
    fn size(&self) -> PySize {
        PySize(self.0.size)
    }

    fn __repr__(&self) -> String {
        format!(
            "Column({:?}, {})",
            self.0.title,
            super::size_repr(self.0.size)
        )
    }
}

fn columns(value: &Bound<'_, PyAny>) -> PyResult<Vec<core::Column>> {
    if value.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "columns must be an iterable of Column",
        ));
    }
    value
        .try_iter()?
        .map(|item| {
            let item = item?;
            if let Ok(column) = item.cast::<PyColumn>() {
                return Ok(column.get().0.clone());
            }
            if let Ok(title) = item.cast::<PyString>() {
                return Ok(core::Column::new(
                    &title.to_cow()?,
                    intuituive::Size::Flex(1),
                ));
            }
            let (title, size): (String, Bound<'_, PyAny>) = item.extract().map_err(|_| {
                PyTypeError::new_err("a column is a Column, a title, or (title, size)")
            })?;
            Ok(core::Column::new(&title, size_arg(&size)?))
        })
        .collect()
}

fn rows_of(value: &Bound<'_, PyAny>) -> PyResult<Vec<Vec<String>>> {
    value.try_iter()?.map(|row| strings(&row?)).collect()
}

fn rows_source(value: &Bound<'_, PyAny>) -> PyResult<impl Fn() -> Vec<Vec<String>> + 'static> {
    enum Source {
        Fixed(Vec<Vec<String>>),
        Call(Callback),
    }
    let source = if value.is_callable() {
        Source::Call(Callback::checked(value, "the table's rows")?)
    } else {
        Source::Fixed(rows_of(value)?)
    };
    Ok(move || match &source {
        Source::Fixed(rows) => rows.clone(),
        Source::Call(f) => f
            .call(|py| Ok(PyTuple::empty(py)), rows_of)
            .unwrap_or_default(),
    })
}

/// A count from `len()`.
fn len_source(value: &Bound<'_, PyAny>) -> PyResult<impl Fn() -> usize + 'static> {
    let len = Callback::checked(value, "len")?;
    Ok(move || {
        len.call(|py| Ok(PyTuple::empty(py)), |n| n.extract::<usize>())
            .unwrap_or(0)
    })
}

fn selected(value: &Bound<'_, PyAny>, what: &str) -> PyResult<Signal<usize>> {
    signal_arg(value, what)?.borrow().usize(value.py(), what)
}

/// `TableOptions()`: what `table_with` adds to a table, set by chaining:
/// `.sort(signal)` (the rows function sorts by it), `.sort_rows(signal)`
/// (the table sorts plain rows), `.cells(signal)` (a cell cursor's column)
/// and `.resizable()`. A sort signal holds `None` or `(column, Order)`.
#[pyclass(name = "TableOptions", module = "rs_rich.tui", unsendable)]
#[derive(Default)]
pub(crate) struct PyTableOptions {
    sort: RefCell<Option<(Py<PySignal>, bool)>>,
    cells: RefCell<Option<Py<PySignal>>>,
    resizable: std::cell::Cell<bool>,
}

impl PyTableOptions {
    fn build(&self, py: Python<'_>) -> PyResult<core::TableOptions> {
        let mut options = core::TableOptions::default();
        if let Some((sort, own)) = &*self.sort.borrow() {
            let sort = sort.borrow(py).sort(py, "a table's sort")?;
            options = if *own {
                options.sort_rows(sort)
            } else {
                options.sort(sort)
            };
        }
        if let Some(cells) = &*self.cells.borrow() {
            options = options.cells(cells.borrow(py).usize(py, "a table's cell cursor")?);
        }
        if self.resizable.get() {
            options = options.resizable();
        }
        Ok(options)
    }
}

#[pymethods]
impl PyTableOptions {
    #[new]
    fn new() -> PyTableOptions {
        PyTableOptions::default()
    }

    /// Sorting by `sort`, which the app's rows function reads.
    fn sort<'py>(slf: Bound<'py, Self>, sort: &Bound<'py, PyAny>) -> PyResult<Bound<'py, Self>> {
        let sort = signal_arg(sort, "sort")?.unbind();
        *slf.borrow().sort.borrow_mut() = Some((sort, false));
        Ok(slf)
    }

    /// Sorting by `sort`, done by the table on plain rows.
    fn sort_rows<'py>(
        slf: Bound<'py, Self>,
        sort: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let sort = signal_arg(sort, "sort")?.unbind();
        *slf.borrow().sort.borrow_mut() = Some((sort, true));
        Ok(slf)
    }

    /// A cell cursor, its column in `column`.
    fn cells<'py>(slf: Bound<'py, Self>, column: &Bound<'py, PyAny>) -> PyResult<Bound<'py, Self>> {
        let column = signal_arg(column, "cells")?.unbind();
        *slf.borrow().cells.borrow_mut() = Some(column);
        Ok(slf)
    }

    /// Columns the mouse resizes.
    fn resizable(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf.borrow().resizable.set(true);
        slf
    }
}

/// `table(columns, rows, selected)`: markup cells under a header that stays
/// in view, with one row selected (its index in the signal `selected`).
/// `rows` is a function returning the rows (lists of str), or the rows.
#[pyfunction]
fn tui_table(
    columns: &Bound<'_, PyAny>,
    rows: &Bound<'_, PyAny>,
    selected: &Bound<'_, PyAny>,
) -> PyResult<PyNode> {
    let columns = self::columns(columns)?;
    let rows = rows_source(rows)?;
    let selected = self::selected(selected, "a table's selection")?;
    Ok(PyNode::new(core::table(columns, rows, selected)))
}

/// `table_with(columns, rows, selected, options)`: a `table` with sorting,
/// a cell cursor or resizable columns.
#[pyfunction]
fn tui_table_with(
    columns: &Bound<'_, PyAny>,
    rows: &Bound<'_, PyAny>,
    selected: &Bound<'_, PyAny>,
    options: &Bound<'_, PyTableOptions>,
) -> PyResult<PyNode> {
    let py = columns.py();
    let columns = self::columns(columns)?;
    let rows = rows_source(rows)?;
    let selected = self::selected(selected, "a table's selection")?;
    let options = options.borrow().build(py)?;
    Ok(PyNode::new(core::table_with(
        columns, rows, selected, options,
    )))
}

/// `virtual_table(columns, len, row, selected)`: a table that asks only for
/// the rows in view: `len()` is how many, `row(i)` makes one.
#[pyfunction]
fn tui_virtual_table(
    columns: &Bound<'_, PyAny>,
    len: &Bound<'_, PyAny>,
    row: &Bound<'_, PyAny>,
    selected: &Bound<'_, PyAny>,
) -> PyResult<PyNode> {
    let columns = self::columns(columns)?;
    let len = len_source(len)?;
    let row = Callback::checked(row, "virtual_table's row")?;
    let selected = self::selected(selected, "a table's selection")?;
    Ok(PyNode::new(core::virtual_table(
        columns,
        len,
        move |i| {
            row.call(|py| PyTuple::new(py, [i]), strings)
                .unwrap_or_default()
        },
        selected,
    )))
}

/// `virtual_list(len, row, selected)`: a list that asks only for the rows
/// in view: `len()` is how many, `row(i)` makes one (markup).
#[pyfunction]
fn tui_virtual_list(
    len: &Bound<'_, PyAny>,
    row: &Bound<'_, PyAny>,
    selected: &Bound<'_, PyAny>,
) -> PyResult<PyNode> {
    let len = len_source(len)?;
    let row = Callback::checked(row, "virtual_list's row")?;
    let selected = self::selected(selected, "a list's selection")?;
    Ok(PyNode::new(core::virtual_list(
        len,
        move |i| {
            row.call(
                |py| PyTuple::new(py, [i]),
                |value| Ok(value.str()?.to_cow()?.into_owned()),
            )
            .unwrap_or_default()
        },
        selected,
    )))
}

/// `tabs(titles, selected)`: a strip of titles; ←/→, 1–9 and clicks move
/// the selection.
#[pyfunction]
fn tui_tabs(titles: &Bound<'_, PyAny>, selected: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let titles = string_source(titles, "tabs' titles")?;
    let selected = self::selected(selected, "the tabs' selection")?;
    Ok(PyNode::new(core::tabs(titles, selected)))
}

// ---------------------------------------------------------------------------
// Trees

/// `TreeItem(label, children=())`: an item of a `tree`, a line of markup
/// and the items under it. `child(item)` and `children(items)` return the
/// item with more under it; `items` reads them.
#[pyclass(name = "TreeItem", module = "rs_rich.tui", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct PyTreeItem(CoreTreeItem);

fn tree_items(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreTreeItem>> {
    if value.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "items must be an iterable of TreeItem",
        ));
    }
    value
        .try_iter()?
        .map(|item| {
            let item = item?;
            if let Ok(item) = item.cast::<PyTreeItem>() {
                return Ok(item.get().0.clone());
            }
            if let Ok(label) = item.cast::<PyString>() {
                return Ok(CoreTreeItem::new(label.to_cow()?.into_owned()));
            }
            Err(PyTypeError::new_err(format!(
                "a tree item is a TreeItem or a str, got {}",
                item.get_type().name()?
            )))
        })
        .collect()
}

#[pymethods]
impl PyTreeItem {
    #[new]
    #[pyo3(signature = (label, children=None))]
    fn new(label: &str, children: Option<&Bound<'_, PyAny>>) -> PyResult<PyTreeItem> {
        let mut item = CoreTreeItem::new(label);
        if let Some(children) = children {
            item = item.children(tree_items(children)?);
        }
        Ok(PyTreeItem(item))
    }

    #[getter]
    fn label(&self) -> &str {
        &self.0.label
    }

    /// The items under it.
    #[getter]
    fn items(&self) -> Vec<PyTreeItem> {
        self.0.children.iter().cloned().map(PyTreeItem).collect()
    }

    /// The item with `child` added under it.
    fn child(&self, child: &Bound<'_, PyAny>) -> PyResult<PyTreeItem> {
        let added = tree_items(PyTuple::new(child.py(), [child])?.as_any())?;
        Ok(PyTreeItem(self.0.clone().children(added)))
    }

    /// The item with `children` added under it.
    fn children(&self, children: &Bound<'_, PyAny>) -> PyResult<PyTreeItem> {
        Ok(PyTreeItem(self.0.clone().children(tree_items(children)?)))
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .cast::<PyTreeItem>()
            .is_ok_and(|other| other.get().0 == self.0)
    }

    fn __repr__(&self) -> String {
        format!(
            "TreeItem({:?}, {} children)",
            self.0.label,
            self.0.children.len()
        )
    }
}

fn items_source(value: &Bound<'_, PyAny>) -> PyResult<impl Fn() -> Vec<CoreTreeItem> + 'static> {
    enum Source {
        Fixed(Vec<CoreTreeItem>),
        Call(Callback),
    }
    let source = if value.is_callable() {
        Source::Call(Callback::checked(value, "the tree's items")?)
    } else {
        Source::Fixed(tree_items(value)?)
    };
    Ok(move || match &source {
        Source::Fixed(items) => items.clone(),
        Source::Call(f) => f
            .call(|py| Ok(PyTuple::empty(py)), tree_items)
            .unwrap_or_default(),
    })
}

/// `tree(items, selected)`: nested `TreeItem`s from `items()` (or a fixed
/// list), the selected item's path (a list of indices) in `selected`.
#[pyfunction]
fn tui_tree(items: &Bound<'_, PyAny>, selected: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let py = items.py();
    let items = items_source(items)?;
    let selected = signal_arg(selected, "tree's selected")?
        .borrow()
        .path(py, "a tree's selection")?;
    Ok(PyNode::new(core::tree(items, selected)))
}

/// `tree_with(items, selected, expanded)`: a `tree` whose expanded items'
/// paths (a set of tuples) are in the signal `expanded`.
#[pyfunction]
fn tui_tree_with(
    items: &Bound<'_, PyAny>,
    selected: &Bound<'_, PyAny>,
    expanded: &Bound<'_, PyAny>,
) -> PyResult<PyNode> {
    let py = items.py();
    let items = items_source(items)?;
    let selected = signal_arg(selected, "tree's selected")?
        .borrow()
        .path(py, "a tree's selection")?;
    let expanded = signal_arg(expanded, "tree's expanded")?
        .borrow()
        .paths(py, "a tree's expanded items")?;
    Ok(PyNode::new(core::tree_with(items, selected, expanded)))
}

/// `LazyItem.leaf(key, label)` and `LazyItem.branch(key, label)`: an item
/// of a `tree_lazy`, with the key its children are loaded by.
#[pyclass(name = "LazyItem", module = "rs_rich.tui", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct PyLazyItem(CoreLazy);

#[pymethods]
impl PyLazyItem {
    /// An item with no children.
    #[staticmethod]
    fn leaf(key: &str, label: &str) -> PyLazyItem {
        PyLazyItem(CoreLazy::leaf(key, label))
    }

    /// An item whose children are loaded when it is first opened.
    #[staticmethod]
    fn branch(key: &str, label: &str) -> PyLazyItem {
        PyLazyItem(CoreLazy::branch(key, label))
    }

    #[getter]
    fn key(&self) -> &str {
        &self.0.key
    }

    #[getter]
    fn label(&self) -> &str {
        &self.0.label
    }

    #[getter]
    fn has_children(&self) -> bool {
        self.0.has_children
    }

    fn __repr__(&self) -> String {
        let kind = if self.0.has_children {
            "branch"
        } else {
            "leaf"
        };
        format!("LazyItem.{kind}({:?}, {:?})", self.0.key, self.0.label)
    }
}

fn lazy_items(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreLazy>> {
    value
        .try_iter()?
        .map(|item| {
            let item = item?;
            item.cast::<PyLazyItem>()
                .map(|item| item.get().0.clone())
                .map_err(|_| {
                    PyTypeError::new_err("expected LazyItem.leaf(...) or LazyItem.branch(...)")
                })
        })
        .collect()
}

/// `tree_lazy(roots, children, selected)`: a tree whose levels load when
/// opened: `children(key)` runs on a worker thread and returns the level's
/// `LazyItem`s; an exception it raises shows in the tree, in red. The
/// selected item's key is in `selected`.
#[pyfunction]
fn tui_tree_lazy(
    roots: &Bound<'_, PyAny>,
    children: &Bound<'_, PyAny>,
    selected: &Bound<'_, PyAny>,
) -> PyResult<PyNode> {
    let py = roots.py();
    let roots = Callback::checked(roots, "tree_lazy's roots")?;
    let children = Callback::checked(children, "tree_lazy's children")?
        .func()
        .clone_ref(py);
    let selected = signal_arg(selected, "tree's selected")?
        .borrow()
        .key(py, "a lazy tree's selection")?;
    Ok(PyNode::new(core::tree_lazy(
        move || {
            roots
                .call(|py| Ok(PyTuple::empty(py)), lazy_items)
                .unwrap_or_default()
        },
        move |key: String| -> Result<Vec<CoreLazy>, String> {
            Python::attach(|py| {
                children
                    .bind(py)
                    .call1((key,))
                    .and_then(|items| lazy_items(&items))
                    .map_err(|error| {
                        error
                            .value(py)
                            .str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|_| "failed".to_string())
                    })
            })
        },
        selected,
    )))
}

// ---------------------------------------------------------------------------
// The calendar and split panes

/// `calendar(selected)`: a month grid of the `datetime.date` in `selected`.
#[pyfunction]
fn tui_calendar(selected: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let selected = signal_arg(selected, "calendar's selected")?
        .borrow()
        .date(selected.py(), "a calendar's day")?;
    Ok(PyNode::new(core::calendar(selected)))
}

/// `calendar_with(selected, today=None)`: a `calendar` that marks `today`.
#[pyfunction]
#[pyo3(signature = (selected, today=None))]
fn tui_calendar_with(
    selected: &Bound<'_, PyAny>,
    today: Option<&Bound<'_, PyAny>>,
) -> PyResult<PyNode> {
    let selected = signal_arg(selected, "calendar's selected")?
        .borrow()
        .date(selected.py(), "a calendar's day")?;
    let today = today.filter(|t| !t.is_none()).map(date_arg).transpose()?;
    Ok(PyNode::new(core::calendar_with(selected, today)))
}

/// A ratio: a signal (made a float one), or a float (a new signal).
fn ratio(value: &Bound<'_, PyAny>) -> PyResult<Signal<f64>> {
    if let Ok(signal) = value.cast::<PySignal>() {
        return signal.borrow().f64(value.py(), "a split's ratio");
    }
    let ratio: f64 = value
        .extract()
        .map_err(|_| PyTypeError::new_err("a ratio is a Signal or a float"))?;
    super::require("a split")?;
    Ok(intuituive::signal(ratio))
}

/// `split(axis, first, second, ratio)`: two panes and a divider that
/// drags; `ratio` (a signal, or a float) is the first pane's share.
#[pyfunction]
fn tui_split(
    axis: &Bound<'_, PyAny>,
    first: &Bound<'_, PyAny>,
    second: &Bound<'_, PyAny>,
    ratio: &Bound<'_, PyAny>,
) -> PyResult<PyNode> {
    let axis = axis_arg(axis)?;
    let ratio = self::ratio(ratio)?;
    Ok(PyNode::new(core::split(
        axis,
        take_node(first)?,
        take_node(second)?,
        ratio,
    )))
}

/// `split_with(axis, first, second, ratio, min)`: a `split` whose panes
/// keep at least `min` cells while there is room.
#[pyfunction]
fn tui_split_with(
    axis: &Bound<'_, PyAny>,
    first: &Bound<'_, PyAny>,
    second: &Bound<'_, PyAny>,
    ratio: &Bound<'_, PyAny>,
    min: u16,
) -> PyResult<PyNode> {
    let axis = axis_arg(axis)?;
    let ratio = self::ratio(ratio)?;
    Ok(PyNode::new(core::split_with(
        axis,
        take_node(first)?,
        take_node(second)?,
        ratio,
        min,
    )))
}

/// `hsplit(first, second, ratio)`: the panes side by side.
#[pyfunction]
fn tui_hsplit(
    first: &Bound<'_, PyAny>,
    second: &Bound<'_, PyAny>,
    ratio: &Bound<'_, PyAny>,
) -> PyResult<PyNode> {
    let ratio = self::ratio(ratio)?;
    Ok(PyNode::new(core::hsplit(
        take_node(first)?,
        take_node(second)?,
        ratio,
    )))
}

/// `vsplit(first, second, ratio)`: the panes one above the other.
#[pyfunction]
fn tui_vsplit(
    first: &Bound<'_, PyAny>,
    second: &Bound<'_, PyAny>,
    ratio: &Bound<'_, PyAny>,
) -> PyResult<PyNode> {
    let ratio = self::ratio(ratio)?;
    Ok(PyNode::new(core::vsplit(
        take_node(first)?,
        take_node(second)?,
        ratio,
    )))
}

// ---------------------------------------------------------------------------
// Menus

/// `MenuItem(label, action)`: a menu line that runs `action(cx)` (the menu
/// closes first); `MenuItem.separator()` is a line between groups, and
/// `.hint(keys)` shows the keys that do the same.
#[pyclass(name = "MenuItem", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyMenuItem(RefCell<CoreMenuItem>);

#[pymethods]
impl PyMenuItem {
    #[new]
    fn new(label: &str, action: &Bound<'_, PyAny>) -> PyResult<PyMenuItem> {
        let action = Callback::checked(action, "the menu item's action")?;
        Ok(PyMenuItem(RefCell::new(CoreMenuItem::new(
            label,
            move |cx| call_with_ctx(&action, None, cx),
        ))))
    }

    #[staticmethod]
    fn separator() -> PyMenuItem {
        PyMenuItem(RefCell::new(CoreMenuItem::separator()))
    }

    /// Show `keys` at the right (`"ctrl+s"`).
    fn hint<'py>(slf: Bound<'py, Self>, keys: &str) -> Bound<'py, Self> {
        {
            let this = slf.borrow();
            let item = this.0.borrow().clone().hint(keys);
            *this.0.borrow_mut() = item;
        }
        slf
    }
}

fn menu_items(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreMenuItem>> {
    value
        .try_iter()?
        .map(|item| {
            let item = item?;
            item.cast::<PyMenuItem>()
                .map(|item| item.borrow().0.borrow().clone())
                .map_err(|_| PyTypeError::new_err("a menu's items are MenuItems"))
        })
        .collect()
}

/// `Menu(title, items)`: a titled menu for a `menu_bar`.
#[pyclass(name = "Menu", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyMenu(CoreMenu);

#[pymethods]
impl PyMenu {
    #[new]
    fn new(title: &str, items: &Bound<'_, PyAny>) -> PyResult<PyMenu> {
        Ok(PyMenu(CoreMenu::new(title, menu_items(items)?)))
    }

    #[getter]
    fn title(&self) -> &str {
        &self.0.title
    }
}

/// `menu_bar(menus)`: a row of menu titles; ←/→ move between them, and
/// Enter, ↓ or a click opens one below its title.
#[pyfunction]
fn tui_menu_bar(menus: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let menus = menus
        .try_iter()?
        .map(|menu| {
            let menu = menu?;
            menu.cast::<PyMenu>()
                .map(|menu| menu.borrow().0.clone())
                .map_err(|_| PyTypeError::new_err("a menu bar's menus are Menus"))
        })
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyNode::new(intuituive::menu::menu_bar(menus)))
}

/// `open_menu(cx, at, placement, items)`: open a menu next to `at` (a
/// node's id or a `Rect`).
#[pyfunction]
fn tui_open_menu(
    cx: &Bound<'_, super::app::PyCtx>,
    at: &Bound<'_, PyAny>,
    placement: &Bound<'_, PyAny>,
    items: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let at = super::app::anchor_arg(at)?;
    let placement = placement_arg(placement)?;
    let items = menu_items(items)?;
    cx.borrow()
        .with(|cx| intuituive::menu::open_menu(cx, at, placement, items))
}

/// `context_menu(cx, items)`: open a menu at the mouse pointer (from a
/// mouse handler); elsewhere, in the middle of the screen.
#[pyfunction]
fn tui_context_menu(cx: &Bound<'_, super::app::PyCtx>, items: &Bound<'_, PyAny>) -> PyResult<()> {
    let items = menu_items(items)?;
    cx.borrow()
        .with(|cx| intuituive::menu::context_menu(cx, items))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("TuiColumn", py.get_type::<PyColumn>()),
        ("TuiTableOptions", py.get_type::<PyTableOptions>()),
        ("TuiTreeItem", py.get_type::<PyTreeItem>()),
        ("TuiLazyItem", py.get_type::<PyLazyItem>()),
        ("TuiMenuItem", py.get_type::<PyMenuItem>()),
        ("TuiMenu", py.get_type::<PyMenu>()),
    ] {
        m.add(name, class)?;
    }
    m.add_function(wrap_pyfunction!(tui_table, m)?)?;
    m.add_function(wrap_pyfunction!(tui_table_with, m)?)?;
    m.add_function(wrap_pyfunction!(tui_virtual_table, m)?)?;
    m.add_function(wrap_pyfunction!(tui_virtual_list, m)?)?;
    m.add_function(wrap_pyfunction!(tui_tabs, m)?)?;
    m.add_function(wrap_pyfunction!(tui_tree, m)?)?;
    m.add_function(wrap_pyfunction!(tui_tree_with, m)?)?;
    m.add_function(wrap_pyfunction!(tui_tree_lazy, m)?)?;
    m.add_function(wrap_pyfunction!(tui_calendar, m)?)?;
    m.add_function(wrap_pyfunction!(tui_calendar_with, m)?)?;
    m.add_function(wrap_pyfunction!(tui_split, m)?)?;
    m.add_function(wrap_pyfunction!(tui_split_with, m)?)?;
    m.add_function(wrap_pyfunction!(tui_hsplit, m)?)?;
    m.add_function(wrap_pyfunction!(tui_vsplit, m)?)?;
    m.add_function(wrap_pyfunction!(tui_menu_bar, m)?)?;
    m.add_function(wrap_pyfunction!(tui_open_menu, m)?)?;
    m.add_function(wrap_pyfunction!(tui_context_menu, m)?)?;
    Ok(())
}
