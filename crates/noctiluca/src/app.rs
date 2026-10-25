//! The browser as an intuiTUIve app: the address line, the page (the
//! projection by default, reader mode one key away), the status line, and
//! the action registry's keys, hints, find, F1 overlay and site keys.
//!
//! The tab runs on a thread of its own ([`crate::tab`]); its updates come
//! to the app's thread through a proxy, into signals the nodes read.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;

use rich_embed::PageState;
use rich_intuituive::interact::keymap::{try_keys, Keymap};
use rich_intuituive::interact::{Key, KeyCode};
use rich_intuituive::prelude::*;
use rich_intuituive::{image_bytes_of, image_graphics, spawn_stream, ImageView, Pixels};

use crate::actions::{self, keys_label, ACTIONS};
use crate::bookmarks::{Bookmark, Bookmarks};
use crate::cdp::CELL;
use crate::clean::{markup, truncate};
use crate::config::{remove_site_key, save_site_key, Config, Images, Site, Target, View};
use crate::engine::{Engine, HistoryStep, NodeId};
use crate::layout::{self, hint_labels, matches, to_text, Line, Marks, Piece};
use crate::model::{Binding, Page, Role};
use crate::tab::{Command, Intent, Refusal, Tab, Update};
use crate::widgets::{web_widget, WebControl};

/// What happens when an action of the tab's is answered.
type Then = Box<dyn FnOnce(Result<(), Refusal>, &mut Ctx)>;

/// Link hints under way: each label and its control, and what was typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hints {
    pub labels: Vec<(String, usize)>,
    pub typed: String,
}

/// How the app starts.
#[derive(Clone, Debug)]
pub struct Options {
    pub config: Config,
    pub bookmarks: Bookmarks,
    /// The first address; else the config's home page.
    pub url: Option<String>,
    /// The first view; else the config's.
    pub view: Option<View>,
    /// How images show; else the config's.
    pub images: Option<Images>,
}

/// The app's state, all of it in signals so it can be copied into every
/// handler and sent with the tab's updates.
#[derive(Clone, Copy)]
pub struct Browser {
    pub page: Signal<Option<Arc<Page>>>,
    pub state: Signal<PageState>,
    pub view: Signal<View>,
    pub hints: Signal<Option<Hints>>,
    pub find: Signal<Option<String>>,
    pub find_at: Signal<usize>,
    pub scroll: Signal<u16>,
    pub message: Signal<String>,
    pub config: Signal<Config>,
    pub keymap: Signal<Keymap>,
    pub bookmarks: Signal<Bookmarks>,
    pub images: Signal<Images>,
    /// How images show where no site says otherwise.
    pub images_default: Signal<Images>,
    /// The terminal's size in cells.
    pub viewport: Memo<(u16, u16)>,
    tab: Signal<Tab>,
    waiting: Signal<HashMap<u64, Then>>,
    controls: Signal<HashMap<NodeId, Rc<WebControl>>>,
}

impl Browser {
    fn send(&self, command: Command) -> u64 {
        self.tab.with_untracked(|t| t.send(command))
    }

    /// Run `intent` on the control `binding` names, then `then` with the
    /// answer.
    pub fn act(
        &self,
        binding: Binding,
        intent: Intent,
        then: impl FnOnce(Result<(), Refusal>, &mut Ctx) + 'static,
    ) {
        let id = self.send(Command::Act { binding, intent });
        self.waiting.update(|w| {
            w.insert(id, Box::new(then));
        });
    }

    fn after(&self, id: u64, then: impl FnOnce(Result<(), Refusal>, &mut Ctx) + 'static) {
        self.waiting.update(|w| {
            w.insert(id, Box::new(then));
        });
    }

    /// The widget state of `control`, made once per control.
    pub fn web_control(&self, control: &crate::model::Control) -> Rc<WebControl> {
        let mut found = None;
        self.controls.update(|all| {
            found = Some(
                all.entry(control.node)
                    .or_insert_with(|| Rc::new(WebControl::new(control)))
                    .clone(),
            );
        });
        found.expect("made")
    }

    /// Make every widget follow the page now.
    pub fn refresh_controls(&self) {
        let page = self.page.with_untracked(Clone::clone);
        let Some(page) = page else { return };
        self.controls.with_untracked(|all| {
            for (node, web) in all {
                if let Some(control) = page.control(*node) {
                    web.sync(control);
                }
            }
        });
    }

    fn url(&self) -> String {
        self.page
            .with(|p| p.as_ref().map(|p| p.url.clone()))
            .unwrap_or_else(|| self.state.with(|s| crate::clean::clean(&s.url)))
    }

    /// The site table for the page open now.
    fn site(&self) -> Option<Site> {
        let url = self.url();
        self.config.with(|c| c.site(&url).cloned())
    }

    /// What the views mark: hint labels and find.
    pub fn marks(&self) -> Marks {
        let hints = self
            .hints
            .with(|h| {
                h.as_ref()
                    .map(|h| h.labels.iter().map(|(l, c)| (*c, l.clone())).collect())
            })
            .unwrap_or_default();
        Marks {
            hints,
            find: self.find.get(),
            current: None,
        }
    }

    fn say(&self, markup: impl Into<String>) {
        self.message.set(markup.into());
    }
}

