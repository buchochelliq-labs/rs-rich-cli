//! Dialogs in the framework's look: [`Ctx::confirm`].

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ctx;
use crate::layout::{Align, Justify, Size, TextAlign};
use crate::node::{column, label, row, Node};
use crate::rich::markup::escape;
use crate::rich::Overflow;

/// What a confirm dialog runs on yes.
type OnYes = Rc<RefCell<Option<Box<dyn FnOnce(&mut Ctx)>>>>;

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
        let (title, body) = (title.into(), body.into());
        let on_yes: OnYes = Rc::new(RefCell::new(Some(Box::new(on_yes))));
        self.modal(Size::Fixed(50), Size::Auto, move || {
            dialog(title, body, items, on_yes)
        });
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

/// The dialog's tree.
fn dialog(title: String, body: String, items: Vec<String>, on_yes: OnYes) -> Node {
    let mut parts = Vec::new();
    if !body.is_empty() {
        parts.push(label(body).text_align(TextAlign::Center).auto());
    }
    if !items.is_empty() {
        parts.push(crate::rule::rule());
        let more = items.len().saturating_sub(ITEMS_SHOWN);
        for item in items.iter().take(ITEMS_SHOWN) {
            parts.push(
                label(escape(item))
                    .no_wrap()
                    .overflow(Overflow::Ellipsis)
                    .fixed(1)
                    .padding(0, 1),
            );
        }
        if more > 0 {
            parts.push(
                label(format!("[dim]… and {more} more"))
                    .fixed(1)
                    .padding(0, 1),
            );
        }
        parts.push(crate::rule::rule());
    } else {
        parts.push(label("").fixed(1));
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
