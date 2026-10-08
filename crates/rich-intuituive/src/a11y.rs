//! Accessibility: what each node is (its [`Role`]) and is called, the
//! tree a screen reader bridge or a browser reads
//! ([`Driver::accessibility`](crate::Driver::accessibility)), announcements
//! ([`Announcer`]), and the text mode an
//! [accessible](crate::App::accessible) app draws in.
//!
//! In accessible mode the terminal's real cursor sits on what has the focus
//! (the selected row, the cell, the input's caret), so NVDA, VoiceOver and
//! Orca read it with no bridge; boxes are drawn as blanks, colour is off,
//! and a selected item is marked with `>`. In [linear](crate::App::linear)
//! mode no screen is drawn at all: the tree is written as lines of text
//! ([`AccessNode::describe`]), and then only the lines that changed.

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
    /// Something [checked](crate::Node::checked_when) or not.
    CheckBox,
    /// A toggle that is on (checked) or off.
    Switch,
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
            Role::CheckBox => "checkbox",
            Role::Switch => "switch",
        }
    }

    /// What a screen reader says: `"button"`, `"list item"`, `"text box"`.
    pub fn spoken(self) -> &'static str {
        match self {
            Role::ListItem => "list item",
            Role::TreeItem => "tree item",
            Role::TabList => "tab list",
            Role::MenuBar => "menu bar",
            Role::MenuItem => "menu item",
            Role::TextBox => "text box",
            Role::CheckBox => "check box",
            role => role.name(),
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

/// ARIA's states of a node, besides focus and
/// [disabled](crate::Node::disabled_when). For a widget of items (a list,
/// a table, a tree, tabs, a menu) they are its selected item's: a DOM
/// renderer puts them on that item's element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessState {
    /// Open (`Some(true)`) or closed: a tree item with children, a
    /// collapsible. `None` when it cannot open.
    pub expanded: Option<bool>,
    /// On or off: a check box, a toggle. `None` when it is neither.
    pub checked: Option<bool>,
    pub selected: bool,
    /// Working: loading, a task running.
    pub busy: bool,
    /// The selected item's place among its widget's items: (from 1, of how
    /// many). ARIA's `aria-posinset` and `aria-setsize`.
    pub position: Option<(usize, usize)>,
}

impl AccessState {
    /// A widget of `count` items with item `index` (from 0) selected:
    /// selected, and its place, when there are any.
    pub fn item(index: usize, count: usize) -> AccessState {
        AccessState {
            selected: count > 0,
            position: (count > 0).then(|| (index.min(count - 1) + 1, count)),
            ..AccessState::default()
        }
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
    /// Expanded, checked, selected, busy, and the selected item's place.
    pub state: AccessState,
    /// Where it is on the screen.
    pub rect: Rect,
}

impl AccessNode {
    /// The node as one line of plain text, as [linear](crate::App::linear)
    /// mode writes it: its name, its role (unless it is text), the selected
    /// item's place, its value after a colon, then its states.
    ///
    /// ```text
    /// Files, list, 3 of 10: main.rs, selected
    /// Save, button
    /// Search, text box: foo
    /// Wrap lines, check box, checked
    /// ```
    ///
    /// Empty for a node with nothing to say (blank text).
    pub fn describe(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if !self.name.is_empty() {
            parts.push(&self.name);
        }
        // Text and a status are read as they are.
        let plain = matches!(self.role, Role::Text | Role::Status);
        if !plain {
            parts.push(self.role.spoken());
        }
        let mut line = parts.join(", ");
        if let Some((at, of)) = self.state.position {
            line.push_str(&format!(", {at} of {of}"));
        }
        // A text box named by what it shows says it once.
        if let Some(value) = self.value.as_deref().filter(|v| *v != self.name) {
            line.push_str(": ");
            line.push_str(value);
        }
        if line.is_empty() {
            return line;
        }
        let state = self.state;
        let words = [
            (state.checked == Some(true), "checked"),
            (state.checked == Some(false), "not checked"),
            (state.selected, "selected"),
            (state.expanded == Some(true), "expanded"),
            (state.expanded == Some(false), "collapsed"),
            (state.busy, "busy"),
            (self.disabled, "disabled"),
        ];
        for (_, word) in words.iter().filter(|(on, _)| *on) {
            line.push_str(", ");
            line.push_str(word);
        }
        line
    }