/// The page's lines in the current view at `width`, as find counts rows:
/// every piece of reader mode in turn (a media region or a control as its
/// rows of nothing), or the projection.
fn view_lines(b: Browser, width: u16) -> Vec<Line> {
    let marks = Marks::default();
    b.page.with_untracked(|p| {
        let Some(page) = p.as_ref() else {
            return Vec::new();
        };
        match b.view.get_untracked() {
            View::Reader => {
                let mut out = Vec::new();
                for piece in layout::reader(page, width.saturating_sub(2), &marks) {
                    match piece {
                        Piece::Text(lines) => out.extend(lines),
                        other => out.extend(std::iter::repeat_n(Line::default(), other.rows())),
                    }
                }
                out
            }
            _ => layout::projection(page, width, (CELL.0 as f64, CELL.1 as f64), &marks),
        }
    })
}

/// Where find's matches are: (row, nth on the row).
fn find_rows(b: Browser, width: u16) -> Vec<(usize, usize)> {
    let Some(query) = b.find.get_untracked().filter(|q| !q.is_empty()) else {
        return Vec::new();
    };
    view_lines(b, width)
        .iter()
        .enumerate()
        .flat_map(|(row, line)| (0..matches(&line.plain(), &query).len()).map(move |k| (row, k)))
        .collect()
}

/// Whether action `id` can run now, or why not. Reads signals, so the
/// overlay that calls it follows them.
pub fn availability(b: Browser, id: &str) -> Result<(), String> {
    let state = b.state.get();
    let has_page = b.page.with(Option::is_some);
    match id {
        "back" if !state.can_go_back => Err("Nothing to go back to.".into()),
        "forward" if !state.can_go_forward => Err("Nothing to go forward to.".into()),
        "open_tab" | "tab.next" | "tab.previous" => {
            Err("One tab for now: tabs come with rs-rich-embed's several pages.".into())
        }
        "hints"
            if !b.page.with(|p| {
                p.as_ref()
                    .is_some_and(|p| p.controls.iter().any(|c| !c.disabled))
            }) =>
        {
            Err("Nothing on this page to follow or use.".into())
        }
        "find.next" | "find.previous"
            if b.find.with(|f| f.as_ref().is_none_or(String::is_empty)) =>
        {
            Err("Find something first.".into())
        }
        "media.play_pause" => Err(
            if b.page.with(|p| {
                p.as_ref().is_some_and(|p| {
                    p.media.iter().any(|m| {
                        matches!(
                            m.kind,
                            crate::model::MediaKind::Video | crate::model::MediaKind::Audio
                        )
                    })
                })
            }) {
                "Video and audio controls come with the media phase.".into()
            } else {
                "No video or audio on this page.".into()
            },
        ),
        "bookmark" | "reload" | "address" | "site.bind" | "site.keys" if !has_page => {
            Err("No page yet.".into())
        }
        "site.bind" | "site.keys" if b.config.with(|c| c.path.is_none()) => {
            Err("No config file to keep site keys in.".into())
        }
        "site.bind" | "site.keys" if crate::url::host(&b.url()).is_none() => {
            Err("This page has no site (a local file): site keys need a host.".into())
        }
        _ => Ok(()),
    }
}

/// Whether a site key's target can run now, or why not.
fn target_availability(b: Browser, target: &Target) -> Result<(), String> {
    match target {
        Target::Action(id) => match actions::get(id) {
            None => Err(format!("{id:?} is not an action.")),
            Some(_) => availability(b, id),
        },
        Target::Element { role, name, nth } => b.page.with(|p| {
            let found = p.as_ref().map_or(0, |p| p.find(role, name).len());
            if found >= *nth {
                Ok(())
            } else if found == 0 {
                Err(format!(
                    "No {role} named {:?} on this page.",
                    truncate(name, 40)
                ))
            } else {
                Err(format!(
                    "Only {found} {role}s named {:?} on this page.",
                    truncate(name, 40)
                ))
            }
        }),
        Target::Sequence(all) => all.iter().try_for_each(|t| target_availability(b, t)),
    }
}

/// Run action `id`, if it is available; else say why.
pub fn run(b: Browser, id: &str, cx: &mut Ctx) {
    if let Err(why) = availability(b, id) {
        cx.toast(format!("[yellow]{}[/]", markup(&why)));
        return;
    }
    let rows = b.viewport.get_untracked().1.saturating_sub(4).max(1);
    let width = b.viewport.get_untracked().0;
    let total = || view_lines(b, width).len() as u16;
    match id {
        "open" => open_prompt(b, String::new(), cx),
        "address" => open_prompt(b, b.url(), cx),
        "hints" => start_hints(b),
        "back" => drop(b.send(Command::History(HistoryStep::Back))),
        "forward" => drop(b.send(Command::History(HistoryStep::Forward))),
        "reload" => drop(b.send(Command::History(HistoryStep::Reload))),
        "find" => find_prompt(b, cx),
        "find.next" | "find.previous" => {
            let found = find_rows(b, width);
            if found.is_empty() {
                cx.toast("[yellow]No matches.[/]");
                return;
            }
            let at = b.find_at.get_untracked();
            let next = match (id, at) {
                ("find.next", usize::MAX) => 0,
                ("find.next", at) => (at + 1) % found.len(),
                (_, usize::MAX) => found.len() - 1,
                (_, at) => (at % found.len() + found.len() - 1) % found.len(),
            };
            b.find_at.set(next);
            b.scroll.set((found[next].0 as u16).saturating_sub(3));
            b.say(format!("match {} of {}", next + 1, found.len()));
        }
        "scroll.down" => b
            .scroll
            .update(|s| *s = (*s + 1).min(total().saturating_sub(1))),
        "scroll.up" => b.scroll.update(|s| *s = s.saturating_sub(1)),
        "page.down" => b
            .scroll
            .update(|s| *s = (*s + rows).min(total().saturating_sub(1))),
        "page.up" => b.scroll.update(|s| *s = s.saturating_sub(rows)),
        "top" => b.scroll.set(0),
        "bottom" => b.scroll.set(total().saturating_sub(rows)),
        "reader" => {
            b.view.update(|v| {
                *v = if *v == View::Reader {
                    View::Projection
                } else {
                    View::Reader
                }
            });
            b.scroll.set(0);
            b.say(format!("{} view", b.view.get_untracked().name()));
        }
        "bookmark" => {
            let (url, title) = b.page.with_untracked(|p| {
                p.as_ref()
                    .map(|p| (p.url.clone(), p.title.clone()))
                    .unwrap_or_default()
            });
            let mut result = Ok(());
            b.bookmarks.update(|marks| {
                result = marks.add(Bookmark {
                    url,
                    title: title.clone(),
                    keyword: None,
                });
            });
            match result {
                Ok(()) => cx.toast(format!("Bookmarked {}.", markup(&title))),
                Err(e) => cx.toast(format!("[red]{}[/]", markup(&e.to_string()))),
            }
        }
        "bookmarks" => bookmarks_list(b, cx),
        "site.bind" => bind_picker(b, cx),
        "site.keys" => site_keys_list(b, cx),
        "actions" => overlay(b, cx),
        "quit" => cx.quit(),
        _ => {}
    }
}

