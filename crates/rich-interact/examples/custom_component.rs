//! A component of your own, built from the kit, composed with built-ins in
//! a split, tabs and a modal (0.0.14 workstream 1):
//! `cargo run -p rs-rich-interact --example custom_component`.
//!
//! - **Checklist** is the new component: a [`FilterState`] for
//!   type-to-filter, a [`ListState`] for the cursor and the ticks, the kit's
//!   line helpers to draw, and a [`Keymap`] for its keys. Space ticks,
//!   Enter finishes with what is ticked.
//! - The **Checklist** tab splits it with a live summary ([`Painted`]);
//!   drag the border with the mouse or move it with Alt+H/Alt+L.
//! - The **Deploy** tab is built-ins only: an [`Input`] and a [`Select`]
//!   side by side; Tab moves between them.
//! - Alt+Left/Alt+Right switch tabs and keep each tab's state. F1 opens a
//!   modal listing the keys, read from the keymap; Ctrl+Q asks, in a modal,
//!   whether to quit.
//!
//! The integration tests (`tests/compose.rs`) include this file and drive
//! [`app`] headless and in a PTY.

use std::cell::RefCell;
use std::rc::Rc;

use rich::Segment;
use rich_interact::compose::{
    Column, ComponentExt, Label, Layer, Layers, Painted, Size, Split, Tabs,
};
use rich_interact::keymap::{keys, Keymap};
use rich_interact::kit::{self, FilterState, ListState, Theme};
use rich_interact::{
    Component, Confirm, Context, Event, Flow, Input, KeyCode, Outcome, RunOptions, Select, View,
};

/// A list of things to tick off, filtered as you type.
pub struct Checklist {
    prompt: String,
    items: Vec<String>,
    filter: FilterState,
    list: ListState,
    keymap: Keymap,
    theme: Theme,
    /// What is ticked, shared with whoever wants to show it.
    ticked: Rc<RefCell<Vec<String>>>,
}

