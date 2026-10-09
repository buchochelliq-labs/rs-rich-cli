//! The DOM renderer's side of the wire: what the server sends a page that
//! draws an app itself ([`Renderer::Dom`](crate::Renderer::Dom)), as a grid
//! of styled spans with the app's accessibility tree laid over it as ARIA.
//!
//! # Protocol, version 1
//!
//! The page and the server talk over the same WebSocket, opened at
//! `/ws?token=…&cols=…&rows=…&renderer=dom`.
//!
//! **From the page**, text messages, as the xterm.js page sends them: `d`
//! and the bytes an xterm sends for keys, SGR mouse reports and bracketed
//! pastes (decoded by [`input::decode`](crate::input::decode)); `r` and
//! `columns,rows` when its size changes; `c` and `number:1` (or `:0`) when
//! the clipboard took (or refused) a copy.
//!
//! **From the server**, text messages, each a JSON object whose `t` names
//! it:
//!
//! - `{"t":"hello","protocol":1}`, first. A page that speaks another
//!   version stops.
//! - `{"t":"frame","cols":C,"rows":R,"styles":[[id,css],…],
//!   "lines":[[y,[run,…]],…],"cursor":[x,y]|null}` after each frame: the
//!   lines that changed (every line after a resize), each as runs of
//!   `[text, style]`, or `[text, style, 2]` for one wide grapheme. A style
//!   is a number; `0` is the page's own colours, and each other is sent,
//!   as CSS declarations, in the first frame that uses it. `cursor` is
//!   where the focused text box's caret is ([`Driver::cursor`]).
//! - `{"t":"tree","focus":id|null,"nodes":[node,…]}` when the
//!   accessibility tree changed: [`Driver::accessibility`], outermost
//!   first, each node `{"id","depth","rect":[x,y,w,h],"attrs":[[name,
//!   value],…],"text"}` and, for a widget of items (a list, a table, a
//!   tree, tabs, a menu, a grid), `"item":{"role","attrs","text"}` for its
//!   selected item. `attrs` are [`AccessNode::aria_attributes`], with the
//!   states of the selected item (`aria-selected`, `aria-expanded`,
//!   `aria-checked`, `aria-posinset`, `aria-setsize`) on the item. `text`
//!   is what the element reads: [`AccessNode::describe`] for text and a
//!   status, a text box's value, the selected item's text. The page nests
//!   each node in the nearest node before it of a smaller depth, and
//!   focuses the focused node (its item, if it has one).
//! - `{"t":"say","text":"…","urgent":bool}` for each announcement
//!   ([`Driver::take_announcements`]), for an `aria-live` region:
//!   assertive when urgent, else polite.
//! - `{"t":"copy","n":number,"text":"…"}` when the app copies text; the
//!   page answers `c` as above.
//!
//! The session ends with a WebSocket close.
//!
//! [`Driver::cursor`]: intuituive::Driver::cursor
//! [`Driver::accessibility`]: intuituive::Driver::accessibility
//! [`Driver::take_announcements`]: intuituive::Driver::take_announcements
//! [`AccessNode::aria_attributes`]: intuituive::a11y::AccessNode::aria_attributes
//! [`AccessNode::describe`]: intuituive::a11y::AccessNode::describe

use std::collections::HashMap;

use intuituive::a11y::{AccessNode, Announcement, Role};
use intuituive::rich::{Color, ColorTriplet, Style, TerminalTheme};
use intuituive::Driver;
use serde_json::{json, Value};

/// The protocol's version, sent in `hello`.
pub const PROTOCOL: u32 = 1;

/// The colours styles resolve to: xterm.js's default palette on the page's
/// background, so an app looks the same drawn either way.
pub const THEME: TerminalTheme = TerminalTheme {
    background: ColorTriplet::new(0x10, 0x10, 0x10),
    foreground: ColorTriplet::new(0xd0, 0xd0, 0xd0),
    ansi: [
        ColorTriplet::new(0x2e, 0x34, 0x36),
        ColorTriplet::new(0xcc, 0x00, 0x00),
        ColorTriplet::new(0x4e, 0x9a, 0x06),
        ColorTriplet::new(0xc4, 0xa0, 0x00),
        ColorTriplet::new(0x34, 0x65, 0xa4),
        ColorTriplet::new(0x75, 0x50, 0x7b),
        ColorTriplet::new(0x06, 0x98, 0x9a),
        ColorTriplet::new(0xd3, 0xd7, 0xcf),
        ColorTriplet::new(0x55, 0x57, 0x53),
        ColorTriplet::new(0xef, 0x29, 0x29),
        ColorTriplet::new(0x8a, 0xe2, 0x34),
        ColorTriplet::new(0xfc, 0xe9, 0x4f),
        ColorTriplet::new(0x72, 0x9f, 0xcf),
        ColorTriplet::new(0xad, 0x7f, 0xa8),
        ColorTriplet::new(0x34, 0xe2, 0xe2),
        ColorTriplet::new(0xee, 0xee, 0xec),
    ],
};