/// Run a site key's target.
fn run_target(b: Browser, target: &Target, cx: &mut Ctx) {
    if let Err(why) = target_availability(b, target) {
        cx.toast(format!("[yellow]{}[/]", markup(&why)));
        return;
    }
    match target {
        Target::Action(id) => run(b, id, cx),
        Target::Element { role, name, nth } => {
            let id = b.send(Command::Activate {
                role: role.clone(),
                name: name.clone(),
                nth: nth.saturating_sub(1),
            });
            b.after(id, |result, cx| {
                if let Err(refusal) = result {
                    cx.toast(format!("[red]{}[/]", markup(&refusal.to_string())));
                }
            });
        }
        Target::Sequence(all) => {
            for t in all {
                run_target(b, t, cx);
            }
        }
    }
}

fn start_hints(b: Browser) {
    let hintable: Vec<usize> = b.page.with_untracked(|p| {
        p.as_ref()
            .map(|p| {
                p.controls
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| !c.disabled)
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default()
    });
    let labels = hint_labels(hintable.len())
        .into_iter()
        .zip(hintable)
        .collect();
    b.hints.set(Some(Hints {
        labels,
        typed: String::new(),
    }));
}

/// Use control `index` of the page: click what clicks, and give the
/// others their widget (focused in reader mode, in a box in the
/// projection).
fn activate(b: Browser, index: usize, cx: &mut Ctx) {
    let page = b.page.with_untracked(Clone::clone);
    let Some(page) = page else { return };
    let Some(control) = page.controls.get(index).cloned() else {
        return;
    };
    let clicks = matches!(
        control.role,
        Role::Link
            | Role::Button
            | Role::CheckBox
            | Role::Switch
            | Role::Radio
            | Role::Tab
            | Role::MenuItem
    );
    if clicks {
        b.act(control.binding(&page), Intent::Click, |result, cx| {
            if let Err(refusal) = result {
                cx.toast(format!("[red]{}[/]", markup(&refusal.to_string())));
            }
        });
        return;
    }
    if b.view.get_untracked() == View::Reader {
        if let Some(id) = b.web_control(&control).widget.get() {
            cx.focus(id);
            return;
        }
    }
    let node = control.node;
    let title = markup(&crate::tab::describe(&control));
    cx.modal(Size::Percent(70), Size::Fixed(3 + 2), move || {
        web_widget(b, vec![node])
            .on_key("esc", |cx| cx.pop())
            .panel(&title)
    });
}

/// A key while hints are under way: whether it was used.
fn hint_key(b: Browser, key: Key, cx: &mut Ctx) -> bool {
    let Some(mut hints) = b.hints.get_untracked() else {
        return false;
    };
    match key.code {
        KeyCode::Escape => {
            b.hints.set(None);
        }
        KeyCode::Backspace => {
            hints.typed.pop();
            b.hints.set(Some(hints));
        }
        KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => {
            hints.typed.push(c);
            if let Some((_, index)) = hints.labels.iter().find(|(l, _)| *l == hints.typed) {
                let index = *index;
                b.hints.set(None);
                activate(b, index, cx);
            } else if hints
                .labels
                .iter()
                .any(|(l, _)| l.starts_with(&hints.typed))
            {
                b.hints.set(Some(hints));
            } else {
                b.hints.set(None);
                cx.toast(format!("[yellow]No hint {}.[/]", markup(&hints.typed)));
            }
        }
        _ => return false,
    }
    true
}

/// The site key `key` runs on the page open now.
fn site_key(b: Browser, key: Key) -> Option<Target> {
    let site = b.site()?;
    site.keys
        .iter()
        .find(|k| try_keys(&k.keys).is_ok_and(|keys| keys == [key]))
        .map(|k| k.target.clone())
}

/// A text box in a box: Enter gives its text to `submit`.
fn prompt(title: &str, initial: String, cx: &mut Ctx, submit: impl Fn(String, &mut Ctx) + 'static) {
    let title = title.to_string();
    let text = signal(initial);
    cx.modal(Size::Percent(80), Size::Fixed(3), move || {
        input(text)
            .on_submit(move |value, cx| {
                let value = value.to_string();
                cx.pop();
                submit(value, cx);
            })
            .on_key("esc", |cx| cx.pop())
            .autofocus()
            .panel(&title)
    });
}

fn open_prompt(b: Browser, initial: String, cx: &mut Ctx) {
    prompt(
        "Open: an address, a keyword, or words to search",
        initial,
        cx,
        move |typed, _| {
            let url = b.url();
            let destination = b.config.with_untracked(|c| {
                b.bookmarks
                    .with_untracked(|m| c.destination(&typed, &url, &m.items))
            });
            b.send(Command::Open(destination.url().to_string()));
            b.say(format!("opening {}", markup(destination.url())));
        },
    );
}

fn find_prompt(b: Browser, cx: &mut Ctx) {
    let initial = b.find.get_untracked().unwrap_or_default();
    prompt("Find in page", initial, cx, move |query, cx| {
        b.find.set((!query.is_empty()).then_some(query));
        b.find_at.set(usize::MAX);
        if b.find.get_untracked().is_some() {
            run(b, "find.next", cx);
        }
    });
}

/// One row of a [`chooser`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Console markup.
    pub text: String,
    /// Why it cannot be chosen now, if it cannot.
    pub unavailable: Option<String>,
}

