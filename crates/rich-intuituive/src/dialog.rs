//! Dialogs in the framework's look: [`Ctx::confirm`], [`Ctx::prompt`]
//! and [`Ctx::pick`].

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ctx;
use crate::layout::{Align, Justify, Size, TextAlign};
use crate::node::{column, label, row, Node};
use crate::place::{Offset, Origin};
use crate::reactive::{signal, watch};
use crate::rich::markup::escape;
use crate::rich::Overflow;
use crate::text_area::Caret;

/// What a confirm dialog runs on yes.
type OnYes = Rc<RefCell<Option<Box<dyn FnOnce(&mut Ctx)>>>>;

/// What a pick menu runs with the index picked.
type OnPick = Rc<RefCell<Option<Box<dyn FnOnce(usize, &mut Ctx)>>>>;

/// The most items a confirm dialog lists; the rest are counted.
const ITEMS_SHOWN: usize = 10;

impl Ctx {
    /// Ask before doing something: a modal centred over the screen with
    /// `title` (markup) on its top edge, `body` (markup) under it, then
    /// `items` (plain text: the files to trash, the rows to drop; ten at
    /// most, and a count of the rest), and `[Y]es` and `(N)o`. `y`, or
    /// Enter on Yes, closes it and runs `on_yes`; `n`, Esc, or Enter on No
    /// closes it. Tab and the arrows move between the two, a click picks
    /// one. Yazi's confirm, in the theme's `popup.title` and `selected`.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let gone = signal(0);
    ///     text!("trashed {gone}").on_key("d", move |cx| {
    ///         let items = vec!["a.txt".to_string(), "b.txt".to_string()];
    ///         cx.confirm("Trash 2 files?", "", items, move |_| gone.set(2))
    ///     })
    /// });
    /// let mut driver = app.driver(40, 10);
    /// driver.press("d");
    /// assert!(driver.screen().plain().iter().any(|r| r.contains("Trash 2 files?")));
    /// driver.press("y");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "trashed 2");
    /// ```
    pub fn confirm(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        items: Vec<String>,
        on_yes: impl FnOnce(&mut Ctx) + 'static,
    ) {
        self.confirm_with(title, body, items, ConfirmOptions::default(), on_yes);
    }

    /// A [`confirm`](Self::confirm) sized and styled by `options`: Yazi's
    /// trash confirm is 70 x 20 with a blue title. In a box taller than its
    /// content, the items take the room and the answers sit at the bottom.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::ConfirmOptions;
    ///
    /// let app = App::new(|| {
    ///     label("").on_key("d", |cx| {
    ///         let look = ConfirmOptions::new().size(Size::Fixed(30), Size::Fixed(8));
    ///         cx.confirm_with("Trash 1 file?", "", vec!["a.txt".into()], look, |_| {})
    ///     })
    /// });
    /// let mut driver = app.driver(40, 10);
    /// driver.press("d");
    /// let rows = driver.screen().plain();
    /// assert!(rows[1].contains("─ Trash 1 file? ─"));
    /// assert!(rows[2].contains("│ a.txt"));
    /// assert!(rows[7].contains("[Y]es"));
    /// ```
    pub fn confirm_with(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        items: Vec<String>,
        options: ConfirmOptions,
        on_yes: impl FnOnce(&mut Ctx) + 'static,
    ) {
        let (title, body) = (title.into(), body.into());
        let on_yes: OnYes = Rc::new(RefCell::new(Some(Box::new(on_yes))));
        let (width, height) = (options.width, options.height);
        self.modal(width, height, move || {
            let mut node = dialog(title, body, items, height != Size::Auto, on_yes);
            if let Some(style) = &options.title_style {
                node = node.title_style(style);
            }
            node
        });
    }

    /// Ask for a line of text: a one-line [`input`](crate::input) holding
    /// `initial`, in a box with `title` on its top edge, at the top centre
    /// of the screen, two rows down and 50 wide (Yazi's input). Enter
    /// closes it and gives `on_submit` the text; Esc closes it. Where it
    /// opens, the caret, a style for the title and what hears each edit
    /// and Esc are [`prompt_with`](Self::prompt_with)'s options.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let name = signal(String::from("notes.txt"));
    ///     text!("{name}").on_key("r", move |cx| {
    ///         cx.prompt("Rename:", name.get_untracked(), move |text, _| name.set(text))
    ///     })
    /// });
    /// let mut driver = app.driver(60, 8);
    /// driver.press("r");
    /// assert!(driver.screen().plain()[2].contains("╭─ Rename: ─"));
    /// driver.press("backspace backspace backspace m d enter");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "notes.md");
    /// ```
    pub fn prompt(
        &mut self,
        title: impl Into<String>,
        initial: impl Into<String>,
        on_submit: impl FnOnce(String, &mut Ctx) + 'static,
    ) {
        self.prompt_with(title, initial, PromptOptions::default(), on_submit);
    }