/// The states that belong to a widget's selected item.
const ITEM_STATES: [&str; 5] = [
    "aria-selected",
    "aria-expanded",
    "aria-checked",
    "aria-posinset",
    "aria-setsize",
];

/// What the page was last sent, so only changes go.
#[derive(Debug, Default)]
pub struct Dom {
    /// Each style's CSS, by the number the page knows it by.
    styles: HashMap<String, u32>,
    /// Each line as last sent.
    lines: Vec<Value>,
    size: (u16, u16),
    cursor: Option<(u16, u16)>,
    /// The tree as last sent.
    tree: Option<Value>,
}

impl Dom {
    pub fn new() -> Dom {
        Dom::default()
    }

    /// The first message.
    pub fn hello(&self) -> String {
        json!({"t": "hello", "protocol": PROTOCOL}).to_string()
    }

    /// The messages that bring the page up to `driver`'s last frame: a
    /// `frame` when a line or the caret changed, and a `tree` when the
    /// accessibility tree did.
    pub fn update(&mut self, driver: &Driver) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(frame) = self.frame(driver) {
            out.push(frame.to_string());
        }
        let tree = tree(&driver.accessibility());
        if self.tree.as_ref() != Some(&tree) {
            out.push(tree.to_string());
            self.tree = Some(tree);
        }
        out
    }

    fn frame(&mut self, driver: &Driver) -> Option<Value> {
        let screen = driver.screen();
        let area = screen.area();
        let size = (area.width, area.height);
        if size != self.size {
            self.size = size;
            self.lines.clear();
        }
        let mut new_styles = Vec::new();
        // Each screen style's number, for this frame.
        let mut known = HashMap::new();
        let mut changed = Vec::new();
        for y in 0..area.height {
            let mut runs: Vec<Value> = Vec::new();
            let mut run = String::new();
            let mut current = 0;
            for x in 0..area.width {
                let cell = screen.cell(x, y);
                if cell.is_continuation() {
                    continue;
                }
                let id = *known.entry(cell.style).or_insert_with(|| {
                    let css = screen.style(cell.style).map(css).unwrap_or_default();
                    self.style_number(css, &mut new_styles)
                });
                if cell.width >= 2 {
                    if !run.is_empty() {
                        runs.push(json!([std::mem::take(&mut run), current]));
                    }
                    runs.push(json!([cell.text, id, 2]));
                    continue;
                }
                if id != current && !run.is_empty() {
                    runs.push(json!([std::mem::take(&mut run), current]));
                }
                current = id;
                run.push_str(&cell.text);
            }
            if !run.is_empty() {
                runs.push(json!([run, current]));
            }
            let line = Value::Array(runs);
            let y = y as usize;
            if self.lines.get(y) != Some(&line) {
                changed.push(json!([y, line.clone()]));
                if y < self.lines.len() {
                    self.lines[y] = line;
                } else {
                    self.lines.push(line);
                }
            }
        }
        let cursor = driver.cursor();
        if changed.is_empty() && new_styles.is_empty() && cursor == self.cursor {
            return None;
        }
        self.cursor = cursor;
        Some(json!({
            "t": "frame",
            "cols": size.0,
            "rows": size.1,
            "styles": new_styles,
            "lines": changed,
            "cursor": cursor.map(|(x, y)| json!([x, y])),
        }))
    }

    /// The number `css` goes by, sending it the first time.
    fn style_number(&mut self, css: String, new_styles: &mut Vec<Value>) -> u32 {
        if css.is_empty() {
            return 0;
        }
        if let Some(&id) = self.styles.get(&css) {
            return id;
        }
        let id = self.styles.len() as u32 + 1;
        new_styles.push(json!([id, css]));
        self.styles.insert(css, id);
        id
    }
}

/// `style` as CSS declarations, in [`THEME`]'s colours. Reverse video with
/// a colour left to the terminal swaps in the theme's, so a focused row
/// drawn in reverse shows.
pub fn css(style: &Style) -> String {
    // Attribute 6 is reverse (as `Style::get_html_style` reads it).
    let reverse = style.attr(6) == Some(true);
    let mut style = style.clone();
    if reverse {
        let rgb = |t: ColorTriplet| Color::from_rgb(t.red, t.green, t.blue);
        if style.color().is_none() {
            style = style.with_color(rgb(THEME.foreground));
        }
        if style.bgcolor().is_none() {
            style = style.with_bgcolor(rgb(THEME.background));
        }
    }
    style.get_html_style(&THEME)
}

