//! Nodes: the retained tree an app is made of.
//!
//! A [`Node`] is built once and kept. It remembers the rectangle it was
//! laid out in, so the app can route a click to it without the author
//! storing anything, and it draws again only when a [signal](crate::signal)
//! it read changed, or it moved. A frame writes the nodes that drew into
//! the [`Screen`] and reports their rectangles as damage.
//!
//! Builders:
//!
//! - [`text`](fn@text) and the [`text!`](crate::text!) macro: console markup that may
//!   read signals; [`label`] for markup that never changes;
//! - [`renderable`]: any rich renderable (a `Table`, `Markdown`, `Syntax`,
//!   a chart), rebuilt when the signals it read change;
//! - [`column`](fn@column) and [`row`]: children laid out along an axis, each sized
//!   [`Size::Fixed`], [`Size::Percent`], [`Size::Flex`] or
//!   [`Size::Auto`], within [`Node::min_size`] and [`Node::max_size`];
//! - [`grid`]: children in rows and columns, spanning several with
//!   [`Node::span`];
//! - [`each`]: one child per key of a list, kept by key;
//! - [`switch`]: one child at a time, chosen by a key, each kept while
//!   hidden (tabs, wizard steps);
//! - [`Node::panel`]: a rounded border with a title, highlighted while the
//!   focus is inside it; [`Node::padding`] for space round a node.
//!
//! and behaviour: [`Node::on_key`] binds keys (they bubble from the focused
//! node to its ancestors), [`Node::on_click`] handles clicks, and
//! [`Node::focusable`] puts a node in the Tab order.
//!
//! Every node is a [`Widget`]: the builders above make built-in ones, and
//! [`widget`](crate::widget()) makes a node of your own.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use rich::{Console, Overflow, Renderable, Segment, Style};
use rich_interact::{Component, Key, Mouse};

use crate::app::{Ctx, Theme};
use crate::border::{Border, Edge, Shadow, Sides, Title};
use crate::builtin::Described;
use crate::builtin::{
    Each, Grid, Host, HostWidget, Layers, Leaf, Pad, Panel, ScrollView, Stack, Switch, TextNode,
};
pub use crate::layout::Size;
use crate::layout::{Align, AlignContent, Justify, StackLayout, TextAlign, Track};
pub use crate::list::list;
use crate::reactive::{next_node, NodeId, Runtime, Signal};
use crate::screen::{Rect, Screen};
use crate::sheet::{Dock, Element, Layout, States, Stylesheet};
use crate::widget::{Canvas, DrawCx, MeasureCx, ScrollCx, Watchers, Widget};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

pub(crate) type Handler = Box<dyn FnMut(&mut Ctx)>;
/// What a key binding runs, told the key that fired it (the first of its
/// keys when the command palette runs it).
pub(crate) type KeyHandler = Box<dyn FnMut(&mut Ctx, Key)>;

/// A node's key binding, or key sequence: its keys, what the help says of
/// it and under which section, and its handler.
pub(crate) struct KeyBinding {
    pub keys: Vec<Key>,
    pub description: String,
    /// The help's section ([`bind_in`](Node::bind_in)); `None` is the
    /// node's name.
    pub section: Option<String>,
    /// The keys the help and the palette show, when not all of `keys`
    /// ([`bind_scaled`](Node::bind_scaled) shows the plain ones).
    pub shown: Option<Vec<Key>>,
    /// When it applies ([`bind_when`](Node::bind_when)); `None`, always.
    pub when: Option<BindingCondition>,
    /// Why it does not apply now, while it is listed
    /// ([`bind_unless`](Node::bind_unless)); `None`, never unavailable.
    pub unavailable: Option<BindingReason>,
    /// Its keys fire with any modifiers held
    /// ([`any_modifiers`](Node::any_modifiers)).
    pub any_modifiers: bool,
    /// The key goes on to the next handler after it
    /// ([`fall_through`](Node::fall_through)).
    pub fall_through: bool,
    /// Bound, but not listed in the help, the which-key pop-up or a
    /// footer ([`unlisted`](Node::unlisted)); the palette still runs it.
    pub hidden: bool,
    /// Held, it fires again after the first of these and then every
    /// second, rather than on the terminal's repeats
    /// ([`repeat`](Node::repeat)).
    pub repeat: Option<(std::time::Duration, std::time::Duration)>,
    pub handler: KeyHandler,
}

/// When a binding applies: read untracked, as a key comes and as the help
/// lists it.
pub(crate) type BindingCondition = Rc<dyn Fn() -> bool>;

/// Why a binding is unavailable, when it is: read as a
/// [`BindingCondition`] is.
pub(crate) type BindingReason = Rc<dyn Fn() -> Option<String>>;

/// What shows a hidden focus scope ([`Node::focus_scope_reveal`]).
pub(crate) type Reveal = Rc<dyn Fn(&mut Ctx)>;

impl KeyBinding {
    pub(crate) fn new(keys: Vec<Key>, description: &str, handler: KeyHandler) -> KeyBinding {
        KeyBinding {
            keys,
            description: description.to_string(),
            section: None,
            shown: None,
            when: None,
            unavailable: None,
            any_modifiers: false,
            fall_through: false,
            hidden: false,
            repeat: None,
            handler,
        }
    }

    /// Whether it takes `key` only as a modifier-blind binding: one of
    /// its keys with other modifiers (Shift+Tab for `tab`).
    fn takes_bare(&self, key: Key) -> bool {
        let code = |key: &Key| match key.code {
            rich_interact::KeyCode::BackTab => rich_interact::KeyCode::Tab,
            code => code,
        };
        self.any_modifiers && self.live_keys().iter().any(|own| code(own) == code(&key))
    }

    /// Whether it applies now: listed, and not unavailable.
    pub(crate) fn active(&self) -> bool {
        self.listed() && self.reason().is_none()
    }

    /// Whether the help and the palette list it: its condition holds. One
    /// listed may still be [unavailable](Self::reason).
    pub(crate) fn listed(&self) -> bool {
        self.when
            .as_ref()
            .is_none_or(|when| crate::reactive::untracked(|| when()))
    }

    /// Why it is unavailable now, if it is ([`bind_unless`](Node::bind_unless)).
    pub(crate) fn reason(&self) -> Option<String> {
        self.unavailable
            .as_ref()
            .and_then(|reason| crate::reactive::untracked(|| reason()))
    }

    /// Its keys while it applies; none while it does not, so a key passes
    /// it by (to another binding of the key, on this node or above).
    pub(crate) fn live_keys(&self) -> &[Key] {
        if self.active() {
            &self.keys
        } else {
            &[]
        }
    }
}
/// Which of `bindings` takes `key`, those in `skip` left out: one that
/// names the key (as [`Key::pick`] chooses), else a modifier-blind one
/// with its key.
pub(crate) fn pick_binding(key: Key, bindings: &[KeyBinding], skip: &[usize]) -> Option<usize> {
    let live = bindings.iter().enumerate().map(|(i, bound)| {
        if skip.contains(&i) {
            &[][..]
        } else {
            bound.live_keys()
        }
    });
    key.pick(live)
        .or_else(|| (0..bindings.len()).find(|i| !skip.contains(i) && bindings[*i].takes_bare(key)))
}

/// A mouse handler: the event in the node's coordinates; whether it used
/// it.
pub(crate) type MouseHandler = Box<dyn FnMut(&mut Ctx, Mouse) -> bool>;

/// What a drop target does with a value dropped on it.
pub(crate) type DropHandler = Box<dyn FnMut(&dyn Any, &mut Ctx)>;

/// What a node takes when something is dropped on it: the type it accepts,
/// and what it does with the value.
pub(crate) struct DropTarget {
    pub accepts: fn(&dyn Any) -> bool,
    pub handler: RefCell<DropHandler>,
}

/// One node of the tree: a [`Widget`] with a place in the layout, key
/// bindings and handlers.
#[must_use = "a `Node` does nothing until it is placed in a tree"]
pub struct Node {
    pub(crate) id: NodeId,
    pub(crate) size: Size,
    /// Its size read from state ([`size_with`](Node::size_with)), over
    /// `size`.
    size_with: Option<Rc<dyn Fn() -> Size>>,
    pub(crate) min: u16,
    pub(crate) max: u16,
    /// Columns and rows it spans in a [`grid`].
    pub(crate) span: (u16, u16),
    pub(crate) body: RefCell<Body>,
    /// Where it was last laid out.
    pub(crate) rect: Cell<Rect>,
    /// Where it is on the screen, for a [`rect_handle`](Node::rect_handle).
    pub(crate) rect_slot: RefCell<Option<Signal<Option<Rect>>>>,
    /// Where it last drew; `None` before its first draw, or while hidden.
    drawn: Cell<Option<Rect>>,
    /// Whether it was last drawn in its focus style.
    highlit: Cell<bool>,
    pub(crate) focusable: bool,
    /// Takes the focus when its screen opens, before the first focusable
    /// node does.
    pub(crate) autofocus: bool,
    /// Asked for the focus with [`autofocus`](Node::autofocus) but not
    /// focusable of its own (a [`switch`], a panel): the focus passes to
    /// the first node inside it that takes it, now or once built.
    pub(crate) focus_passes: bool,
    /// A [focus scope](Node::focus_scope), and the node inside it that had
    /// the focus last.
    focus_scope: bool,
    scope_last: Cell<Option<NodeId>>,
    /// Shows it when scope cycling reaches it hidden
    /// ([`focus_scope_reveal`](Self::focus_scope_reveal)).
    scope_reveal: Option<(BindingCondition, Reveal)>,
    /// What runs when the focus comes into it, or a node inside it, from
    /// outside (`true`), and when it leaves (`false`).
    pub(crate) focus_handlers: RefCell<Vec<(bool, Handler)>>,
    pub(crate) keys: RefCell<Vec<KeyBinding>>,
    /// Bindings run when a key is let go ([`on_key_up`](Node::on_key_up)).
    pub(crate) releases: RefCell<Vec<KeyBinding>>,
    /// Key sequences ([`bind_sequence`](Node::bind_sequence)), their keys
    /// in order.
    pub(crate) sequences: RefCell<Vec<KeyBinding>>,
    /// What [`on_any_key`](Node::on_any_key) runs.
    pub(crate) any_key: RefCell<Option<crate::sequence::AnyKey>>,
    /// Its bindings see keys before the focused node inside it
    /// ([`previews_keys`](Node::previews_keys)).
    pub(crate) previews: bool,
    pub(crate) click: RefCell<Option<Handler>>,
    pub(crate) mouse: RefCell<Option<MouseHandler>>,
    /// Where the widget wants the text caret, on its surface.
    pub(crate) caret: Cell<Option<(u16, u16)>>,
    /// What the inspector calls it: its [`name`](Node::name), else the
    /// builder that made it.
    name: Option<String>,
    what: &'static str,
    /// A style (or theme style name) laid over the node while it has the
    /// focus.
    focus_style: Option<String>,
    /// The same, while the pointer is over it.
    hover_style: Option<String>,
    /// Whether it was last drawn in its hover style.
    hover_lit: Cell<bool>,
    /// Markup shown when the pointer rests on it, or F1 is pressed while it
    /// has the focus.
    pub(crate) tooltip: Option<String>,
    /// The value a drag from this node carries.
    pub(crate) drag: Option<Rc<dyn Any>>,
    pub(crate) drop: Option<DropTarget>,
    /// What code set, which a stylesheet leaves alone: the size, the
    /// minimum, the maximum, the gap, a grid's rows, and a stack's
    /// alignment, justification, wrapping and line alignment.
    set: [bool; 9],
    /// Where it sits across its parent's axis, when code said.
    align_self: Option<Align>,
    /// Its classes, for stylesheets: fixed ones, and ones that hold while a
    /// condition does.
    classes: Vec<String>,
    class_when: Vec<(String, Condition)>,
    selected_when: Option<Condition>,
    disabled_when: Option<Condition>,
    error_when: Option<Condition>,
    /// Why it is in error, while it is ([`error_message`](Node::error_message)).
    error_message: Option<Box<dyn Fn() -> Option<String>>>,
    /// Conditions on the app's [`viewport`](crate::viewport()) that must
    /// all hold for it to be shown ([`hide_below`](Node::hide_below) and
    /// the like), each with the rule the inspector names it by.
    shown_when: Vec<(String, Condition)>,
    /// Whether it was last drawn in the theme's error look.
    error_lit: Cell<bool>,
    /// What the stylesheet decided for it.
    sheet: RefCell<SheetNode>,
    /// Where its widget lays out and draws: its rectangle less the
    /// stylesheet's border and padding.
    pub(crate) inner: Cell<Rect>,
    /// For assistive technology: its role and name when code gave them,
    /// whether its changes are announced, and what it showed last.
    role: Option<crate::a11y::Role>,
    access_label: Option<String>,
    live: bool,
    last_live: RefCell<Option<String>>,
    /// Left out, with what is inside, for assistive technology; and the
    /// states code gave it.
    access_hidden: bool,
    expanded_when: Option<Condition>,
    checked_when: Option<Condition>,
    busy_when: Option<Condition>,
    /// A [`list`]'s selection and scroll, for
    /// [`reorderable`](Node::reorderable).
    pub(crate) list_model: Option<Rc<crate::reorder::ListModel>>,
    /// The list options given a [`switch`] to pass on to the lists it
    /// builds, now and later.
    pub(crate) list_forward: Option<crate::list::Forward>,
    /// How many pages a [`switch`] keeps ([`keep_last`](Node::keep_last)).
    keep_pages: Option<Rc<Cell<usize>>>,
    /// Its own colours and text style ([`style`](Node::style),
    /// [`bg`](Node::bg), [`fg`](Node::fg)), under what it draws.
    own_style: Option<OwnStyle>,
    /// Theme styles that hold inside it ([`theme_styles`](Node::theme_styles)),
    /// and the console and theme its subtree draws with, made from them.
    theme_styles: Vec<(String, String)>,
    scope: RefCell<Option<Rc<Scope>>>,
    /// In [`layers`]: its inset from the layers' edges, and where it sits
    /// at its content's size.
    pub(crate) inset: Option<[u16; 4]>,
    pub(crate) place: Option<(Align, Align)>,
    /// What runs when it leaves the tree ([`on_unmount`](Node::on_unmount)).
    unmount: Vec<Unmount>,
}

/// An [`on_unmount`](Node::on_unmount) handler, run when its node is
/// dropped: when it leaves the tree.
struct Unmount(Option<Box<dyn FnOnce()>>);

impl Drop for Unmount {
    fn drop(&mut self) {
        if let Some(handler) = self.0.take() {
            handler();
        }
    }
}

/// A node's own style: a style (or theme style name), a foreground and a
/// background, each a colour or a theme style's colour.
#[derive(Clone, Debug, Default)]
struct OwnStyle {
    style: Option<String>,
    fg: Option<String>,
    bg: Option<String>,
}

impl OwnStyle {
    /// The style, its names looked up in `console`'s theme.
    fn resolve(&self, console: &Console) -> Style {
        let mut out = match &self.style {
            Some(name) => crate::widget::theme_style(console, name, ""),
            None => Style::default(),
        };
        // A colour, or a theme style's: its colour, else its background,
        // as a stylesheet's `$name` is.
        let color = |name: &str, background: bool| {
            let style = crate::widget::theme_style(console, name, "");
            let (first, second) = match background {
                true => (style.bgcolor(), style.color()),
                false => (style.color(), style.bgcolor()),
            };
            first
                .or(second)
                .cloned()
                .or_else(|| rich::Color::parse(name).ok())
        };
        if let Some(fg) = self.fg.as_deref().and_then(|name| color(name, false)) {
            out = out.combine(&Style::from_color(Some(fg), None));
        }
        if let Some(bg) = self.bg.as_deref().and_then(|name| color(name, true)) {
            out = out.combine(&Style::from_color(None, Some(bg)));
        }
        out
    }
}

impl Node {
    /// Whether its style (code's or the stylesheet's) sets a background.
    fn has_background(&self, console: &Console) -> bool {
        let own = self
            .own_style
            .as_ref()
            .is_some_and(|own| own.resolve(console).bgcolor().is_some());
        own || self
            .sheet
            .borrow()
            .style
            .as_ref()
            .is_some_and(|style| style.bgcolor().is_some())
    }
}

/// The console and theme a subtree draws with when a node gives it theme
/// styles of its own ([`Node::theme_styles`]): copies made once, and again
/// when what they were made over changes.
pub(crate) struct Scope {
    pub console: Console,
    pub theme: Theme,
    /// The theme it was made over (`None`: made while measuring, before a
    /// theme was at hand), and its console's size.
    made_over: Option<Theme>,
    size: (usize, usize),
}

impl Scope {
    fn new(console: &Console, theme: Option<&Theme>, styles: &[(String, String)]) -> Scope {
        let mut scoped = console.clone();
        let mut own = theme.cloned().unwrap_or_default();
        let mut named = rich::Theme::new();
        for (name, definition) in styles {
            // Theme names in a definition are the ones around it.
            let style = crate::widget::theme_style(console, definition, "");
            named.insert(name.clone(), style.clone());
            match name.as_str() {
                "border" => own.border = style,
                "border.focused" => own.border_focused = style,
                "title" => own.title = style,
                _ => own = own.style(name, style),
            }
        }
        scoped.push_theme(named, true);
        Scope {
            console: scoped,
            theme: own,
            made_over: theme.cloned(),
            size: (console.width(), console.height()),
        }
    }
}

/// Whether two themes style everything the same.
fn same_theme(a: &Theme, b: &Theme) -> bool {
    a.border == b.border
        && a.border_focused == b.border_focused
        && a.title == b.title
        && a.styles == b.styles
}

/// A condition a node's class or state follows.
type Condition = Box<dyn Fn() -> bool>;

const SET_SIZE: usize = 0;
const SET_MIN: usize = 1;
const SET_MAX: usize = 2;
const SET_GAP: usize = 3;
const SET_ROWS: usize = 4;
const SET_ALIGN_ITEMS: usize = 5;
const SET_JUSTIFY: usize = 6;
const SET_WRAP: usize = 7;
const SET_ALIGN_CONTENT: usize = 8;

/// A node's part of the stylesheet.
#[derive(Default)]
struct SheetNode {
    /// The sheet it was matched against (0: none yet).
    generation: u64,
    layout: Layout,
    /// Whether a rule with a state could match it, and which states.
    watch: (bool, bool, bool),
    /// The border it draws round its widget (a panel draws its own); its
    /// titles and sides are the layout's.
    decor: Option<(Border, Style)>,
    insets: [u16; 4],
    /// The style it was last drawn in.
    style: Option<Style>,
    /// Its conditional classes that held, and whether it was selected,
    /// disabled and in error, when it last drew.
    active: Vec<String>,
    selected: bool,
    disabled: bool,
    error: bool,
}

/// The stylesheet as a frame draws with it.
pub(crate) struct SheetCx<'a> {
    pub sheet: &'a Stylesheet,
    pub generation: u64,
    pub vars: &'a dyn Fn(&str) -> Option<Style>,
    /// The app's [`viewport`](crate::viewport()), for `@media` rules.
    pub viewport: (u16, u16),
}

/// A node's widget, and what the framework keeps for it between frames.
pub(crate) struct Body {
    pub widget: Box<dyn Widget>,
    /// Where its children were laid out last, and which children.
    areas: Vec<Rect>,
    laid: Vec<NodeId>,
    /// A viewport's content, drawn offscreen, and what of it is shown.
    view: Option<View>,
}

/// A [viewport](Widget::viewport)'s offscreen content.
struct View {
    buffer: Screen,
    /// The window on the widget's surface, and the content's point at its
    /// top left, when it was last copied out.
    shown: Option<(Rect, (u16, u16))>,
}