    /// A [`prompt`](Self::prompt) placed, and with the caret, the title
    /// style and the handlers `options` gives.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{Caret, Offset, Origin, PromptOptions};
    ///
    /// let app = App::new(|| {
    ///     let typed = signal(String::new());
    ///     text!("{typed}").on_key("f", move |cx| {
    ///         let options = PromptOptions::new()
    ///             .at(Origin::BottomLeft, Offset::new(0, 0, 30, 3))
    ///             .caret(Caret::Start)
    ///             .on_change(move |text, _| typed.set(text.to_string()));
    ///         cx.prompt_with("Find:", "", options, |_, _| {})
    ///     })
    /// });
    /// let mut driver = app.driver(40, 8);
    /// driver.press("f x y");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "xy");
    /// assert!(driver.screen().plain()[5].starts_with("╭─ Find: ─"));
    /// ```
    pub fn prompt_with(
        &mut self,
        title: impl Into<String>,
        initial: impl Into<String>,
        options: PromptOptions,
        on_submit: impl FnOnce(String, &mut Ctx) + 'static,
    ) {
        let (title, initial) = (title.into(), initial.into());
        let PromptOptions {
            origin,
            offset,
            caret,
            title_style,
            change,
            cancel,
        } = options;
        let submit = RefCell::new(Some(on_submit));
        self.popup_at(origin, offset, move || {
            let value = signal(initial);
            if let Some(mut change) = change {
                let first = value.get_untracked();
                watch(
                    move || value.get(),
                    move |text, cx| {
                        // The text it opened with is not an edit.
                        if text != first {
                            change(&text, cx)
                        }
                    },
                );
            }
            let cancel = RefCell::new(cancel);
            let mut node = crate::input::input(value)
                .caret(caret)
                .on_submit(move |text, cx| {
                    cx.pop();
                    if let Some(submit) = submit.borrow_mut().take() {
                        submit(text.to_string(), cx);
                    }
                })
                .on_key("esc", move |cx| {
                    cx.pop();
                    if let Some(cancel) = cancel.borrow_mut().take() {
                        cancel(cx);
                    }
                })
                .autofocus()
                .panel(&title);
            if let Some(style) = &title_style {
                node = node.title_style(style);
            }
            node
        });
    }

    /// Pick one of `items` (console markup) from a short list in a box
    /// with `title`, placed from `origin` as [`popup_at`](Self::popup_at)
    /// places one, as wide and tall as the items: Yazi's "Open with:"
    /// below the hovered row. Enter or a click picks the selected item,
    /// giving `on_pick` its index; Esc or `q` closes it with nothing
    /// picked. [`pick_with`](Self::pick_with) takes a size and a title
    /// style.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::Origin;
    ///
    /// let app = App::new(|| {
    ///     let chosen = signal(String::from("-"));
    ///     text!("{chosen}").on_key("o", move |cx| {
    ///         let openers = vec!["vim".to_string(), "less".into()];
    ///         let names = openers.clone();
    ///         cx.pick("Open with:", Origin::Center, openers, move |i, _| {
    ///             chosen.set(names[i].clone())
    ///         })
    ///     })
    /// });
    /// let mut driver = app.driver(30, 7);
    /// driver.press("o");
    /// assert!(driver.screen().plain().iter().any(|r| r.contains("╭─ Open with: ─╮")));
    /// driver.press("down enter");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "less");
    /// ```
    pub fn pick(
        &mut self,
        title: impl Into<String>,
        origin: Origin,
        items: Vec<String>,
        on_pick: impl FnOnce(usize, &mut Ctx) + 'static,
    ) {
        self.pick_with(title, origin, items, PickOptions::default(), on_pick);
    }