/// The role a widget of items gives its selected item.
fn item_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::List => "listitem",
        Role::Table => "row",
        Role::Tree => "treeitem",
        Role::TabList => "tab",
        Role::Menu | Role::MenuBar => "menuitem",
        Role::Grid => "gridcell",
        _ => return None,
    })
}

fn pairs(attrs: &[(&'static str, String)]) -> Value {
    Value::Array(
        attrs
            .iter()
            .map(|(name, value)| json!([name, value]))
            .collect(),
    )
}

/// One node of the `tree` message.
pub fn node(node: &AccessNode) -> Value {
    let attrs = node.aria_attributes();
    let item = node
        .value
        .as_ref()
        .and_then(|value| item_role(node.role).map(|role| (role, value)));
    let text = match node.role {
        Role::Text | Role::Status => node.describe(),
        Role::TextBox => node.value.clone().unwrap_or_default(),
        _ => String::new(),
    };
    let rect = node.rect;
    let mut out = json!({
        "id": node.id,
        "depth": node.depth,
        "rect": [rect.x, rect.y, rect.width, rect.height],
        "text": text,
    });
    match item {
        Some((role, value)) => {
            let (on_item, own): (Vec<_>, Vec<_>) = attrs
                .into_iter()
                .partition(|(name, _)| ITEM_STATES.contains(name));
            out["attrs"] = pairs(&own);
            out["item"] = json!({"role": role, "attrs": pairs(&on_item), "text": value});
        }
        None => out["attrs"] = pairs(&attrs),
    }
    out
}

/// The `tree` message for `nodes`.
pub fn tree(nodes: &[AccessNode]) -> Value {
    let focus = nodes.iter().find(|n| n.focused).map(|n| n.id);
    json!({
        "t": "tree",
        "focus": focus,
        "nodes": nodes.iter().map(node).collect::<Vec<_>>(),
    })
}

/// The `say` message for `announcement`.
pub fn say(announcement: &Announcement) -> String {
    json!({"t": "say", "text": announcement.text, "urgent": announcement.urgent}).to_string()
}

/// The `copy` message for copy `number` of `text`.
pub fn copy(number: u64, text: &str) -> String {
    json!({"t": "copy", "n": number, "text": text}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use intuituive::rich::Style;

    #[test]
    fn styles_become_css() {
        let parse = |s: &str| Style::parse(s).unwrap();
        assert_eq!(css(&Style::new()), "");
        assert_eq!(
            css(&parse("bold red")),
            "color: #cc0000; text-decoration-color: #cc0000; font-weight: bold"
        );
        // Reverse with nothing set still shows: the theme's colours swap.
        assert_eq!(
            css(&parse("reverse")),
            "color: #101010; text-decoration-color: #101010; background-color: #d0d0d0"
        );
        assert_eq!(css(&parse("on #123456")), "background-color: #123456");
    }

    #[test]
    fn items_take_the_item_states() {
        use intuituive::a11y::AccessState;
        use intuituive::screen::Rect;
        let list = AccessNode {
            depth: 0,
            id: 7,
            role: Role::List,
            name: "Files".into(),
            value: Some("b.txt".into()),
            focused: true,
            disabled: false,
            state: AccessState::item(1, 3),
            rect: Rect::new(0, 1, 10, 3),
        };
        let json = node(&list);
        assert_eq!(
            json["attrs"],
            json!([["role", "list"], ["aria-label", "Files"]])
        );
        assert_eq!(
            json["item"],
            json!({
                "role": "listitem",
                "attrs": [["aria-selected", "true"], ["aria-posinset", "2"], ["aria-setsize", "3"]],
                "text": "b.txt",
            })
        );
        assert_eq!(json["rect"], json!([0, 1, 10, 3]));
        // A button keeps its own attributes and reads by its label.
        let button = AccessNode {
            role: Role::Button,
            name: "Save".into(),
            value: None,
            state: AccessState::default(),
            ..list.clone()
        };
        let json = node(&button);
        assert_eq!(
            json["attrs"],
            json!([["role", "button"], ["aria-label", "Save"]])
        );
        assert!(json.get("item").is_none());
        assert_eq!(json["text"], "");
        // Text reads as linear mode writes it.
        let text = AccessNode {
            role: Role::Text,
            name: "Hello".into(),
            ..button
        };
        assert_eq!(node(&text)["text"], "Hello");
    }
}
