//! Keys beyond a binding of one key: key sequences (`g g`) with a
//! which-key pop-up, a handler for any key, and containers whose bindings
//! see keys before the focused node inside them.
//!
//! How a key is routed, from the top:
//!
//! 1. a key sequence under way takes the key if it goes on with it (or
//!    ends it); Esc cancels it; any other key cancels it and is then
//!    routed as below, as if no sequence had started;
//! 2. containers that [preview keys](Node::previews_keys), outermost
//!    first, try their bindings, sequences and [`on_any_key`](Node::on_any_key);
//! 3. from the focused node up: its widget, then its bindings, then the
//!    first key of its sequences, then its `on_any_key`.

use std::time::Duration;

use rich::{Console, Segment};
use rich_interact::Key;

use crate::app::{Ctx, Theme};
use crate::node::{parse_keys, KeyBinding, KeyHandler, Node};
use crate::place::{place, Offset, Origin};
use crate::reactive::NodeId;
use crate::screen::{Rect, Screen};

/// What [`Node::on_any_key`] runs: whether it used the key.
pub(crate) type AnyKey = Box<dyn FnMut(&mut Ctx, Key) -> bool>;

/// A key sequence under way: the node whose sequences it follows, the keys
/// pressed so far, when it started and on how deep a stack of screens.
pub(crate) struct Pending {
    pub node: NodeId,
    pub keys: Vec<Key>,
    pub since: Duration,
    pub depth: usize,
    /// Whether a frame has drawn its which-key pop-up (or found nothing to
    /// draw) since it went on.
    pub shown: bool,
}

/// Whether `typed` is the start of `sequence` (or all of it).
pub(crate) fn starts(sequence: &[Key], typed: &[Key]) -> bool {
    typed.len() <= sequence.len()
        && sequence
            .iter()
            .zip(typed)
            .all(|(step, key)| key.matches_any(std::slice::from_ref(step)))
}

/// What can follow `typed` among `node`'s sequences: the keys left to
/// press, as a hint shows them, and the description.
pub(crate) fn continuations(node: &Node, typed: &[Key]) -> Vec<(String, String)> {
    node.sequences
        .borrow()
        .iter()
        .filter(|bound| bound.keys.len() > typed.len() && starts(bound.live_keys(), typed))
        .map(|bound| {
            let rest = bound.keys[typed.len()..]
                .iter()
                .map(|key| rich_interact::chrome::key_hint(*key))
                .collect::<Vec<_>>()
                .join(" ");
            (rest, bound.description.clone())
        })
        .collect()
}

impl Node {
    /// Run `handler` when `keys` (key names separated by spaces, pressed one
    /// after another: `"g g"`, `"c c"`; the comma key is `comma`, as in any
    /// binding: `"comma s"`) are pressed while this node
    /// or a node inside it has the focus. The first key starts the
    /// sequence where a binding of that key would run (a deeper node's
    /// binding of it wins, as for any key); each next key that goes on with
    /// one of this node's sequences waits for the next, and the last runs
    /// it. Esc cancels a sequence under way; another key cancels it and is
    /// then handled as usual, so `g` then `j` moves down. With
    /// [`App::which_key`](crate::App::which_key) a pop-up lists what can
    /// follow. The help and the command palette list sequences, as
    /// `g g`. On one node, a binding of a sequence's first key wins over
    /// the sequence.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let at = signal("middle");
    ///     text!("{at}")
    ///         .bind_sequence("g g", "Go to the top", move |_| at.set("top"))
    ///         .bind_sequence("g e", "Go to the end", move |_| at.set("end"))
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("g e");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "end");
    /// driver.press("g g");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "top");
    /// ```
    ///
    /// Spaces mean "then" here, where in [`bind`](Self::bind) they mean
    /// "or": `bind("k up", ..)` is either key, `bind_sequence("g g", ..)`
    /// is `g` and then `g`. A sequence is two keys or more; one key is a
    /// `bind`.
    ///
    /// # Panics
    /// On a name that is not a key, on no names at all, or on one key only
    /// (bind it with [`bind`](Self::bind)).
    pub fn bind_sequence(
        self,
        keys: &str,
        description: &str,
        mut handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        let parsed = parse_keys(keys);
        assert!(
            parsed.len() > 1,
            "bind_sequence({keys:?}) has one key: a sequence is keys pressed one after \
             another, so bind one key with bind() (where spaces separate keys that each \
             run it)"
        );
        let handler: KeyHandler = Box::new(move |cx, _| handler(cx));
        self.sequences
            .borrow_mut()
            .push(KeyBinding::new(parsed, description, handler));
        self
    }

    /// [`bind_sequence`](Self::bind_sequence), listed in the help (and the
    /// palette) under `section`, as [`bind_in`](Self::bind_in) lists a
    /// binding.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     label("files")
    ///         .bind_sequence_in("goto", "g h", "Go home", |_| {})
    ///         .bind_in("tabs", "t", "New tab", |_| {})
    ///         .on_key("?", |cx| cx.help())
    /// });
    /// let mut driver = app.driver(60, 16);
    /// driver.press("?");
    /// let rows = driver.screen().plain().join("\n");
    /// assert!(rows.find("goto") < rows.find("Go home"));
    /// assert!(rows.find("tabs") < rows.find("New tab"));
    /// ```
    ///
    /// # Panics
    /// As [`bind_sequence`](Self::bind_sequence).
    pub fn bind_sequence_in(
        self,
        section: &str,
        keys: &str,
        description: &str,
        handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        let node = self.bind_sequence(keys, description, handler);
        if let Some(sequence) = node.sequences.borrow_mut().last_mut() {
            sequence.section = Some(section.to_string());
        }
        node
    }

    /// [`bind_sequence`](Self::bind_sequence) without a description.
    pub fn on_sequence(self, keys: &str, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.bind_sequence(keys, "", handler)
    }

    /// Run `handler` with any key that reaches this node (the focus is on
    /// it or inside it, and nothing deeper used the key) and that none of
    /// its bindings or sequences takes. It returns whether it used the key;
    /// one it did not use goes on to the node's ancestors. A pop-up that
    /// closes on any other key, a key logger, a game.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let last = signal(String::from("-"));
    ///     text!("{last}")
    ///         .on_key("q", |cx| cx.quit())
    ///         .on_any_key(move |_, key| {
    ///             last.set(key.to_string());
    ///             true
    ///         })
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("x ctrl+d");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "ctrl+d");
    /// ```
    pub fn on_any_key(self, handler: impl FnMut(&mut Ctx, Key) -> bool + 'static) -> Node {
        *self.any_key.borrow_mut() = Some(Box::new(handler));
        self
    }

    /// Let this node's bindings, sequences and
    /// [`on_any_key`](Self::on_any_key) see keys before the focused node
    /// inside it does: an app's own keymap that wins over a focused list's
    /// `j`, `k` and `g`, without making the container focusable. Outer
    /// previewing containers go first. A key one of them uses goes no
    /// further.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let picked = signal(0usize);
    ///     let jumped = signal(false);
    ///     column([
    ///         list(|| vec!["a".into(), "b".into(), "c".into()], picked).fixed(3),
    ///         text!("{jumped}").fixed(1),
    ///     ])
    ///     .previews_keys()
    ///     .on_key("end", move |_| jumped.set(true))
    /// });
    /// let mut driver = app.driver(10, 4);
    /// driver.press("end");
    /// // The column took End before the list could.
    /// assert_eq!(driver.screen().plain()[3].trim_end(), "true");
    /// ```
    pub fn previews_keys(mut self) -> Node {
        self.previews = true;
        self
    }
}