    /// A [`pick`](Self::pick) with the size, offset and title style
    /// `options` gives.
    pub fn pick_with(
        &mut self,
        title: impl Into<String>,
        origin: Origin,
        items: Vec<String>,
        options: PickOptions,
        on_pick: impl FnOnce(usize, &mut Ctx) + 'static,
    ) {
        let title = title.into();
        let offset = options.offset.unwrap_or_else(|| {
            // As wide as the widest item or the title, and as tall as the
            // items, in a box.
            let widest = items
                .iter()
                .map(|item| crate::widget::markup_width(item))
                .max()
                .unwrap_or(0);
            let title_width = crate::widget::markup_width(&title).saturating_add(4);
            let w = widest.max(title_width).saturating_add(2);
            let h = (items.len().min(u16::MAX as usize - 2) as u16).saturating_add(2);
            Offset::new(0, 0, w, h)
        });
        let picked: OnPick = Rc::new(RefCell::new(Some(Box::new(on_pick))));
        let title_style = options.title_style;
        let len = items.len();
        self.popup_at(origin, offset, move || {
            let selected = signal(0usize);
            let pick = move |cx: &mut Ctx, picked: &OnPick| {
                cx.pop();
                let picked = picked.borrow_mut().take();
                if let (Some(on_pick), true) = (picked, len > 0) {
                    on_pick(selected.get_untracked().min(len - 1), cx);
                }
            };
            let (on_enter, on_click) = (picked.clone(), picked);
            let mut node = crate::node::list(move || items.clone(), selected)
                .autofocus()
                .on_key("enter", move |cx| pick(cx, &on_enter))
                .on_click(move |cx| pick(cx, &on_click))
                .on_key("esc q", |cx| cx.pop())
                .panel(&title);
            if let Some(style) = &title_style {
                node = node.title_style(style);
            }
            node
        });
    }
}

/// How a [`confirm_with`](Ctx::confirm_with) dialog looks: its size (50
/// cells wide and as tall as its content by default) and its title's
/// style (the theme's `popup.title` by default).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmOptions {
    pub width: Size,
    pub height: Size,
    /// A style or theme style name for the title.
    pub title_style: Option<String>,
}

impl Default for ConfirmOptions {
    fn default() -> ConfirmOptions {
        ConfirmOptions {
            width: Size::Fixed(50),
            height: Size::Auto,
            title_style: None,
        }
    }
}

impl ConfirmOptions {
    /// The default: 50 wide, as tall as the content.
    pub fn new() -> ConfirmOptions {
        ConfirmOptions::default()
    }

    /// The box's size, as a [modal](Ctx::modal)'s: cells, a share of the
    /// screen, or the content's.
    pub fn size(mut self, width: Size, height: Size) -> ConfirmOptions {
        (self.width, self.height) = (width, height);
        self
    }

    /// The title's style (or theme style name), over the border's.
    pub fn title_style(mut self, style: impl Into<String>) -> ConfirmOptions {
        self.title_style = Some(style.into());
        self
    }
}

/// What a [`prompt_with`](Ctx::prompt_with) runs on Esc.
type OnCancel = Box<dyn FnOnce(&mut Ctx)>;

/// What a [`prompt_with`](Ctx::prompt_with) hears from its input.
type OnChange = Box<dyn FnMut(&str, &mut Ctx)>;

/// Where a [`prompt_with`](Ctx::prompt_with) opens, its caret, its
/// title's style, and what hears each edit and Esc.
pub struct PromptOptions {
    origin: Origin,
    offset: Offset,
    caret: Caret,
    title_style: Option<String>,
    change: Option<OnChange>,
    cancel: Option<OnCancel>,
}

impl Default for PromptOptions {
    fn default() -> PromptOptions {
        PromptOptions {
            origin: Origin::TopCenter,
            offset: Offset::new(0, 2, 50, 3),
            caret: Caret::End,
            title_style: None,
            change: None,
            cancel: None,
        }
    }
}

impl PromptOptions {
    /// At the top centre, two rows down, 50 x 3; the caret at the end.
    pub fn new() -> PromptOptions {
        PromptOptions::default()
    }

    /// Open from `origin`, moved and sized by `offset`, as
    /// [`popup_at`](Ctx::popup_at): `Origin::At(row)` puts it below a row.
    pub fn at(mut self, origin: Origin, offset: Offset) -> PromptOptions {
        (self.origin, self.offset) = (origin, offset);
        self
    }