impl Node {
    pub(crate) fn from_widget(widget: Box<dyn Widget>, what: &'static str) -> Node {
        Node {
            id: next_node(),
            size: Size::Flex(1),
            size_with: None,
            min: 0,
            max: u16::MAX,
            span: (1, 1),
            body: RefCell::new(Body {
                widget,
                areas: Vec::new(),
                laid: Vec::new(),
                view: None,
            }),
            rect: Cell::new(Rect::default()),
            rect_slot: RefCell::new(None),
            drawn: Cell::new(None),
            highlit: Cell::new(false),
            focusable: false,
            autofocus: false,
            focus_passes: false,
            focus_scope: false,
            scope_last: Cell::new(None),
            scope_reveal: None,
            focus_handlers: RefCell::new(Vec::new()),
            keys: RefCell::new(Vec::new()),
            releases: RefCell::new(Vec::new()),
            sequences: RefCell::new(Vec::new()),
            any_key: RefCell::new(None),
            previews: false,
            click: RefCell::new(None),
            mouse: RefCell::new(None),
            caret: Cell::new(None),
            name: None,
            what,
            focus_style: None,
            hover_style: None,
            hover_lit: Cell::new(false),
            tooltip: None,
            drag: None,
            drop: None,
            set: [false; 9],
            align_self: None,
            classes: Vec::new(),
            class_when: Vec::new(),
            selected_when: None,
            disabled_when: None,
            error_when: None,
            error_message: None,
            shown_when: Vec::new(),
            error_lit: Cell::new(false),
            sheet: RefCell::new(SheetNode::default()),
            inner: Cell::new(Rect::default()),
            role: None,
            access_label: None,
            live: false,
            last_live: RefCell::new(None),
            access_hidden: false,
            expanded_when: None,
            checked_when: None,
            busy_when: None,
            list_model: None,
            list_forward: None,
            keep_pages: None,
            own_style: None,
            theme_styles: Vec::new(),
            scope: RefCell::new(None),
            inset: None,
            place: None,
            unmount: Vec::new(),
        }
    }

    pub(crate) fn what(mut self, what: &'static str) -> Node {
        self.what = what;
        self
    }

    /// Name this node for the [inspector](crate::App::inspector).
    pub fn name(mut self, name: &str) -> Node {
        self.name = Some(name.to_string());
        self
    }

    /// Its [name](Self::name), if it was given one.
    pub(crate) fn given_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Its name, if it was given one: how the command palette and the help
    /// group its bindings.
    pub(crate) fn group_name(&self) -> String {
        self.name.clone().unwrap_or_default()
    }

    /// Say what this node is, for assistive technology, when its widget
    /// does not (a [`label`](fn@label) that acts as a button, a `row` that
    /// is a toolbar).
    pub fn role(mut self, role: crate::a11y::Role) -> Node {
        self.role = Some(role);
        self
    }

    /// Its accessible name: what a screen reader calls it. Without one, a
    /// node is named by the text it shows (text, buttons, text boxes) or
    /// its title (a panel).
    pub fn label(mut self, name: impl Into<String>) -> Node {
        self.access_label = Some(name.into());
        self
    }

    /// Announce what this node shows whenever it changes (a status line, a
    /// counter): its role becomes [`Status`](crate::a11y::Role::Status)
    /// unless set.
    pub fn live(mut self) -> Node {
        self.live = true;
        self
    }

    /// Leave this node, and everything inside it, out of what assistive
    /// technology gets (ARIA's `aria-hidden`): the accessibility tree,
    /// linear mode's lines, announcements and the names of what holds it.
    /// It is still drawn. Nothing inside it is in the Tab order or takes
    /// the focus when its screen opens (a click still reaches it). For what
    /// is only decoration: a divider, a spinner's glyph, a logo.
    pub fn access_hidden(mut self, hidden: bool) -> Node {
        self.access_hidden = hidden;
        self
    }

    /// Whether it is [hidden](Self::access_hidden) from assistive
    /// technology.
    pub(crate) fn is_access_hidden(&self) -> bool {
        self.access_hidden
    }

    /// Expanded while `condition` holds, collapsed while it does not (ARIA's
    /// `aria-expanded`): a section that opens, a disclosure. `condition`
    /// may read signals.
    pub fn expanded_when(mut self, condition: impl Fn() -> bool + 'static) -> Node {
        self.expanded_when = Some(Box::new(condition));
        self
    }

    /// Checked while `condition` holds, not checked while it does not
    /// (ARIA's `aria-checked`). Its role becomes
    /// [`CheckBox`](crate::a11y::Role::CheckBox) unless set; set
    /// [`Switch`](crate::a11y::Role::Switch) for a toggle. The stock
    /// [`checkbox`](crate::checkbox) and
    /// [`switch_toggle`](crate::switch_toggle) are checked by their
    /// signals already; this is for a node of your own that is on or off.
    ///
    /// ```
    /// use intuituive::a11y::Role;
    /// use intuituive::checkbox;
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let (wrap, recording) = (signal(true), signal(false));
    ///     column([
    ///         checkbox("Wrap lines", wrap),
    ///         text(move || if recording.get() { "● REC" } else { "○ REC" }.into())
    ///             .label("Recording")
    ///             .role(Role::Switch)
    ///             .checked_when(move || recording.get()),
    ///     ])
    /// });
    /// let mut driver = app.driver(16, 2);
    /// driver.settle();
    /// let read: Vec<String> = driver.accessibility().iter().map(|n| n.describe()).collect();
    /// assert_eq!(read, ["Wrap lines, check box, checked", "Recording, switch, not checked"]);
    /// ```
    pub fn checked_when(mut self, condition: impl Fn() -> bool + 'static) -> Node {
        self.checked_when = Some(Box::new(condition));
        self
    }

    /// Busy while `condition` holds (ARIA's `aria-busy`): loading, a task
    /// running.
    pub fn busy_when(mut self, condition: impl Fn() -> bool + 'static) -> Node {
        self.busy_when = Some(Box::new(condition));
        self
    }

    /// Its states: its widget's, with what code set over them
    /// ([`selected_when`](Self::selected_when),
    /// [`expanded_when`](Self::expanded_when) and the like).
    pub fn access_state(&self) -> crate::a11y::AccessState {
        self.access_state_of(&*self.body.borrow().widget)
    }

    /// [`access_state`](Self::access_state), with its widget in hand (while
    /// it draws, the body is borrowed).
    fn access_state_of(&self, widget: &dyn Widget) -> crate::a11y::AccessState {
        let mut state = widget.access_state();
        let holds = |condition: &Option<Condition>| condition.as_ref().map(|c| c());
        if let Some(open) = holds(&self.expanded_when) {
            state.expanded = Some(open);
        }
        if let Some(on) = holds(&self.checked_when) {
            state.checked = Some(on);
        }
        state.selected |= holds(&self.selected_when).unwrap_or(false);
        state.busy |= holds(&self.busy_when).unwrap_or(false);
        state.invalid |= holds(&self.error_when).unwrap_or(false) || self.error().is_some();
        state
    }

    /// What it is: as code said, else as its widget says, else by the
    /// builder that made it (text, a list), a clickable text being a button.
    pub fn access_role(&self) -> crate::a11y::Role {
        use crate::a11y::Role;
        if let Some(role) = self.role {
            return role;
        }
        if self.live {
            return Role::Status;
        }
        if self.checked_when.is_some() {
            return Role::CheckBox;
        }
        let role = self.body.borrow().widget.role();
        if role != Role::Group {
            return role;
        }
        // A widget that says what it shows is read as text.
        if self.body.borrow().widget.access_text().is_some() {
            return Role::Text;
        }
        let clickable = self.click.borrow().is_some();
        match self.what {
            "text" | "label" | "renderable" | "leaf" if clickable => Role::Button,
            "text" | "label" | "renderable" => Role::Text,
            "list" => Role::List,
            _ => Role::Group,
        }
    }

    /// Its accessible name, if code gave one; a panel's is its title. A
    /// [`described`] node's is what it says, after the name code gave, as
    /// is a widget's [`access_text`](Widget::access_text).
    pub(crate) fn access_label(&self) -> Option<String> {
        let says = self
            .widget_ref(|d: &Described| d.text.clone())
            .flatten()
            .or_else(|| self.body.borrow().widget.access_text())
            .filter(|text| !text.is_empty());
        match (self.access_label.clone(), says) {
            (Some(label), Some(text)) => return Some(format!("{label}: {text}")),
            (label, text) => {
                if let Some(name) = label.or(text) {
                    return Some(name);
                }
            }
        }
        self.widget_mut(|panel: &mut Panel| panel.shown_title())
            .filter(|title| !title.is_empty())
    }

    /// Its widget's [value](Widget::access_value) for assistive
    /// technology, if it gives one.
    pub(crate) fn access_value(&self) -> Option<String> {
        self.body.borrow().widget.access_value()
    }

    /// Where its selected item is, on its widget's surface.
    pub(crate) fn cursor(&self) -> Option<Rect> {
        self.body.borrow().widget.cursor()
    }

    /// For a [live](Self::live) node: what it shows now, when that changed
    /// since it was last asked.
    pub(crate) fn live_change(&self, shown: String) -> Option<String> {
        if !self.live {
            return None;
        }
        let mut last = self.last_live.borrow_mut();
        let first = last.is_none();
        if last.as_deref() == Some(shown.as_str()) {
            return None;
        }
        *last = Some(shown.clone());
        // Its first text is not a change.
        (!first && !shown.is_empty()).then_some(shown)
    }

    /// How the inspector shows this node: its name or builder, and what it
    /// holds.
    pub(crate) fn describe(&self) -> String {
        let classes: String = self.classes.iter().map(|c| format!(".{c}")).collect();
        let base = match &self.name {
            Some(name) => format!("{name} ({}{classes})", self.what),
            None => format!("{}{classes}", self.what),
        };
        let base = match self.align_self {
            Some(align) => format!("{base} align-self:{}", crate::builtin::css_align(align)),
            None => base,
        };
        match self.body.borrow().widget.describe() {
            Some(more) => format!("{base} {more}"),
            None => base,
        }
    }