/// Draw the which-key pop-up: `entries` (the keys left and what they do)
/// in up to three columns, in a box at the bottom of `area`, a cell in
/// from its edges. Where it drew.
pub(crate) fn draw_which_key(
    console: &Console,
    screen: &mut Screen,
    area: Rect,
    entries: &[(String, String)],
    theme: &Theme,
) -> Option<Rect> {
    if entries.is_empty() || area.width < 8 || area.height < 3 {
        return None;
    }
    let columns = entries.len().min(3);
    let rows = entries.len().div_ceil(columns);
    let width = area.width.saturating_sub(2);
    let height = (rows as u16).saturating_add(2);
    let offset = Offset::new(0, -1, width, height);
    let rect = place(console, None, area, Origin::BottomCenter, offset);
    let border = theme.border(false);
    let lines = crate::node::border("", &border, &border, rect);
    screen.write_lines(rect, &lines);
    let own = |name: &str| {
        theme
            .styles
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, style)| style.clone())
    };
    let key = own("which_key.key")
        .or_else(|| own("accent"))
        .unwrap_or_default();
    let separator = own("which_key.sep").unwrap_or_default();
    let described = own("which_key.desc").unwrap_or_default();
    let inner = Rect::new(
        rect.x + 1,
        rect.y + 1,
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    );
    let cell = inner.width / columns as u16;
    let keys = entries
        .iter()
        .map(|(keys, _)| rich::cells::cell_len(keys))
        .max()
        .unwrap_or(0);
    // What a description has: the column less the key, the separator and
    // the gap before the next column's key.
    let room = (cell as usize).saturating_sub(1 + keys + SEPARATOR.len() + WHICH_KEY_GAP);
    for (i, (keys_left, description)) in entries.iter().enumerate() {
        let (column, row) = ((i / rows) as u16, (i % rows) as u16);
        if row >= inner.height {
            continue;
        }
        let pad = " ".repeat(keys - rich::cells::cell_len(keys_left));
        let line = vec![
            Segment::new(format!(" {keys_left}"), Some(key.clone())),
            Segment::new(pad, None),
            Segment::new(SEPARATOR.to_string(), Some(separator.clone())),
            Segment::new(cut(description, room), Some(described.clone())),
        ];
        let line = Segment::adjust_line_length(&line, cell as usize, None);
        let at = Rect::new(inner.x + column * cell, inner.y + row, cell, 1);
        screen.write_lines(at, &[line]);
    }
    Some(rect)
}

/// What goes between a which-key entry's keys and its description.
const SEPARATOR: &str = "  ";

/// The blank cells, at least, between a which-key column's descriptions
/// and the next column's keys.
const WHICH_KEY_GAP: usize = 2;

/// `text` in at most `width` cells, ending in `…` when cut.
fn cut(text: &str, width: usize) -> String {
    if rich::cells::cell_len(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let kept = rich::cells::set_cell_size(text, width - 1);
    format!("{}…", kept.trim_end())
}