impl Checklist {
    pub fn new<I, S>(prompt: impl Into<String>, items: I) -> Checklist
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let items: Vec<String> = items.into_iter().map(Into::into).collect();
        let mut list = ListState::new();
        list.clear_selection(items.len());
        list.set_len(items.len());
        Checklist {
            prompt: prompt.into(),
            filter: FilterState::new(items.iter().cloned()),
            items,
            list,
            keymap: checklist_keymap(),
            theme: Theme::default(),
            ticked: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// What is ticked, kept up to date as it changes.
    pub fn ticked(&self) -> Rc<RefCell<Vec<String>>> {
        Rc::clone(&self.ticked)
    }

    fn refilter(&mut self) {
        self.filter.refilter();
        self.list.set_len(self.filter.len());
        self.list.reset(0, self.page());
    }

    fn page(&self) -> usize {
        8
    }

    fn publish(&self) {
        *self.ticked.borrow_mut() = self
            .list
            .selected()
            .into_iter()
            .map(|index| self.items[index].clone())
            .collect();
    }
}

/// The checklist's keys, in context `checklist`.
pub fn checklist_keymap() -> Keymap {
    Keymap::new("checklist")
        .bind("up", keys("up"), "move up")
        .bind("down", keys("down"), "move down")
        .bind("tick", keys("space"), "tick or untick")
        .bind("done", keys("enter"), "finish")
        .bind("clear", keys("ctrl+u"), "clear the filter")
        .bind("delete", keys("backspace"), "delete a character")
}

impl Component for Checklist {
    type Output = Vec<String>;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<Vec<String>> {
        let Some(key) = event.key() else {
            return Flow::Ignored;
        };
        let page = self.page();
        match self.keymap.action(key) {
            Some("up") => self.list.step(-1, page),
            Some("down") => self.list.step(1, page),
            Some("tick") => {
                if let Some(index) = self.filter.index(self.list.cursor()) {
                    self.list.toggle(index);
                    self.publish();
                }
            }
            Some("done") => return Flow::Done(self.ticked.borrow().clone()),
            Some("clear") => {
                self.filter.query_mut().clear();
                self.refilter();
            }
            Some("delete") => {
                if self.filter.query_mut().pop().is_some() {
                    self.refilter();
                }
            }
            _ => match key.code {
                KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => {
                    self.filter.query_mut().push(c);
                    self.refilter();
                }
                // Not ours: the container may want it (Tab, Escape, F1).
                _ => return Flow::Ignored,
            },
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        let mut header = kit::question(theme, &self.prompt);
        let column = kit::width(&header) + rich::cells::cell_len(self.filter.query());
        header.push(kit::plain(self.filter.query().to_string()));
        let mut lines = vec![kit::fit(header, width)];
        for position in self.list.visible(self.page()) {
            let (index, positions) = &self.filter.matches()[position];
            let focused = position == self.list.cursor();
            let mut line: Vec<Segment> = if focused {
                vec![kit::text(
                    format!("{} ", theme.pointer),
                    &theme.pointer_style,
                )]
            } else {
                vec![kit::plain("  ")]
            };
            line.push(if self.list.is_selected(*index) {
                kit::text(format!("{} ", theme.checked), &theme.checked_style)
            } else {
                kit::text(format!("{} ", theme.unchecked), &theme.hint)
            });
            let base = focused.then_some(&theme.focused);
            line.extend(kit::highlight(
                &self.items[*index],
                positions,
                base,
                &theme.matched,
            ));
            lines.push(kit::fit(line, width));
        }
        let hint = format!(
            "  {} of {} ticked · space tick · enter done",
            self.list.selected_count(),
            self.items.len()
        );
        lines.push(kit::fit(vec![kit::text(hint, &theme.hint)], width));
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }

    fn keymap(&self) -> Keymap {
        self.keymap.clone()
    }
}

/// What the app answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum App {
    /// The checklist finished with these ticked.
    Checked(Vec<String>),
    /// A region was picked, with the tag typed.
    Deploy { region: String, tag: String },
    /// Quit, from the Ctrl+Q dialog.
    Quit,
}

/// The keys, as the F1 dialog lists them: the checklist's and the
/// containers', from their keymaps.
fn help() -> String {
    let mut keymap = checklist_keymap();
    keymap.extend(Tabs::<App>::new().own_keymap().clone());
    let mut markup = String::new();
    for binding in keymap.bindings() {
        let keys = rich::markup::escape(&binding.keys_label());
        markup.push_str(&format!("[bold]{keys:<20}[/] {}\n", binding.description));
    }
    markup.push_str(
        "[bold]alt+h/alt+l[/]          move the border\n[bold]ctrl+q[/]               quit",
    );
    markup
}

/// The whole app: tabs in a layer host.
pub fn app<'a>() -> Layers<'a, App> {
    let checklist = Checklist::new(
        "Release checklist",
        [
            "bump versions",
            "update the changelog",
            "run the tests",
            "tag the release",
            "publish",
        ],
    );
    let ticked = checklist.ticked();
    let summary = Painted::new(move |context: &Context<'_>| {
        let ticked = ticked.borrow();
        let mut markup = format!("[bold]Ticked[/] ({})\n", ticked.len());
        for item in ticked.iter() {
            markup.push_str(&format!("• {}\n", rich::markup::escape(item)));
        }
        View::new(context.markup(&markup))
    });
    let checklist_tab = Split::horizontal(
        checklist.map(|ticked| Flow::Done(App::Checked(ticked))),
        summary,
    )
    .ratio(60)
    .min(20)
    .with_mouse(true);

    let tag = Rc::new(RefCell::new(String::new()));
    let typed = Rc::clone(&tag);
    let input = Input::new("Tag")
        .placeholder("v1.0.0")
        .map(move |text| {
            *typed.borrow_mut() = text;
            Flow::Continue
        })
        .on_cancel(|| Flow::Continue);
    let region = Select::new("Region", ["eu-west", "us-east", "ap-south"]).map(move |region| {
        Flow::Done(App::Deploy {
            region: region.to_string(),
            tag: tag.borrow().clone(),
        })
    });
    let deploy_tab = Column::new()
        .child(Label::new(
            "[bold]Where to?[/] Tab moves between the fields.",
        ))
        .sized(Size::Flex(1), Split::horizontal(input, region).ratio(40));

    let tabs = Tabs::new()
        .tab("Checklist", checklist_tab)
        .tab("Deploy", deploy_tab);
    Layers::new(tabs)
        .open_on("help", keys("f1"), "show the keys", || {
            Layer::modal(Label::new(help())).title("Keys").size(56, 15)
        })
        .open_on("quit", keys("ctrl+q"), "quit", || {
            let confirm = Confirm::new("Quit?").map(|answer| {
                if answer == "yes" {
                    Flow::Done(App::Quit)
                } else {
                    Flow::Continue
                }
            });
            Layer::modal(confirm).title("Quit").size(30, 6)
        })
}

#[allow(dead_code)]
fn main() {
    match rich_interact::run(app(), &RunOptions::default()) {
        Ok(Outcome::Done(answer)) => println!("{answer:?}"),
        Ok(other) => println!("{other:?}"),
        Err(error) => eprintln!("{error}"),
    }
}