    /// How the inspector's trace and timings name it: its name, else its
    /// builder and id (`text#7`).
    pub(crate) fn short(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("{}#{}", self.what, self.id),
        }
    }

    /// The widget, if it is a `W`, read only: for a node whose children are
    /// being visited, which holds its widget borrowed meanwhile.
    pub(crate) fn widget_ref<W: Widget, R>(&self, f: impl FnOnce(&W) -> R) -> Option<R> {
        let body = self.body.borrow();
        let widget: &dyn std::any::Any = &*body.widget;
        widget.downcast_ref::<W>().map(f)
    }

    /// Visit this node and every node inside it, depth first, with its
    /// path and whether it is hidden: what [`walk`](Self::walk) leaves out
    /// is visited too (for the inspector), but not what is inside it.
    pub(crate) fn walk_all(&self, f: &mut dyn FnMut(&Node, &[NodeId], bool)) {
        fn inner(node: &Node, path: &mut Vec<NodeId>, f: &mut dyn FnMut(&Node, &[NodeId], bool)) {
            path.push(node.id);
            let hidden = node.hidden();
            f(node, path, hidden);
            if !hidden {
                node.each_child(&mut |child| inner(child, path, f));
            }
            path.pop();
        }
        inner(self, &mut Vec::new(), f);
    }

    /// For a [`grid`]: where its child `id` is placed, its first column and
    /// row and how many of each it spans (0-based), if it is shown.
    pub(crate) fn grid_place(&self, id: NodeId) -> Option<crate::layout::Cellspan> {
        self.widget_ref(|grid: &Grid| {
            let shown: Vec<&Node> = grid.children.iter().filter(|c| !c.hidden()).collect();
            let spans: Vec<(u16, u16)> = shown.iter().map(|c| c.span).collect();
            let (places, _) = crate::layout::place(grid.columns.len().max(1), &spans);
            shown
                .iter()
                .position(|c| c.id == id)
                .and_then(|at| places.get(at).copied())
        })
        .flatten()
    }

    /// What the inspector's node pane shows of it: its layout as code and
    /// the stylesheet set it, its focus, its rules and states. Reads the
    /// signals its conditions read (call it untracked).
    pub(crate) fn facts(&self) -> crate::inspect::Facts {
        let sheet = self.sheet.borrow();
        let layout = &sheet.layout;
        let (size, size_from) = match (self.set[SET_SIZE], layout.size) {
            (true, _) => (self.size, Some("code")),
            (false, Some(size)) => (size, Some("stylesheet")),
            (false, None) => (self.size, None),
        };
        let min = if self.set[SET_MIN] {
            self.min
        } else {
            layout.min.unwrap_or(self.min)
        };
        let max = if self.set[SET_MAX] {
            self.max
        } else {
            layout.max.unwrap_or(self.max)
        };
        let dock = layout.dock.map(|dock| match dock {
            Dock::Top => "top",
            Dock::Bottom => "bottom",
            Dock::Left => "left",
            Dock::Right => "right",
        });
        let border = sheet.decor.as_ref().map(|(kind, _)| kind.css());
        let padding = if sheet.decor.is_some() || sheet.insets != [0; 4] {
            layout.padding
        } else {
            [0; 4]
        };
        let sheet_hidden = layout.hidden;
        drop(sheet);
        let flow = self.widget_ref(|stack: &Stack| (stack.axis, stack.flow));
        let grid =
            self.widget_ref(|grid: &Grid| (grid.columns.clone(), grid.rows.clone(), grid.gap));
        let edges = self
            .widget_ref(|pad: &Pad| pad.edges)
            .or_else(|| self.widget_ref(|panel: &Panel| panel.look.padding));
        let (active, selected, disabled, invalid) = self.conditions();
        let inactive = self
            .class_when
            .iter()
            .map(|(class, _)| class.clone())
            .filter(|class| !active.contains(class))
            .collect();
        let (_, focus, hover) = self.sheet.borrow().watch;
        let joined = |keys: &[Key], with: &str| {
            keys.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(with)
        };
        let mut keys: Vec<String> = self
            .keys
            .borrow()
            .iter()
            .map(|bound| joined(&bound.keys, "/"))
            .collect();
        keys.extend(
            self.sequences
                .borrow()
                .iter()
                .map(|bound| joined(&bound.keys, " ")),
        );
        let mut handlers = Vec::new();
        if self.any_key.borrow().is_some() {
            handlers.push("on_any_key");
        }
        if self.previews {
            handlers.push("previews_keys");
        }
        if self.click.borrow().is_some() {
            handlers.push("on_click");
        }
        if self.mouse.borrow().is_some() {
            handlers.push("on_mouse");
        }
        if self.drag.is_some() {
            handlers.push("draggable");
        }
        if self.drop.is_some() {
            handlers.push("on_drop");
        }
        if self.tooltip.is_some() {
            handlers.push("tooltip");
        }
        crate::inspect::Facts {
            size,
            size_from,
            min,
            max,
            span: self.span,
            align_self: self.self_align(),
            dock,
            border,
            padding,
            flow,
            grid,
            edges,
            focusable: self.focusable,
            autofocus: self.autofocus,
            disabled,
            rules: self
                .shown_when
                .iter()
                .map(|(rule, holds)| (rule.clone(), holds()))
                .collect(),
            sheet_hidden,
            access_hidden: self.access_hidden,
            classes: self.classes.clone(),
            active,
            inactive,
            selected,
            invalid,
            watches: (focus, hover),
            role: self.access_role(),
            keys,
            handlers,
            focus_style: self.focus_style.clone(),
            hover_style: self.hover_style.clone(),
        }
    }

    /// The widget, if it is a `W`.
    pub(crate) fn widget_mut<W: Widget, R>(&self, f: impl FnOnce(&mut W) -> R) -> Option<R> {
        let mut body = self.body.borrow_mut();
        let widget: &mut dyn std::any::Any = &mut *body.widget;
        widget.downcast_mut::<W>().map(f)
    }

    /// Size this node along its parent's axis.
    pub fn size(mut self, size: Size) -> Node {
        self.size = size;
        self.size_with = None;
        self.set[SET_SIZE] = true;
        self
    }

    /// Size this node along its parent's axis by `size`, read whenever the
    /// parent lays out: it may read signals, and the parent lays out again
    /// when they change. Panes that trade room as the focus moves (40/60
    /// one way, 60/40 the other), a pane that takes the whole width while
    /// it is zoomed. Code's size wins over a stylesheet's, as
    /// [`size`](Self::size)'s does.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let wide = signal(false);
    ///     row([
    ///         label("a").size_with(move || match wide.get() {
    ///             true => Size::Fixed(6),
    ///             false => Size::Fixed(2),
    ///         }),
    ///         label("|b"),
    ///     ])
    ///     .on_key("w", move |_| wide.set(true))
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "a |b");
    /// driver.press("w");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "a     |b");
    /// ```
    pub fn size_with(mut self, size: impl Fn() -> Size + 'static) -> Node {
        self.size_with = Some(Rc::new(size));
        self.set[SET_SIZE] = true;
        self
    }

    /// Exactly `cells` along the parent's axis.
    pub fn fixed(self, cells: u16) -> Node {
        self.size(Size::Fixed(cells))
    }

    /// A flexible share, by `weight`, of the space left over.
    pub fn flex(self, weight: u16) -> Node {
        self.size(Size::Flex(weight))
    }

    /// `percent` of the parent's axis.
    pub fn percent(self, percent: u16) -> Node {
        self.size(Size::Percent(percent))
    }

    /// As much of the parent's axis as the content needs (see
    /// [`Size::Auto`]).
    pub fn auto(self) -> Node {
        self.size(Size::Auto)
    }

    /// Never less than `cells` along the parent's axis, while there is
    /// room.
    pub fn min_size(mut self, cells: u16) -> Node {
        self.min = cells;
        self.set[SET_MIN] = true;
        self
    }

    /// Never more than `cells` along the parent's axis.
    pub fn max_size(mut self, cells: u16) -> Node {
        self.max = cells;
        self.set[SET_MAX] = true;
        self
    }

    /// In a [`grid`]: span `columns` columns and `rows` rows.
    pub fn span(mut self, columns: u16, rows: u16) -> Node {
        self.span = (columns.max(1), rows.max(1));
        self
    }

    /// For a [`column`](fn@column), [`row`] or [`grid`]: leave `cells` empty
    /// between neighbouring children. Ignored on other nodes.
    pub fn gap(mut self, cells: u16) -> Node {
        self.widget_mut(|stack: &mut Stack| stack.flow.gap = cells);
        self.widget_mut(|grid: &mut Grid| grid.gap = (cells, cells));
        self.set[SET_GAP] = true;
        self
    }

    /// For a [`column`](fn@column) or [`row`]: where the children sit
    /// across its axis, each as wide (in a column) or tall (in a row) as its
    /// content, unless it says otherwise with [`align_self`](Self::align_self).
    /// [`Align::Stretch`], the default, gives them the whole width or
    /// height. CSS's `align-items`. Ignored on other nodes.
    pub fn align_items(mut self, align: Align) -> Node {
        self.widget_mut(|stack: &mut Stack| stack.flow.align_items = align);
        self.set[SET_ALIGN_ITEMS] = true;
        self
    }

    /// In a [`column`](fn@column) or [`row`]: where this child sits across
    /// the parent's axis, whatever the parent's
    /// [`align_items`](Self::align_items) says. CSS's `align-self`.
    pub fn align_self(mut self, align: Align) -> Node {
        self.align_self = Some(align);
        self
    }

    /// For a [`column`](fn@column) or [`row`]: where the children sit along
    /// its axis when they leave room over (a flexible child leaves none).
    /// CSS's `justify-content`. Ignored on other nodes.
    pub fn justify(mut self, justify: Justify) -> Node {
        self.widget_mut(|stack: &mut Stack| stack.flow.justify = justify);
        self.set[SET_JUSTIFY] = true;
        self
    }

    /// For a [`column`](fn@column) or [`row`]: children that do not fit go
    /// on a new line (a new column, in a column), each line laid out on its
    /// own, [`gap`](Self::gap) apart. Each child starts at its own size, or
    /// its content's for a flexible one, and the flexible ones share what
    /// their line leaves. Children are not docked in a wrapping stack. CSS's
    /// `flex-wrap: wrap`. Ignored on other nodes.
    pub fn wrap(mut self) -> Node {
        self.widget_mut(|stack: &mut Stack| stack.flow.wrap = true);
        self.set[SET_WRAP] = true;
        self
    }

    /// For a [`column`](fn@column) or [`row`]: how the lengths of its
    /// children are solved. [`Solver::Ratatui`](crate::Solver::Ratatui)
    /// solves them as ratatui's `Layout` does: percentages and ratios of
    /// the whole length rather than of what the gaps leave, and what does
    /// not fit squeezed as ratatui squeezes it, so a row ported from a
    /// ratatui `Table` or `Layout` puts its cells in the same columns. The
    /// [`justify`](Self::justify) is its `Flex` (`Start` by default). A
    /// wrapping stack keeps the native solver. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::Solver;
    ///
    /// let cells = || [label("a").percent(50), label("b").percent(50)];
    /// // Native: half each of the 19 cells the gap leaves.
    /// let native = App::new(move || row(cells()).gap(1));
    /// let mut driver = native.driver(20, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "a         b         ");
    /// // Ratatui: half each of the 20, the second squeezed by the gap.
    /// let ratatui = App::new(move || row(cells()).gap(1).solver(Solver::Ratatui));
    /// let mut driver = ratatui.driver(20, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "a          b        ");
    /// ```
    pub fn solver(self, solver: crate::solver::Solver) -> Node {
        self.widget_mut(|stack: &mut Stack| stack.flow.solver = solver);
        self
    }

    /// For a [`wrap`](Self::wrap)ping [`column`](fn@column) or [`row`]:
    /// where the lines sit across its axis when they leave room over.
    /// [`AlignContent::Stretch`], the default, grows them to share it.
    /// CSS's `align-content`. Ignored on other nodes.
    pub fn align_content(mut self, align: AlignContent) -> Node {
        self.widget_mut(|stack: &mut Stack| stack.flow.align_content = align);
        self.set[SET_ALIGN_CONTENT] = true;
        self
    }

    /// For a [`grid`]: the rows' sizes. Rows past the last one given take
    /// its size, so `.rows([Size::Auto])` makes every row as tall as its
    /// content; without any, rows share the height evenly. Ignored on other
    /// nodes.
    pub fn rows(mut self, rows: impl IntoIterator<Item = Size>) -> Node {
        let rows: Vec<Size> = rows.into_iter().collect();
        self.widget_mut(|grid: &mut Grid| grid.rows = rows);
        self.set[SET_ROWS] = true;
        self
    }

    /// Give this node `classes` (space-separated), for a
    /// [stylesheet](crate::App::stylesheet) to match with `.name`.
    pub fn class(mut self, classes: &str) -> Node {
        self.classes
            .extend(classes.split_whitespace().map(str::to_string));
        self
    }

    /// Give this node `class` while `condition` holds: a stylesheet's
    /// colours and text styles for `.class` follow it (its layout does not;
    /// that comes from fixed classes). `condition` may read signals.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let load = signal(95);
    ///     text!("load {load}").class_when("danger", move || load.get() > 90)
    /// })
    /// .stylesheet(".danger { text-style: bold; }");
    /// ```
    pub fn class_when(mut self, class: &str, condition: impl Fn() -> bool + 'static) -> Node {
        self.class_when
            .push((class.to_string(), Box::new(condition)));
        self
    }

    /// Match a stylesheet's `:selected` while `condition` holds.
    pub fn selected_when(mut self, condition: impl Fn() -> bool + 'static) -> Node {
        self.selected_when = Some(Box::new(condition));
        self
    }

    /// Match a stylesheet's `:disabled` while `condition` holds. A disabled
    /// node also leaves the Tab order, and its click and key handlers do
    /// not run.
    pub fn disabled_when(mut self, condition: impl Fn() -> bool + 'static) -> Node {
        self.disabled_when = Some(Box::new(condition));
        self
    }

    /// Whether it is [disabled](Self::disabled_when) now. Callers outside a
    /// draw read it untracked.
    pub(crate) fn disabled(&self) -> bool {
        self.disabled_when
            .as_ref()
            .is_some_and(|condition| condition())
    }

    /// In error while `condition` holds: a field whose value is wrong, a
    /// form that cannot be sent. It matches a stylesheet's `:error` (or
    /// CSS's `:invalid`), and the theme gives it a look of its own: a
    /// [panel](Self::panel)'s border in the theme's `border.error` style,
    /// another node's text in its `error` style (under the text's own
    /// colours, as a stylesheet's go), each the theme's `bad` style unless
    /// the theme names it. Assistive technology is told it is invalid
    /// (ARIA's `aria-invalid`). `condition` may read signals.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let age = signal(String::from("abc"));
    ///     text!("Age: {age}")
    ///         .label("Age")
    ///         .error_when(move || age.with(|a| a.parse::<u8>().is_err()))
    /// })
    /// .stylesheet(":error { text-style: underline; }");
    /// let mut driver = app.driver(12, 1);
    /// driver.settle();
    /// assert!(driver.accessibility()[0].state.invalid);
    /// assert_eq!(driver.accessibility()[0].describe(), "Age, invalid");
    /// ```
    pub fn error_when(mut self, condition: impl Fn() -> bool + 'static) -> Node {
        self.error_when = Some(Box::new(condition));
        self
    }

    /// Whether it is [in error](Self::error_when) now.
    pub(crate) fn invalid(&self) -> bool {
        self.error_when
            .as_ref()
            .is_some_and(|condition| condition())
            || self.error().is_some()
    }

    /// In error while `message` returns one, which says why: as
    /// [`error_when`](Self::error_when) (either makes the node invalid),
    /// and assistive technology is given the message
    /// ([`AccessNode::error`](crate::a11y::AccessNode::error)). An
    /// [`input`](crate::input)'s [`validate`](Self::validate) uses it and
    /// shows the message too. `message` may read signals.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let name = signal(String::new());
    ///     text!("Name: {name}")
    ///         .label("Name")
    ///         .error_message(move || name.with(|n| n.is_empty().then(|| "required".to_string())))
    /// });
    /// let mut driver = app.driver(12, 1);
    /// driver.settle();
    /// assert_eq!(driver.accessibility()[0].describe(), "Name, invalid: required");
    /// ```
    pub fn error_message(mut self, message: impl Fn() -> Option<String> + 'static) -> Node {
        self.error_message = Some(Box::new(message));
        self
    }

    /// Why it is in error now, from [`error_message`](Self::error_message).
    pub(crate) fn error(&self) -> Option<String> {
        self.error_message.as_ref().and_then(|message| message())
    }

    /// The caret's shape its widget wants while it has the focus.
    pub(crate) fn wanted_caret_shape(&self) -> Option<rich_interact::caret::CaretShape> {
        self.body.borrow().widget.caret_shape()
    }

    /// Hide this node while the app is narrower than `columns` cells (its
    /// [`viewport`](crate::viewport())'s width): a sidebar that gives way
    /// on a small terminal. Hidden, it is as if a stylesheet said
    /// `display: none`: it takes no room, and it and what is inside it
    /// leave the Tab order. It comes back when the terminal is wide enough.
    /// `@media (max-width: N)` in a stylesheet does the same from outside
    /// the code (`hide_below(80)` is `max-width: 79`).
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| row([label("nav").fixed(5).hide_below(20), label("main")]));
    /// let mut driver = app.driver(24, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "nav  main");
    /// driver.resize(12, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "main");
    /// ```
    pub fn hide_below(self, columns: u16) -> Node {
        let rule = format!("hide_below {columns}");
        self.when_viewport(rule, move |(width, _)| width >= columns)
    }

    /// Show this node only while the app is narrower than `columns` cells:
    /// a compact stand-in for what [`hide_below`](Self::hide_below) hides
    /// (`@media (min-width: N)` with `display: none` in a stylesheet).
    pub fn show_below(self, columns: u16) -> Node {
        let rule = format!("show_below {columns}");
        self.when_viewport(rule, move |(width, _)| width < columns)
    }

    /// Hide this node while the app is shorter than `rows` cells, as
    /// [`hide_below`](Self::hide_below) does for the width.
    pub fn hide_below_height(self, rows: u16) -> Node {
        let rule = format!("hide_below_height {rows}");
        self.when_viewport(rule, move |(_, height)| height >= rows)
    }

    /// Show this node only while the app is shorter than `rows` cells.
    pub fn show_below_height(self, rows: u16) -> Node {
        let rule = format!("show_below_height {rows}");
        self.when_viewport(rule, move |(_, height)| height < rows)
    }

    /// Show this node only while `shown` holds: it may read signals, and
    /// the parent lays out again when they change. Hidden, it takes no room
    /// and no gap, is not drawn, takes no clicks and is out of the Tab order
    /// (a focused node inside it lets the focus go), and keeps its state
    /// for when it is shown again: a sidebar toggled by a key, a details
    /// pane, a search box under a table. With several conditions
    /// ([`hide_below`](Self::hide_below) and the like), all must hold.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let sidebar = signal(true);
    ///     row([label("nav").fixed(4).shown_when(move || sidebar.get()), label("main")])
    ///         .on_key("s", move |_| sidebar.update(|on| *on = !*on))
    /// });
    /// let mut driver = app.driver(12, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "nav main");
    /// driver.press("s");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "main");
    /// ```
    pub fn shown_when(self, shown: impl Fn() -> bool + 'static) -> Node {
        self.shown_if("shown_when", shown)
    }

    /// Hide this node while `hidden` holds: [`shown_when`](Self::shown_when)
    /// the other way round.
    pub fn hidden_when(self, hidden: impl Fn() -> bool + 'static) -> Node {
        self.shown_if("hidden_when", move || !hidden())
    }

    /// For a [`switch`]: keep at most `pages` of the pages it built, the
    /// shown one included, dropping the one shown longest ago when another
    /// is built (its [`on_unmount`](Self::on_unmount) handlers run and its
    /// subscriptions end; shown again, it is built afresh). A switch keyed
    /// by live state (a layout, a column set, a size) would otherwise keep
    /// every page it ever built. `0`, the default, keeps them all, as tabs
    /// want. [`keyed`] is a switch that keeps one. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let width = signal(10u16);
    ///     switch(move || width.get(), |w| label(format!("{w} wide")))
    ///         .keep_last(1)
    ///         .on_key("+", move |_| width.update(|w| *w += 1))
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("+ +");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "12 wide");
    /// ```
    pub fn keep_last(self, pages: usize) -> Node {
        if let Some(keep) = &self.keep_pages {
            keep.set(pages);
        }
        self
    }

    /// Shown only while `shown` holds (it may read signals: the parent lays
    /// out again when they change); `rule` is how the inspector names it.
    pub(crate) fn shown_if(mut self, rule: &str, shown: impl Fn() -> bool + 'static) -> Node {
        self.shown_when.push((rule.to_string(), Box::new(shown)));
        self
    }

    /// Shown only while `shown` holds of the app's viewport; `rule` is how
    /// the inspector names the condition.
    fn when_viewport(mut self, rule: String, shown: impl Fn((u16, u16)) -> bool + 'static) -> Node {
        let viewport = crate::reactive::viewport();
        self.shown_when
            .push((rule, Box::new(move || shown(viewport.get()))));
        self
    }

    /// Whether it can take the focus now.
    pub(crate) fn takes_focus(&self) -> bool {
        self.focusable && !self.disabled()
    }

    /// Put this node in the Tab order, so it can hold the focus.
    pub fn focusable(mut self) -> Node {
        self.focusable = true;
        self
    }

    /// Give this node the focus when its screen opens (and whenever the
    /// screen has nothing focused), instead of the first focusable node:
    /// a search box, or the list a screen is about. It also puts the node
    /// in the Tab order. With several, the first in the Tab order wins.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let picked = signal("");
    ///     column([
    ///         label("menu").focusable().on_key("x", move |_| picked.set("menu")),
    ///         label("list").autofocus().on_key("x", move |_| picked.set("list")),
    ///         text!("{picked}"),
    ///     ])
    ///     .on_key("q", |cx| cx.quit())
    /// });
    /// let screen = app.render_with(&["x", "q"], 10, 3).unwrap();
    /// assert_eq!(screen[2].trim_end(), "list");
    /// ```
    ///
    /// On a node that is not focusable of its own and holds others (a
    /// [`switch`], a panel or padding round a list), the focus passes to
    /// the first node inside it that takes the focus, now or once it is
    /// built: a list in a `switch` gets its own keys, and its ancestors'
    /// bindings still see what it leaves.
    pub fn autofocus(mut self) -> Node {
        self.focus_passes = !self.focusable;
        self.focusable = true;
        self.autofocus = true;
        self
    }

    /// Where the focus goes when this node is given it: the first node
    /// inside that takes the focus, for one whose focus
    /// [passes](Self::autofocus) inside; `None` to keep it.
    pub(crate) fn focus_target(&self) -> Option<NodeId> {
        if !self.focus_passes {
            return None;
        }
        let mut found = None;
        self.each_child(&mut |child| {
            child.walk_accessible(&mut |node, _| {
                if found.is_none()
                    && node.takes_focus()
                    && (!node.focus_passes || !node.has_children())
                {
                    found = Some(node.id);
                }
            });
        });
        found
    }

    /// Make this node a focus scope: a pane that remembers which node
    /// inside it had the focus last, and gives the focus back to it when
    /// the focus comes to the pane as a whole
    /// ([`Ctx::focus_within`](crate::Ctx::focus_within),
    /// [`Ctx::focus_next_scope`](crate::Ctx::focus_next_scope)), rather than
    /// to its first focusable node. Tab still moves through every node.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     column([
    ///         column([label("a").focusable().name("a"), label("b").focusable().name("b")])
    ///             .focus_scope()
    ///             .name("left"),
    ///         label("c").focusable().name("c").focus_scope(),
    ///     ])
    ///     .on_key("w", |cx| cx.focus_next_scope())
    /// });
    /// let mut driver = app.driver(4, 3);
    /// driver.press("tab");
    /// driver.assert_focused("b");
    /// driver.press("w");
    /// driver.assert_focused("c");
    /// // Back to the pane: to `b`, where the focus was.
    /// driver.press("w");
    /// driver.assert_focused("b");
    /// ```
    pub fn focus_scope(mut self) -> Node {
        self.focus_scope = true;
        self
    }

    /// Make this node a [focus scope](Self::focus_scope) that
    /// [`Ctx::focus_next_scope`](crate::Ctx::focus_next_scope) and
    /// [`focus_previous_scope`](crate::Ctx::focus_previous_scope) visit even
    /// while it is hidden ([`shown_when`](Self::shown_when)), as long as
    /// `when` holds: reaching it, they call `reveal` to show it (set what
    /// its `shown_when` reads), and the focus goes inside it once it is
    /// laid out. A pane shown alone (fullscreen, zoomed) that Tab moves on
    /// from: the next pane is shown in its place. A hidden scope without
    /// it, or whose `when` does not hold (a closed sidebar), is passed by.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let shown = signal(0);
    ///     let pane = |n: usize, name: &'static str| {
    ///         label(name)
    ///             .focusable()
    ///             .name(name)
    ///             .focus_scope_reveal(|| true, move |_| shown.set(n))
    ///             .shown_when(move || shown.get() == n)
    ///     };
    ///     column([pane(0, "one"), pane(1, "two")]).on_key("tab", |cx| cx.focus_next_scope())
    /// });
    /// let mut driver = app.driver(5, 2);
    /// driver.settle();
    /// driver.assert_focused("one");
    /// driver.press("tab");
    /// driver.assert_focused("two");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "two");
    /// ```
    pub fn focus_scope_reveal(
        mut self,
        when: impl Fn() -> bool + 'static,
        reveal: impl Fn(&mut Ctx) + 'static,
    ) -> Node {
        self.focus_scope = true;
        self.scope_reveal = Some((Rc::new(when), Rc::new(reveal)));
        self
    }

    /// What shows it when scope cycling reaches it hidden, while it may be.
    pub(crate) fn scope_reveal(&self) -> Option<Reveal> {
        let (when, reveal) = self.scope_reveal.as_ref()?;
        crate::reactive::untracked(|| when()).then(|| reveal.clone())
    }

    /// [`walk`](Self::walk) into hidden nodes too, telling `f` whether
    /// each is hidden (it or an ancestor); what is hidden from assistive
    /// technology is left out, as in the focus order.
    pub(crate) fn walk_hidden(&self, f: &mut dyn FnMut(&Node, bool)) {
        self.walk_hidden_inner(false, f);
    }

    fn walk_hidden_inner(&self, above: bool, f: &mut dyn FnMut(&Node, bool)) {
        if self.access_hidden {
            return;
        }
        let hidden = above || self.hidden();
        f(self, hidden);
        self.each_child(&mut |child| child.walk_hidden_inner(hidden, f));
    }

    /// Whether it is a [focus scope](Self::focus_scope).
    pub(crate) fn is_focus_scope(&self) -> bool {
        self.focus_scope
    }

    /// For a focus scope: the node inside it that had the focus last.
    pub(crate) fn scope_last(&self) -> Option<NodeId> {
        self.focus_scope.then(|| self.scope_last.get()).flatten()
    }

    /// For a focus scope: note `id`, inside it, has the focus.
    pub(crate) fn note_scope_focus(&self, id: NodeId) {
        if self.focus_scope {
            self.scope_last.set(Some(id));
        }
    }

    /// Run `handler` when the focus comes to this node or a node inside it
    /// from outside: a pane that lights its title, loads what it shows, or
    /// tells the rest of the app which pane is in use. Moving between
    /// nodes inside it runs nothing. Told after the frame that lays out
    /// the change.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let said = signal(String::new());
    ///     column([
    ///         text!("{said}").fixed(1),
    ///         label("menu").focusable().fixed(1),
    ///         column([label("a").focusable(), label("b").focusable()])
    ///             .on_focus(move |_| said.update(|s| s.push_str("in ")))
    ///             .on_blur(move |_| said.update(|s| s.push_str("out "))),
    ///     ])
    /// });
    /// let mut driver = app.driver(16, 4);
    /// driver.press("tab tab tab");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "in out");
    /// ```
    pub fn on_focus(self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.focus_handlers
            .borrow_mut()
            .push((true, Box::new(handler)));
        self
    }

    /// Run `handler` when the focus leaves this node and every node inside
    /// it (a modal opening over it included), as
    /// [`on_focus`](Self::on_focus) does when it comes.
    pub fn on_blur(self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.focus_handlers
            .borrow_mut()
            .push((false, Box::new(handler)));
        self
    }

    /// Bind `left`, `right`, `up` and `down` (each one or more keys, as
    /// [`bind`](Self::bind) takes them: `"ctrl+left H"`) to moving the
    /// focus to the neighbour on that side
    /// ([`Ctx::focus_direction`](crate::Ctx::focus_direction)), listed in
    /// the help as "Focus left" and so on. An empty string binds nothing.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     row([
    ///         label("a").focusable().name("a").panel("A").focus_scope(),
    ///         label("b").focusable().name("b").panel("B").focus_scope(),
    ///     ])
    ///     .bind_directional_focus("h", "l", "k", "j")
    /// });
    /// let mut driver = app.driver(12, 3);
    /// driver.press("l");
    /// driver.assert_focused("b");
    /// driver.press("h");
    /// driver.assert_focused("a");
    /// ```
    pub fn bind_directional_focus(self, left: &str, right: &str, up: &str, down: &str) -> Node {
        use crate::focus::Direction;
        let mut node = self;
        for (keys, direction, description) in [
            (left, Direction::Left, "Focus left"),
            (right, Direction::Right, "Focus right"),
            (up, Direction::Up, "Focus up"),
            (down, Direction::Down, "Focus down"),
        ] {
            if !keys.trim().is_empty() {
                node = node.bind(keys, description, move |cx| cx.focus_direction(direction));
            }
        }
        node
    }

    /// Take this node out of the Tab order: a [`list`] that only shows, or
    /// a component you drive yourself.
    pub fn no_focus(mut self) -> Node {
        self.focusable = false;
        self
    }

    /// While this node has the focus, draw it in `style` over its own: a
    /// style (`"reverse"`, `"on grey23"`) or a theme style name
    /// (`"accent"`). Its whole rectangle takes the style, its children's
    /// cells included, so a list shows which row is selected even when a
    /// row is a [`row`] of several parts. The node is also made
    /// [focusable](Self::focusable).
    pub fn focus_style(mut self, style: &str) -> Node {
        self.focus_style = Some(style.to_string());
        self.focusable = true;
        self
    }

    /// While the pointer is over this node (or a node inside it), draw it
    /// in `style` over its own, as [`focus_style`](Self::focus_style) does
    /// for the focus. A [stylesheet](crate::App::stylesheet)'s `:hover`
    /// rules do the same from outside the code.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use intuituive::interact::{Event, Mouse, MouseKind};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| column([label("one").hover_style("reverse"), label("two")]));
    /// let mut driver = app.driver(10, 2);
    /// driver.update(Duration::ZERO);
    /// let _ = driver.render();
    /// driver.event(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 0)));
    /// driver.update(Duration::ZERO);
    /// let frame = driver.render().expect("the hover redraws");
    /// assert!(frame.contains("\x1b[0;7m"), "{frame:?}");
    /// ```
    pub fn hover_style(mut self, style: &str) -> Node {
        self.hover_style = Some(style.to_string());
        self
    }

    /// Show `markup` in a small box by the pointer once it has rested on
    /// this node (or a node inside it without a tooltip of its own) for
    /// 600 ms, or below the node at once when F1 is pressed while it has
    /// the focus and nothing binds F1. A key, a click, or the pointer
    /// moving to another node hides it.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use intuituive::interact::{Event, Mouse, MouseKind};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("save").tooltip("Write the file to disk"));
    /// let mut driver = app.driver(30, 3);
    /// driver.update(Duration::ZERO);
    /// let _ = driver.render();
    /// driver.event(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 0)));
    /// driver.update(Duration::from_millis(700));
    /// let _ = driver.render();
    /// assert!(driver.screen().plain()[1].contains("Write the file to disk"));
    /// ```
    pub fn tooltip(mut self, markup: impl Into<String>) -> Node {
        self.tooltip = Some(markup.into());
        self
    }

    /// Let this node be dragged with the left button, carrying `value` to
    /// a node that takes it with [`on_drop`](Self::on_drop). A press still
    /// focuses and clicks as before; the drag starts once the pointer moves
    /// a cell. While it lasts the node is dimmed and the drop target under
    /// the pointer is highlighted (the theme's `drop.target` style, reverse
    /// unless set); Esc cancels it.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use intuituive::interact::{Button, Event, Mouse, MouseKind};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let done = signal(Vec::<String>::new());
    ///     column([
    ///         label("task").draggable("task".to_string()),
    ///         text(move || format!("done: {}", done.get().join(", ")))
    ///             .on_drop(move |task: &String, _| done.update(|d| d.push(task.clone()))),
    ///     ])
    /// });
    /// let mut driver = app.driver(20, 2);
    /// driver.update(Duration::ZERO);
    /// let _ = driver.render();
    /// let mut send = |kind, row| {
    ///     driver.event(Event::Mouse(Mouse::new(kind, 1, row)));
    ///     driver.update(Duration::ZERO);
    ///     let _ = driver.render();
    /// };
    /// send(MouseKind::Down(Button::Left), 0);
    /// send(MouseKind::Drag(Button::Left), 1);
    /// send(MouseKind::Up(Button::Left), 1);
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "done: task");
    /// ```
    pub fn draggable<T: 'static>(mut self, value: T) -> Node {
        self.drag = Some(Rc::new(value));
        self
    }

    /// Take values of type `T` dropped on this node (or on a node inside
    /// it that does not take them itself): `handler` gets the value a
    /// [`draggable`](Self::draggable) node carried. A drag carrying another
    /// type passes this node by: it is not highlighted, and nothing drops.
    pub fn on_drop<T: 'static>(mut self, mut handler: impl FnMut(&T, &mut Ctx) + 'static) -> Node {
        self.drop = Some(DropTarget {
            accepts: |value| value.is::<T>(),
            handler: RefCell::new(Box::new(move |value, cx| {
                if let Some(value) = value.downcast_ref::<T>() {
                    handler(value, cx);
                }
            })),
        });
        self
    }

    /// Run `handler` when one of `keys` (space-separated names: `"q"`,
    /// `"ctrl+s"`, `"up k"`) is pressed while this node or a node inside it
    /// has the focus, and nothing deeper used the key. The binding also
    /// shows in the app's help.
    pub fn on_key(self, keys: &str, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.bind(keys, "", handler)
    }

    /// [`on_key`](Self::on_key) with a description for the help.
    pub fn bind(
        self,
        keys: &str,
        description: &str,
        mut handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        let parsed = parse_keys(keys);
        let handler: KeyHandler = Box::new(move |cx, _| handler(cx));
        self.keys
            .borrow_mut()
            .push(KeyBinding::new(parsed, description, handler));
        self
    }

    /// [`bind`](Self::bind), listed in the help (and the command palette)
    /// under `section` rather than under the node's
    /// [name](Self::name): one node's keys in groups, as an app with modes
    /// lists them.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     label("scope")
    ///         .bind_in("oscilloscope", "t", "Toggle the trigger", |_| {})
    ///         .bind_in("spectroscope", "l", "Logarithmic scale", |_| {})
    ///         .on_key("?", |cx| cx.help())
    /// });
    /// let mut driver = app.driver(60, 16);
    /// driver.press("?");
    /// let rows = driver.screen().plain().join("\n");
    /// assert!(rows.find("oscilloscope") < rows.find("Toggle the trigger"));
    /// assert!(rows.find("spectroscope") < rows.find("Logarithmic scale"));
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key, or on no names at all.
    pub fn bind_in(
        self,
        section: &str,
        keys: &str,
        description: &str,
        handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        let node = self.bind(keys, description, handler);
        if let Some(binding) = node.keys.borrow_mut().last_mut() {
            binding.section = Some(section.to_string());
        }
        node
    }

    /// Bind `keys` and the same keys with modifiers, each variant with its
    /// own value, shown in the help as one row: `variants` pairs the
    /// modifiers (`""` for none, `"shift"`, `"ctrl"`, `"alt"`,
    /// `"ctrl+alt"`) with a value, and `handler` gets the value of the
    /// variant pressed. scope-tui's ↑, ten times with Shift and five with
    /// Ctrl. The help lists the keys of the first variant, its description
    /// followed by the others' (`(shift ×10, ctrl ×5)`); the palette runs
    /// the first. A variant may say itself its own way, with a label in
    /// place of `×value`: `("alt", 0.2, "⅕")` is listed `alt ⅕` (every
    /// variant then has one; see [`Scaled`](crate::Scaled)).
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let scale = signal(1.0);
    ///     text!("{scale}").bind_scaled(
    ///         "up",
    ///         &[("", 1.0), ("shift", 10.0), ("ctrl", 5.0)],
    ///         "Scale up",
    ///         move |_, by| scale.update(|s| *s += by),
    ///     )
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("up shift+up ctrl+up");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "17");
    /// ```
    ///
    /// With labels:
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let scale = signal(1.0);
    ///     text!("{scale}")
    ///         .bind_scaled(
    ///             "up",
    ///             &[("", 1.0, "one"), ("alt", 0.2, "⅕")],
    ///             "Scale up",
    ///             move |_, by| scale.update(|s| *s += by),
    ///         )
    ///         .on_key("?", |cx| cx.help())
    /// });
    /// let mut driver = app.driver(50, 12);
    /// driver.press("?");
    /// assert!(driver.screen().plain().join("\n").contains("Scale up (alt ⅕)"));
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key, on no names or no variants, or on a
    /// modifier that makes no key with one of them (`"shift"` with `","`).
    pub fn bind_scaled<V: crate::Scaled>(
        self,
        keys: &str,
        variants: &[V],
        description: &str,
        mut handler: impl FnMut(&mut Ctx, f64) + 'static,
    ) -> Node {
        assert!(
            !variants.is_empty(),
            "bind_scaled({keys:?}) has no variants"
        );
        let plain = parse_keys(keys);
        // What the help says of the others: `shift ×10`, or its label.
        let others: Vec<String> = variants[1..]
            .iter()
            .map(|variant| {
                let modifiers = variant.modifiers().trim_end_matches('+');
                match variant.label() {
                    Some(label) => format!("{modifiers} {label}"),
                    None => format!("{modifiers} ×{}", variant.value()),
                }
            })
            .collect();
        let variants: Vec<(Vec<Key>, f64)> = variants
            .iter()
            .map(|variant| (variant.modifiers(), variant.value()))
            .map(|(modifiers, value)| {
                let modifiers = modifiers.trim_end_matches('+');
                let keys = match modifiers {
                    "" => plain.clone(),
                    modifiers => plain
                        .iter()
                        .map(|key| {
                            let name = format!("{modifiers}+{key}");
                            Key::parse(&name).unwrap_or_else(|| {
                                panic!("bind_scaled({keys:?}): {name:?} is not a key")
                            })
                        })
                        .collect(),
                };
                (keys, value)
            })
            .collect();
        let description = if others.is_empty() {
            description.to_string()
        } else {
            format!("{description} ({})", others.join(", "))
        };
        let all: Vec<Key> = variants.iter().flat_map(|(keys, _)| keys.clone()).collect();
        let shown = variants[0].0.clone();
        let handler: KeyHandler = Box::new(move |cx, key| {
            // The variant that names the key, before one it could be (as
            // a binding picks among its keys).
            let index = key
                .pick(variants.iter().map(|(keys, _)| keys.as_slice()))
                .unwrap_or(0);
            handler(cx, variants[index].1)
        });
        let mut binding = KeyBinding::new(all, &description, handler);
        binding.shown = Some(shown);
        self.keys.borrow_mut().push(binding);
        self
    }

    /// Bind `keys` to move `mode` on to the next of `modes`, round to the
    /// first after the last: a mode switcher (scope-tui's Tab through the
    /// oscilloscope, the vectorscope and the spectroscope). A value that is
    /// not among `modes` moves to the first.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let mode = signal("scope");
    ///     text!("{mode}").bind_cycle("tab", "Next mode", mode, &["scope", "vector", "spectrum"])
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("tab tab tab tab");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "vector");
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key, or on no names or no modes.
    pub fn bind_cycle<T: PartialEq + Clone + 'static>(
        self,
        keys: &str,
        description: &str,
        mode: Signal<T>,
        modes: &[T],
    ) -> Node {
        assert!(!modes.is_empty(), "bind_cycle({keys:?}) has no modes");
        let modes = modes.to_vec();
        self.bind(keys, description, move |_| {
            let at = mode.with_untracked(|now| modes.iter().position(|m| m == now));
            let next = at.map_or(0, |i| (i + 1) % modes.len());
            mode.set(modes[next].clone());
        })
    }

    /// [`bind_cycle`](Self::bind_cycle) the other way: `keys` move `mode`
    /// back to the one before, round to the last from the first (Shift+Tab
    /// to Tab's [`bind_cycle`](Self::bind_cycle)). A value that is not
    /// among `modes` moves to the last.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let mode = signal("scope");
    ///     let modes = ["scope", "vector", "spectrum"];
    ///     text!("{mode}")
    ///         .bind_cycle("tab", "Next mode", mode, &modes)
    ///         .bind_cycle_back("shift+tab", "Previous mode", mode, &modes)
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("shift+tab");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "spectrum");
    /// driver.press("tab shift+tab shift+tab");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "vector");
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key, or on no names or no modes.
    pub fn bind_cycle_back<T: PartialEq + Clone + 'static>(
        self,
        keys: &str,
        description: &str,
        mode: Signal<T>,
        modes: &[T],
    ) -> Node {
        assert!(!modes.is_empty(), "bind_cycle_back({keys:?}) has no modes");
        let modes = modes.to_vec();
        self.bind(keys, description, move |_| {
            let at = mode.with_untracked(|now| modes.iter().position(|m| m == now));
            let back = at.map_or(modes.len() - 1, |i| (i + modes.len() - 1) % modes.len());
            mode.set(modes[back].clone());
        })
    }

    /// Run `handler` once, when this node leaves the tree: its screen or
    /// pop-up closes, its [`each`] row goes, or the app ends. (A
    /// [`switch`] keeps the pages it built, so one shown no more is still
    /// in the tree.) Stop what it started there: a background
    /// walk (set the flag it reads), a watcher, a device. The app may be
    /// ending, so leave its signals alone.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use std::sync::atomic::{AtomicBool, Ordering};
    /// use std::sync::Arc;
    ///
    /// let stop = Arc::new(AtomicBool::new(false));
    /// let flag = stop.clone();
    /// let app = App::new(move || {
    ///     label("files").on_key("s", move |cx| {
    ///         let flag = flag.clone();
    ///         cx.push(move || {
    ///             label("searching…")
    ///                 .on_unmount(move || flag.store(true, Ordering::SeqCst))
    ///                 .on_key("esc", |cx| cx.pop())
    ///         })
    ///     })
    /// });
    /// let mut driver = app.driver(20, 1);
    /// driver.press("s");
    /// assert!(!stop.load(Ordering::SeqCst));
    /// driver.press("esc");
    /// assert!(stop.load(Ordering::SeqCst));
    /// ```
    pub fn on_unmount(mut self, handler: impl FnOnce() + 'static) -> Node {
        self.unmount.push(Unmount(Some(Box::new(handler))));
        self
    }

    /// Run `handler` when this node is clicked. A clickable node is also
    /// focusable, and a click focuses it.
    pub fn on_click(mut self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        *self.click.borrow_mut() = Some(Box::new(handler));
        self.focusable = true;
        self
    }

    /// Run `handler` for every mouse event over this node (presses, releases,
    /// drags, movement, the wheel), in the node's own coordinates, before
    /// its ancestors see it. It returns whether it used the event; one it
    /// did not use bubbles up. A press does not focus the node unless it is
    /// [focusable](Self::focusable).
    pub fn on_mouse(self, handler: impl FnMut(&mut Ctx, Mouse) -> bool + 'static) -> Node {
        *self.mouse.borrow_mut() = Some(Box::new(handler));
        self
    }

    /// For a [`component`] node: run `handler` when the component is
    /// cancelled (Esc). Without one, the cancelling key bubbles to the
    /// node's ancestors like any unused key. Ignored on other nodes.
    pub fn on_cancel(self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.widget_mut(|host: &mut HostWidget| host.host.set_cancel(Box::new(handler)));
        self
    }

    /// Wrap this node in a rounded border with `title`. The border is
    /// highlighted while the focus is on the node or inside it. The panel
    /// takes the node's size.
    pub fn panel(self, title: &str) -> Node {
        let title = title.to_string();
        self.wrap_in(|child| Box::new(Panel::new(title, child)), "panel")
    }

    /// Leave `vertical` empty rows above and below this node, and
    /// `horizontal` empty columns either side.
    pub fn padding(self, vertical: u16, horizontal: u16) -> Node {
        self.wrap_in(
            |child| {
                Box::new(Pad {
                    child,
                    edges: [vertical, horizontal, vertical, horizontal],
                })
            },
            "padding",
        )
    }

    /// For a [panel](Self::panel): where its own title sits on the top edge,
    /// [`Align::Start`] (the default), [`Align::Center`] or [`Align::End`].
    /// A stylesheet's `border-title-align` does the same. Ignored on other
    /// nodes.
    pub fn title_align(self, align: Align) -> Node {
        self.widget_mut(|panel: &mut Panel| panel.code.title_align = Some(align));
        self
    }

    /// For a [panel](Self::panel): its titles at the corners, with no
    /// space added either side, as a ratatui `Block` draws its title:
    /// `┌Staged Changes───┐`, and a right-hand title against the right
    /// corner (`──{main}┘`). Put the spaces in the title for termusic's
    /// `╭ Library ─`. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     label("…")
    ///         .panel("Staged Changes")
    ///         .border(Border::Plain)
    ///         .title_flush()
    ///         .add_title(Edge::Bottom, Align::End, "{main}")
    /// });
    /// let mut driver = app.driver(20, 3);
    /// driver.settle();
    /// let rows = driver.screen().plain();
    /// assert_eq!(rows[0], "┌Staged Changes────┐");
    /// assert_eq!(rows[2], "└────────────{main}┘");
    /// ```
    pub fn title_flush(self) -> Node {
        self.widget_mut(|panel: &mut Panel| panel.code.flush = true);
        self
    }

    /// Wrap this node in a [panel](Self::panel) titled `title` as plain
    /// text: brackets show as they are (`Recipe [r]`, `History [ctrl h]`),
    /// where [`panel`](Self::panel) reads them as console markup.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("…").panel_text("Recipe [r]"));
    /// let mut driver = app.driver(18, 3);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "╭─ Recipe [r] ───╮");
    /// ```
    pub fn panel_text(self, title: &str) -> Node {
        self.panel(&rich::markup::escape(title))
    }

    /// [`panel_with`](Self::panel_with), its title read as plain text, as
    /// [`panel_text`](Self::panel_text)'s is.
    pub fn panel_text_with(self, title: impl Fn() -> String + 'static) -> Node {
        self.panel_with(move || rich::markup::escape(&title()))
    }

    /// [`add_title`](Self::add_title) with `text` as plain text: brackets
    /// show as they are. Ignored on nodes that are not panels.
    pub fn add_title_text(self, edge: Edge, align: Align, text: &str) -> Node {
        self.add_title(edge, align, rich::markup::escape(text))
    }

    /// For a [panel](Self::panel): another title, console markup, at the
    /// right of the top edge. Ignored on other nodes.
    pub fn title_right(self, markup: impl Into<String>) -> Node {
        self.add_title(Edge::Top, Align::End, markup)
    }

    /// For a [panel](Self::panel): a title, console markup, at the left of
    /// the bottom edge (a stylesheet's `border-subtitle`). Ignored on other
    /// nodes.
    pub fn title_bottom(self, markup: impl Into<String>) -> Node {
        self.add_title(Edge::Bottom, Align::Start, markup)
    }

    /// For a [panel](Self::panel): a title, console markup, on `edge`,
    /// placed by `align`. Titles with the same alignment sit side by side,
    /// one cell apart, in the order given. When they do not fit, the
    /// right-hand ones give way first, then the centred ones, and the left
    /// ones are cut with `…`. An edge with a title keeps its row even when
    /// its line is not drawn. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     label("…")
    ///         .panel("Pull requests")
    ///         .title_right("[dim]loading")
    ///         .add_title(Edge::Bottom, Align::End, "1/20")
    /// });
    /// let mut driver = app.driver(32, 3);
    /// driver.settle();
    /// let rows = driver.screen().plain();
    /// assert_eq!(rows[0], "╭─ Pull requests ──── loading ─╮");
    /// assert!(rows[2].starts_with("╰───") && rows[2].ends_with("─ 1/20 ─╯"));
    /// ```
    pub fn add_title(self, edge: Edge, align: Align, markup: impl Into<String>) -> Node {
        let title = crate::builtin::CodeTitle::Fixed(Title::new(edge, align, markup));
        self.widget_mut(|panel: &mut Panel| panel.code.titles.push(title));
        self
    }

    /// For a [panel](Self::panel): a title on `edge`, placed by `align`, as
    /// [`add_title`](Self::add_title) adds, read from `markup` whenever the
    /// panel lays out and draws: it may read signals, and the panel draws
    /// again when they change (a count, a position, a load average).
    /// Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let at = signal(1);
    ///     label("…")
    ///         .panel("Commits")
    ///         .add_title_with(Edge::Top, Align::End, move || format!("{at}/10"))
    ///         .on_key("j", move |_| at.update(|n| *n += 1))
    /// });
    /// let mut driver = app.driver(20, 3);
    /// driver.press("j");
    /// assert_eq!(driver.screen().plain()[0], "╭─ Commits ─ 2/10 ─╮");
    /// ```
    pub fn add_title_with(
        self,
        edge: Edge,
        align: Align,
        markup: impl Fn() -> String + 'static,
    ) -> Node {
        let markup = crate::builtin::Dynamic(Rc::new(markup) as Rc<dyn Fn() -> String>);
        let title = crate::builtin::CodeTitle::With(edge, align, markup);
        self.widget_mut(|panel: &mut Panel| panel.code.titles.push(title));
        self
    }

    /// For a [panel](Self::panel): its own title, read from `markup`
    /// whenever the panel lays out and draws, in place of the one it was
    /// made with: it may read signals (`Diff: src/lib.rs`, `Processes (3 of
    /// 210)`). It is also the panel's accessible name. Ignored on other
    /// nodes; [`panel_with`](Self::panel_with) makes the panel.
    pub fn title_with(self, markup: impl Fn() -> String + 'static) -> Node {
        let markup: Rc<dyn Fn() -> String> = Rc::new(markup);
        self.widget_mut(|panel: &mut Panel| panel.title_with = Some(markup));
        self
    }

    /// Wrap this node in a [panel](Self::panel) whose title is read from
    /// `markup` whenever the panel lays out and draws: it may read signals,
    /// and the panel draws again when they change.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let file = signal("lib.rs");
    ///     label("+ one line")
    ///         .panel_with(move || format!("Diff: {file}"))
    ///         .on_key("n", move |_| file.set("main.rs"))
    /// });
    /// let mut driver = app.driver(20, 3);
    /// driver.press("n");
    /// assert_eq!(driver.screen().plain()[0], "╭─ Diff: main.rs ──╮");
    /// ```
    pub fn panel_with(self, markup: impl Fn() -> String + 'static) -> Node {
        self.panel("").title_with(markup)
    }

    /// For a [panel](Self::panel): the kind of box it draws, read from
    /// `border` whenever it lays out and draws (it may read signals), over
    /// [`border`](Self::border)'s. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let alert = signal(false);
    ///     label("ok")
    ///         .panel("")
    ///         .border_with(move || if alert.get() { Border::Double } else { Border::Plain })
    ///         .on_key("a", move |_| alert.set(true))
    /// });
    /// let mut driver = app.driver(6, 3);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "┌────┐");
    /// driver.press("a");
    /// assert_eq!(driver.screen().plain()[0], "╔════╗");
    /// ```
    pub fn border_with(self, border: impl Fn() -> Border + 'static) -> Node {
        let border = crate::builtin::Dynamic(Rc::new(border) as Rc<dyn Fn() -> Border>);
        self.widget_mut(|panel: &mut Panel| panel.code.border_with = Some(border));
        self
    }

    /// For a [panel](Self::panel): the kind of box it draws, which wins over
    /// a stylesheet's `border`. [`Border::None`] draws no lines, but its
    /// titles keep their rows. For a container with
    /// [collapsed borders](Self::collapse_borders): the kind of its lines.
    /// For a [`rule`](crate::rule()) on its own: the kind of its line.
    /// Ignored on other nodes.
    pub fn border(self, border: Border) -> Node {
        self.widget_mut(|panel: &mut Panel| panel.code.border = Some(border));
        self.widget_mut(|rule: &mut crate::rule::Rule| rule.kind = Some(border));
        self.widget_mut(|stack: &mut Stack| stack.collapse.border = Some(border));
        self.widget_mut(|grid: &mut Grid| grid.collapse.border = Some(border));
        self
    }

    /// For a [panel](Self::panel): the sides of the box it draws,
    /// [`Sides::ALL`] by default, or some of them joined with `|`. The
    /// inside grows into the sides it does not draw. A stylesheet's
    /// `border-sides: top bottom` does the same. On a node that is not a
    /// panel, it puts the node in a panel without a title first, so
    /// `.borders(Sides::BOTTOM)` underlines anything.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("body").panel("Notes").borders(Sides::TOP));
    /// let mut driver = app.driver(12, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain(), ["── Notes ───", "body        "]);
    ///
    /// let app = App::new(|| column([label("Header").borders(Sides::BOTTOM).fixed(2), label("body")]));
    /// let mut driver = app.driver(8, 3);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain(), ["Header  ", "────────", "body    "]);
    /// ```
    pub fn borders(self, sides: Sides) -> Node {
        if self.widget_ref(|_: &Panel| ()).is_none() {
            return self.panel("").borders(sides);
        }
        self.widget_mut(|panel: &mut Panel| panel.code.sides = Some(sides));
        self
    }

    /// For a [panel](Self::panel) or a container with
    /// [collapsed borders](Self::collapse_borders): the border's style (or a
    /// theme style's name) while the focus is elsewhere; with the focus
    /// inside, it takes the theme's `border.focused`. Ignored on other
    /// nodes.
    pub fn border_style(self, style: &str) -> Node {
        let style = style.to_string();
        self.widget_mut(|panel: &mut Panel| panel.code.border_style = Some(style.clone()));
        self.widget_mut(|stack: &mut Stack| stack.collapse.border_style = Some(style.clone()));
        self.widget_mut(|grid: &mut Grid| grid.collapse.border_style = Some(style));
        self
    }

    /// For a [panel](Self::panel): its titles' style (or a theme style's
    /// name), over the border's colour, instead of the theme's `title`.
    /// Markup in a title goes over it. Ignored on other nodes.
    pub fn title_style(self, style: &str) -> Node {
        let style = style.to_string();
        self.widget_mut(|panel: &mut Panel| panel.code.title_style = Some(style));
        self
    }

    /// For a [panel](Self::panel): a shadow down its right and along its
    /// bottom, inside its own rectangle (the box is a cell narrower and
    /// shorter for it), in the theme's `shadow` style. Ignored on other
    /// nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("hi").panel("").shadow(Shadow::MediumShade));
    /// let mut driver = app.driver(6, 4);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain(), ["╭───╮ ", "│hi │▒", "╰───╯▒", " ▒▒▒▒▒"]);
    /// ```
    pub fn shadow(self, shadow: Shadow) -> Node {
        self.widget_mut(|panel: &mut Panel| panel.code.shadow = Some(shadow));
        self
    }

    /// For a [`column`](fn@column), [`row`] or [`grid`] of
    /// [panels](Self::panel): draw one frame for them all, each panel's box
    /// sharing its edges with its neighbours', joined where lines meet
    /// (`┬ ┴ ├ ┤ ┼`). Each panel's titles sit on its own edges and its
    /// content between the lines; the lines round the panel with the focus
    /// are lit in the theme's `border.focused` (and drawn in
    /// [`focus_border`](Self::focus_border)'s kind). Gaps are ignored. A
    /// stylesheet's `border-collapse: collapse` does the same. Ignored on
    /// other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     row([label("a").panel("A"), label("b").panel("B")]).collapse_borders()
    /// });
    /// let mut driver = app.driver(13, 3);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain(), ["╭─ A ─┬─ B ─╮", "│a    │b    │", "╰─────┴─────╯"]);
    /// ```
    pub fn collapse_borders(self) -> Node {
        let on = |collapse: &mut crate::builtin::Collapse| {
            collapse.on = true;
            collapse.code = true;
        };
        self.widget_mut(|stack: &mut Stack| on(&mut stack.collapse));
        self.widget_mut(|grid: &mut Grid| on(&mut grid.collapse));
        self
    }

    /// For a container with [collapsed borders](Self::collapse_borders):
    /// the kind of the lines round the child with the focus
    /// ([`Border::Thick`] makes it stand out, as ratatui's
    /// `collapsed-borders` example does). For a [panel](Self::panel): the
    /// kind of its box while the focus is on it or inside it (Slumber's
    /// double-bordered pane); it keeps the cells its other kind takes.
    /// Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     column([
    ///         label("a").focusable().panel("").border(Border::Plain).focus_border(Border::Double),
    ///         label("b").focusable().panel("").border(Border::Plain).focus_border(Border::Double),
    ///     ])
    /// });
    /// let mut driver = app.driver(5, 6);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "╔═══╗");
    /// assert_eq!(driver.screen().plain()[3], "┌───┐");
    /// ```
    pub fn focus_border(self, border: Border) -> Node {
        self.widget_mut(|stack: &mut Stack| stack.collapse.focus_border = Some(border));
        self.widget_mut(|grid: &mut Grid| grid.collapse.focus_border = Some(border));
        self.widget_mut(|panel: &mut Panel| panel.code.focus_border = Some(border));
        self
    }

    /// Draw this node in `style` (`"white on #101830"`, or a theme style's
    /// name: `"accent"`): ratatui's `Block::style`. It goes under what the
    /// node and its children draw, as a stylesheet's `color` and
    /// `background` do, and fills the rest of the node's rectangle, gaps
    /// and borders included. Code wins over a stylesheet; markup inside
    /// wins over both.
    pub fn style(mut self, style: &str) -> Node {
        self.own_style.get_or_insert_with(OwnStyle::default).style = Some(style.to_string());
        self
    }

    /// [`style`](Self::style) with only a background: a colour
    /// (`"#101830"`, `"grey11"`) or a theme style's colour.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| column([label("a"), label("b")]).gap(1).bg("blue"));
    /// let mut driver = app.driver(3, 3);
    /// driver.settle();
    /// // Every cell is blue, the gap included.
    /// let blue = rich::Style::parse("on blue").unwrap();
    /// let gap = driver.screen().cell(1, 1).style;
    /// assert_eq!(driver.screen().style(gap), Some(&blue));
    /// ```
    pub fn bg(mut self, color: &str) -> Node {
        self.own_style.get_or_insert_with(OwnStyle::default).bg = Some(color.to_string());
        self
    }

    /// [`style`](Self::style) with only a foreground: a colour or a theme
    /// style's colour.
    pub fn fg(mut self, color: &str) -> Node {
        self.own_style.get_or_insert_with(OwnStyle::default).fg = Some(color.to_string());
        self
    }

    /// For a [`text`](fn@text) or [`label`]: how its lines sit across its
    /// width, ratatui's `Paragraph::alignment`. A stylesheet's `text-align`
    /// does the same. It does not change the text's natural width (an
    /// [`auto`](Self::auto) node is as wide as its longest line). Ignored on
    /// other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("hi").text_align(TextAlign::Center));
    /// let mut driver = app.driver(6, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "  hi  ");
    /// ```
    pub fn text_align(self, align: TextAlign) -> Node {
        self.widget_mut(|text: &mut TextNode| {
            text.align = Some(align);
            text.code[0] = true;
        });
        self
    }

    /// For a [`text`](fn@text) or [`label`]: lines are cut at the edge
    /// rather than wrapped (with `…` after [`overflow`](Self::overflow)'s
    /// `Ellipsis`). A stylesheet's `text-wrap: nowrap` does the same.
    /// Ignored on other nodes.
    pub fn no_wrap(self) -> Node {
        self.widget_mut(|text: &mut TextNode| {
            text.no_wrap = Some(true);
            text.code[1] = true;
        });
        self
    }

    /// For a [`text`](fn@text) or [`label`]: what happens to what does not
    /// fit a line: [`Overflow::Fold`] (words longer than the line are
    /// broken, the default), [`Overflow::Crop`] (cut) or
    /// [`Overflow::Ellipsis`] (cut, with `…`). A stylesheet's
    /// `text-overflow: fold`, `clip` or `ellipsis` does the same. Ignored on
    /// other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("a long line").no_wrap().overflow(Overflow::Ellipsis));
    /// let mut driver = app.driver(6, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "a lon…");
    /// ```
    pub fn overflow(self, overflow: Overflow) -> Node {
        self.widget_mut(|text: &mut TextNode| {
            text.overflow = Some(overflow);
            text.code[2] = true;
        });
        self
    }

    /// For a [`text`](fn@text) or [`label`]: a line too long for the width
    /// loses its start rather than its end, with `…` where it was cut, and
    /// lines do not wrap. A path in a header keeps where you are, as
    /// Yazi's does (rich's [`Overflow`] cuts only at the end). Ignored on
    /// other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("~/projects/rich/src").truncate_start());
    /// let mut driver = app.driver(10, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "…/rich/src");
    /// ```
    pub fn truncate_start(self) -> Node {
        self.widget_mut(|text: &mut TextNode| {
            text.cut_start = true;
            text.cut_middle = false;
            text.no_wrap = Some(true);
            text.code[1] = true;
        });
        self
    }

    /// For a [`text`](fn@text) or [`label`]: a line too long for the width
    /// loses its middle, with `…` where it was cut, keeping its start and
    /// its end (`long_na…me.txt`), and lines do not wrap: names and paths
    /// in tiles, a status line, a delete prompt. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("long_file_name.txt").truncate_middle());
    /// let mut driver = app.driver(11, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "long_…e.txt");
    /// ```
    pub fn truncate_middle(self) -> Node {
        self.widget_mut(|text: &mut TextNode| {
            text.cut_middle = true;
            text.cut_start = false;
            text.no_wrap = Some(true);
            text.code[1] = true;
        });
        self
    }

    /// `a / b` of the parent's axis (see [`Size::Ratio`]): three
    /// `.ratio(1, 3)` children fill it exactly.
    pub fn ratio(self, a: u32, b: u32) -> Node {
        self.size(Size::Ratio(a, b))
    }

    /// In [`layers`]: keep `top`, `right`, `bottom` and `left` cells
    /// between this layer and the layers' edges, as CSS's `inset`. Ignored
    /// elsewhere.
    pub fn inset(mut self, top: u16, right: u16, bottom: u16, left: u16) -> Node {
        self.inset = Some([top, right, bottom, left]);
        self
    }

    /// In [`layers`]: make this layer as big as its content, placed by
    /// `across` (left to right) and `down` ([`Align::Stretch`] takes the
    /// whole width or height). Ignored elsewhere.
    pub fn place(mut self, across: Align, down: Align) -> Node {
        self.place = Some((across, down));
        self
    }

    /// Theme styles that hold for this node and everything inside it, over
    /// the app's theme, as CSS custom properties do: `[selected]…[/]` in
    /// markup, a list's selected row, a table's `table.header`, a tab
    /// strip's `tabs.selected`, a panel's `border` and `title`. The styles
    /// are definitions or the names of the theme's other styles. It is the
    /// one answer to ratatui's `highlight_style` and the like, per
    /// instance, with no style option on each widget.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let selected = signal(0usize);
    ///     list(|| vec!["one".into(), "two".into()], selected)
    ///         .theme_styles([("selected", "bold yellow on #303030")])
    /// });
    /// let mut driver = app.driver(5, 2);
    /// driver.settle();
    /// let picked = rich::Style::parse("bold yellow on #303030").unwrap();
    /// let cell = driver.screen().cell(0, 0).style;
    /// assert_eq!(driver.screen().style(cell), Some(&picked));
    /// ```
    pub fn theme_styles<N: Into<String>, S: Into<String>>(
        mut self,
        styles: impl IntoIterator<Item = (N, S)>,
    ) -> Node {
        self.theme_styles
            .extend(styles.into_iter().map(|(n, s)| (n.into(), s.into())));
        *self.scope.get_mut() = None;
        self
    }

    /// This node's [scope](Self::theme_styles) over `console` and `theme`,
    /// made again when they changed; `None` without theme styles. While
    /// measuring, `theme` is not at hand: any copy made already does.
    fn scope_over(&self, console: &Console, theme: Option<&Theme>) -> Option<Rc<Scope>> {
        if self.theme_styles.is_empty() {
            return None;
        }
        let size = (console.width(), console.height());
        if let Some(scope) = self.scope.borrow().as_ref() {
            let current = match theme {
                None => true,
                Some(theme) => scope
                    .made_over
                    .as_ref()
                    .is_some_and(|made| same_theme(made, theme)),
            };
            if current && scope.size == size {
                return Some(scope.clone());
            }
        }
        let scope = Rc::new(Scope::new(console, theme, &self.theme_styles));
        *self.scope.borrow_mut() = Some(scope.clone());
        Some(scope)
    }

    /// A node of `widget` round this one, taking over its size and span.
    pub(crate) fn wrap_in(
        mut self,
        widget: impl FnOnce(Node) -> Box<dyn Widget>,
        what: &'static str,
    ) -> Node {
        let (size, min, max, span, set) = (self.size, self.min, self.max, self.span, self.set);
        let size_with = self.size_with.take();
        let align_self = self.align_self;
        // Where it goes in layers is the box's place.
        let (inset, place) = (self.inset.take(), self.place.take());
        // Shown or hidden by the viewport as a whole, box and all.
        let shown_when = std::mem::take(&mut self.shown_when);
        let mut outer = Node::from_widget(widget(self), what);
        outer.shown_when = shown_when;
        outer.size = size;
        outer.size_with = size_with;
        outer.min = min;
        outer.max = max;
        outer.span = span;
        outer.align_self = align_self;
        outer.inset = inset;
        outer.place = place;
        outer.set[SET_SIZE] = set[SET_SIZE];
        outer.set[SET_MIN] = set[SET_MIN];
        outer.set[SET_MAX] = set[SET_MAX];
        outer
    }

    /// Where the node was last laid out.
    pub fn rect(&self) -> Rect {
        self.rect.get()
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// How it asks to be sized along its parent's axis: as code set it,
    /// else as the stylesheet does.
    pub fn size_hint(&self) -> Size {
        self.dims().0
    }

    /// Its size, minimum and maximum: what code set, else what the
    /// stylesheet says; nothing at all when the sheet hides it.
    fn dims(&self) -> (Size, u16, u16) {
        if self.hidden() {
            return (Size::Fixed(0), 0, 0);
        }
        let (size, min, max) = {
            let sheet = self.sheet.borrow();
            let layout = &sheet.layout;
            fn pick<T>(set: bool, own: T, from: Option<T>) -> T {
                if set {
                    own
                } else {
                    from.unwrap_or(own)
                }
            }
            (
                pick(self.set[SET_SIZE], self.own_size(), layout.size),
                pick(self.set[SET_MIN], self.min, layout.min),
                pick(self.set[SET_MAX], self.max, layout.max),
            )
        };
        if size != Size::Auto {
            return (size, min, max);
        }
        // Sized by its content, a switch is as big as its page asks to be.
        let page = match self.body.try_borrow_mut() {
            Ok(mut body) => body.widget.sized_by().map(|page| page.dims()),
            Err(_) => None,
        };
        match page {
            Some((page_size, page_min, page_max)) => {
                let size = match page_size {
                    Size::Fixed(_) | Size::Percent(_) | Size::Ratio(..) => page_size,
                    Size::Auto | Size::Flex(_) => size,
                };
                (size, min.max(page_min), max.min(page_max))
            }
            None => (size, min, max),
        }
    }

    /// Its size as code set it: [`size_with`](Self::size_with)'s, read now,
    /// else [`size`](Self::size)'s.
    pub(crate) fn own_size(&self) -> Size {
        match &self.size_with {
            Some(size) => size(),
            None => self.size,
        }
    }

    /// Whether a stylesheet hides it (`display: none`), or the app's
    /// viewport does ([`hide_below`](Self::hide_below) and the like). A
    /// parent laying out reads the viewport, so it lays out again when the
    /// terminal's size changes.
    pub(crate) fn hidden(&self) -> bool {
        self.sheet.borrow().layout.hidden || self.shown_when.iter().any(|(_, shown)| !shown())
    }

    /// The edge a stylesheet docks it to, if any.
    pub(crate) fn dock(&self) -> Option<Dock> {
        self.sheet.borrow().layout.dock
    }

    /// Where it sits across its parent's axis, if code or a stylesheet
    /// (`align-self`) said; code wins.
    pub(crate) fn self_align(&self) -> Option<Align> {
        self.align_self.or(self.sheet.borrow().layout.align_self)
    }

    pub(crate) fn track(&self) -> Track {
        let (size, min, max) = self.dims();
        Track {
            size,
            min,
            max,
            content: 0,
        }
    }

    /// The node as selectors see it: its fixed classes, then its
    /// conditional ones that held when it last drew.
    fn element(&self) -> Element {
        let sheet = self.sheet.borrow();
        let mut classes = self.classes.clone();
        classes.extend(sheet.active.iter().cloned());
        Element {
            id: self.id,
            kind: self.what,
            name: self.name.clone(),
            fixed: self.classes.len(),
            maybe: self.class_when.iter().map(|(c, _)| c.clone()).collect(),
            classes,
            selected: sheet.selected,
            disabled: sheet.disabled,
            error: sheet.error,
        }
    }

    /// Match the stylesheet's rules to this node, the last of `chain`'s
    /// descendants, once per sheet: its layout, the border it draws, what
    /// its widget takes (a stack's gap, a grid's tracks, a panel's look).
    /// Whether it was matched now, rather than already.
    pub(crate) fn resolve(&self, cx: &SheetCx, chain: &mut Vec<Element>) -> bool {
        if self.sheet.borrow().generation == cx.generation {
            return false;
        }
        chain.push(self.element());
        let layout = cx.sheet.layout(chain, cx.vars, cx.viewport);
        let watch = cx.sheet.watches(chain, cx.viewport);
        chain.pop();
        if !self.set[SET_GAP] {
            let gap = layout.gap.unwrap_or(0);
            self.widget_mut(|stack: &mut Stack| stack.flow.gap = gap);
            self.widget_mut(|grid: &mut Grid| grid.gap = (gap, gap));
        }
        let set = self.set;
        self.widget_mut(|stack: &mut Stack| {
            let flow = &mut stack.flow;
            if !set[SET_ALIGN_ITEMS] {
                flow.align_items = layout.align_items.unwrap_or_default();
            }
            if !set[SET_JUSTIFY] {
                flow.justify = layout.justify.unwrap_or_default();
            }
            if !set[SET_WRAP] {
                flow.wrap = layout.wrap.unwrap_or_default();
            }
            if !set[SET_ALIGN_CONTENT] {
                flow.align_content = layout.align_content.unwrap_or_default();
            }
        });
        let rows_set = self.set[SET_ROWS];
        self.widget_mut(|grid: &mut Grid| {
            if !rows_set {
                grid.rows = layout.grid_rows.clone().unwrap_or_default();
            }
            if !grid.code_columns {
                grid.columns = layout.grid_columns.clone().unwrap_or_default();
            }
        });
        let is_panel = self
            .widget_mut(|panel: &mut Panel| {
                panel.look = crate::builtin::PanelLook {
                    border: layout.border.clone(),
                    no_border: layout.no_border,
                    title: layout.border_title.clone(),
                    title_align: layout.title_align,
                    subtitle: layout.subtitle.clone(),
                    subtitle_align: layout.subtitle_align,
                    sides: layout.sides,
                    padding: layout.padding,
                };
            })
            .is_some();
        self.widget_mut(|text: &mut TextNode| {
            let [align, wrap, overflow] = text.code;
            if !align {
                text.align = layout.text_align;
            }
            if !wrap {
                text.no_wrap = layout.text_wrap.map(|wrap| !wrap);
            }
            if !overflow {
                text.overflow = layout.text_overflow;
            }
        });
        // A container that collapses its children's borders takes a
        // stylesheet's border as its lines', rather than a box round it.
        let collapse = |collapse: &mut crate::builtin::Collapse| {
            if !collapse.code {
                collapse.on = layout.collapse.unwrap_or(false);
            }
            collapse.sheet = layout.border.clone().filter(|_| collapse.on);
            collapse.on
        };
        let collapsed = self
            .widget_mut(|stack: &mut Stack| collapse(&mut stack.collapse))
            .or_else(|| self.widget_mut(|grid: &mut Grid| collapse(&mut grid.collapse)))
            .unwrap_or(false);
        let (decor, insets) = if is_panel {
            (None, [0; 4])
        } else if collapsed {
            (None, layout.padding)
        } else {
            (layout.border.clone(), layout.insets())
        };
        let mut sheet = self.sheet.borrow_mut();
        sheet.generation = cx.generation;
        sheet.layout = layout;
        sheet.watch = watch;
        sheet.decor = decor;
        sheet.insets = insets;
        true
    }

    /// Its conditional classes that hold now, and whether it is selected,
    /// disabled and in error, reading the signals they read.
    fn conditions(&self) -> (Vec<String>, bool, bool, bool) {
        (
            self.class_when
                .iter()
                .filter(|(_, condition)| condition())
                .map(|(class, _)| class.clone())
                .collect(),
            self.selected_when.as_ref().is_some_and(|c| c()),
            self.disabled_when.as_ref().is_some_and(|c| c()),
            self.invalid(),
        )
    }

    /// Visit this node and every node inside it that is shown, depth first.
    pub(crate) fn walk(&self, f: &mut dyn FnMut(&Node, &[NodeId])) {
        let mut path = Vec::new();
        self.walk_inner(&mut path, f);
    }

    fn walk_inner(&self, path: &mut Vec<NodeId>, f: &mut dyn FnMut(&Node, &[NodeId])) {
        self.walk_some(path, false, f);
    }

    /// [`walk`](Self::walk), leaving out what is
    /// [hidden](Self::access_hidden) from assistive technology: the focus
    /// order.
    pub(crate) fn walk_accessible(&self, f: &mut dyn FnMut(&Node, &[NodeId])) {
        self.walk_some(&mut Vec::new(), true, f);
    }

    fn walk_some(
        &self,
        path: &mut Vec<NodeId>,
        accessible: bool,
        f: &mut dyn FnMut(&Node, &[NodeId]),
    ) {
        if self.hidden() || (accessible && self.access_hidden) {
            // A stylesheet's `display: none`: not shown, nor anything inside.
            return;
        }
        path.push(self.id);
        f(self, path);
        self.each_child(&mut |child| child.walk_some(path, accessible, f));
        path.pop();
    }

    /// Whether its widget holds other nodes.
    pub(crate) fn has_children(&self) -> bool {
        !self.body.borrow().widget.children().is_empty()
    }

    /// Call `f` with each child that is shown.
    pub(crate) fn each_child(&self, f: &mut dyn FnMut(&Node)) {
        for child in self.body.borrow().widget.children() {
            f(child);
        }
    }

    /// Whether drawing it into `rect` this frame would draw anything: it
    /// or something inside it is dirty, or it moved. Read without drawing,
    /// so a layers stack composites in one pass.
    fn will_draw(&self, frame: &FrameState, rect: Rect) -> bool {
        if self.hidden() {
            return self.drawn.get().is_some();
        }
        if self.drawn.get() != Some(rect) {
            return true;
        }
        !frame.dirty.is_empty() && self.dirty_within(&frame.dirty)
    }

    /// It or a node inside it is in `dirty`.
    fn dirty_within(&self, dirty: &HashSet<NodeId>) -> bool {
        if dirty.contains(&self.id) {
            return true;
        }
        let mut found = false;
        self.each_child(&mut |child| found = found || child.dirty_within(dirty));
        found
    }

    /// Not shown this frame: no rectangle (so no clicks reach it or
    /// anything inside it), and it draws afresh when it comes back.
    fn hide(&self) {
        if let Some(handle) = *self.rect_slot.borrow() {
            if handle.get_untracked().is_some() {
                handle.set(None);
            }
        }
        self.rect.set(Rect::default());
        self.inner.set(Rect::default());
        self.drawn.set(None);
        self.highlit.set(false);
        self.hover_lit.set(false);
        // Its sheet style is applied afresh when it comes back.
        self.sheet.borrow_mut().style = None;
        self.each_child(&mut |child| child.hide());
    }

    /// How many cells of `axis` the content needs, given `width` x
    /// `height` to lay out in (a height of 0: as tall as it likes).
    pub(crate) fn measure(&self, console: &Console, axis: Axis, width: u16, height: u16) -> u16 {
        if self.hidden() {
            return 0;
        }
        let [top, right, bottom, left] = self.sheet.borrow().insets;
        let height = if height == 0 {
            0
        } else {
            height.saturating_sub(top.saturating_add(bottom)).max(1)
        };
        let width = width.saturating_sub(left.saturating_add(right));
        // With theme styles of its own, it measures with them.
        let scope = self.scope_over(console, None);
        let console = scope.as_ref().map_or(console, |scope| &scope.console);
        let inner =
            self.body
                .borrow_mut()
                .widget
                .measure(&MeasureCx { console }, axis, width, height);
        inner.saturating_add(match axis {
            Axis::Vertical => top.saturating_add(bottom),
            Axis::Horizontal => left.saturating_add(right),
        })
    }

    /// Its extent along `axis` as a stack sizes it: its size, within its
    /// minimum and maximum.
    pub(crate) fn extent(&self, console: &Console, axis: Axis, width: u16, height: u16) -> u16 {
        let along = match axis {
            Axis::Vertical => height,
            Axis::Horizontal => width,
        };
        let (size, min, max) = self.dims();
        let cells = match size {
            Size::Fixed(n) => n,
            Size::Percent(p) => (along as u32 * p.min(100) as u32 / 100) as u16,
            Size::Ratio(a, b) => crate::layout::ratio_of(along as u32, a, b) as u16,
            Size::Auto | Size::Flex(_) => self.measure(console, axis, width, height),
        };
        cells.min(max).max(min)
    }

    /// Lay out and draw into `screen`. A node draws if it is dirty, has
    /// moved or changed size, or `force` says its area was overwritten;
    /// damage records what was written.
    pub(crate) fn draw(
        &self,
        frame: &mut FrameState,
        rect: Rect,
        screen: &mut Screen,
        force: bool,
    ) {
        // Hidden (`display: none`, `hide_below` and the like): not drawn,
        // whatever room its parent gave it (a split's pane, a scroll's or a
        // switch's child), so nothing inside it is shown or clicked.
        if self.hidden() {
            self.hide();
            return;
        }
        // While the inspector is open: how long this node takes, less what
        // its children take.
        let clock = frame.timings.is_some().then(|| {
            frame.time_stack.push(std::time::Duration::ZERO);
            crate::clock::Instant::now()
        });
        self.rect.set(rect);
        // Hidden from assistive technology: nothing inside is announced,
        // and a live node holding it says what it shows without it.
        if self.access_hidden {
            frame.hidden += 1;
            frame.hidden_rects.push(rect);
        }
        let hidden_mark = frame.hidden_rects.len();
        let last = self.drawn.get();
        let moved = last != Some(rect);
        let dirty = frame.dirty.contains(&self.id);
        let redraw = moved || force || dirty;
        let id = self.id;
        // Inside the stylesheet's border and padding, the widget's own.
        let (decor, inner) = {
            let sheet = self.sheet.borrow();
            let [top, right, bottom, left] = sheet.insets;
            let inner = Rect::new(
                rect.x.saturating_add(left),
                rect.y.saturating_add(top),
                rect.width.saturating_sub(left.saturating_add(right)),
                rect.height.saturating_sub(top.saturating_add(bottom)),
            );
            (sheet.decor.clone(), inner)
        };
        self.inner.set(inner);
        let styled = frame.sheet.is_some();
        if styled {
            frame.chain.push(self.element());
        }
        // With theme styles of its own, it and everything inside it draw
        // with a console and theme that have them, over those around it.
        let around = frame.scoped.last().cloned();
        let own_scope = self.scope_over(
            around
                .as_ref()
                .map_or(frame.console, |scope| &scope.console),
            Some(around.as_ref().map_or(frame.theme, |scope| &scope.theme)),
        );
        if let Some(scope) = &own_scope {
            frame.scoped.push(scope.clone());
        }
        let scope = own_scope.clone().or(around);
        let (app_console, app_theme) = (frame.console, frame.theme);
        let console: &Console = scope.as_ref().map_or(app_console, |scope| &scope.console);
        let theme: &Theme = scope.as_ref().map_or(app_theme, |scope| &scope.theme);
        let mut body = self.body.borrow_mut();
        let Body {
            widget,
            areas,
            laid,
            view,
        } = &mut *body;
        let viewport = widget.viewport();
        let focused = self.focus_style.is_some()
            && frame
                .focus_path
                .with_untracked(|path| path.last() == Some(&id));
        let hovered = self.hover_style.is_some()
            && frame.hover_path.with_untracked(|path| path.contains(&id));
        // Damage from here on is this node's and its children's: where
        // the focus style goes over what they drew.
        let mark = frame.damage.len();
        // What the widget wrote, and whether it painted all of its rectangle.
        let mut written: Vec<Rect> = Vec::new();
        let mut repaint = false;
        let mut cleared = false;
        let mut relaid = false;
        let mut restyled = false;
        let mut invalid = self.error_lit.get();
        if redraw {
            // Told its size the first time it is laid out (or shown again),
            // and whenever the size changes.
            let changed = match last.filter(|last| !last.is_empty()) {
                Some(last) => (last.width, last.height) != (rect.width, rect.height),
                None => true,
            };
            if changed && !rect.is_empty() {
                frame.resized.push(id);
            }
            // The children's layout from the stylesheet, before they are
            // laid out.
            if let Some(sheet) = &frame.sheet {
                for child in widget.children() {
                    child.resolve(sheet, &mut frame.chain);
                }
            }
            // Lay out first, subscribing to what that reads; a viewport
            // lays out in its content's coordinates.
            let space = if viewport {
                Rect::new(0, 0, inner.width, inner.height)
            } else {
                inner
            };
            let mut new_areas = frame
                .runtime
                .observe_node(id, || widget.layout(&MeasureCx { console }, space));
            // Children the layout built (a switch's page, a new keyed row)
            // match the stylesheet before they draw, and it lays out again
            // with what that says (`display: none`, a size).
            if let Some(sheet) = &frame.sheet {
                let mut fresh = false;
                for child in widget.children() {
                    fresh |= child.resolve(sheet, &mut frame.chain);
                }
                if fresh {
                    new_areas = frame
                        .runtime
                        .observe_node(id, || widget.layout(&MeasureCx { console }, space));
                }
            }
            let ids: Vec<NodeId> = widget.children().iter().map(|c| c.id).collect();
            relaid =
                (!laid.is_empty() || !areas.is_empty()) && (ids != *laid || new_areas != *areas);
            *areas = new_areas;
            *laid = ids;
            // Its classes and states for the stylesheet now, and the style
            // they give it; a change draws it and what is inside again.
            if styled {
                restyled = self.restyle(frame);
            }
            // What assistive technology is told of it (its states, whether
            // it is disabled): read here, so a change draws it again and
            // the accessibility tree, linear mode's lines with it, follow.
            frame.runtime.observe_node_more(id, || {
                self.access_state_of(&**widget);
                self.disabled();
            });
            // In error: a panel draws its border in the theme's error
            // style; another node's cells take it under their own.
            if self.error_when.is_some() {
                invalid = frame.runtime.observe_node_more(id, || self.invalid());
                let any: &mut dyn Any = &mut **widget;
                if let Some(panel) = any.downcast_mut::<Panel>() {
                    panel.invalid = invalid;
                }
            }
            // A widget that keeps what it drew is cleared only when its own
            // children moved: when it moved, or was drawn over, whoever did
            // that cleared the area already.
            // Gaining or losing the focus style redraws the lot; keeping it
            // costs only what is drawn again.
            cleared = !widget.retained()
                || focused != self.highlit.get()
                || hovered != self.hover_lit.get()
                || invalid != self.error_lit.get()
                || restyled
                || (relaid && !viewport);
            repaint = moved || force || cleared;
            // In a layers stack drawn afresh the area is clear already, and
            // the layers below show where this draws nothing.
            if cleared && frame.composing == 0 {
                screen.clear(rect);
            }
            // A node with a background covers its whole rectangle, in a
            // layers stack too: the layers below do not show through the
            // gaps its children leave (an inset, room between them).
            if repaint && frame.composing > 0 && self.has_background(console) {
                screen.clear(rect);
            }
            if let (true, Some((kind, style))) = (repaint, &decor) {
                let title_style = style.combine(&theme.title);
                let (sides, titles) = {
                    let sheet = self.sheet.borrow();
                    (sheet.layout.sides(), sheet.layout.titles())
                };
                let look = crate::border::Frame {
                    border: *kind,
                    sides,
                    style,
                    title_style: &title_style,
                    titles: &titles,
                    flush: false,
                };
                for edge in crate::border::draw_frame(screen, console, &look, rect) {
                    frame.damage.push(edge);
                }
            }
        }
        // A viewport's children draw offscreen before it decides where to
        // look; it draws itself after, so its scrollbar matches.
        let mut window = None;
        if viewport {
            window = Some(self.draw_view(frame, widget, areas, view, inner, relaid));
        }
        if redraw {
            let focus_path = frame.focus_path;
            let focus_style = self.focus_style.is_some();
            let hover_path = frame.hover_path;
            let hover_style = self.hover_style.is_some();
            if self.tooltip.is_some() {
                // A tooltip waits for the pointer to rest: its movement
                // must be reported.
                frame.wants_hover.set(true);
            }
            if hover_style {
                // The pointer's movement is reported once something reads
                // it, and this node draws again when the pointer comes or
                // goes.
                frame.wants_hover.set(true);
                frame.watchers.hover.borrow_mut().insert(id);
            }
            let caret = {
                let mut cx = DrawCx {
                    console,
                    theme,
                    id,
                    rect: inner,
                    repaint,
                    focus_path,
                    hover_path: frame.hover_path,
                    pointer: frame.pointer,
                    shift: frame.shift,
                    wants_hover: &frame.wants_hover,
                    watchers: frame.watchers,
                    disabled: self.disabled(),
                };
                let mut canvas = Canvas {
                    screen: &mut *screen,
                    rect: inner,
                    written: &mut written,
                };
                frame.runtime.observe_node_more(id, || {
                    widget.draw(&mut cx, &mut canvas);
                    // Reading the focus subscribes the node, so it draws
                    // again when the focus comes or goes.
                    if focus_style {
                        focus_path.with(|path| path.last() == Some(&id));
                    }
                    if hover_style {
                        hover_path.with(|path| path.contains(&id));
                    }
                });
                widget.caret()
            };
            self.caret.set(caret.and_then(|(x, y)| {
                (x < inner.width && y < inner.height).then_some((inner.x + x, inner.y + y))
            }));
            if cleared {
                frame.damage.push(rect);
            } else {
                frame.damage.extend(written.iter().copied());
            }
            if cleared || !written.is_empty() {
                frame.drew(id);
            }
        }
        match window {
            Some((window, offset, inner)) => {
                // The rows in view, copied out when they changed.
                let shown = view.as_ref().and_then(|v| v.shown);
                if redraw || inner || shown != Some((window, offset)) {
                    let view = view.as_mut().expect("a viewport has a view");
                    screen.clear(window);
                    screen.blit(
                        &view.buffer,
                        Rect::new(offset.0, offset.1, window.width, window.height),
                        window.x,
                        window.y,
                    );
                    frame.damage.push(window);
                    if !redraw {
                        frame.drew(id);
                    }
                    view.shown = Some((window, offset));
                }
            }
            None => {
                let overlapping = widget.overlapping();
                let siblings = frame.damage.len();
                // Layers composite: where a layer draws nothing (a gap
                // between its children, no background), the ones below it
                // show. When any of them is to draw, they all draw once
                // over a clear area, bottom first, none clearing what is
                // under it.
                let shown = areas.iter().filter(|area| !area.is_empty()).count();
                let layered = overlapping && shown > 1;
                let compose = layered
                    && (repaint
                        || !written.is_empty()
                        || widget
                            .children()
                            .iter()
                            .zip(areas.iter())
                            .any(|(child, area)| {
                                let area = area.intersection(inner);
                                !area.is_empty() && child.will_draw(frame, area)
                            }));
                if compose {
                    if frame.composing == 0 {
                        screen.clear(inner);
                    }
                    frame.damage.push(inner);
                    frame.composing += 1;
                }
                for (i, child) in widget.children().iter().enumerate() {
                    match areas.get(i).map(|area| area.intersection(inner)) {
                        Some(area) if !area.is_empty() => {
                            // A child draws again where the widget drew over
                            // it, and, over its earlier siblings, where they
                            // drew this frame, on a clear area.
                            let over = compose || written.iter().any(|w| w.intersects(area));
                            let under = overlapping
                                && !compose
                                && frame.damage[siblings..].iter().any(|d| d.intersects(area));
                            if under {
                                screen.clear(area);
                                frame.damage.push(area);
                            }
                            child.draw(frame, area, screen, repaint || over || under);
                        }
                        _ => child.hide(),
                    }
                }
                if compose {
                    frame.composing -= 1;
                }
                // Something drew that was not foreseen: the stack draws
                // again, as above, so the layers still composite.
                if layered && !compose && frame.damage.len() > siblings {
                    if frame.composing == 0 {
                        screen.clear(inner);
                    }
                    frame.damage.push(inner);
                    frame.composing += 1;
                    for (i, child) in widget.children().iter().enumerate() {
                        if let Some(area) = areas.get(i).map(|area| area.intersection(inner)) {
                            if !area.is_empty() {
                                child.draw(frame, area, screen, true);
                            }
                        }
                    }
                    frame.composing -= 1;
                }
                // An effect sees what is inside it whole: when a child
                // drew and the effect did not, everything inside draws
                // again before it runs, so that it never runs over what it
                // wrote itself. An effect that wrote nothing last time left
                // only what was drawn inside, so it runs over that as it is.
                let effect = (&**widget as &dyn Any)
                    .downcast_ref::<crate::effect::Effect>()
                    .is_some_and(|effect| effect.wrote);
                if effect && !repaint && frame.damage.len() > siblings {
                    screen.clear(inner);
                    frame.damage.push(inner);
                    for (i, child) in widget.children().iter().enumerate() {
                        if let Some(area) = areas.get(i).map(|area| area.intersection(inner)) {
                            if !area.is_empty() {
                                child.draw(frame, area, screen, true);
                            }
                        }
                    }
                }
            }
        }
        // What it draws over its children, in a frame where it or one of
        // them drew.
        if redraw || frame.damage.len() > mark {
            let mut over: Vec<Rect> = Vec::new();
            {
                let mut cx = DrawCx {
                    console,
                    theme,
                    id,
                    rect: inner,
                    repaint,
                    focus_path: frame.focus_path,
                    hover_path: frame.hover_path,
                    pointer: frame.pointer,
                    shift: frame.shift,
                    wants_hover: &frame.wants_hover,
                    watchers: frame.watchers,
                    disabled: self.disabled(),
                };
                let mut canvas = Canvas {
                    screen: &mut *screen,
                    rect: inner,
                    written: &mut over,
                };
                frame
                    .runtime
                    .observe_node_more(id, || widget.draw_over(&mut cx, &mut canvas));
            }
            if let Some(effect) =
                (&mut **widget as &mut dyn Any).downcast_mut::<crate::effect::Effect>()
            {
                effect.wrote = !over.is_empty();
            }
            if !over.is_empty() {
                if !redraw {
                    frame.drew(id);
                }
                frame.damage.extend(over);
            }
        }
        // The stylesheet's colours go under everything drawn inside the
        // node where nothing deeper set them, once its children have drawn;
        // the node's own style (code's) goes over the sheet's.
        let sheet_style = self.sheet.borrow().style.clone();
        let own_style = self.own_style.as_ref().map(|own| own.resolve(console));
        let under = match (sheet_style, own_style) {
            (Some(sheet), Some(own)) => Some(sheet.combine(&own)),
            (sheet, own) => own.or(sheet),
        };
        if let Some(style) = under {
            let mut parts: Vec<Rect> = frame.damage[mark..]
                .iter()
                .map(|area| area.intersection(rect))
                .filter(|area| !area.is_empty())
                .collect();
            if repaint {
                parts = vec![rect];
            }
            for area in parts {
                for row in area.y..area.bottom() {
                    for column in area.x..area.right() {
                        screen.underlay(column, row, &style);
                    }
                }
                frame.damage.push(area);
            }
        }
        // The theme's error look goes under what the stylesheet left, the
        // same way (a panel shows it on its border instead).
        if invalid && self.what != "panel" {
            let style = theme.error_style("error");
            let mut parts: Vec<Rect> = frame.damage[mark..]
                .iter()
                .map(|area| area.intersection(rect))
                .filter(|area| !area.is_empty())
                .collect();
            if repaint {
                parts = vec![rect];
            }
            for area in parts {
                for row in area.y..area.bottom() {
                    for column in area.x..area.right() {
                        screen.underlay(column, row, &style);
                    }
                }
                frame.damage.push(area);
            }
        }
        self.error_lit.set(invalid);
        if styled {
            frame.chain.pop();
        }
        // A live node says what it shows when that changed.
        if self.live && frame.hidden == 0 && frame.damage.len() > mark {
            let hidden = &frame.hidden_rects[hidden_mark..];
            let shown = crate::a11y::screen_text(screen, rect, hidden);
            if let Some(text) = self.live_change(shown) {
                frame.announcements.push(crate::a11y::Announcement {
                    text,
                    urgent: false,
                });
            }
        }
        // The focus style goes over everything drawn inside the node, its
        // children's cells included, once they have drawn.
        if let (true, Some(name)) = (focused, self.focus_style.as_deref()) {
            let style = crate::widget::theme_style(console, name, "reverse");
            let mut parts: Vec<Rect> = frame.damage[mark..]
                .iter()
                .map(|area| area.intersection(rect))
                .filter(|area| !area.is_empty())
                .collect();
            // Newly focused, or drawn in full (a restack clears the whole
            // screen first, before this node's damage is counted): the
            // whole rectangle, gaps and padding included.
            if !self.highlit.get() || repaint {
                parts = vec![rect];
            }
            for area in parts {
                for row in area.y..area.bottom() {
                    for column in area.x..area.right() {
                        screen.restyle(column, row, &style);
                    }
                }
                // Restyled cells are sent even where nothing drew (a
                // container's whole rectangle).
                frame.damage.push(area);
            }
        }
        self.highlit.set(focused);
        // The hover style, the same way, over the focus style.
        if let (true, Some(name)) = (hovered, self.hover_style.as_deref()) {
            let style = crate::widget::theme_style(console, name, "reverse");
            let mut parts: Vec<Rect> = frame.damage[mark..]
                .iter()
                .map(|area| area.intersection(rect))
                .filter(|area| !area.is_empty())
                .collect();
            if !self.hover_lit.get() || repaint {
                parts = vec![rect];
            }
            for area in parts {
                for row in area.y..area.bottom() {
                    for column in area.x..area.right() {
                        screen.restyle(column, row, &style);
                    }
                }
                // Restyled cells are sent even where nothing drew (a
                // container's whole rectangle).
                frame.damage.push(area);
            }
        }
        self.hover_lit.set(hovered);
        self.drawn.set(Some(rect));
        if self.access_hidden {
            frame.hidden -= 1;
        }
        if own_scope.is_some() {
            frame.scoped.pop();
        }
        if let Some(start) = clock {
            frame.timed(id, start, redraw);
        }
    }

    /// Read its conditions and states for the stylesheet (subscribing to
    /// what they read) and work out its style. Whether anything changed
    /// that it, or a node inside it, must draw again for.
    fn restyle(&self, frame: &mut FrameState) -> bool {
        let Some(cx) = &frame.sheet else {
            return false;
        };
        let (any, focus, hover) = self.sheet.borrow().watch;
        let (active, selected, disabled, error) = frame
            .runtime
            .observe_node_more(self.id, || self.conditions());
        let changed = {
            let sheet = self.sheet.borrow();
            sheet.active != active
                || sheet.selected != selected
                || sheet.disabled != disabled
                || sheet.error != error
        };
        if changed {
            let mut sheet = self.sheet.borrow_mut();
            sheet.active = active;
            sheet.selected = selected;
            sheet.disabled = disabled;
            sheet.error = error;
        }
        if let Some(top) = frame.chain.last_mut() {
            *top = self.element();
        }
        let style = if any {
            if hover {
                frame.wants_hover.set(true);
            }
            let read = |path: Signal<Vec<NodeId>>, track: bool| {
                frame.runtime.observe_node_more(self.id, || {
                    if track {
                        path.get()
                    } else {
                        path.get_untracked()
                    }
                })
            };
            let focus_path = read(frame.focus_path, focus);
            let hover_path = read(frame.hover_path, hover);
            let states = States {
                focus: &focus_path,
                hover: &hover_path,
            };
            cx.sheet.style(&frame.chain, &states, cx.vars, cx.viewport)
        } else {
            None
        };
        let mut sheet = self.sheet.borrow_mut();
        let restyled = sheet.style != style;
        sheet.style = style;
        changed || restyled
    }

    /// Draw a viewport's children into its offscreen content and ask it
    /// where to look: the window on the screen, the content's point at its
    /// top left, and whether anything inside drew.
    fn draw_view(
        &self,
        frame: &mut FrameState,
        widget: &mut Box<dyn Widget>,
        areas: &[Rect],
        view: &mut Option<View>,
        rect: Rect,
        relaid: bool,
    ) -> (Rect, (u16, u16), bool) {
        let content = areas.iter().fold((0u16, 0u16), |(w, h), area| {
            (w.max(area.right()), h.max(area.bottom()))
        });
        let view = view.get_or_insert_with(|| View {
            buffer: Screen::new(0, 0),
            shown: None,
        });
        let previous = view.shown;
        let mut force = relaid;
        let size = view.buffer.area();
        if (size.width, size.height) != content {
            view.buffer = Screen::new(content.0, content.1);
            force = true;
        } else if relaid {
            // The children moved: what they leave between them (gaps,
            // padding) must not keep old cells.
            view.buffer.clear(size);
        }
        if force {
            view.shown = None;
        }
        // Inside, the screen's coordinates are shifted by the window and
        // the scroll, as they were last shown.
        let outer = frame.shift;
        if let Some((window, offset)) = previous {
            frame.shift = (
                outer.0 - window.x as i32 + offset.0 as i32,
                outer.1 - window.y as i32 + offset.1 as i32,
            );
        }
        let saved = std::mem::take(&mut frame.damage);
        let all = view.buffer.area();
        for (i, child) in widget.children().iter().enumerate() {
            match areas.get(i).map(|area| area.intersection(all)) {
                Some(area) if !area.is_empty() => child.draw(frame, area, &mut view.buffer, force),
                _ => child.hide(),
            }
        }
        let inner = !std::mem::replace(&mut frame.damage, saved).is_empty();
        frame.shift = outer;
        // The content is copied onto the screen by style, not by id, so
        // its own table drops what no cell uses once that piles up.
        if view.buffer.styles_piled_up() {
            view.buffer.compact_styles();
        }
        // Where the focus is inside, for scrolling it into view.
        let focused = frame.focus_path.with_untracked(|path| {
            let last = *path.last()?;
            if !path.contains(&self.id) || last == self.id {
                return None;
            }
            let mut found = None;
            for child in widget.children() {
                with_node(child, last, &mut |node| found = Some(node.rect()));
            }
            found.map(|area| (last, area))
        });
        let cx = ScrollCx { content, focused };
        let (local, offset) = frame.runtime.untracked(|| widget.scroll(&cx));
        let window = Rect::new(
            rect.x.saturating_add(local.x),
            rect.y.saturating_add(local.y),
            local.width,
            local.height,
        )
        .intersection(rect);
        let offset = (
            offset.0.min(content.0.saturating_sub(window.width)),
            offset.1.min(content.1.saturating_sub(window.height)),
        );
        if previous != Some((window, offset)) {
            // Widgets inside that read the pointer drew with the old
            // scroll: they draw again with the new one.
            let watchers = frame.watchers.pointer.borrow();
            if !watchers.is_empty() {
                for child in widget.children() {
                    child.walk(&mut |node, _| {
                        if watchers.contains(&node.id) {
                            frame.runtime.mark_dirty(node.id);
                        }
                    });
                }
            }
        }
        (window, offset, inner)
    }

    /// Forget this subtree's subscriptions (it left the tree), including
    /// the nodes a widget keeps hidden.
    pub(crate) fn forget(&self, runtime: &Runtime) {
        runtime.forget(self.id);
        let body = self.body.borrow();
        for child in body.widget.children() {
            child.forget(runtime);
        }
        for child in body.widget.hidden_children() {
            child.forget(runtime);
        }
    }
}