    /// Where the caret starts: Yazi's rename puts it before the extension,
    /// `Caret::At(stem)`.
    pub fn caret(mut self, caret: Caret) -> PromptOptions {
        self.caret = caret;
        self
    }

    /// The title's style (or theme style name), over the border's, instead
    /// of the theme's `popup.title`: Yazi's input titles are plain,
    /// `"default"`.
    pub fn title_style(mut self, style: impl Into<String>) -> PromptOptions {
        self.title_style = Some(style.into());
        self
    }

    /// Run `change` with the text after each edit: a find that moves as
    /// you type.
    pub fn on_change(mut self, change: impl FnMut(&str, &mut Ctx) + 'static) -> PromptOptions {
        self.change = Some(Box::new(change));
        self
    }

    /// Run `cancel` when Esc closes it.
    pub fn on_cancel(mut self, cancel: impl FnOnce(&mut Ctx) + 'static) -> PromptOptions {
        self.cancel = Some(Box::new(cancel));
        self
    }
}

/// The size and title style of a [`pick_with`](Ctx::pick_with) menu.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickOptions {
    /// Where it goes from its origin and its size; `None` sizes it to the
    /// items and the title.
    pub offset: Option<Offset>,
    pub title_style: Option<String>,
}

impl PickOptions {
    pub fn new() -> PickOptions {
        PickOptions::default()
    }

    /// Move and size it by `offset` rather than to its items.
    pub fn offset(mut self, offset: Offset) -> PickOptions {
        self.offset = Some(offset);
        self
    }

    /// The title's style (or theme style name), over the border's.
    pub fn title_style(mut self, style: impl Into<String>) -> PickOptions {
        self.title_style = Some(style.into());
        self
    }
}

/// Close the dialog, and run `on_yes` if it was yes.
fn answer(cx: &mut Ctx, yes: bool, on_yes: &OnYes) {
    cx.pop();
    if yes {
        if let Some(on_yes) = on_yes.borrow_mut().take() {
            on_yes(cx);
        }
    }
}

/// The dialog's tree: the body, the items between rules, and the
/// answers. In a box of a set height (`fill`), the items take the room
/// left, so the answers sit at the bottom.
fn dialog(title: String, body: String, items: Vec<String>, fill: bool, on_yes: OnYes) -> Node {
    let mut parts = Vec::new();
    let has_body = !body.is_empty();
    if has_body {
        parts.push(label(body).text_align(TextAlign::Center).auto());
    }
    if !items.is_empty() {
        // Under the title, the items need no rule above them.
        if has_body {
            parts.push(crate::rule::rule());
        }
        let more = items.len().saturating_sub(ITEMS_SHOWN);
        let mut listed = Vec::new();
        for item in items.iter().take(ITEMS_SHOWN) {
            listed.push(
                label(escape(item))
                    .no_wrap()
                    .overflow(Overflow::Ellipsis)
                    .fixed(1)
                    .padding(0, 1),
            );
        }
        if more > 0 {
            listed.push(
                label(format!("[dim]… and {more} more"))
                    .fixed(1)
                    .padding(0, 1),
            );
        }
        let listed = column(listed);
        parts.push(if fill { listed.flex(1) } else { listed.auto() });
        parts.push(crate::rule::rule());
    } else if has_body {
        parts.push(label("").fixed(1));
    }
    if fill && items.is_empty() {
        parts.push(label("").flex(1));
    }
    let button = |text: &str, yes: bool, on_yes: &OnYes| {
        let (click, enter) = (on_yes.clone(), on_yes.clone());
        label(escape(text))
            .auto()
            .padding(0, 1)
            .focus_style("selected")
            .on_click(move |cx| answer(cx, yes, &click))
            .on_key("enter", move |cx| answer(cx, yes, &enter))
            .on_key("left right", |cx| cx.focus_next())
    };
    let yes = button("[Y]es", true, &on_yes).autofocus();
    let no = button("(N)o", false, &on_yes);
    parts.push(
        row([yes, no])
            .justify(Justify::SpaceEvenly)
            .align_items(Align::Center)
            .fixed(1),
    );
    let (y, n, esc) = (on_yes.clone(), on_yes.clone(), on_yes);
    column(parts)
        .panel(&title)
        .title_align(Align::Center)
        .on_key("y", move |cx| answer(cx, true, &y))
        .on_key("n", move |cx| answer(cx, false, &n))
        .on_key("esc", move |cx| answer(cx, false, &esc))
}