    /// Its ARIA attributes, for a DOM renderer: `role`, `aria-label`, and
    /// each state that is set (`aria-expanded`, `aria-checked`,
    /// `aria-selected`, `aria-busy`, `aria-disabled`, `aria-posinset` and
    /// `aria-setsize`). A widget of items' states go on its selected
    /// item's element.
    pub fn aria_attributes(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![("role", self.role.name().to_string())];
        if !self.name.is_empty() {
            out.push(("aria-label", self.name.clone()));
        }
        let state = self.state;
        if let Some(open) = state.expanded {
            out.push(("aria-expanded", open.to_string()));
        }
        if let Some(on) = state.checked {
            out.push(("aria-checked", on.to_string()));
        }
        for (on, name) in [
            (state.selected, "aria-selected"),
            (state.busy, "aria-busy"),
            (self.disabled, "aria-disabled"),
        ] {
            if on {
                out.push((name, "true".to_string()));
            }
        }
        if let Some((at, of)) = state.position {
            out.push(("aria-posinset", at.to_string()));
            out.push(("aria-setsize", of.to_string()));
        }
        out
    }
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
/// space; without the cells inside `hidden`, where nodes
/// [hidden](crate::Node::access_hidden) from assistive technology are.
pub(crate) fn screen_text(screen: &Screen, rect: Rect, hidden: &[Rect]) -> String {
    // Cell by cell, so wide and many-codepoint graphemes keep to their
    // cells: a wide one belongs to the rectangle its first cell is in.
    let rect = rect.intersection(screen.area());
    let mut out: Vec<String> = Vec::new();
    for row in rect.y..rect.bottom() {
        let text: String = (rect.x..rect.right())
            .filter(|&x| !hidden.iter().any(|h| h.contains(x, row)))
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

/// What the environment asks for: (accessible, linear). See [`modes`].
pub(crate) fn from_env() -> (bool, bool) {
    let get = |name: &str| std::env::var(name).ok();
    modes(
        get("INTUITUIVE_ACCESSIBLE").as_deref(),
        get("RICH_A11Y").as_deref(),
    )
}

/// (accessible, linear) for the values of `INTUITUIVE_ACCESSIBLE` and
/// rs-rich's `RICH_A11Y`: `linear` asks for linear mode, anything else but
/// `0` or `false` for the cursor-tracking mode; with it unset or empty,
/// `RICH_A11Y` naming `screen-reader` asks for the cursor-tracking mode.
pub(crate) fn modes(intuituive: Option<&str>, rich: Option<&str>) -> (bool, bool) {
    fn set(value: Option<&str>) -> Option<&str> {
        value.map(str::trim).filter(|v| !v.is_empty())
    }
    if let Some(value) = set(intuituive) {
        if value.eq_ignore_ascii_case("linear") {
            return (true, true);
        }
        return (value != "0" && !value.eq_ignore_ascii_case("false"), false);
    }
    let reader = set(rich).is_some_and(|list| {
        list.split(',')
            .any(|item| item.trim().eq_ignore_ascii_case("screen-reader"))
    });
    (reader, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_picks_the_mode() {
        let cases = [
            (None, None, (false, false)),
            (Some(""), None, (false, false)),
            (Some("1"), None, (true, false)),
            (Some("yes"), None, (true, false)),
            (Some("0"), None, (false, false)),
            (Some("FALSE"), None, (false, false)),
            (Some("linear"), None, (true, true)),
            (Some(" Linear "), None, (true, true)),
            // INTUITUIVE_ACCESSIBLE wins over RICH_A11Y.
            (Some("0"), Some("screen-reader"), (false, false)),
            (Some("linear"), Some("screen-reader"), (true, true)),
            (None, Some("motion, screen-reader"), (true, false)),
            (Some(""), Some("screen-reader"), (true, false)),
            (None, Some("motion"), (false, false)),
        ];
        for (intuituive, rich, mode) in cases {
            assert_eq!(modes(intuituive, rich), mode, "{intuituive:?} {rich:?}");
        }
    }

    fn node(role: Role, name: &str, value: Option<&str>, state: AccessState) -> AccessNode {
        AccessNode {
            depth: 0,
            id: NodeId::default(),
            role,
            name: name.into(),
            value: value.map(str::to_string),
            focused: false,
            disabled: false,
            state,
            rect: Rect::default(),
        }
    }

    #[test]
    fn nodes_describe_themselves_on_one_line() {
        let none = AccessState::default();
        let item = AccessState {
            selected: true,
            position: Some((3, 10)),
            ..none
        };
        let cases = [
            (
                node(Role::List, "Files", Some("main.rs"), item),
                "Files, list, 3 of 10: main.rs, selected",
            ),
            (node(Role::Button, "Save", None, none), "Save, button"),
            (
                node(Role::TextBox, "Search", Some("foo"), none),
                "Search, text box: foo",
            ),
            (
                node(Role::TextBox, "Name: Ada", Some("Name: Ada"), none),
                "Name: Ada, text box",
            ),
            (node(Role::Text, "Ready", None, none), "Ready"),
            (node(Role::Status, "3 done", None, none), "3 done"),
            (node(Role::Text, "", None, none), ""),
            (
                node(Role::TabList, "", Some("Logs"), item),
                "tab list, 3 of 10: Logs, selected",
            ),
            (
                node(
                    Role::CheckBox,
                    "Wrap",
                    None,
                    AccessState {
                        checked: Some(false),
                        ..none
                    },
                ),
                "Wrap, check box, not checked",
            ),
            (
                node(
                    Role::Tree,
                    "",
                    Some("src"),
                    AccessState {
                        expanded: Some(false),
                        busy: true,
                        ..item
                    },
                ),
                "tree, 3 of 10: src, selected, collapsed, busy",
            ),
        ];
        for (node, line) in cases {
            assert_eq!(node.describe(), line);
        }
        let mut off = node(Role::Button, "Send", None, none);
        off.disabled = true;
        assert_eq!(off.describe(), "Send, button, disabled");
    }

    #[test]
    fn states_map_to_aria_attributes() {
        let mut tree = node(
            Role::Tree,
            "Files",
            Some("src"),
            AccessState {
                expanded: Some(true),
                checked: None,
                selected: true,
                busy: true,
                position: Some((1, 4)),
            },
        );
        tree.disabled = true;
        let expected = [
            ("role", "tree"),
            ("aria-label", "Files"),
            ("aria-expanded", "true"),
            ("aria-selected", "true"),
            ("aria-busy", "true"),
            ("aria-disabled", "true"),
            ("aria-posinset", "1"),
            ("aria-setsize", "4"),
        ];
        let expected: Vec<(&str, String)> =
            expected.iter().map(|(k, v)| (*k, v.to_string())).collect();
        assert_eq!(tree.aria_attributes(), expected);
        let check = AccessState {
            checked: Some(false),
            ..AccessState::default()
        };
        assert_eq!(
            node(Role::CheckBox, "", None, check).aria_attributes(),
            [
                ("role", "checkbox".to_string()),
                ("aria-checked", "false".to_string())
            ]
        );
    }
}