/// A box's four edges: top and bottom rows, left and right columns between
/// them.
pub(crate) fn edges(rect: Rect) -> [Rect; 4] {
    let inner = rect.height.saturating_sub(2);
    [
        Rect::new(rect.x, rect.y, rect.width, 1),
        Rect::new(rect.x, rect.bottom().saturating_sub(1), rect.width, 1),
        Rect::new(rect.x, rect.y + 1, 1, inner),
        Rect::new(rect.right().saturating_sub(1), rect.y + 1, 1, inner),
    ]
}

/// A box's lines (from [`border`]) as its four edges, in the order of
/// [`edges`]: the top and bottom rows whole, and the first and last
/// segment of each row between them. Whole segments, so a title's
/// grapheme clusters (an emoji sequence) stay intact.
pub(crate) fn edge_lines(lines: &[Vec<Segment>]) -> [Vec<Vec<Segment>>; 4] {
    let middle = &lines[1.min(lines.len())..lines.len().saturating_sub(1).max(1)];
    let side = |pick: fn(&Vec<Segment>) -> Option<&Segment>| -> Vec<Vec<Segment>> {
        middle
            .iter()
            .map(|line| pick(line).cloned().into_iter().collect())
            .collect()
    };
    [
        lines.first().cloned().into_iter().collect(),
        lines.last().cloned().into_iter().collect(),
        side(|line| line.first()),
        side(|line| line.last()),
    ]
}

