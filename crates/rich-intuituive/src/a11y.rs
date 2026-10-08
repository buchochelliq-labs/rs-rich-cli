//! Accessibility: what each node is (its [`Role`]) and is called, the
//! tree a screen reader bridge or a browser reads
//! ([`Driver::accessibility`](crate::Driver::accessibility)), announcements
//! ([`Announcer`]), and the text mode an
//! [accessible](crate::App::accessible) app draws in.
//!
//! In accessible mode the terminal's real cursor sits on what has the focus
//! (the selected row, the cell, the input's caret), so NVDA, VoiceOver and
//! Orca read it with no bridge; boxes are drawn as blanks, colour is off,
//! and a selected item is marked with `>`.

use std::cell::Cell;

use crate::reactive::NodeId;
use crate::screen::{Rect, Screen};

/// What a node is, for assistive technology: ARIA's roles, by name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Role {
    /// Layout only: a column, a row, a grid, padding. Left out of the
    /// accessibility tree unless it has a [label](crate::Node::label).
    #[default]
    Group,
    /// A titled part of the screen: a panel.
    Region,
    Text,
    Button,
    List,
    ListItem,
    Table,
    Row,
    Cell,
    Tree,
    TreeItem,
    Tab,
    TabList,
    Dialog,
    Menu,
    MenuBar,
    MenuItem,
    TextBox,
    /// A region whose changes are announced: [`live`](crate::Node::live).
    Status,
    /// A log whose new lines are announced.
    Log,
    /// A month of days.
    Grid,
    /// Two panes and the divider between them.
    Separator,
}

impl Role {
    /// The ARIA name: `"button"`, `"listitem"`, `"tablist"`.
    pub fn name(self) -> &'static str {
        match self {
            Role::Group => "group",
            Role::Region => "region",
            Role::Text => "text",
            Role::Button => "button",
            Role::List => "list",
            Role::ListItem => "listitem",
            Role::Table => "table",
            Role::Row => "row",
            Role::Cell => "cell",
            Role::Tree => "tree",
            Role::TreeItem => "treeitem",
            Role::Tab => "tab",
            Role::TabList => "tablist",
            Role::Dialog => "dialog",
            Role::Menu => "menu",
            Role::MenuBar => "menubar",
            Role::MenuItem => "menuitem",
            Role::TextBox => "textbox",
            Role::Status => "status",
            Role::Log => "log",
            Role::Grid => "grid",
            Role::Separator => "separator",
        }
    }

    /// Whether the widget holds items and has one selected: its value is
    /// the selected item.
    pub fn has_items(self) -> bool {
        matches!(
            self,
            Role::List
                | Role::Table
                | Role::Tree
                | Role::TabList
                | Role::Menu
                | Role::MenuBar
                | Role::Grid
        )
    }
}

/// One node of the accessibility tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessNode {
    /// How many shown ancestors it has in the tree.
    pub depth: usize,
    pub id: NodeId,
    pub role: Role,
    /// Its [label](crate::Node::label), else the text it shows (for text,
    /// buttons and text boxes) or its title (a panel).
    pub name: String,
    /// What is selected in it (a list's row, a table's row), or what a text
    /// box holds.
    pub value: Option<String>,
    pub focused: bool,
    pub disabled: bool,
    /// Where it is on the screen.
    pub rect: Rect,
}

/// Something to say: a toast, a dialog opening, a status that changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub text: String,
    /// Say it now, interrupting (a dialog, an error), rather than when the
    /// reader is idle.
    pub urgent: bool,
}

/// Where announcements go as they happen, besides
/// [`Driver::take_announcements`](crate::Driver::take_announcements): a
/// speech command, a log, a browser's live region.
pub trait Announcer {
    fn announce(&mut self, announcement: &Announcement);
}

impl<F: FnMut(&Announcement)> Announcer for F {
    fn announce(&mut self, announcement: &Announcement) {
        self(announcement)
    }
}

thread_local! {
    /// Whether the app drawing on this thread is in text mode.
    static TEXT_MODE: Cell<bool> = const { Cell::new(false) };
}

/// Whether the frame being drawn is in text mode: no box drawing, a marker
/// on the selected item. Widgets of your own can ask too.
pub fn text_mode() -> bool {
    TEXT_MODE.with(Cell::get)
}

pub(crate) fn set_text_mode(on: bool) {
    TEXT_MODE.with(|mode| mode.set(on));
}

/// What `rect` of `screen` shows, row by row, trimmed, rows joined with a
/// space.
pub(crate) fn screen_text(screen: &Screen, rect: Rect) -> String {
    // Cell by cell, so wide and many-codepoint graphemes keep to their
    // cells: a wide one belongs to the rectangle its first cell is in.
    let rect = rect.intersection(screen.area());
    let mut out: Vec<String> = Vec::new();
    for row in rect.y..rect.bottom() {
        let text: String = (rect.x..rect.right())
            .map(|x| screen.cell(x, row))
            .filter(|cell| !cell.is_continuation())
            .map(|cell| cell.text.as_str())
            .collect();
        let text = text.trim();
        if !text.is_empty() {
            out.push(text.to_string());
        }
    }
    out.join(" ")
}

/// Whether accessibility is asked for by the environment:
/// `INTUITUIVE_ACCESSIBLE` set to anything but `0`, or rs-rich's
/// `RICH_A11Y` naming `screen-reader`.
pub(crate) fn from_env() -> bool {
    let set = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    if let Some(value) = set("INTUITUIVE_ACCESSIBLE") {
        return value != "0" && !value.eq_ignore_ascii_case("false");
    }
    set("RICH_A11Y").is_some_and(|list| {
        list.split(',')
            .any(|item| item.trim().eq_ignore_ascii_case("screen-reader"))
    })
}