type Choose = Rc<dyn Fn(usize, &mut Ctx)>;

/// A live list in a box: `items` is read again whenever a signal it reads
/// changes, so it follows the page while open; the selection is kept by
/// position. ↑/↓ (or k/j) move, Esc closes, and each of `keys` runs on
/// the selected row.
fn chooser(
    title: &str,
    hint: &str,
    items: Rc<dyn Fn() -> Vec<Entry>>,
    keys: Vec<(&'static str, Choose)>,
    cx: &mut Ctx,
) {
    let title = title.to_string();
    let hint = hint.to_string();
    cx.modal(Size::Percent(90), Size::Percent(85), move || {
        let selected = signal(0usize);
        let shown = items.clone();
        // The rows the box has for the list: it shows a window of them
        // that keeps the selection in view.
        let room = (viewport().get_untracked().1 as usize * 85 / 100)
            .saturating_sub(4)
            .max(3);
        let list = text(move || {
            let all = shown();
            let at = selected.get().min(all.len().saturating_sub(1));
            let first = at.saturating_sub(room.saturating_sub(2));
            let mut out = Vec::new();
            for (i, entry) in all.iter().enumerate().skip(first).take(room) {
                let mark = if i == at { "[reverse]▸" } else { " " };
                let end = if i == at { "[/]" } else { "" };
                let body = match &entry.unavailable {
                    None => entry.text.clone(),
                    Some(why) => format!("[dim]{}  — {}[/]", entry.text, markup(why)),
                };
                out.push(format!("{mark} {body}{end}"));
            }
            if out.is_empty() {
                out.push("[dim]Nothing here yet.[/]".into());
            }
            out.join("\n")
        })
        .live();
        let count = items.clone();
        let mut node = column([list.flex(1), label(format!("[dim]{}[/]", hint)).fixed(1)])
            .focusable()
            .autofocus()
            .on_key("up k", move |_| {
                selected.update(|s| *s = s.saturating_sub(1))
            })
            .on_key("down j", move |_| {
                let n = count().len();
                selected.update(|s| *s = (*s + 1).min(n.saturating_sub(1)))
            })
            .on_key("esc", |cx| cx.pop());
        for (keys, choose) in keys.clone() {
            node = node.on_key(keys, move |cx| choose(selected.get_untracked(), cx));
        }
        node.panel(&title)
    });
}

/// The F1 overlay: every action of the registry with its keys and, when
/// it cannot run now, why; then this site's keys, marked as the site's.
/// It is read again while open, so it follows the page.
pub fn overlay_entries(b: Browser) -> Vec<(Entry, Option<OverlayItem>)> {
    let keymap = b.keymap.get();
    let mut out = Vec::new();
    if let Some(site) = b.site() {
        out.push((
            Entry {
                text: format!("[bold]site {}[/]", markup(&site.pattern)),
                unavailable: None,
            },
            None,
        ));
        for key in &site.keys {
            out.push((
                Entry {
                    text: format!(
                        "{:>14}  {}  [cyan]\\[site {}][/]",
                        markup(&key.keys),
                        markup(&key.target.describe()),
                        markup(&site.pattern)
                    ),
                    unavailable: target_availability(b, &key.target).err(),
                },
                Some(OverlayItem::Site(key.target.clone())),
            ));
        }
    }
    let mut section = "";
    for action in ACTIONS {
        if action.section != section {
            section = action.section;
            out.push((
                Entry {
                    text: format!("[bold]{section}[/]"),
                    unavailable: None,
                },
                None,
            ));
        }
        let keys = keys_label(&keymap, action.id);
        out.push((
            Entry {
                text: format!("{:>14}  {}", markup(&keys), markup(action.label)),
                unavailable: availability(b, action.id).err(),
            },
            Some(OverlayItem::Action(action.id)),
        ));
    }
    out
}

/// What a row of the overlay runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayItem {
    Action(&'static str),
    Site(Target),
}

fn overlay(b: Browser, cx: &mut Ctx) {
    let items: Rc<dyn Fn() -> Vec<Entry>> =
        Rc::new(move || overlay_entries(b).into_iter().map(|(e, _)| e).collect());
    let choose: Choose = Rc::new(move |i, cx| {
        let item = overlay_entries(b)
            .into_iter()
            .nth(i)
            .and_then(|(_, item)| item);
        cx.pop();
        match item {
            // Checked again as it runs: it may have stopped applying.
            Some(OverlayItem::Action(id)) => run(b, id, cx),
            Some(OverlayItem::Site(target)) => run_target(b, &target, cx),
            None => {}
        }
    });
    let bind: Choose = Rc::new(move |i, cx| {
        let item = overlay_entries(b)
            .into_iter()
            .nth(i)
            .and_then(|(_, item)| item);
        if let Some(OverlayItem::Action(id)) = item {
            cx.pop();
            if let Err(why) = availability(b, "site.bind") {
                cx.toast(format!("[yellow]{}[/]", markup(&why)));
                return;
            }
            capture_key(b, Target::Action(id.to_string()), None, cx);
        }
    });
    chooser(
        "Actions",
        "Enter runs · b binds a key to it on this site · Esc closes",
        items,
        vec![("enter", choose), ("b", bind)],
        cx,
    );
}

/// Choose what to bind a key to on this site: an action, or an element
/// of the page by its role and name.
fn bind_picker(b: Browser, cx: &mut Ctx) {
    let targets = move || -> Vec<(Entry, Target, Option<String>)> {
        let mut out = Vec::new();
        b.page.with(|p| {
            if let Some(page) = p {
                for (i, c) in page.controls.iter().enumerate() {
                    if c.name.is_empty() {
                        continue;
                    }
                    let same = page.find(c.role.name(), &c.name);
                    let nth = same
                        .iter()
                        .position(|o| std::ptr::eq(*o, &page.controls[i]))
                        .unwrap_or(0)
                        + 1;
                    let note = (same.len() > 1).then(|| {
                        format!(
                            "{} elements are named {:?}; this binds match {nth}.",
                            same.len(),
                            truncate(&c.name, 40)
                        )
                    });
                    out.push((
                        Entry {
                            text: format!("{} {}", c.role.name(), markup(&format!("{:?}", c.name))),
                            unavailable: None,
                        },
                        Target::Element {
                            role: c.role.name().to_string(),
                            name: c.name.clone(),
                            nth,
                        },
                        note,
                    ));
                }
            }
        });
        for action in ACTIONS {
            out.push((
                Entry {
                    text: format!("action {}  [dim]{}[/]", action.id, markup(action.label)),
                    unavailable: None,
                },
                Target::Action(action.id.to_string()),
                None,
            ));
        }
        out
    };
    let items: Rc<dyn Fn() -> Vec<Entry>> =
        Rc::new(move || targets().into_iter().map(|(e, _, _)| e).collect());
    let choose: Choose = Rc::new(move |i, cx| {
        let Some((_, target, note)) = targets().into_iter().nth(i) else {
            return;
        };
        cx.pop();
        capture_key(b, target, note, cx);
    });
    chooser(
        "Bind a key on this site: to what?",
        "Enter chooses · Esc closes",
        items,
        vec![("enter", choose)],
        cx,
    );
}

/// Wait for the key to bind `target` to, then ask to confirm it, saying
/// what it does now, and where it applies.
fn capture_key(b: Browser, target: Target, note: Option<String>, cx: &mut Ctx) {
    let Some(host) = crate::url::host(&b.url()) else {
        return;
    };
    let key: Signal<Option<Key>> = signal(None);
    let describe = target.describe();
    cx.modal(Size::Percent(80), Size::Fixed(8), move || {
        let host2 = host.clone();
        let target2 = target.clone();
        let prompt = text(move || {
            let mut lines = vec![format!(
                "Bind a key to [bold]{}[/] on [bold]{}[/].",
                markup(&describe),
                markup(&host2)
            )];
            if let Some(note) = &note {
                lines.push(format!("[yellow]{}[/]", markup(note)));
            }
            match key.get() {
                None => lines.push("Press the key. (Esc cancels.)".into()),
                Some(k) => {
                    if let Some(now) = conflict(b, k) {
                        lines.push(format!(
                            "[yellow]{} already does {} here: Enter replaces it.[/]",
                            markup(&k.to_string()),
                            markup(&now)
                        ));
                    }
                    lines.push(format!(
                        "{}: Enter binds it on {} · w on *.{} · Esc cancels",
                        markup(&k.to_string()),
                        markup(&host2),
                        markup(&host2)
                    ));
                }
            }
            lines.join("\n")
        });
        let host3 = host.clone();
        prompt
            .focusable()
            .autofocus()
            .on_any_key(move |cx, pressed| {
                match (key.get_untracked(), pressed.code) {
                    (_, KeyCode::Escape) => cx.pop(),
                    (None, _) => key.set(Some(pressed)),
                    (Some(k), KeyCode::Enter) => save_binding(b, &host3, k, &target2, cx),
                    (Some(k), KeyCode::Char('w')) => {
                        save_binding(b, &format!("*.{host3}"), k, &target2, cx)
                    }
                    _ => {}
                }
                true
            })
            .panel("Bind a key on this site")
    });
}

/// What `key` does on this page now, if anything: a site key or an action.
fn conflict(b: Browser, key: Key) -> Option<String> {
    if let Some(target) = site_key(b, key) {
        return Some(target.describe());
    }
    b.keymap.with_untracked(|k| actions::action_for(k, key))
}

fn save_binding(b: Browser, pattern: &str, key: Key, target: &Target, cx: &mut Ctx) {
    cx.pop();
    let Some(path) = b.config.with_untracked(|c| c.path.clone()) else {
        return;
    };
    match save_site_key(&path, pattern, &key.to_string(), target) {
        Ok(_) => {
            reload_config(b);
            cx.toast(format!(
                "{} now does {} on {}.",
                markup(&key.to_string()),
                markup(&target.describe()),
                markup(pattern)
            ));
        }
        Err(e) => cx.toast(format!("[red]{}[/]", markup(&e.to_string()))),
    }
}

/// Read the config file again (after an edit of ours).
fn reload_config(b: Browser) {
    let path = b.config.with_untracked(|c| c.path.clone());
    if let Ok(config) = Config::load(path.as_deref()) {
        b.config.set(config);
    }
}

/// This site's keys: Enter tests one, d removes it, e binds it to another
/// key, s moves it between this host and `*.host`.
fn site_keys_list(b: Browser, cx: &mut Ctx) {
    let rows = move || -> Vec<(String, String, Target)> {
        b.site()
            .map(|s| {
                s.keys
                    .iter()
                    .map(|k| (s.pattern.clone(), k.keys.clone(), k.target.clone()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let items: Rc<dyn Fn() -> Vec<Entry>> = Rc::new(move || {
        rows()
            .into_iter()
            .map(|(pattern, keys, target)| Entry {
                text: format!(
                    "{:>10}  {}  [cyan]\\[{}][/]",
                    markup(&keys),
                    markup(&target.describe()),
                    markup(&pattern)
                ),
                unavailable: target_availability(b, &target).err(),
            })
            .collect()
    });
    let test: Choose = Rc::new(move |i, cx| {
        if let Some((_, _, target)) = rows().into_iter().nth(i) {
            cx.pop();
            run_target(b, &target, cx);
        }
    });
    let path = move || b.config.with_untracked(|c| c.path.clone());
    let remove: Choose = Rc::new(move |i, cx| {
        let (Some((pattern, keys, _)), Some(path)) = (rows().into_iter().nth(i), path()) else {
            return;
        };
        match remove_site_key(&path, &pattern, &keys) {
            Ok(_) => {
                reload_config(b);
                cx.toast(format!(
                    "Removed {} from {}.",
                    markup(&keys),
                    markup(&pattern)
                ));
            }
            Err(e) => cx.toast(format!("[red]{}[/]", markup(&e.to_string()))),
        }
    });
    let edit: Choose = Rc::new(move |i, cx| {
        let (Some((pattern, keys, target)), Some(path)) = (rows().into_iter().nth(i), path())
        else {
            return;
        };
        let _ = remove_site_key(&path, &pattern, &keys);
        reload_config(b);
        cx.pop();
        capture_key(b, target, None, cx);
    });
    let scope: Choose = Rc::new(move |i, cx| {
        let (Some((pattern, keys, target)), Some(path)) = (rows().into_iter().nth(i), path())
        else {
            return;
        };
        let other = match pattern.strip_prefix("*.") {
            Some(host) => host.to_string(),
            None => format!("*.{pattern}"),
        };
        let moved = save_site_key(&path, &other, &keys, &target)
            .and_then(|_| remove_site_key(&path, &pattern, &keys));
        reload_config(b);
        match moved {
            Ok(_) => cx.toast(format!(
                "{} now applies on {}.",
                markup(&keys),
                markup(&other)
            )),
            Err(e) => cx.toast(format!("[red]{}[/]", markup(&e.to_string()))),
        }
    });
    chooser(
        "Site shortcuts",
        "Enter tests · d removes · e binds another key · s: this host or *.host · Esc closes",
        items,
        vec![("enter", test), ("d", remove), ("e", edit), ("s", scope)],
        cx,
    );
}

fn bookmarks_list(b: Browser, cx: &mut Ctx) {
    let items: Rc<dyn Fn() -> Vec<Entry>> = Rc::new(move || {
        b.bookmarks.with(|m| {
            m.items
                .iter()
                .map(|bm| Entry {
                    text: format!(
                        "{}  [dim]{}[/]{}",
                        markup(&bm.title),
                        markup(&bm.url),
                        bm.keyword
                            .as_ref()
                            .map(|k| format!("  [cyan]{}[/]", markup(k)))
                            .unwrap_or_default()
                    ),
                    unavailable: None,
                })
                .collect()
        })
    });
    let open: Choose = Rc::new(move |i, cx| {
        if let Some(url) = b
            .bookmarks
            .with_untracked(|m| m.items.get(i).map(|bm| bm.url.clone()))
        {
            cx.pop();
            b.send(Command::Open(url));
        }
    });
    let remove: Choose = Rc::new(move |i, _| {
        if let Some(url) = b
            .bookmarks
            .with_untracked(|m| m.items.get(i).map(|bm| bm.url.clone()))
        {
            b.bookmarks.update(|m| {
                let _ = m.remove(&url);
            });
        }
    });
    chooser(
        "Bookmarks",
        "Enter opens · d removes · Esc closes",
        items,
        vec![("enter", open), ("d", remove)],
        cx,
    );
}

/// A new document's site settings: its view, whether images show, and the
/// width Chrome lays it out at. (`audio` and `fps` are read, and wait for
/// the media phase: sound is a switch of the browser's launch.)
fn apply_site(b: Browser, url: &str) {
    let site = b.config.with_untracked(|c| c.site(url).cloned());
    let base = b.images_default.get_untracked();
    let images = match site.as_ref().and_then(|s| s.images) {
        Some(false) => Images::None,
        _ => base,
    };
    if b.images.get_untracked() != images {
        b.images.set(images);
    }
    if let Some(view) = site
        .as_ref()
        .and_then(|s| s.view)
        .filter(|v| *v != View::Raster)
    {
        b.view.set(view);
    }
    let (columns, rows) = b.viewport.get_untracked();
    let width = site.as_ref().and_then(|s| s.width).unwrap_or(columns);
    b.send(Command::Resize(width, rows.saturating_sub(2)));
}

/// Apply one of the tab's updates, on the app's thread.
fn apply(b: Browser, update: Update, cx: &mut Ctx) {
    match update {
        Update::Page(page) => {
            let new_document =
                b.page.with_untracked(|p| p.as_ref().map(|p| p.document)) != Some(page.document);
            if new_document {
                b.controls.update(HashMap::clear);
                b.scroll.set(0);
                b.hints.set(None);
                b.find_at.set(usize::MAX);
            }
            let url = page.url.clone();
            b.page.set(Some(page));
            b.refresh_controls();
            if new_document {
                apply_site(b, &url);
            }
        }
        Update::State(state) => b.state.set(state),
        Update::Done(id, result) => {
            let mut then = None;
            b.waiting.update(|w| then = w.remove(&id));
            match then {
                Some(then) => then(result, cx),
                None => {
                    if let Err(refusal) = result {
                        cx.toast(format!("[red]{}[/]", markup(&refusal.to_string())));
                    }
                }
            }
        }
        Update::Download { name, .. } => cx.toast(format!(
            "[yellow]A download, {}, waits in quarantine.[/]",
            markup(&name)
        )),
        Update::DownloadDone { .. } => {}
    }
}

/// The key of a piece of reader mode, for `each`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PieceKey {
    Text(usize, u64),
    Media(NodeId, u16),
    Controls(Vec<NodeId>),
}

fn piece_keys(page: &Page, pieces: &[Piece], images: Images) -> Vec<PieceKey> {
    use std::hash::{Hash, Hasher};
    pieces
        .iter()
        .enumerate()
        .filter_map(|(i, piece)| match piece {
            Piece::Text(lines) => {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                lines.hash(&mut h);
                Some(PieceKey::Text(i, h.finish()))
            }
            Piece::Media { index, rows } => {
                (images != Images::None).then(|| PieceKey::Media(page.media[*index].node, *rows))
            }
            Piece::Controls(c) => Some(PieceKey::Controls(
                c.iter().map(|i| page.controls[*i].node).collect(),
            )),
        })
        .collect()
}

fn pixels(images: Images) -> Pixels {
    match images {
        Images::Auto => Pixels::detect(),
        Images::Kitty => Pixels::Kitty,
        Images::Sixel => Pixels::Sixel,
        Images::Iterm2 => Pixels::Iterm,
        Images::Ansi | Images::None => Pixels::Cells,
    }
}

fn reader_view(b: Browser) -> Node {
    let doc = memo(move || {
        let width = b.viewport.get().0;
        let marks = b.marks();
        b.page.with(|p| {
            p.as_ref()
                .map(|p| layout::reader(p, width.saturating_sub(2), &marks))
                .unwrap_or_default()
        })
    });
    let keys = move || {
        b.page.with(|p| {
            p.as_ref()
                .map(|p| doc.with(|d| piece_keys(p, d, b.images.get())))
                .unwrap_or_default()
        })
    };
    let build = move |key: PieceKey| -> Node {
        match key {
            PieceKey::Text(i, _) => {
                let rows = doc.with_untracked(|d| d.get(i).map_or(1, Piece::rows)) as u16;
                renderable(move || {
                    let mut marks = Marks {
                        find: b.find.get(),
                        ..Marks::default()
                    };
                    doc.with(|d| {
                        let start: usize = d.iter().take(i).map(Piece::rows).sum();
                        let found = find_rows(b, b.viewport.get_untracked().0);
                        marks.current = found.get(b.find_at.get()).copied();
                        match d.get(i) {
                            Some(Piece::Text(lines)) => to_text(lines, &marks, start),
                            _ => rich::Text::new(""),
                        }
                    })
                })
                .fixed(rows)
            }
            PieceKey::Media(node, rows) => {
                let width = b.page.with_untracked(|p| {
                    p.as_ref()
                        .and_then(|p| p.media.iter().find(|m| m.node == node))
                        .map_or(10, |m| {
                            ((m.rect.width / CELL.0 as f64).ceil() as u16).clamp(4, 120)
                        })
                });
                let view = ImageView::new().pixels(pixels(b.images.get_untracked()));
                let picture = image_bytes_of(
                    move || {
                        b.page.with(|p| {
                            p.as_ref()?
                                .media
                                .iter()
                                .find(|m| m.node == node)?
                                .still
                                .clone()
                        })
                    },
                    view,
                );
                row([picture.fixed(width), label("").flex(1)]).fixed(rows)
            }
            PieceKey::Controls(nodes) => web_widget(b, nodes),
        }
    };
    scroll_with(each(keys, build), b.scroll)
        .autofocus()
        .name("page")
}

fn projection_view(b: Browser) -> Node {
    let lines = memo(move || {
        let width = b.viewport.get().0;
        let marks = b.marks();
        b.page.with(|p| {
            p.as_ref()
                .map(|p| layout::projection(p, width, (CELL.0 as f64, CELL.1 as f64), &marks))
                .unwrap_or_default()
        })
    });
    let canvas = renderable(move || {
        let found = find_rows(b, b.viewport.get_untracked().0);
        let marks = Marks {
            find: b.find.get(),
            current: found.get(b.find_at.get()).copied(),
            ..Marks::default()
        };
        lines.with(|l| to_text(l, &marks, 0))
    });
    scroll_with(canvas, b.scroll).autofocus().name("page")
}

/// The browser app over `engine`.
pub fn app(engine: Box<dyn Engine>, options: Options) -> App {
    let Options {
        config,
        bookmarks,
        url,
        view,
        images,
    } = options;
    let start = url
        .or_else(|| config.home.clone())
        .unwrap_or_else(|| "about:blank".into());
    let first_view = view.unwrap_or(config.view);
    let images = images.unwrap_or(config.images);
    App::new(move || {
        let (keymap, warnings) = actions::keymap(config.preset, &config.keys);
        let mut warnings = [config.warnings.clone(), warnings].concat();
        warnings.truncate(3);
        let (send, receive) = mpsc::channel::<Update>();
        let tab = Tab::spawn(engine, move |update| {
            let _ = send.send(update);
        });
        let b = Browser {
            page: signal(None),
            state: signal(PageState::default()),
            view: signal(if first_view == View::Raster {
                View::Projection
            } else {
                first_view
            }),
            hints: signal(None),
            find: signal(None),
            find_at: signal(usize::MAX),
            scroll: signal(0),
            message: signal(
                warnings
                    .iter()
                    .map(|w| format!("[yellow]config: {}[/]", markup(w)))
                    .collect::<Vec<_>>()
                    .join(" · "),
            ),
            config: signal(config),
            keymap: signal(keymap.clone()),
            bookmarks: signal(bookmarks),
            images: signal(images),
            images_default: signal(images),
            viewport: viewport(),
            tab: signal(tab),
            waiting: signal(HashMap::new()),
            controls: signal(HashMap::new()),
        };
        spawn_stream(move |stream| {
            let proxy = stream.proxy().clone();
            while let Ok(update) = receive.recv() {
                if !proxy.run_with(move |cx| apply(b, update, cx)) {
                    break;
                }
            }
        });
        // Chrome lays the page out at the terminal's size.
        watch(
            move || b.viewport.get(),
            move |(columns, rows), _| {
                b.send(Command::Resize(columns, rows.saturating_sub(2)));
            },
        );
        b.send(Command::Open(start.clone()));

        let address = text(move || {
            let state = b.state.get();
            let back = if state.can_go_back {
                "◀"
            } else {
                "[dim]◀[/]"
            };
            let forward = if state.can_go_forward {
                "▶"
            } else {
                "[dim]▶[/]"
            };
            let loading = if state.loading { " [yellow]…[/]" } else { "" };
            let (title, url) = b.page.with(|p| {
                p.as_ref()
                    .map(|p| (p.title.clone(), p.url.clone()))
                    .unwrap_or_default()
            });
            let view = b.view.get().name();
            format!(
                "{back}{forward}{loading} [bold]{}[/] [dim]{}[/] [cyan]{view}[/]",
                markup(&truncate(&title, 60)),
                markup(&truncate(&url, 120))
            )
        })
        .no_wrap()
        .fixed(1)
        .name("address");
        let keys_for = move |id: &str| b.keymap.with(|k| keys_label(k, id));
        let status = text(move || {
            if let Some(hints) = b.hints.get() {
                return format!(
                    "[bold black on yellow] hints [/] type a label: {}  [dim]Esc cancels[/]",
                    markup(&hints.typed)
                );
            }
            let message = b.message.get();
            if !message.is_empty() {
                return message;
            }
            if let Some(find) = b.find.get() {
                return format!(
                    "find: {}  [dim]{} next · {} previous[/]",
                    markup(&find),
                    markup(&keys_for("find.next")),
                    markup(&keys_for("find.previous"))
                );
            }
            format!(
                "[dim]{} actions · {} hints · {} open · {} reader · {} quit[/]",
                markup(&keys_for("actions")),
                markup(&keys_for("hints")),
                markup(&keys_for("open")),
                markup(&keys_for("reader")),
                markup(&keys_for("quit"))
            )
        })
        .no_wrap()
        .fixed(1)
        .name("status");
        let body = switch(
            move || b.view.get(),
            move |view| match view {
                View::Reader => reader_view(b),
                _ => projection_view(b),
            },
        )
        .flex(1);
        let mut root = column([address, body, status]);
        for action in ACTIONS {
            let Some((keys, sequence)) = actions::keys_of(&keymap, action.id) else {
                continue;
            };
            let id = action.id;
            let handler = move |cx: &mut Ctx| {
                b.message.set(String::new());
                // A site key overrides a global one, on its site only.
                if let Some(target) = cx.key().and_then(|key| site_key(b, key)) {
                    run_target(b, &target, cx);
                    return;
                }
                run(b, id, cx);
            };
            root = if sequence {
                root.bind_sequence(&keys, action.label, handler)
            } else {
                root.bind(&keys, action.label, handler)
            };
        }
        // Hints and site keys see keys before the page and its widgets
        // (a text box keeps what is typed into it).
        column([root.flex(1)])
            .previews_keys()
            .on_any_key(move |cx, key| {
                if b.hints.with_untracked(Option::is_some) {
                    return hint_key(b, key, cx);
                }
                if let Some(target) = site_key(b, key) {
                    b.message.set(String::new());
                    run_target(b, &target, cx);
                    return true;
                }
                false
            })
    })
    .graphics(image_graphics())
}