/// A visit in [`walk_screen`]: the node, its path, the shift to screen
/// coordinates and the part of the screen it can show in.
pub(crate) type ScreenVisit<'a> = dyn FnMut(&Node, &[NodeId], (i32, i32), Rect) + 'a;

/// Visit every node that is shown, with the translation that takes its
/// coordinates to the screen's and the part of the screen it can show in
/// (both change inside a [`scroll`]).
pub(crate) fn walk_screen(root: &Node, f: &mut ScreenVisit) {
    fn inner(
        node: &Node,
        path: &mut Vec<NodeId>,
        shift: (i32, i32),
        clip: Rect,
        f: &mut ScreenVisit,
    ) {
        if node.hidden() {
            // Not shown, nor anything inside: no clicks reach it, and
            // assistive technology is not told of it.
            return;
        }
        path.push(node.id);
        f(node, path, shift, clip);
        let scrolled = node.body.borrow().view.as_ref().and_then(|view| {
            let (window, offset) = view.shown?;
            let window = translate(window, shift);
            Some((
                (
                    window.x as i32 - offset.0 as i32,
                    window.y as i32 - offset.1 as i32,
                ),
                window.intersection(clip),
            ))
        });
        let (shift, clip) = scrolled.unwrap_or((shift, clip));
        node.each_child(&mut |child| inner(child, path, shift, clip, f));
        path.pop();
    }
    let all = Rect::new(0, 0, u16::MAX, u16::MAX);
    inner(root, &mut Vec::new(), (0, 0), all, f);
}

/// `rect` moved by `shift`, clamped to the screen.
pub(crate) fn translate(rect: Rect, (dx, dy): (i32, i32)) -> Rect {
    let x = rect.x as i32 + dx;
    let y = rect.y as i32 + dy;
    let (x0, y0) = (x.max(0), y.max(0));
    let width = (rect.width as i32 - (x0 - x)).max(0);
    let height = (rect.height as i32 - (y0 - y)).max(0);
    Rect::new(
        x0.min(u16::MAX as i32) as u16,
        y0.min(u16::MAX as i32) as u16,
        width.min(u16::MAX as i32) as u16,
        height.min(u16::MAX as i32) as u16,
    )
}

/// What a frame needs while it draws.
pub(crate) struct FrameState<'a> {
    pub console: &'a Console,
    pub runtime: &'a Runtime,
    pub dirty: HashSet<NodeId>,
    pub damage: Vec<Rect>,
    /// How many layers stacks being drawn afresh this node is inside: it
    /// draws over what the layers below drew, clearing nothing.
    pub composing: usize,
    pub focus_path: Signal<Vec<NodeId>>,
    /// The nodes under the mouse pointer, outermost first.
    pub hover_path: Signal<Vec<NodeId>>,
    /// A widget asked whether it is hovered: the app turns on the
    /// terminal's pointer movement reports.
    pub wants_hover: Cell<bool>,
    /// The nodes that asked about the pointer.
    pub watchers: &'a Watchers,
    /// Where the pointer last was, on the screen.
    pub pointer: Option<(u16, u16)>,
    /// From the screen to the surface being drawn on (inside a viewport,
    /// its content).
    pub shift: (i32, i32),
    /// Nodes laid out at a new size this frame.
    pub resized: Vec<NodeId>,
    pub theme: &'a crate::app::Theme,
    /// Nodes drawn this frame.
    pub drawn: usize,
    /// Which, when the inspector is watching.
    pub drawn_ids: Option<Vec<NodeId>>,
    /// The app's stylesheet, if it has one.
    pub sheet: Option<SheetCx<'a>>,
    /// The nodes being drawn, outermost first, as selectors see them.
    pub chain: Vec<Element>,
    /// What [live](Node::live) nodes now say.
    pub announcements: Vec<crate::a11y::Announcement>,
    /// How many [hidden](Node::access_hidden) nodes hold the one drawing,
    /// and where hidden nodes drew this frame.
    pub hidden: u32,
    pub hidden_rects: Vec<Rect>,
    /// While the inspector is open: each node that drew, and the time it
    /// took itself (its children's not counted); `None` measures nothing.
    pub timings: Option<Vec<(NodeId, std::time::Duration)>>,
    /// The time the children of each node being drawn took, innermost
    /// last.
    pub time_stack: Vec<std::time::Duration>,
    /// The [theme styles](Node::theme_styles) of the nodes being drawn,
    /// innermost last: the console and theme to draw with.
    pub scoped: Vec<Rc<Scope>>,
}

impl FrameState<'_> {
    /// `id` drew this frame.
    pub fn drew(&mut self, id: NodeId) {
        self.drawn += 1;
        if let Some(ids) = &mut self.drawn_ids {
            ids.push(id);
        }
    }

    /// Node `id`, which started at `start`, is done: its own time is what
    /// its children did not take, noted if it `drew`; all of it counts
    /// towards its parent's children.
    pub fn timed(&mut self, id: NodeId, start: crate::clock::Instant, drew: bool) {
        let all = start.elapsed();
        let inner = self.time_stack.pop().unwrap_or_default();
        if let Some(outer) = self.time_stack.last_mut() {
            *outer += all;
        }
        if let (true, Some(times)) = (drew, &mut self.timings) {
            times.push((id, all.saturating_sub(inner)));
        }
    }
}

/// A rounded border round `rect`, with `title` (plain text) in the top
/// edge; the inside is left to the child.
pub(crate) fn border(
    title: &str,
    style: &Style,
    title_style: &Style,
    rect: Rect,
) -> Vec<Vec<Segment>> {
    border_box(Border::Rounded, title, style, title_style, rect)
}

/// [`border`] with a `kind` of box.
pub(crate) fn border_box(
    kind: Border,
    title: &str,
    style: &Style,
    title_style: &Style,
    rect: Rect,
) -> Vec<Vec<Segment>> {
    // In text mode a box is blank: a screen reader reads no lines, and the
    // title stays.
    let set = kind.set().unwrap_or(crate::border::BorderSet::ROUNDED);
    let [top_left, across, top_right, side, bottom_left, bottom_right] = if crate::a11y::text_mode()
    {
        [" "; 6]
    } else {
        [
            set.top_left,
            set.top,
            set.top_right,
            set.left,
            set.bottom_left,
            set.bottom_right,
        ]
    };
    let (w, h) = (rect.width as usize, rect.height as usize);
    if w < 2 || h < 2 {
        return Vec::new();
    }
    let edge = |s: String| Segment::new(s, Some(style.clone()));
    let title = if title.is_empty() {
        String::new()
    } else {
        format!(" {title} ")
    };
    let room = w.saturating_sub(4);
    let title = rich::cells::set_cell_size(&title, rich::cells::cell_len(&title).min(room));
    let title_len = rich::cells::cell_len(&title);
    let mut lines = Vec::with_capacity(h);
    if w < 4 {
        // Too narrow for a title: a plain box.
        lines.push(vec![edge(format!(
            "{top_left}{}{top_right}",
            across.repeat(w - 2)
        ))]);
    } else {
        // `title_len` is at most `w - 4`, so one dash always follows it.
        lines.push(vec![
            edge(format!("{top_left}{across}")),
            Segment::new(title, Some(title_style.clone())),
            edge(format!("{}{top_right}", across.repeat(w - 3 - title_len))),
        ]);
    }
    for _ in 0..h - 2 {
        lines.push(vec![
            edge(side.into()),
            Segment::new(" ".repeat(w - 2), None),
            edge(side.into()),
        ]);
    }
    lines.push(vec![edge(format!(
        "{bottom_left}{}{bottom_right}",
        across.repeat(w - 2)
    ))]);
    lines
}

/// Draw the edges of a `kind` box round `rect`, with `title` in its top
/// edge; the edges written.
pub(crate) fn draw_box(
    screen: &mut Screen,
    kind: Border,
    title: &str,
    style: &Style,
    title_style: &Style,
    rect: Rect,
) -> Vec<Rect> {
    let lines = border_box(kind, title, style, title_style, rect);
    if lines.is_empty() {
        return Vec::new();
    }
    let mut written = Vec::new();
    for (edge, part) in edges(rect).into_iter().zip(edge_lines(&lines)) {
        if !edge.is_empty() {
            screen.write_lines(edge, &part);
            written.push(edge);
        }
    }
    written
}

// Builders.

/// A node that draws itself from rendered lines; `draw` gets the console,
/// the width and the height. Signals it reads make it draw again. A height
/// of 0 asks how tall the node would like to be: a parent laying out a
/// [`Size::Auto`] child measures it so.
pub fn leaf(draw: impl Fn(&Console, u16, u16) -> Vec<Vec<Segment>> + 'static) -> Node {
    Node::from_widget(Box::new(Leaf(Box::new(draw))), "leaf")
}

/// Console markup, from a closure that may read signals:
/// `text(move || format!("[b]{}[/] items", count.get()))`. The
/// [`text!`](crate::text!) macro writes the closure for you. Its lines
/// wrap at its width; [`Node::text_align`], [`Node::no_wrap`] and
/// [`Node::overflow`] change how they sit and where they are cut.
pub fn text(markup: impl Fn() -> String + 'static) -> Node {
    Node::from_widget(Box::new(TextNode::new(Box::new(markup))), "text")
}

/// Text that shows the longest of its `forms` that fits its width: console
/// markup, longest first (`SELECTED: docs (29.3K, 1 files)`, `SELECTED:
/// docs (29.3K)`, `docs`), read from a closure that may read signals. None
/// fits: the last, cut with `…`. Lines do not wrap. As wide as the longest
/// form that fits the room it is given, so `.auto()` in a row takes no
/// more than it shows.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::fit_text;
///
/// let app = App::new(|| {
///     fit_text(|| vec!["Press q to quit".into(), "q: quit".into(), "q".into()])
/// });
/// let mut driver = app.driver(10, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "q: quit");
/// driver.resize(20, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Press q to quit");
/// ```
pub fn fit_text(forms: impl Fn() -> Vec<String> + 'static) -> Node {
    let mut text = TextNode::new(Box::new(String::new));
    text.forms = Some(Box::new(forms));
    text.no_wrap = Some(true);
    text.overflow = Some(Overflow::Ellipsis);
    text.code = [false, true, true];
    Node::from_widget(Box::new(text), "text")
}

/// Console markup that never changes.
pub fn label(markup: impl Into<String>) -> Node {
    let markup = markup.into();
    text(move || markup.clone()).what("label")
}

/// Any rich renderable, built by `f` (which may read signals) and rendered
/// at the node's size: a `Table`, `Markdown`, `Syntax`, `Panel`, a chart.
pub fn renderable<R: Renderable + 'static>(f: impl Fn() -> R + 'static) -> Node {
    leaf(move |console, width, height| {
        let options = console.options().update_width(width.max(1) as usize);
        // Height 0 is a measurement: as tall as the renderable likes.
        let options = if height == 0 {
            options
        } else {
            options.update_height(height as usize)
        };
        console.render_lines(&f(), &options, false)
    })
    .what("renderable")
}

/// A [`renderable`] that says what it shows: a chart, a table, a tree, or
/// anything else with rich-ext's
/// [`AccessibleText`](rich_ext::a11y::AccessibleText). Its accessible name
/// is that text at the width it was last drawn, its lines joined with
/// `. `, so the accessibility tree, [linear](crate::App::linear) mode and
/// a screen reader read `Line chart, 2 series, x from 0 to 23, …` instead
/// of the glyphs. A [`label`](Node::label) goes first: `CPU: Line chart,
/// …`.
///
/// ```
/// use intuituive::prelude::*;
/// use rich_ext::chart::Sparkline;
///
/// let app = App::new(|| {
///     let load = signal(vec![2.0, 4.0, 3.0, 9.0]);
///     described(move || Sparkline::new(load.get())).label("Load")
/// });
/// let mut driver = app.driver(20, 1);
/// driver.update(std::time::Duration::ZERO);
/// driver.render();
/// let tree = driver.accessibility();
/// assert_eq!(
///     tree[0].describe(),
///     "Load: Sparkline, 4 values: min 2, max 9, last 9, trend up"
/// );
/// ```
pub fn described<R>(f: impl Fn() -> R + 'static) -> Node
where
    R: Renderable + rich_ext::a11y::AccessibleText + 'static,
{
    let draw = move |console: &Console, width: u16, height: u16, say: bool| {
        let value = f();
        let options = console.options().update_width(width.max(1) as usize);
        let options = if height == 0 {
            options
        } else {
            options.update_height(height as usize)
        };
        let lines = console.render_lines(&value, &options, false);
        let text = say.then(|| {
            value
                .accessible_text(width.max(1) as usize)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(". ")
        });
        (lines, text)
    };
    let widget = Described {
        draw: Box::new(draw),
        text: None,
    };
    Node::from_widget(Box::new(widget), "described").what("renderable")
}

/// A `rich-interact` component as a node: an `Input`, a `Select`, a
/// `Form`, a `Pager`, or one of your own. It takes keys while it has the
/// focus (keys it does not use bubble on), shows its text caret, and calls
/// `on_done` with its answer (Enter in an `Input`, a pick in a `Select`).
///
/// ```
/// use intuituive::prelude::*;
/// use rich_interact::Input;
///
/// let app = App::new(|| {
///     let name = signal(String::new());
///     column([
///         component(Input::new("Name"), move |value, _| name.set(value)),
///         text!("Hello, {name}"),
///     ])
///     .on_key("esc", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["A", "d", "a", "enter", "esc"], 30, 3).unwrap();
/// assert!(screen.iter().any(|l| l.contains("Hello, Ada")), "{screen:?}");
/// ```
pub fn component<C: Component + 'static>(
    component: C,
    on_done: impl FnMut(C::Output, &mut Ctx) + 'static,
) -> Node {
    let mut node = Node::from_widget(
        Box::new(HostWidget::new::<C>(Box::new(Host {
            component,
            make: None,
            on_done: Box::new(on_done),
            on_cancel: None,
        }))),
        "component",
    );
    node.focusable = true;
    node.what("component")
}

/// A [`component`] that starts again after each answer: `make` builds it,
/// and builds a fresh one each time it is done, so an `Input` clears for
/// the next entry (a chat box, a to-do entry, a command line).
///
/// ```
/// use intuituive::prelude::*;
/// use rich_interact::Input;
///
/// let app = App::new(|| {
///     let added = signal(Vec::<String>::new());
///     column([
///         repeating(|| Input::new("Add"), move |item, _| added.update(|v| v.push(item))),
///         text(move || added.get().join(", ")),
///     ])
///     .on_key("esc", |cx| cx.quit())
/// });
/// let keys = ["a", "enter", "b", "enter", "esc"];
/// let screen = app.render_with(&keys, 30, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "a, b");
/// ```
pub fn repeating<C: Component + 'static>(
    make: impl Fn() -> C + 'static,
    on_done: impl FnMut(C::Output, &mut Ctx) + 'static,
) -> Node {
    let mut node = Node::from_widget(
        Box::new(HostWidget::new::<C>(Box::new(Host {
            component: make(),
            make: Some(Box::new(make)),
            on_done: Box::new(on_done),
            on_cancel: None,
        }))),
        "component",
    );
    node.focusable = true;
    node.what("component")
}

/// A viewport onto `child`, laid out at its full height (up to 4000 rows)
/// and scrolled with the arrow keys, PgUp/PgDn and Home/End (when nothing
/// inside used them) and the mouse wheel, with a scrollbar when it does not
/// fit. Focus moving to a node inside scrolls it into view. Only the nodes
/// inside that changed draw; the rows in view are copied out.
///
/// It is for content of a modest size: the child is laid out and drawn
/// whole, and rows past the 4000th (`MAX_SCROLL_ROWS`) are cut off and
/// cannot be reached. A debug build says so once on stderr when that
/// happens. For long or endless content (a feed, a log, search results),
/// use [`scroll_virtual`](crate::scroll_virtual), which builds only the
/// items near the view and has no row cap.
pub fn scroll(child: Node) -> Node {
    scroll_with(child, crate::reactive::signal(0))
}

/// [`scroll`], with the first row in view in `offset`: read it, or set it
/// to scroll from code.
pub fn scroll_with(child: Node, offset: Signal<u16>) -> Node {
    Node::from_widget(Box::new(ScrollView::new(child, offset)), "scroll").focusable()
}

/// `child` in a view that scrolls across: the content is as wide as it
/// asks for (lines are not wrapped to the view) and as tall as the view.
/// ←/→, Shift+PgUp/PgDn, the wheel and Home/End scroll it, and a bar
/// along the bottom shows where it is.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     scroll_x(label("0123456789abcdefghij")).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["right", "right", "q"], 8, 2).unwrap();
/// assert_eq!(screen[0], "23456789");
/// ```
pub fn scroll_x(child: Node) -> Node {
    scroll_both_with(
        child,
        crate::reactive::signal(0),
        crate::reactive::signal(0),
        true,
        false,
    )
}

/// `child` in a view that scrolls down and across, with a bar for each
/// way it overflows. Shift with the wheel scrolls across.
pub fn scroll_both(child: Node) -> Node {
    scroll_both_with(
        child,
        crate::reactive::signal(0),
        crate::reactive::signal(0),
        true,
        true,
    )
}

/// A scrolling view with its first column and row in view in `x` and `y`,
/// scrolling across, down, or both.
pub fn scroll_both_with(
    child: Node,
    x: Signal<u16>,
    y: Signal<u16>,
    across: bool,
    down: bool,
) -> Node {
    let view = ScrollView::both(child, x, y, across, down);
    Node::from_widget(Box::new(view), "scroll").focusable()
}

/// Children one above the other.
pub fn column(children: impl IntoIterator<Item = Node>) -> Node {
    stack(Axis::Vertical, children).what("column")
}

/// Children side by side.
pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    stack(Axis::Horizontal, children).what("row")
}

fn stack(axis: Axis, children: impl IntoIterator<Item = Node>) -> Node {
    Node::from_widget(
        Box::new(Stack {
            axis,
            flow: StackLayout::default(),
            children: children.into_iter().collect(),
            collapse: Default::default(),
        }),
        "stack",
    )
}

/// Children stacked on one rectangle, later ones over earlier ones: a
/// background under its content, a badge over a chart, a panel over a
/// ticking board. Each takes the whole rectangle, less its
/// [`inset`](Node::inset), or only its content's size where
/// [`place`](Node::place) puts it. A layer draws again wherever one under
/// it drew, so the one on top stays on top; a click goes to the top layer
/// under the pointer. The Tab order, and what assistive technology reads,
/// follow the order of the children.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     layers([
///         label("........\n........\n........"),
///         label("[b]3[/]").place(Align::End, Align::Start),
///     ])
/// });
/// let mut driver = app.driver(8, 3);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0], ".......3");
/// ```
pub fn layers(children: impl IntoIterator<Item = Node>) -> Node {
    Node::from_widget(
        Box::new(Layers {
            children: children.into_iter().collect(),
        }),
        "layers",
    )
}

/// Children in a grid with `columns`, filled row by row. A child can span
/// several columns and rows with [`Node::span`]; it takes the first place
/// where it fits. Rows share the height evenly unless sized with
/// [`Node::rows`]; [`Node::gap`] spaces the cells.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     grid(
///         [Size::Flex(1), Size::Flex(1)],
///         [
///             label("CPU").panel("1"),
///             label("Memory").panel("2"),
///             label("Disk").panel("3").span(2, 1),
///         ],
///     )
///     .rows([Size::Fixed(3)])
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 20, 6).unwrap();
/// assert!(screen[1].contains("CPU") && screen[1].contains("Memory"));
/// assert!(screen[4].contains("Disk"));
/// ```
pub fn grid(
    columns: impl IntoIterator<Item = Size>,
    children: impl IntoIterator<Item = Node>,
) -> Node {
    let columns: Vec<Size> = columns.into_iter().collect();
    Node::from_widget(
        Box::new(Grid {
            code_columns: !columns.is_empty(),
            columns,
            rows: Vec::new(),
            gap: (0, 0),
            children: children.into_iter().collect(),
            collapse: Default::default(),
        }),
        "grid",
    )
}

/// One child per key, top to bottom, each one row tall unless it is
/// [`fixed`](Node::fixed) to more or [`auto`](Node::auto) (as tall as its
/// content). `keys` may read signals; a key that stays
/// keeps its node (its state, its focus, its cached drawing) however the
/// list is reordered, and only new keys are built (`build` gets a clone of
/// the key).
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let items = signal(vec!["a", "b"]);
///     each(move || items.get(), |item| text!("• {item}"))
/// });
/// ```
pub fn each<K, F>(keys: impl Fn() -> Vec<K> + 'static, build: F) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K) -> Node + 'static,
{
    Node::from_widget(
        Box::new(Each {
            keys: Rc::new(keys),
            build,
            order: Vec::new(),
            children: Vec::new(),
        }),
        "each",
    )
}

/// The child for the current `key`, built by `build` the first time the key
/// is shown and kept while other keys are: tabs, the steps of a wizard, a
/// detail pane per item. `key` may read signals; when it changes, the
/// switch shows the other child, which keeps its state (and its focus
/// order) from when it was last shown. Only the shown child is in the Tab
/// order and takes clicks. A [list](crate::list)'s options given the
/// switch ([`highlight_layer`](Node::highlight_layer),
/// [`wheel_step`](Node::wheel_step), …) reach each list it shows: a list
/// with a "No items" text in its place keeps them.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let tab = signal(0usize);
///     column([
///         label("[b]1[/] Overview · [b]2[/] Logs").fixed(1),
///         switch(move || tab.get(), |tab| match tab {
///             0 => label("All systems go"),
///             _ => label("No logs yet"),
///         }),
///     ])
///     .on_key("1", move |_| tab.set(0))
///     .on_key("2", move |_| tab.set(1))
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["2", "q"], 30, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "No logs yet");
/// ```
pub fn switch<K, F>(key: impl Fn() -> K + 'static, build: F) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K) -> Node + 'static,
{
    // List options given the switch reach each child it builds: the list
    // shown, or the one shown later.
    let forward = crate::list::Forward::default();
    let options = forward.clone();
    let build = move |key: K| {
        let node = build(key);
        options.apply(&node);
        node
    };
    let switch = Switch::new(Box::new(key), build);
    let keep = switch.keep.clone();
    let mut node = Node::from_widget(Box::new(switch), "switch");
    node.list_forward = Some(forward);
    node.keep_pages = Some(keep);
    node
}

/// One child, built by `build` for the current `key` and built again when
/// the key changes, the old child dropped (its [`on_unmount`](Node::on_unmount)
/// handlers run, its subscriptions end): a pane rebuilt from state whose
/// shape changes (a map rebuilt on every scan batch, a table rebuilt when
/// its column set changes), where a [`switch`] would keep every child it
/// ever built. While the key stays the same the child stays, with its own
/// state. When the old child held the focus, the new one's first focusable
/// node takes it. It is a flexible child; `.auto()` sizes it by the child,
/// the child's own [`fixed`](Node::fixed) size included.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::keyed;
///
/// let app = App::new(|| {
///     let columns = signal(2usize);
///     keyed(move || columns.get(), |n| label(format!("{n} columns")))
///         .on_key("+", move |_| columns.update(|n| *n += 1))
/// });
/// let mut driver = app.driver(12, 1);
/// driver.press("+");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "3 columns");
/// ```
pub fn keyed<K, F>(key: impl Fn() -> K + 'static, build: F) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K) -> Node + 'static,
{
    let own = Rc::new(Cell::new(None));
    let mut switch = Switch::new(Box::new(key), build);
    switch.keep.set(1);
    switch.refocus = Some(own.clone());
    let node = Node::from_widget(Box::new(switch), "keyed");
    own.set(Some(node.id));
    node
}

/// The nodes under `column`, `row`, outermost first (the path to the
/// deepest node whose shown rectangle holds the point), each with the
/// translation from screen coordinates to its own. Where children overlap
/// ([`layers`]), the later one's path wins, however deep the earlier one's
/// goes: it is on top.
pub(crate) fn hit_path(root: &Node, column: u16, row: u16) -> Vec<(NodeId, (i32, i32))> {
    let mut found: Vec<(NodeId, (i32, i32))> = Vec::new();
    let mut shifts: Vec<(i32, i32)> = Vec::new();
    // The nodes whose children overlap.
    let mut overlapping: HashSet<NodeId> = HashSet::new();
    walk_screen(root, &mut |node, path, shift, clip| {
        shifts.truncate(path.len() - 1);
        shifts.push(shift);
        if node.body.borrow().widget.overlapping() {
            overlapping.insert(node.id);
        }
        let shown = translate(node.rect(), shift).intersection(clip);
        if !shown.contains(column, row) {
            return;
        }
        // Where this path leaves the one found: under a node whose
        // children overlap, this one is the later child.
        let common = found
            .iter()
            .zip(path)
            .take_while(|((id, _), other)| id == *other)
            .count();
        let above = common > 0
            && common < found.len()
            && common < path.len()
            && overlapping.contains(&path[common - 1]);
        if path.len() > found.len() || above {
            found = path
                .iter()
                .zip(&shifts)
                .map(|(id, (dx, dy))| (*id, (-dx, -dy)))
                .collect();
        }
    });
    found
}

/// For each node under a later layer ([`layers`]), where those layers are
/// on the screen: what reading its text from the screen leaves out.
pub(crate) fn covered(root: &Node) -> HashMap<NodeId, Vec<Rect>> {
    // Each child of a node whose children overlap: its later siblings.
    let mut over: HashMap<NodeId, Vec<Rect>> = HashMap::new();
    let mut out = HashMap::new();
    walk_screen(root, &mut |node, path, shift, clip| {
        let body = node.body.borrow();
        if body.widget.overlapping() {
            let children = body.widget.children();
            let rects: Vec<Rect> = children
                .iter()
                .map(|child| translate(child.rect(), shift).intersection(clip))
                .collect();
            for (i, child) in children.iter().enumerate() {
                over.insert(child.id, rects[i + 1..].to_vec());
            }
        }
        let mine: Vec<Rect> = path
            .iter()
            .filter_map(|id| over.get(id))
            .flatten()
            .copied()
            .collect();
        if !mine.is_empty() {
            out.insert(node.id, mine);
        }
    });
    out
}

/// Every node shown, with the translation from screen coordinates to its
/// own (they differ inside a viewport).
pub(crate) fn shifts(root: &Node) -> Vec<(NodeId, (i32, i32))> {
    let mut out = Vec::new();
    walk_screen(root, &mut |node, _, shift, _| {
        out.push((node.id, (-shift.0, -shift.1)))
    });
    out
}

/// Run `f` on the node with `id`.
/// The keys named in `names`, for a binding.
///
/// # Panics
/// On a name that is not a key, or on no names at all (`""`, or `" "`,
/// which is a separator: the space key is `space`).
pub(crate) fn parse_keys(names: &str) -> Vec<Key> {
    let keys = rich_interact::keymap::keys(names);
    assert!(
        !keys.is_empty(),
        "no keys in {names:?}: name them separated by spaces; the space key is \"space\""
    );
    keys
}

pub(crate) fn with_node(root: &Node, id: NodeId, f: &mut dyn FnMut(&Node)) {
    let mut done = false;
    root.walk(&mut |node, _| {
        if !done && node.id == id {
            f(node);
            done = true;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_without_theme_styles_draws_with_no_copy_and_one_with_them_with_one() {
        let console = Console::builder().width(10).build();
        let (dark, light) = (Theme::dark(), Theme::light());
        assert!(label("plain").scope_over(&console, Some(&dark)).is_none());
        let styled = label("x").theme_styles([("x", "red")]);
        let first = styled.scope_over(&console, Some(&dark)).expect("a scope");
        // Made once, and kept while the theme and the console's size hold.
        let again = styled.scope_over(&console, Some(&dark)).expect("a scope");
        assert!(Rc::ptr_eq(&first, &again));
        let measuring = styled.scope_over(&console, None).expect("measuring");
        assert!(Rc::ptr_eq(&first, &measuring));
        // Made again for another theme, or another size.
        let other = styled.scope_over(&console, Some(&light)).expect("a scope");
        assert!(!Rc::ptr_eq(&first, &other));
        let wider = Console::builder().width(20).build();
        let resized = styled.scope_over(&wider, Some(&light)).expect("a scope");
        assert!(!Rc::ptr_eq(&other, &resized));
        let named = rich::style::StyleType::Name("x".into());
        assert_eq!(
            resized.console.get_style(&named).ok(),
            Style::parse("red").ok()
        );
    }
}
