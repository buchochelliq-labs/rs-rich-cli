//! The browser as an intuiTUIve app: the address line, the page (the
//! projection by default, reader mode one key away), the status line, and
//! the action registry's keys, hints, find, F1 overlay and site keys.
//!
//! The tab runs on a thread of its own ([`crate::tab`]); its updates come
//! to the app's thread through a proxy, into signals the nodes read.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rich_embed::PageState;
#[cfg(test)]
use rich_intuituive::image_graphics;
use rich_intuituive::interact::keymap::{Keymap, try_keys};
use rich_intuituive::interact::{Key, KeyCode, MouseKind};
use rich_intuituive::prelude::*;
use rich_intuituive::{
    Action, ImageView, Picture, Pixels, button_of, picture_bytes, picture_of, scroll_virtual_with,
    slider, spawn_stream,
};

use crate::actions::{self, ACTIONS, keys_label};
use crate::adapt::{Adapt, RULES};
use crate::bookmarks::{Bookmark, Bookmarks};
use crate::cdp::CELL;
use crate::clean::{markup, truncate};
use crate::config::{Config, Images, Site, Target, View, remove_site_key, save_site_key};
use crate::engine::{Engine, HistoryStep, MediaOp, NodeId, Playback};
use crate::layout::{
    self, Line, LogScroll, Marks, Piece, Projection, hint_labels, matches, scroll_log, to_text,
};
use crate::model::{Binding, MediaKind, Page, Role, Shelf};
use crate::pictures::{self, Live};
use crate::shelf::page_by;
use crate::tab::{Command, Intent, Refusal, Tab, Update};
use crate::video::{Encoder, Job, Pacing, VideoFrame};
use crate::widgets::{WebControl, web_widget};

type UpdateSink = Arc<dyn Fn(Update) + Send + Sync>;

/// The video regions on screen and the pacing's intervals, as told to the
/// tab.
type RegionsSent = (Vec<NodeId>, Vec<(NodeId, Duration)>, f64);

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
    /// `--ssh HOST`'s tunnel, which the app watches and reconnects: the
    /// status bar says when it drops ([`crate::ssh`]).
    pub tunnel: Option<crate::ssh::TunnelLink>,
    /// The control socket the app answers, with `--control` or
    /// `--read-only` ([`crate::control`]).
    pub control: Option<crate::control::ControlLink>,
}

/// The app's state, all of it in signals so it can be copied into every
/// handler and sent with the tab's updates.
#[derive(Clone, Copy)]
pub struct Browser {
    pub(crate) presentation: rich_intuituive::ImagePresentation,
    pub page: Signal<Option<Arc<Page>>>,
    pub state: Signal<PageState>,
    pub view: Signal<View>,
    pub hints: Signal<Option<Hints>>,
    pub find: Signal<Option<String>>,
    pub find_at: Signal<usize>,
    /// The first row in view.
    pub scroll: Signal<u64>,
    /// The current view's rows, laid out once per reading, width, view
    /// and hints: what is drawn, what find searches and what the scroll
    /// keys measure. Not laid out again per key or per draw.
    pub layout: Memo<Rc<Projection>>,
    /// Where find's matches are in [`layout`](Self::layout): (row, nth on
    /// the row).
    pub found: Memo<Rc<Vec<(usize, usize)>>>,
    /// Where Chrome's own scroll is, in CSS pixels, as last sent or read.
    pub chrome_y: Signal<f64>,
    /// The pending send of our scroll to Chrome.
    follow: Signal<Option<Timer>>,
    pub message: Signal<String>,
    pub config: Signal<Config>,
    pub keymap: Signal<Keymap>,
    pub bookmarks: Signal<Bookmarks>,
    pub images: Signal<Images>,
    /// How images show where no site says otherwise.
    pub images_default: Signal<Images>,
    /// The terminal's size in cells.
    pub viewport: Memo<(u16, u16)>,
    /// Each video region's newest frame, as its pacing let it through, and
    /// the protocol the pacing chose: merged into [`live`](Self::live).
    pub videos: Signal<Rc<HashMap<NodeId, Live>>>,
    /// The frames' pacing within the terminal's byte budget.
    pacing: Signal<Rc<RefCell<Pacing>>>,
    /// Encodes the frames the pacing lets through, off the thread that
    /// draws.
    encoder: Signal<Rc<Encoder>>,
    prepared: Signal<Arc<std::sync::Mutex<Prepared>>>,
    /// The regions on screen and the pacing's intervals, as last told to
    /// the tab.
    regions_sent: Signal<RegionsSent>,
    /// The terminal adaptations configured.
    pub adapt: Signal<Adapt>,
    /// Whether the faithful projection is shown: no adaptation at all.
    pub faithful: Signal<bool>,
    /// Where the user has scrolled each live log.
    pub logs: Signal<HashMap<NodeId, LogScroll>>,
    /// The protocol pictures are drawn with, from [`images`](Self::images).
    pub pixels: Memo<Pixels>,
    /// Whether moving pictures are held still (`media.freeze`).
    pub frozen: Signal<bool>,
    /// The frame each moving picture in view shows now, by node: an
    /// animated GIF's, stepped within the byte budget.
    pub live: Signal<Rc<HashMap<NodeId, Live>>>,
    tab: Signal<Tab>,
    waiting: Signal<HashMap<u64, Then>>,
    controls: Signal<HashMap<NodeId, Rc<WebControl>>>,
    /// Whether the start page shows: started with no address, until the
    /// first page is opened ([`crate::start`]).
    pub start: Signal<bool>,
    /// What the status bar says about `--ssh`'s tunnel, as markup; `None`
    /// without one.
    pub tunnel: Signal<Option<String>>,
    /// Reconnect a dropped tunnel now, not after its wait (reload asks).
    tunnel_now: Signal<bool>,
    /// How many readings of the page have come: an agent waits for the
    /// next after an action ([`crate::control`]).
    pub reads: Signal<u64>,
    /// Whether the control socket refuses every verb that changes
    /// anything.
    pub read_only: Signal<bool>,
    /// The hints under way pick an element to send to the agent
    /// (`agent.send`), not one to use.
    pub(crate) agent_pick: Signal<bool>,
    /// The pixel view's tiler and pace ([`crate::pixel_view`]).
    pub(crate) screen: Signal<Rc<RefCell<crate::pixel_view::Screen>>>,
    /// The pixel view's tiles, as shown.
    pub screen_tiles: Signal<Vec<rich_art::tiles::Tile>>,
    /// What the pixel view has sent.
    pub screen_stats: Signal<crate::pixel_view::ScreenStats>,
}

impl Browser {
    pub(crate) fn send(&self, command: Command) -> u64 {
        if self.start.get_untracked() && crate::start::leaves(&command) {
            self.start.set(false);
        }
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
        // Out of the signal first: a widget's write can reach it.
        let all: Vec<(NodeId, Rc<WebControl>)> = self
            .controls
            .with_untracked(|all| all.iter().map(|(n, w)| (*n, w.clone())).collect());
        for (node, web) in all {
            if let Some(control) = page.control(node) {
                web.sync(&page, control);
            }
        }
    }

    pub(crate) fn url(&self) -> String {
        self.page
            .with(|p| p.as_ref().map(|p| p.url.clone()))
            .unwrap_or_else(|| self.state.with(|s| crate::clean::clean(&s.url)))
    }

    /// The site table for the page open now.
    fn site(&self) -> Option<Site> {
        let url = self.url();
        self.config.with(|c| c.site(&url).cloned())
    }

    /// The adaptations in effect: none while the faithful projection is
    /// shown.
    pub fn effective_adapt(&self) -> Adapt {
        if self.faithful.get() {
            Adapt::none()
        } else {
            self.adapt.get()
        }
    }

    /// What the views lay out with: hint labels, the adaptations and the
    /// live logs' scroll (find marks are drawn over the layout, not part
    /// of it).
    pub fn hint_marks(&self) -> Marks {
        let hints = self
            .hints
            .with(|h| {
                h.as_ref()
                    .map(|h| h.labels.iter().map(|(l, c)| (*c, l.clone())).collect())
            })
            .unwrap_or_default();
        Marks {
            hints,
            adapt: self.effective_adapt(),
            logs: self.logs.get(),
            ..Marks::default()
        }
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
            adapt: self.effective_adapt(),
            logs: self.logs.get(),
        }
    }

    pub(crate) fn say(&self, markup: impl Into<String>) {
        self.message.set(markup.into());
    }
}

/// The page's lines in the current view at `width`, as find counts rows:
/// every piece of reader mode in turn (a media region or a control as its
/// rows of nothing), or the projection, with each row's key and place on
/// the page.
#[cfg(test)]
pub(crate) fn lay_out(page: &Page, view: View, width: u16, marks: &Marks) -> Projection {
    lay_out_with_logs(page, view, width, marks, &mut Vec::new())
}

pub(crate) fn lay_out_with_logs(
    page: &Page,
    view: View,
    width: u16,
    marks: &Marks,
    slots: &mut Vec<layout::LogSlot>,
) -> Projection {
    #[cfg(test)]
    TEST_LAYOUTS.with(|n| n.set(n.get() + 1));
    match view {
        View::Reader => {
            let mut lines = Vec::new();
            for piece in layout::reader(page, width.saturating_sub(2), marks) {
                match piece {
                    Piece::Text(text) => lines.extend(text),
                    other => lines.extend(std::iter::repeat_n(Line::default(), other.rows())),
                }
            }
            // Reader rows stand for the page's height in proportion.
            let rows = lines.len().max(1) as f64;
            let y = (0..lines.len())
                .map(|r| r as f64 / rows * page.height)
                .collect();
            let keys: Vec<u64> = (0..lines.len() as u64).collect();
            let rows = keys.iter().map(|&k| (k, k as usize)).collect();
            Projection {
                lines,
                keys,
                y,
                rows,
                ..Projection::default()
            }
        }
        _ => {
            // Fixed source dialogs use source-positioned underlay while visible;
            // otherwise adaptive gap collapse can move covered content outside
            // the source rectangle. Restore the configured rules on close.
            let mut underlay = marks.clone();
            if !page.overlays.is_empty() {
                underlay.adapt = crate::adapt::Adapt::none()
                    .with(
                        crate::adapt::Rule::Shelves,
                        marks.adapt.on(crate::adapt::Rule::Shelves),
                    )
                    .with(
                        crate::adapt::Rule::PageColors,
                        marks.adapt.on(crate::adapt::Rule::PageColors),
                    )
                    .with(
                        crate::adapt::Rule::ScreenReaderTwins,
                        marks.adapt.on(crate::adapt::Rule::ScreenReaderTwins),
                    );
            }
            layout::project_with_logs(
                page,
                width,
                (CELL.0 as f64, CELL.1 as f64),
                &underlay,
                slots,
            )
        }
    }
}

/// Where find's matches are: (row, nth on the row).
fn find_rows(b: Browser) -> Rc<Vec<(usize, usize)>> {
    b.found.get_untracked()
}

/// Our first row in view as Chrome's scroll: a wheel turn for a move of a
/// few screens, a jump for more, sent once the view rests.
fn follow_scroll(b: Browser, row: u64, cx: &mut Ctx) {
    if let Some(timer) = b.follow.get_untracked() {
        timer.cancel();
    }
    let timer = cx.after(FOLLOW, move |_| {
        b.follow.set(None);
        // The first row is the page's top, whatever its margin.
        let to = match row {
            0 => 0.0,
            _ => b.layout.with_untracked(|l| l.y_of(row as usize)),
        };
        let from = b.chrome_y.get_untracked();
        let screen = b.viewport.get_untracked().1 as f64 * CELL.1 as f64;
        let delta = to - from;
        if delta.abs() < 1.0 {
            return;
        }
        b.chrome_y.set(to);
        if delta.abs() > 3.0 * screen.max(1.0) {
            b.send(Command::Scroll(to));
        } else {
            b.send(Command::Wheel(delta));
        }
    });
    b.follow.set(Some(timer));
}

/// How often moving pictures are looked at: a GIF's frame steps on the
/// first look after its time (and its byte budget) allows.
const MOTION: std::time::Duration = std::time::Duration::from_millis(25);

/// Step the moving pictures in view, and publish their frames when one
/// changed.
fn step_motion(b: Browser, motion: &mut pictures::Motion) {
    // GIFs and videos share one byte budget, from the config and the
    // bandwidth rule.
    let budget = byte_budget(b);
    let videos = b.pacing.with_untracked(|p| {
        let mut pacing = p.borrow_mut();
        pacing.budget = budget;
        pacing.moving()
    });
    motion.share_with(budget, videos);
    let cell = rich_intuituive::cell_pixels()
        .get_untracked()
        .map(|(w, h)| rich_art::graphics::CellPixels::new(w.into(), h.into()))
        .unwrap_or_default();
    motion.set_cell_pixels(cell);
    let page = b.page.get_untracked();
    let images = b.images.get_untracked();
    let next = match page {
        Some(page) if images != Images::None && b.view.get_untracked() == View::Projection => {
            let height = usize::from(b.viewport.get_untracked().1);
            b.layout.with_untracked(|l| {
                // The first row in view; "the end" is a scroll past it.
                let first = usize::try_from(b.scroll.get_untracked())
                    .unwrap_or(usize::MAX)
                    .min(l.lines.len().saturating_sub(height));
                let rows = first..first.saturating_add(height);
                motion.step(
                    &page,
                    &l.regions,
                    rows,
                    b.pixels.get_untracked(),
                    b.frozen.get_untracked(),
                    std::time::Instant::now(),
                )
            })
        }
        _ => {
            motion.hide();
            HashMap::new()
        }
    };
    b.pacing
        .with_untracked(|p| p.borrow_mut().others = motion.moving_count());
    // The videos' frames, which come from the tab, with them.
    let mut next = next;
    b.videos.with_untracked(|v| {
        for (node, live) in v.iter() {
            next.insert(*node, live.clone());
        }
    });
    if b.live.with_untracked(|l| **l != next) {
        b.live.set(Rc::new(next));
    }
}

/// How long the view rests before Chrome's scroll follows it.
const FOLLOW: std::time::Duration = std::time::Duration::from_millis(120);

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
        "media.play_pause" | "media.mute" | "media.volume_up" | "media.volume_down"
        | "media.seek_back" | "media.seek_forward" => {
            if current_media(b).is_some() {
                Ok(())
            } else if b.page.with(|p| {
                p.as_ref().is_some_and(|p| {
                    p.media
                        .iter()
                        .any(|m| matches!(m.kind, MediaKind::Video | MediaKind::Audio))
                })
            }) {
                Err("The media is not ready yet.".into())
            } else {
                Err("No video or audio on this page.".into())
            }
        }
        "shelf.previous" | "shelf.next"
            if b.page
                .with(|p| p.as_ref().is_none_or(|p| p.shelves.is_empty())) =>
        {
            Err("No shelf on this page: nothing here scrolls sideways.".into())
        }
        "log.older" | "log.newer"
            if b.page
                .with(|p| p.as_ref().is_none_or(|p| p.logs.is_empty())) =>
        {
            Err("No live log on this page.".into())
        }
        "view.pixels" if !crate::pixel_view::available(b) => {
            Err(crate::pixel_view::NEEDS_KITTY.into())
        }
        "agent.send"
            if !b.page.with(|p| {
                p.as_ref()
                    .is_some_and(|p| p.controls.iter().any(|c| !c.disabled))
            }) =>
        {
            Err(
                "Nothing on this page to pick: the links and controls are what Ctrl+G sends."
                    .into(),
            )
        }
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
    let rows = b.viewport.get_untracked().1.saturating_sub(4).max(1) as u64;
    match id {
        "open" => open_prompt(b, String::new(), cx),
        "address" => open_prompt(b, b.url(), cx),
        "hints" => start_hints(b),
        "agent.send" => {
            start_hints(b);
            b.agent_pick.set(true);
        }
        "view.pixels" => {
            b.view.update(|v| {
                *v = if *v == View::Pixels {
                    View::Projection
                } else {
                    View::Pixels
                }
            });
            b.say(format!(
                "{} view · {} leaves",
                b.view.get_untracked().name(),
                markup(&keys_label_of(b, "view.pixels"))
            ));
        }
        "back" => drop(b.send(Command::History(HistoryStep::Back))),
        "forward" => drop(b.send(Command::History(HistoryStep::Forward))),
        "reload" => {
            // A dropped tunnel is reconnected first, at once.
            b.tunnel_now.set(true);
            b.send(Command::History(HistoryStep::Reload));
        }
        "media.freeze" => {
            let frozen = !b.frozen.get_untracked();
            b.frozen.set(frozen);
            // Videos too: no frames while frozen.
            send_regions(b);
            b.say(if frozen {
                "[yellow]Moving pictures frozen[/] · F4 resumes"
            } else {
                "Moving pictures resumed"
            });
        }
        "find" => find_prompt(b, cx),
        "find.next" | "find.previous" => {
            let found = find_rows(b);
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
            b.scroll.set((found[next].0 as u64).saturating_sub(3));
            b.say(format!("match {} of {}", next + 1, found.len()));
        }
        // The view keeps the offset within the page.
        "scroll.down" => b.scroll.update(|s| *s = s.saturating_add(1)),
        "scroll.up" => b.scroll.update(|s| *s = s.saturating_sub(1)),
        "page.down" => b.scroll.update(|s| *s = s.saturating_add(rows)),
        "page.up" => b.scroll.update(|s| *s = s.saturating_sub(rows)),
        "top" => b.scroll.set(0),
        "bottom" => b.scroll.set(u64::MAX),
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
        "adapt" => {
            b.faithful.update(|f| *f = !*f);
            if b.faithful.get_untracked() {
                b.say("[yellow]faithful[/] projection: no terminal adaptations");
            } else {
                let on = b.adapt.get_untracked().active().count();
                b.say(format!("adapted projection: {on} adaptations on"));
            }
        }
        "log.older" | "log.newer" => {
            let Some((node, rows)) = current_log(b) else {
                return;
            };
            let step = (rows / 2).max(1) as isize;
            let by = if id == "log.older" { -step } else { step };
            let next = b.page.with_untracked(|p| {
                let log = p.as_ref()?.logs.iter().find(|l| l.node == node)?;
                let now = b
                    .logs
                    .with_untracked(|l| l.get(&node).copied().unwrap_or_default());
                Some(scroll_log(log, now, by))
            });
            if let Some(next) = next {
                b.logs.update(|l| {
                    if next == LogScroll::Follow {
                        l.remove(&node);
                    } else {
                        l.insert(node, next);
                    }
                });
                b.say(if next == LogScroll::Follow {
                    "live log: following its newest entries".to_string()
                } else {
                    format!(
                        "live log: [yellow]paused[/] · {} to follow again",
                        markup(&keys_label_of(b, "log.newer"))
                    )
                });
            }
        }
        "shelf.previous" | "shelf.next" => {
            let forward = id == "shelf.next";
            let found = b.page.with_untracked(|p| {
                let p = p.as_ref()?;
                // What the screen shows of it, as the projection draws it.
                let right = b.viewport.get_untracked().0 as f64 * CELL.0 as f64;
                let shelf = current_shelf(b, p)?.clipped(right);
                Some((
                    p.document,
                    shelf.node,
                    shelf.can_page(),
                    page_by(&shelf, forward),
                ))
            });
            let Some((document, node, (back, on), by)) = found else {
                return;
            };
            if (forward && !on) || (!forward && !back) {
                b.say(if forward {
                    "shelf: at its end"
                } else {
                    "shelf: at its start"
                });
                return;
            }
            b.send(Command::ScrollBox { document, node, by });
        }
        "about" => drop(b.send(Command::Open(crate::about::ADDRESS.into()))),
        "media.play_pause" | "media.mute" | "media.volume_up" | "media.volume_down"
        | "media.seek_back" | "media.seek_forward" => {
            let Some((node, playback)) = current_media(b) else {
                return;
            };
            let op = match id {
                "media.play_pause" if playback.paused => MediaOp::Play,
                "media.play_pause" => MediaOp::Pause,
                "media.mute" => MediaOp::Mute(!playback.muted),
                // Ten points at a time, kept in 0 to 100%; a muted element
                // stays muted.
                "media.volume_up" => MediaOp::Volume((playback.volume + 0.1).min(1.0)),
                "media.seek_back" => MediaOp::Seek(-SEEK),
                "media.seek_forward" => MediaOp::Seek(SEEK),
                _ => MediaOp::Volume((playback.volume - 0.1).max(0.0)),
            };
            media_op(b, node, op);
        }
        "site.bind" => bind_picker(b, cx),
        "site.keys" => site_keys_list(b, cx),
        "actions" => overlay(b, cx),
        "quit" => cx.quit(),
        _ => {}
    }
}

fn keys_label_of(b: Browser, id: &str) -> String {
    b.keymap.with_untracked(|k| keys_label(k, id))
}

/// The live log the log keys scroll, and its rows: the first in view in
/// the projection, else the page's first.
pub fn current_log(b: Browser) -> Option<(NodeId, usize)> {
    let first = b
        .page
        .with_untracked(|p| p.as_ref()?.logs.first().map(|l| (l.node, 10)));
    if b.view.get_untracked() == View::Reader {
        return first;
    }
    let top = b.scroll.get_untracked().min(usize::MAX as u64) as usize;
    let height = b.viewport.get_untracked().1 as usize;
    b.layout
        .with_untracked(|l| {
            let top = top.min(l.lines.len().saturating_sub(1));
            l.logs
                .iter()
                .find(|(_, row, rows)| *row < top + height && top < row + rows)
                .map(|(node, _, rows)| (*node, *rows))
        })
        .or(first)
}

/// The shelf the shelf keys page: the first in view in the projection,
/// else the page's first.
pub fn current_shelf(b: Browser, page: &Page) -> Option<&Shelf> {
    let first = page.shelves.first();
    if b.view.get_untracked() == View::Reader {
        return first;
    }
    let top = b.scroll.get_untracked().min(usize::MAX as u64) as usize;
    let height = b.viewport.get_untracked().1 as usize;
    let (from, to) = b.layout.with_untracked(|l| {
        let top = top.min(l.lines.len().saturating_sub(1));
        (l.y_of(top), l.y_of(top + height))
    });
    page.shelves
        .iter()
        .find(|s| s.rect.y < to && s.rect.bottom() > from)
        .or(first)
}

/// The about page (`about:noctiluca`): the version, the browser, every
/// terminal adaptation, on or off, and what the browser can play, as its
/// `canPlayType` `answers` to [`crate::about::CODECS`] say.
fn about(b: Browser, answers: Vec<String>, product: String, cx: &mut Ctx) {
    cx.modal(Size::Percent(90), Size::Percent(90), move || {
        let answers = answers.clone();
        let product = product.clone();
        let body = text(move || {
            let configured = b.adapt.get();
            let faithful = b.faithful.get();
            let mut lines = vec![
                format!("[bold]Noctiluca[/] {}", env!("CARGO_PKG_VERSION")),
                format!("Browser: {}", markup(&product)),
                String::new(),
                if faithful {
                    "[bold]Terminal adaptations[/]: [yellow]off, the faithful projection[/]".into()
                } else {
                    "[bold]Terminal adaptations[/] (active rules):".into()
                },
            ];
            for rule in RULES {
                let on = configured.on(rule.rule) && !faithful;
                lines.push(format!(
                    " {} {:<20} [dim]{}[/]",
                    if on { "[green]on [/]" } else { "[dim]off[/]" },
                    rule.name,
                    markup(rule.what)
                ));
            }
            lines.push(String::new());
            lines.push(
                "[bold]Codecs[/]: what this browser says it can play (canPlayType). DRM playback is never tried."
                    .into(),
            );
            for ((name, kind), answer) in crate::about::CODECS.iter().zip(&answers) {
                lines.push(format!(
                    " {:<14} {:<15} [dim]{}[/]",
                    name,
                    crate::about::verdict(answer),
                    markup(kind)
                ));
            }
            lines.push(String::new());
            lines.push(format!(
                "[dim]{} toggles adapted and faithful · \\[adapt] in the config or --adapt=-RULE switch one off · Esc closes[/]",
                markup(&keys_label_of(b, "adapt"))
            ));
            lines.join("\n")
        });
        scroll_with(body, signal(0u16))
            .focusable()
            .autofocus()
            .on_key("esc q", |cx| cx.pop())
            .panel(crate::about::TITLE)
    });
}

/// The video or audio the media actions work on: the one playing, else
/// the first that is ready.
pub fn current_media(b: Browser) -> Option<(NodeId, Playback)> {
    b.page.with(|p| {
        let page = p.as_ref()?;
        let ready = || {
            page.media
                .iter()
                .filter_map(|m| Some((m.node, m.playback?)))
        };
        ready().find(|(_, p)| !p.paused).or_else(|| ready().next())
    })
}

/// Do `op` to the media `node` of the page now.
fn media_op(b: Browser, node: NodeId, op: MediaOp) {
    let Some(document) = b.page.with_untracked(|p| p.as_ref().map(|p| p.document)) else {
        return;
    };
    let id = b.send(Command::Media { document, node, op });
    b.after(id, |result, cx| {
        if let Err(refusal) = result {
            cx.toast(format!("[red]{}[/]", markup(&refusal.to_string())));
        }
    });
}

use crate::bars::clock;

/// How far the seek keys move the media, in seconds: the PoC's step.
const SEEK: f64 = 5.0;

fn offer_video_frame(
    view: View,
    pacing: &mut Pacing,
    frame: &VideoFrame,
    cols: usize,
    rows: usize,
    cell: rich_art::graphics::CellPixels,
    now: Instant,
) -> (Option<Picture>, Pixels) {
    if view != View::Projection {
        return (None, pacing.pixels(frame.node));
    }
    pacing.set_cell_pixels(cell);
    let picture = pacing.offer(frame, cols, rows, now, picture_bytes);
    (picture, pacing.pixels(frame.node))
}

/// A video's frame from the tab: offered to its pacing, and drawn when due.
fn video_frame(b: Browser, frame: &VideoFrame) {
    let Some(rect) = b.page.with_untracked(|p| {
        p.as_ref()?
            .media
            .iter()
            .find(|m| m.node == frame.node)
            .map(|m| m.rect)
    }) else {
        return;
    };
    let cols = ((rect.width / CELL.0 as f64).ceil() as usize).clamp(1, 400);
    let rows = ((rect.height / CELL.1 as f64).ceil() as usize).clamp(1, 200);
    let (cols, rows) = drawn_cells(b, frame.node).unwrap_or((cols, rows));
    let metrics = rich_intuituive::cell_pixels().get_untracked();
    let cell = metrics
        .map(|(w, h)| rich_art::graphics::CellPixels::new(w.into(), h.into()))
        .unwrap_or_default();
    let view = b.view.get_untracked();
    let (picture, pixels) = b.pacing.with_untracked(|p| {
        offer_video_frame(
            view,
            &mut p.borrow_mut(),
            frame,
            cols,
            rows,
            cell,
            Instant::now(),
        )
    });
    match picture {
        // Cells are made as they are drawn, and cheaply.
        Some(picture) if pixels == Pixels::Cells => {
            show_frame(b, frame.node, Live { picture, pixels })
        }
        // Pixels are encoded on the encoder's thread first, in the cells
        // the region is drawn in, so the painter finds the escape made.
        Some(picture) => {
            let job = Job {
                node: frame.node,
                picture,
                pixels,
                cols,
                rows,
            };
            b.prepared
                .with_untracked(|queue| queue.lock().unwrap().admit(&job));
            b.encoder
                .with_untracked(|encoder| encoder.encode_with_metrics(job, metrics));
        }
        None => {}
    }
    send_regions(b);
}

/// The cells a video region is drawn in, where the projection placed it.
fn drawn_cells(b: Browser, node: NodeId) -> Option<(usize, usize)> {
    b.layout.with_untracked(|l| {
        b.page.with_untracked(|p| {
            let page = p.as_ref()?;
            l.regions
                .iter()
                .find(|r| page.media.get(r.media).is_some_and(|m| m.node == node))
                .map(|r| (r.cols, r.rows))
        })
    })
}

/// Show a video's frame in its region, with the moving GIFs: unless the
/// region is gone (a new document) or shows a newer frame already.
fn show_frame(b: Browser, node: NodeId, live: Live) {
    let shown = b.videos.with_untracked(|v| v.get(&node).cloned());
    let accepted = b
        .pacing
        .with_untracked(|p| accepts_frame(&p.borrow(), shown.as_ref(), node, &live));
    if !accepted {
        return;
    }
    let mut videos = (*b.videos.get_untracked()).clone();
    videos.insert(node, live.clone());
    b.videos.set(Rc::new(videos));
    let mut shown = (*b.live.get_untracked()).clone();
    shown.insert(node, live);
    b.live.set(Rc::new(shown));
}

fn accepts_frame(pacing: &Pacing, shown: Option<&Live>, node: NodeId, live: &Live) -> bool {
    pacing.owns_picture(node, &live.picture)
        && pacing.pixels(node) == live.pixels
        && !shown.is_some_and(|shown| {
            shown.picture.id() == live.picture.id()
                && shown.picture.version() > live.picture.version()
        })
}

/// The moving regions the view shows (videos, and canvases, which the
/// tab watches as live regions): those whose box meets the page's rows in
/// view.
fn regions_on_screen(b: Browser) -> Vec<NodeId> {
    let rows = b.viewport.get().1 as usize;
    let first = b.scroll.get() as usize;
    let (top, bottom) = b.layout.with(|l| {
        let top = if first == 0 { 0.0 } else { l.y_of(first) };
        let bottom = if first.saturating_add(rows) >= l.lines.len() {
            f64::MAX
        } else {
            l.y_of(first + rows)
        };
        (top, bottom)
    });
    b.page.with(|p| {
        p.as_ref()
            .map(|p| {
                p.media
                    .iter()
                    .filter(|m| matches!(m.kind, MediaKind::Video | MediaKind::Canvas))
                    .filter(|m| m.rect.bottom() > top && m.rect.y < bottom)
                    .map(|m| m.node)
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// `interval` as a whole number of frames a second, rounded down (whole
/// seconds below one a second), so a small change in the pacing's
/// estimate does not tell the tab again, and the tab never cuts frames
/// faster than the pacing takes them.
fn whole_rate(interval: Duration) -> Duration {
    let fps = 1.0 / interval.as_secs_f64().max(f64::MIN_POSITIVE);
    if fps >= 1.0 {
        Duration::from_secs_f64(1.0 / fps.floor())
    } else {
        Duration::from_secs(interval.as_secs_f64().ceil() as u64)
    }
}

/// Tell the tab which video regions are on screen and how often each may
/// have a frame, when that changed.
fn send_regions(b: Browser) {
    // Frozen, no region gets frames: the screencast stops.
    let on_screen = if b.frozen.get_untracked() {
        Vec::new()
    } else {
        regions_on_screen(b)
    };
    let intervals: Vec<(NodeId, Duration)> = b.pacing.with_untracked(|p| {
        p.borrow()
            .intervals()
            .into_iter()
            .map(|(node, d)| (node, whole_rate(d)))
            .collect()
    });
    let scale = screencast_scale(b, &on_screen);
    let next = (on_screen, intervals, scale);
    if b.regions_sent.with_untracked(|sent| *sent != next) {
        b.regions_sent.set(next.clone());
        b.send(Command::Regions {
            on_screen: next.0,
            intervals: next.1,
            scale: next.2,
        });
    }
}

/// The screencast's scale the regions on screen need: the most of the
/// page's pixels any is drawn in, as a power of two (so a small change in
/// a region's size does not restart the screencast), from a sixteenth to
/// all of them. Character art draws two pixels a cell, far fewer than the
/// page has; the pixel protocols draw about as many as it has.
fn screencast_scale(b: Browser, on_screen: &[NodeId]) -> f64 {
    let need = b.page.with_untracked(|p| {
        let Some(page) = p.as_ref() else {
            return 1.0;
        };
        b.pacing.with_untracked(|pacing| {
            let pacing = pacing.borrow();
            on_screen
                .iter()
                .filter_map(|node| page.media.iter().find(|m| m.node == *node))
                .map(|m| {
                    if pacing.pixels(m.node) != Pixels::Cells {
                        return 1.0;
                    }
                    let cols = (m.rect.width / CELL.0 as f64).ceil().max(1.0);
                    let rows = (m.rect.height / CELL.1 as f64).ceil().max(1.0);
                    (cols / m.rect.width.max(1.0)).max(2.0 * rows / m.rect.height.max(1.0))
                })
                .fold(0.0, f64::max)
        })
    });
    if need <= 0.0 {
        return 1.0;
    }
    2f64.powi(need.log2().ceil() as i32).clamp(1.0 / 16.0, 1.0)
}

/// A video's region in reader mode: its newest frame while it has played,
/// else its poster, in the protocol its pacing chose; with the play mark
/// under it while it is not playing.
fn video_region(b: Browser, node: NodeId) -> Node {
    let picture = switch(
        move || shown_picture(b, node).map_or(b.pixels.get(), |(_, pixels)| pixels),
        move |pixels| {
            picture_of(
                move || shown_picture(b, node).map(|(p, _)| p),
                ImageView::new().pixels(pixels),
            )
        },
    )
    .flex(1);
    let mark = text(move || {
        let playing = b.page.with(|p| {
            p.as_ref()
                .and_then(|p| p.media.iter().find(|m| m.node == node)?.playback)
                .is_some_and(|p| p.playing())
        });
        if playing {
            String::new()
        } else {
            "[bold black on white] ▶ [/]".into()
        }
    })
    .no_wrap()
    .fixed(1);
    column([picture, mark])
}

/// A video's or an audio's controls, from the element's real state:
/// Play/Pause and Mute/Unmute with live labels (C1), a volume slider, and
/// its time. Sound plays on the machine Chrome runs on.
fn player(b: Browser, node: NodeId) -> Node {
    let playback = move || {
        b.page
            .with(|p| p.as_ref()?.media.iter().find(|m| m.node == node)?.playback)
    };
    let volume = signal(
        b.page
            .with_untracked(|p| p.as_ref()?.media.iter().find(|m| m.node == node)?.playback)
            .map_or(100.0, |p| (p.volume * 100.0).round()),
    );
    // The slider follows the element, and moving it sets the element.
    watch(
        move || playback().map(|p| (p.volume * 100.0).round() as i64),
        move |v, _| {
            if let Some(v) = v
                && volume.get_untracked() != v as f64
            {
                volume.set(v as f64);
            }
        },
    );
    on_write(volume, move |v, _| {
        let now = b
            .page
            .with_untracked(|p| p.as_ref()?.media.iter().find(|m| m.node == node)?.playback);
        if now.is_some_and(|p| ((p.volume * 100.0).round() - v).abs() >= 1.0) {
            media_op(b, node, MediaOp::Volume(v / 100.0));
        }
    });
    let play = button_of(
        move || {
            if playback().is_some_and(|p| !p.paused) {
                "Pause".into()
            } else {
                "Play".into()
            }
        },
        move |_| {
            if let Some(p) = playback() {
                media_op(
                    b,
                    node,
                    if p.paused {
                        MediaOp::Play
                    } else {
                        MediaOp::Pause
                    },
                );
            }
        },
    )
    .disabled_when(move || playback().is_none())
    .auto();
    let mute = button_of(
        move || {
            if playback().is_some_and(|p| p.muted) {
                "Unmute".into()
            } else {
                "Mute".into()
            }
        },
        move |_| {
            if let Some(p) = playback() {
                media_op(b, node, MediaOp::Mute(!p.muted));
            }
        },
    )
    .disabled_when(move || playback().is_none())
    .auto();
    let status = text(move || {
        let sound = if b.config.with(|c| c.audio) {
            ""
        } else {
            "  [dim]sound is off: start with --audio[/]"
        };
        match playback() {
            None => format!("[dim]not ready[/]{sound}"),
            Some(p) => format!(
                " {} / {}{}{sound}",
                clock(p.time),
                p.duration.map_or("live".into(), clock),
                if p.muted { " [yellow]muted[/]" } else { "" }
            ),
        }
    })
    .auto();
    row([
        play,
        label(" ").fixed(1),
        mute,
        label(" Volume ").auto(),
        slider(volume, 0.0..=100.0, 10.0).fixed(16),
        status,
        label("").flex(1),
    ])
    .fixed(1)
    .name(&format!("player-{node}"))
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
                    // A hover-only control is not drawn: no label for it.
                    .filter(|(_, c)| !c.disabled && !c.hover_only)
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
    if b.view.get_untracked() == View::Reader
        && let Some(id) = b.web_control(&control).widget.get()
    {
        cx.focus(id);
        return;
    }
    let node = control.node;
    let title = markup(&crate::tab::describe(&control));
    let height = if control.role.is_text() {
        Size::Auto
    } else {
        Size::Fixed(3 + 2)
    };
    cx.modal(Size::Percent(70), height, move || {
        web_widget(b, vec![node])
            .on_key("esc", |cx| cx.pop())
            .panel(&title)
    });
}

/// What "send to agent" sends for control `index`: what it is, where it
/// goes, and the page it is on.
pub fn agent_text(page: &Page, index: usize) -> Option<String> {
    let control = page.controls.get(index)?;
    let mut text = format!(
        "[Noctiluca] {} {:?}",
        control.role.name(),
        crate::clean::clean(&control.name)
    );
    if let Some(href) = &control.href {
        text.push_str(&format!(" -> {}", crate::clean::clean(href)));
    }
    if let Some(value) = control.value.as_deref().filter(|v| !v.is_empty()) {
        text.push_str(&format!(" = {:?}", crate::clean::clean(value)));
    }
    text.push_str(&format!(
        "\non {} ({})\n",
        crate::clean::clean(&page.url),
        crate::clean::clean(&page.title)
    ));
    Some(text)
}

/// Send control `index`'s text and the page's address to where
/// `[agent] send` says: the clipboard, the pane beside this one, or a
/// pane by its id, through the terminal's API.
fn send_to_agent(b: Browser, index: usize, cx: &mut Ctx) {
    use crate::config::SendTo;
    let Some(text) = b
        .page
        .with_untracked(|p| p.as_ref().and_then(|p| agent_text(p, index)))
    else {
        return;
    };
    let to = b.config.with_untracked(|c| c.agent.send.clone());
    let pane = |host: &dyn rich_interact::host::TerminalHost| -> Result<rich_interact::host::Pane, String> {
        match &to {
            SendTo::Pane(id) => Ok(rich_interact::host::Pane::new(id.clone())),
            _ => [
                rich_interact::host::Direction::Right,
                rich_interact::host::Direction::Left,
                rich_interact::host::Direction::Down,
                rich_interact::host::Direction::Up,
            ]
            .into_iter()
            .find_map(|d| host.neighbor(d).ok().flatten())
            .ok_or_else(|| "no pane beside this one".to_string()),
        }
    };
    match to {
        SendTo::Clipboard => {
            cx.copy(text);
            b.say("[cyan]Sent to the clipboard[/] for the agent");
        }
        SendTo::Neighbor | SendTo::Pane(_) => {
            let sent = rich_interact::host::detect()
                .ok_or_else(|| "no terminal API here (tmux, WezTerm, kitty, a socket)".to_string())
                .and_then(|host| {
                    let pane = pane(host.as_ref())?;
                    host.send_text(&pane, &text)
                        .map(|()| pane.id.clone())
                        .map_err(|e| e.to_string())
                });
            match sent {
                Ok(id) => b.say(format!("[cyan]Sent to pane {}[/]", markup(&id))),
                Err(why) => {
                    cx.copy(text);
                    cx.toast(format!(
                        "[yellow]Could not send to a pane ({}): copied instead.[/]",
                        markup(&why)
                    ));
                }
            }
        }
    }
}

/// Answer the control socket: each request runs on the app's thread
/// ([`crate::control::handle`]), and its connection waits for the answer.
fn serve_control(b: Browser, server: rich_interact::control::ControlServer) {
    spawn_stream(move |stream| {
        let proxy = std::sync::Mutex::new(stream.proxy().clone());
        server.serve(move |request| {
            let (reply, answer) = mpsc::channel();
            let (verb, args) = (request.verb.clone(), request.args.clone());
            let posted = proxy
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .run_with(move |cx| {
                    let _ = reply.send(crate::control::handle(b, &verb, &args, cx));
                });
            if !posted {
                return Err("the browser is closing".into());
            }
            answer
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| Err("the browser did not answer in time".into()))
        });
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
            b.agent_pick.set(false);
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
                if b.agent_pick.get_untracked() {
                    b.agent_pick.set(false);
                    send_to_agent(b, index, cx);
                } else {
                    activate(b, index, cx);
                }
            } else if hints
                .labels
                .iter()
                .any(|(l, _)| l.starts_with(&hints.typed))
            {
                b.hints.set(Some(hints));
            } else {
                b.hints.set(None);
                b.agent_pick.set(false);
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

/// One row of a `chooser`.
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
        // A click on a row selects it; a click on the selected row chooses
        // it, as the first of `keys` (Enter) does. The wheel moves.
        let clicked = items.clone();
        let first_choice = keys.first().map(|(_, choose)| choose.clone());
        let list = list
            .on_click_at(move |cx, _, row| {
                let n = clicked().len();
                let at = selected.get_untracked().min(n.saturating_sub(1));
                let first = at.saturating_sub(room.saturating_sub(2));
                let index = first + usize::from(row);
                if index >= n {
                    return false;
                }
                if index == at {
                    if let Some(choose) = &first_choice {
                        choose(index, cx);
                    }
                } else {
                    selected.set(index);
                }
                true
            })
            .on_mouse({
                let count = items.clone();
                move |_, mouse| match mouse.kind {
                    MouseKind::ScrollUp => {
                        selected.update(|s| *s = s.saturating_sub(1));
                        true
                    }
                    MouseKind::ScrollDown => {
                        let n = count().len();
                        selected.update(|s| *s = (*s + 1).min(n.saturating_sub(1)));
                        true
                    }
                    _ => false,
                }
            });
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

/// The F1 overlay's rows: this site's keys first, marked as the site's,
/// then every action of the registry, each with its keys and, when it
/// cannot run now, why. It reads signals, so the overlay (intuiTUIve's
/// live `action_overlay`, C4) follows the page while open.
pub fn overlay_actions(b: Browser) -> Vec<Action> {
    let keymap = b.keymap.get();
    let mut out = Vec::new();
    if let Some(site) = b.site() {
        for (n, key) in site.keys.iter().enumerate() {
            out.push(Action {
                id: format!("site:{n}"),
                label: format!("{} [site {}]", key.target.describe(), site.pattern),
                keys: try_keys(&key.keys).unwrap_or_default(),
                unavailable: target_availability(b, &key.target).err(),
            });
        }
    }
    for action in ACTIONS {
        let keys = keymap
            .bindings()
            .into_iter()
            .find(|binding| binding.action == action.id)
            .map(|binding| binding.keys)
            .unwrap_or_default();
        // The media actions say what they would do now.
        let label = match (action.id, current_media(b)) {
            ("media.play_pause", Some((_, p))) if p.paused => "Play the media",
            ("media.play_pause", Some(_)) => "Pause the media",
            ("media.mute", Some((_, p))) if p.muted => "Unmute the media",
            ("media.mute", Some(_)) => "Mute the media",
            _ => action.label,
        };
        out.push(Action {
            id: action.id.to_string(),
            label: label.to_string(),
            keys,
            unavailable: availability(b, action.id).err(),
        });
    }
    out
}

fn overlay(b: Browser, cx: &mut Ctx) {
    cx.action_palette(
        move || overlay_actions(b),
        move |id, cx| match id
            .strip_prefix("site:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            Some(n) => {
                if let Some(target) = b
                    .site()
                    .and_then(|s| s.keys.get(n).map(|k| k.target.clone()))
                {
                    run_target(b, &target, cx);
                }
            }
            None => run(b, id, cx),
        },
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
        .filter(|v| *v != View::Pixels || crate::pixel_view::available(b))
    {
        b.view.set(view);
    }
}

/// Source CSS layout and terminal layout are separate. Narrow source flex
/// layouts can collapse a video to zero pixels before projection can reflow it.
/// Only the adapted projection requests a wider source; pixel/faithful views
/// retain their real viewport, and no physical cell metric is changed.
fn source_viewport(b: Browser) -> (u16, u16) {
    let (columns, rows) = b.viewport.get();
    let view = b.view.get();
    let reflow = view == View::Projection
        && !b.faithful.get()
        && b.adapt.get().on(crate::adapt::Rule::SnapGrids);
    let configured = b.page.with(|page| {
        page.as_ref()
            .and_then(|page| b.config.with(|c| c.site(&page.url).and_then(|s| s.width)))
    });
    let columns = if reflow {
        configured.unwrap_or(columns).max(80)
    } else if view == View::Pixels {
        columns
    } else {
        configured.unwrap_or(columns)
    };
    (columns, rows.saturating_sub(CHROME_ROWS))
}

fn reconcile_view_media(
    projection: bool,
    pacing: &mut Pacing,
    videos: &mut HashMap<NodeId, Live>,
    live: &mut HashMap<NodeId, Live>,
) {
    if !projection {
        pacing.clear();
        videos.clear();
        live.clear();
    }
}

/// Reconcile media state at a page transition.
fn reconcile_page_media(
    new_document: bool,
    nodes: &std::collections::HashSet<NodeId>,
    pacing: &mut Pacing,
    videos: &mut HashMap<NodeId, Live>,
    live: &mut HashMap<NodeId, Live>,
) {
    if new_document {
        pacing.clear();
        videos.clear();
        live.clear();
    } else {
        pacing.retain_regions(nodes);
        videos.retain(|node, _| nodes.contains(node));
        live.retain(|node, _| nodes.contains(node));
    }
}

/// Apply one of the tab's updates, on the app's thread.
fn apply(b: Browser, update: Update, cx: &mut Ctx) {
    if crate::diagnostics::enabled() {
        match &update {
            Update::Frame(f) => eprintln!(
                "RICH_MEDIA {{\"stage\":\"frame_apply\",\"seq\":{},\"bytes\":{}}}",
                f.seq,
                f.pixels.rgb.len()
            ),
            Update::Page(_) => eprintln!("RICH_MEDIA {{\"stage\":\"page_apply\"}}"),
            _ => {}
        }
    }
    match update {
        Update::Page(page) => {
            let new_document =
                b.page.with_untracked(|p| p.as_ref().map(|p| p.document)) != Some(page.document);
            let nodes = page.media.iter().map(|media| media.node).collect();
            prune_media_work(b, new_document, &nodes);
            let mut videos = (*b.videos.get_untracked()).clone();
            let mut live = (*b.live.get_untracked()).clone();
            b.pacing.with_untracked(|p| {
                reconcile_page_media(
                    new_document,
                    &nodes,
                    &mut p.borrow_mut(),
                    &mut videos,
                    &mut live,
                )
            });
            if *b.videos.get_untracked() != videos {
                b.videos.set(Rc::new(videos));
            }
            if *b.live.get_untracked() != live {
                b.live.set(Rc::new(live));
            }
            if new_document {
                crate::pixel_view::invalidate(b);
                b.controls.update(HashMap::clear);
                b.logs.update(HashMap::clear);
                b.scroll.set(0);
                b.hints.set(None);
                b.find_at.set(usize::MAX);
            }
            let url = page.url.clone();
            // A site whose config says `audio = false` keeps its media
            // muted (sound is a switch of the browser's launch, so a site
            // cannot turn it on for one run that started without it).
            if b.config
                .with_untracked(|c| c.site(&url).is_some_and(|s| s.audio == Some(false)))
            {
                for media in &page.media {
                    if media.playback.is_some_and(|p| !p.muted) {
                        b.send(Command::Media {
                            document: page.document,
                            node: media.node,
                            op: MediaOp::Mute(true),
                        });
                    }
                }
            }
            b.page.set(Some(page));
            b.reads.update(|n| *n += 1);
            b.refresh_controls();
            if new_document {
                apply_site(b, &url);
            }
        }
        Update::State(state) => b.state.set(state),
        Update::Metrics(metrics) => b.chrome_y.set(metrics.scroll_y),
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
        Update::Frame(frame) => {
            video_frame(b, &frame);
            if crate::diagnostics::enabled() {
                eprintln!(
                    "RICH_MEDIA {{\"stage\":\"frame_done\",\"seq\":{}}}",
                    frame.seq
                );
            }
        }
        Update::Screen(frame) => crate::pixel_view::frame(b, &frame),
        Update::Still(node) => {
            b.pacing.with_untracked(|p| p.borrow_mut().still(node));
            send_regions(b);
        }
        Update::Download { name, .. } => cx.toast(format!(
            "[yellow]A download, {}, waits in quarantine.[/]",
            markup(&name)
        )),
        Update::DownloadDone { .. } => {}
        Update::About { answers, product } => about(b, answers, product, cx),
    }
}

// One marker holds no image. It consumes only the initial bounded batch;
// newly arriving frames stay replaceable until the next UI turn.
fn forward_updates(
    b: Browser,
    receive: mpsc::Receiver<()>,
    updates: Arc<std::sync::Mutex<crate::delivery::Mailbox>>,
    proxy: rich_intuituive::Proxy,
) {
    while receive.recv().is_ok() {
        while !updates.lock().unwrap().queue.is_empty() {
            let inbox = updates.clone();
            let (done, wait) = mpsc::sync_channel(1);
            if !proxy.run_with(move |cx| {
                let count = inbox.lock().unwrap().queue.len().min(16);
                let started = Instant::now();
                for _ in 0..count {
                    let next = inbox.lock().unwrap().pop();
                    let Some((document, update)) = next else {
                        break;
                    };
                    let frame = matches!(update, Update::Frame(_) | Update::Screen(_));
                    if !frame
                        || b.page
                            .with_untracked(|p| p.as_ref().is_some_and(|p| p.document == document))
                    {
                        apply(b, update, cx);
                    }
                    if started.elapsed() >= Duration::from_millis(8) {
                        break;
                    }
                }
                let _ = done.send(());
            }) {
                return;
            }
            if wait.recv().is_err() {
                return;
            }
        }
    }
}

#[derive(Default)]
struct Prepared {
    waiting: std::collections::VecDeque<Job>,
    admitted: HashMap<NodeId, (u32, Option<u32>)>,
}

impl Prepared {
    fn admit(&mut self, job: &Job) {
        let id = job.picture.id();
        let entry = self.admitted.entry(job.node).or_insert((id, None));
        if entry.0 != id {
            *entry = (id, None);
            self.waiting.retain(|old| old.node != job.node);
        }
    }

    fn complete(&mut self, job: Job) -> bool {
        let Some((id, completed)) = self.admitted.get_mut(&job.node) else {
            return false;
        };
        if *id != job.picture.id()
            || completed.is_some_and(|version| version > job.picture.version())
        {
            return false;
        }
        // An executing frame can finish while a newer one waits. Requiring the
        // latest requested version here would starve a codec slower than offers.
        *completed = Some(job.picture.version());
        self.waiting.retain(|old| old.node != job.node);
        self.waiting.push_back(job);
        true
    }

    fn retain(&mut self, nodes: &std::collections::HashSet<NodeId>) {
        self.admitted.retain(|node, _| nodes.contains(node));
        self.waiting.retain(|job| nodes.contains(&job.node));
    }
}

fn prune_media_work(b: Browser, clear: bool, nodes: &std::collections::HashSet<NodeId>) {
    b.encoder.with_untracked(|encoder| {
        if clear {
            encoder.clear_waiting();
        } else {
            encoder.retain_waiting(nodes);
        }
    });
    b.prepared.with_untracked(|queue| {
        let mut queue = queue.lock().unwrap();
        if clear {
            *queue = Prepared::default();
        } else {
            queue.retain(nodes);
        }
    });
}

// Prepared pictures retain their original identity; show_frame rejects a
// completion invalidated by navigation, a view change or a newer picture.
fn forward_prepared(
    b: Browser,
    ready: mpsc::Receiver<()>,
    prepared: Arc<std::sync::Mutex<Prepared>>,
    proxy: rich_intuituive::Proxy,
) {
    while ready.recv().is_ok() {
        while !prepared.lock().unwrap().waiting.is_empty() {
            let inbox = prepared.clone();
            let (done, wait) = mpsc::sync_channel(1);
            if !proxy.run_with(move |_| {
                let next = inbox.lock().unwrap().waiting.pop_front();
                if let Some(job) = next {
                    show_frame(
                        b,
                        job.node,
                        Live {
                            picture: job.picture,
                            pixels: job.pixels,
                        },
                    );
                }
                let _ = done.send(());
            }) {
                return;
            }
            if wait.recv().is_err() {
                return;
            }
        }
    }
}

/// The key of a piece of reader mode, for `each`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PieceKey {
    Text(usize, u64),
    Media(NodeId, u16),
    Controls(Vec<NodeId>),
    Player(NodeId),
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
            Piece::Player(i) => Some(PieceKey::Player(page.media[*i].node)),
        })
        .collect()
}

/// The picture media region `node` shows now, and the protocol it is
/// drawn with: a moving picture's live frame, else its decoded picture.
fn shown_picture(b: Browser, node: NodeId) -> Option<(Picture, Pixels)> {
    if let Some(live) = b.live.with(|l| l.get(&node).cloned()) {
        return Some((b.presentation.picture(&live.picture), live.pixels));
    }
    let (picture, video) = b.page.with(|p| {
        let media = p.as_ref()?.media.iter().find(|m| m.node == node)?;
        Some((
            media.picture.clone()?,
            layout::play_mark(media, 3).is_some(),
        ))
    })?;
    let pixels = b.pixels.get();
    // A video not playing shows its poster with a play mark: in pixels,
    // drawn into them (cells over a Sixel or iTerm2 picture would be
    // covered); in cells, the mark is cells ([`play_marks`]).
    if video && pixels != Pixels::Cells {
        return Some((b.presentation.picture(&marked(&picture)), pixels));
    }
    Some((b.presentation.picture(&picture), pixels))
}

thread_local! {
    /// Posters with their play mark drawn in, by picture frame.
    static MARKED: std::cell::RefCell<HashMap<(u32, u32), Picture>> =
        std::cell::RefCell::new(HashMap::new());
}

/// `picture` with a play mark drawn into its pixels, made once.
fn marked(picture: &Picture) -> Picture {
    let key = (picture.id(), picture.version());
    MARKED.with(|m| {
        let mut made = m.borrow_mut();
        if made.len() > 64 {
            made.clear();
        }
        made.entry(key)
            .or_insert_with(|| Picture::new(rich_art::graphics::play_mark(picture.image())))
            .clone()
    })
}

/// The play mark over the bottom left of each video region on `row` that
/// is not playing, as the PoC drew it over the poster.
fn play_marks(
    line: Vec<rich::Segment>,
    width: usize,
    row: usize,
    regions: &[layout::Region],
    page: &Page,
) -> Vec<rich::Segment> {
    let mut line = line;
    for region in regions {
        let Some(media) = page.media.get(region.media) else {
            continue;
        };
        let last = region.row + region.rows.saturating_sub(1);
        if row != last || region.cols < 3 || region.column + 3 > width {
            continue;
        }
        let Some(mark) = layout::play_mark(media, region.cols) else {
            continue;
        };
        let style = rich::Style::parse(&mark.spans[0].style).ok();
        let mut pieces = rich::Segment::divide(&line, &[region.column, region.column + 3, width]);
        pieces.resize(3, Vec::new());
        let mut joined = std::mem::take(&mut pieces[0]);
        joined.push(rich::Segment::new(mark.spans[0].text.clone(), style));
        joined.extend(std::mem::take(&mut pieces[2]));
        line = joined;
    }
    line
}

fn reader_view(b: Browser) -> Node {
    let retained = RefCell::new(crate::presentation::Reading::default());
    // The pieces and their keys, made together so they always agree.
    let reading = memo(move || {
        let width = b.viewport.get().0;
        let marks = b.hint_marks();
        let images = b.images.get();
        b.page.with(|p| {
            p.as_ref()
                .map(|p| {
                    let pieces = retained
                        .borrow_mut()
                        .update(p, width.saturating_sub(2), &marks);
                    let keys = piece_keys(p, &pieces, images);
                    (pieces, keys)
                })
                .unwrap_or_default()
        })
    });
    let build = move |key: PieceKey| -> Node {
        match key {
            PieceKey::Text(i, _) => {
                let rows = reading.with_untracked(|(d, _)| d.get(i).map_or(1, Piece::rows)) as u16;
                renderable(move || {
                    let mut marks = Marks {
                        find: b.find.get(),
                        ..Marks::default()
                    };
                    reading.with(|(d, _)| {
                        let start: usize = d.iter().take(i).map(Piece::rows).sum();
                        marks.current = b.found.with(|f| f.get(b.find_at.get()).copied());
                        match d.get(i) {
                            Some(Piece::Text(lines)) => to_text(lines, &marks, start),
                            _ => rich::Text::new(""),
                        }
                    })
                })
                // A link in the text is used as its hint would use it.
                .on_click_at(move |cx, column, row| {
                    let control = reading.with_untracked(|(d, _)| match d.get(i) {
                        Some(Piece::Text(lines)) => lines
                            .get(usize::from(row))
                            .and_then(|line| layout::control_at(line, usize::from(column))),
                        _ => None,
                    });
                    control.is_some_and(|index| {
                        click_control(b, index, cx);
                        true
                    })
                })
                .fixed(rows)
            }
            PieceKey::Media(node, rows)
                if b.page.with_untracked(|p| {
                    p.as_ref().is_some_and(|p| {
                        p.media
                            .iter()
                            .any(|m| m.node == node && m.kind == MediaKind::Video)
                    })
                }) =>
            {
                let width = b.page.with_untracked(|p| {
                    p.as_ref()
                        .and_then(|p| p.media.iter().find(|m| m.node == node))
                        .map_or(10, |m| {
                            ((m.rect.width / CELL.0 as f64).ceil() as u16).clamp(4, 120)
                        })
                });
                row([video_region(b, node).fixed(width), label("").flex(1)]).fixed(rows)
            }
            PieceKey::Media(node, rows) => {
                let width = b.page.with_untracked(|p| {
                    p.as_ref()
                        .and_then(|p| p.media.iter().find(|m| m.node == node))
                        .map_or(10, |m| {
                            ((m.rect.width / CELL.0 as f64).ceil() as u16).clamp(4, 120)
                        })
                });
                let view = rich_intuituive::ImageView::new().pixels(b.pixels.get_untracked());
                let picture = picture_of(move || shown_picture(b, node).map(|(p, _)| p), view)
                    // A click on a picture, a canvas or a frame reaches the
                    // page where the pointer was in it (#819).
                    .on_mouse(move |cx, mouse| {
                        mouse.is_click()
                            && click_media_at(
                                b,
                                node,
                                mouse.precise(),
                                (width.into(), rows.into()),
                                cx,
                            )
                    });
                row([picture.fixed(width), label("").flex(1)]).fixed(rows)
            }
            PieceKey::Controls(nodes) => web_widget(b, nodes),
            PieceKey::Player(node) => player(b, node),
        }
    };
    // Each piece an item, built only near the view, keyed by what it
    // shows: a piece that stays keeps its node across readings.
    let key_of = |key: &PieceKey| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        h.finish()
    };
    // A piece's rows: text and media are as tall as they say; controls
    // and players are measured once drawn.
    let rows_of = move |i: usize| -> u16 {
        reading.with_untracked(|(d, k)| match k.get(i) {
            Some(PieceKey::Text(at, _)) => d.get(*at).map_or(1, Piece::rows) as u16,
            Some(PieceKey::Media(_, rows)) => *rows,
            _ => 1,
        })
    };
    scroll_virtual_with(
        move || reading.with(|(_, k)| k.len()),
        rows_of,
        move |i| match reading.with_untracked(|(_, k)| k.get(i).cloned()) {
            Some(key) => build(key),
            None => label(""),
        },
        b.scroll,
    )
    .item_key(move |i| reading.with_untracked(|(_, k)| k.get(i).map_or(i as u64, key_of)))
    .on_near_end(near_end(b), move |_| load_more(b))
    .autofocus()
    .name("page")
}

/// How near the end, in rows, the view asks the page for more: a screen.
fn near_end(b: Browser) -> u64 {
    b.viewport.get_untracked().1.max(1) as u64
}

/// Near the end of what was read: scroll Chrome to its bottom, so an
/// infinite feed loads more. The page is read again on its change ping;
/// the view asks again only once it has more rows. Not for a page that
/// fits in two screens, whose every row is near its end: it is not a feed
/// waiting to grow (one that is sees its end in Chrome's viewport
/// already), and scrolled, its sticky and fixed boxes would be read where
/// they are stuck.
fn load_more(b: Browser) {
    let rows = b.viewport.get_untracked().1.saturating_sub(CHROME_ROWS);
    let screen = f64::from(rows) * f64::from(CELL.1);
    let long = b
        .page
        .with_untracked(|p| p.as_ref().is_some_and(|p| p.height > 2.0 * screen));
    if long {
        b.send(Command::LoadMore);
    }
}

/// A click at `column` of the projection's row `row`: the control there
/// is used as its link hint would use it ([`activate`]), and a video region
/// there plays or pauses. A click on anything else is not used, so a drag
/// from it selects text.
fn click_row(b: Browser, row: usize, column: usize, cx: &mut Ctx) -> bool {
    let (control, video) = b.layout.with_untracked(|l| {
        let control = l
            .lines
            .get(row)
            .and_then(|line| layout::control_at(line, column));
        let video = l
            .regions
            .iter()
            .find(|r| r.on(row) && (r.column..r.column + r.cols).contains(&column))
            .map(|r| r.media);
        (control, video)
    });
    if let Some(index) = control {
        click_control(b, index, cx);
        return true;
    }
    let video = video.and_then(|media| {
        b.page.with_untracked(|p| {
            let m = p.as_ref()?.media.get(media)?;
            (m.kind == MediaKind::Video).then_some((m.node, m.playback))
        })
    });
    match video {
        Some((node, playback)) => {
            let paused = playback.is_none_or(|p| p.paused || p.ended);
            media_op(
                b,
                node,
                if paused {
                    MediaOp::Play
                } else {
                    MediaOp::Pause
                },
            );
            true
        }
        None => false,
    }
}

/// A click at `at` (fractional cells from the drawing's top left) of the
/// media box `node` drawn in `cells` (columns, rows): a picture, a canvas,
/// an SVG drawing or a frame is clicked in the page at that point, to the
/// pixel when the terminal reports the mouse in pixels (#819), else at the
/// cell's centre. Whether it was one of those.
fn click_media_at(
    b: Browser,
    node: NodeId,
    at: (f32, f32),
    cells: (usize, usize),
    _cx: &mut Ctx,
) -> bool {
    let target = b.page.with_untracked(|p| {
        let page = p.as_ref()?;
        let media = page.media.iter().find(|m| m.node == node)?;
        let pointable = matches!(
            media.kind,
            MediaKind::Image | MediaKind::Canvas | MediaKind::Svg | MediaKind::Frame
        );
        pointable.then(|| (page.document, media.rect.width, media.rect.height))
    });
    let Some((document, width, height)) = target else {
        return false;
    };
    let (x, y) = layout::point_in(width, height, cells.0, cells.1, at);
    let id = b.send(Command::ClickAt {
        document,
        node,
        x,
        y,
    });
    b.after(id, |result, cx| {
        if let Err(refusal) = result {
            cx.toast(format!("[red]{}[/]", markup(&refusal.to_string())));
        }
    });
    true
}

/// A click on the page's control `index`: what a link hint does.
fn click_control(b: Browser, index: usize, cx: &mut Ctx) {
    b.hints.set(None);
    b.message.set(String::new());
    activate(b, index, cx);
}

/// The page's own colours as a backdrop for its rows, when the page
/// colours rule is off: transform the source background together with its
/// foreground. Where no source background is set, use the configured terminal
/// fallback, or the terminal's own surface with `--transparent`.
fn backdrop(b: Browser) -> Option<String> {
    if b.effective_adapt().on(crate::adapt::Rule::PageColors) {
        return None;
    }
    let transparent = b.config.with(|c| c.transparent);
    let page = b
        .page
        .with(|p| p.as_ref().and_then(|p| p.background.clone()));
    crate::adapt::page_backdrop(page.as_deref(), transparent, crate::adapt::background())
}

/// Row `row` of the projection `l` (the page, or one of its strips) drawn
/// `width` cells wide: its text, the pictures over it spliced in, the short
/// texts over a picture drawn over it again, and the play marks.
fn draw_row(
    b: Browser,
    console: &rich::Console,
    cells: &pictures::Cells,
    l: &Projection,
    row: usize,
    width: usize,
    marks: &Marks,
) -> Vec<rich::Segment> {
    let Some(line) = l.lines.get(row) else {
        return Vec::new();
    };
    let render = |line: &Line, marks: &Marks| {
        let text = to_text(std::slice::from_ref(line), marks, row);
        let options = console.options().update_width(width);
        console
            .render_lines(&text, &options, false)
            .into_iter()
            .next()
            .unwrap_or_default()
    };
    let segments = render(line, marks);
    // Only a row a picture covers reads the pictures.
    let inline: Vec<&(layout::Region, String)> =
        l.inline.iter().filter(|(r, _)| r.on(row)).collect();
    if b.images.get() == Images::None || (inline.is_empty() && !l.regions.iter().any(|r| r.on(row)))
    {
        return segments;
    }
    let nodes: Vec<Option<NodeId>> = b.page.with(|p| {
        l.regions
            .iter()
            .map(|r| p.as_ref()?.media.get(r.media).map(|m| m.node))
            .collect()
    });
    // The media regions, then the small pictures in the row's lines, each
    // with its picture.
    let mut on_row: Vec<layout::Region> = l.regions.iter().filter(|r| r.on(row)).copied().collect();
    on_row.extend(inline.iter().map(|(r, _)| *r));
    let pixels = b.pixels.get();
    let small = |region: &layout::Region| -> Option<(Picture, Pixels)> {
        let (_, src) = inline.iter().find(|(r, _)| r == region)?;
        let picture = b
            .page
            .with(|p| p.as_ref()?.inline_pictures.get(src).cloned())?;
        Some((picture, pixels))
    };
    let metrics = rich_intuituive::cell_pixels().get();
    let mut spliced = pictures::splice_with_metrics(
        console,
        cells,
        segments,
        width,
        row,
        &on_row,
        |region| {
            if region.media == usize::MAX {
                return small(region);
            }
            let at = l.regions.iter().position(|r| r == region)?;
            shown_picture(b, nodes.get(at).copied().flatten()?)
        },
        metrics,
    );
    // A badge over a picture stays on it: its cells are text again.
    for (_, column, over) in l.overlays.iter().filter(|(r, _, _)| *r == row) {
        let drawn = render(over, &Marks::default());
        let end = (column + over.width()).min(width);
        if *column >= end {
            continue;
        }
        let mut pieces = rich::Segment::divide(&spliced, &[*column, end, width]);
        pieces.resize(3, Vec::new());
        let mut joined = std::mem::take(&mut pieces[0]);
        joined.extend(rich::Segment::adjust_line_length(
            &drawn,
            end - column,
            None,
        ));
        joined.extend(std::mem::take(&mut pieces[2]));
        spliced = joined;
    }
    // The play mark as cells only where pictures are cells: in pixels it
    // is drawn into the poster ([`marked`]).
    b.page.with(|p| match p {
        Some(page) if pixels == Pixels::Cells => play_marks(spliced, width, row, &l.regions, page),
        _ => spliced,
    })
}

/// The most rows a pinned or docked strip takes: a quarter of the screen.
fn strip_rows(b: Browser) -> usize {
    usize::from(b.viewport.get().1 / 4)
}

/// The page's pinned strip (its top bars, `docked` false) or its docked
/// strip (its bottom bars: cookie banners, sign-up bars): drawn above or
/// under the page, never scrolled, at most [`strip_rows`] tall.
fn strip_view(b: Browser, docked: bool) -> Node {
    fn strip(l: &Projection, docked: bool) -> Option<&Projection> {
        if docked {
            l.docked.as_deref()
        } else {
            l.pinned.as_deref()
        }
    }
    let cells = Rc::new(pictures::Cells::with_presentation(b.presentation));
    let rows = move || {
        b.layout
            .with(|l| strip(l, docked).map_or(0, |s| s.lines.len()))
            .min(strip_rows(b))
    };
    leaf(move |console, width, _| {
        let width = usize::from(width.max(1));
        let (rows, backdrop) = (rows(), backdrop(b));
        b.layout.with(|l| {
            let Some(s) = strip(l, docked) else {
                return Vec::new();
            };
            (0..rows)
                .map(|row| {
                    let line = draw_row(b, console, &cells, s, row, width, &Marks::default());
                    match &backdrop {
                        Some(backdrop) => pictures::on_backdrop(line, width, backdrop),
                        None => line,
                    }
                })
                .collect()
        })
    })
    .size_with(move || Size::Fixed(rows() as u16))
    .on_click_at(move |cx, column, row| {
        let control = b.layout.with_untracked(|l| {
            let s = strip(l, docked)?;
            layout::control_at(s.lines.get(usize::from(row))?, usize::from(column))
        });
        match control {
            Some(index) => {
                click_control(b, index, cx);
                true
            }
            None => false,
        }
    })
    .name(if docked { "docked" } else { "pinned" })
}

fn segment_columns(segments: Vec<rich::Segment>, start: usize, width: usize) -> Vec<rich::Segment> {
    let mut out = Vec::new();
    let mut at = 0usize;
    let end = start.saturating_add(width);
    for segment in segments {
        let next = at.saturating_add(segment.cell_length());
        if next > start && at < end {
            let (_, right) = segment.split_cells(start.saturating_sub(at));
            let (part, _) = right.split_cells(end.min(next).saturating_sub(start.max(at)));
            out.push(part);
        }
        at = next;
        if at >= end {
            break;
        }
    }
    out
}

// Source-controlled fixed dialogs compose over the page without changing
// the source's dismiss behavior or inferring modality from geometry.
// Paint source blocks above a fixed surface without moving or removing their
// underlay model. Their visible rectangle and pointer target share one node.
fn source_foreground(b: Browser, nodes: Vec<NodeId>, bounds: (u16, u16, u16, u16)) -> Node {
    keyed(
        move || {
            let (columns, height) = b.viewport.get();
            let rows = height.saturating_sub(CHROME_ROWS).max(1);
            let scroll = b.scroll.get() as usize;
            let y = if scroll == 0 {
                0.0
            } else {
                b.layout.with(|layout| layout.y_of(scroll))
            };
            b.page.with(|page| {
                page.as_ref()
                    .map(|page| {
                        nodes
                            .iter()
                            .filter_map(|node| {
                                let block = page.blocks.iter().find(|block| block.node == *node)?;
                                let x = (block.rect.x / f64::from(CELL.0)).floor() as i32;
                                let y = ((block.rect.y - y) / f64::from(CELL.1)).floor() as i32;
                                let width =
                                    (block.rect.width / f64::from(CELL.0)).ceil().max(1.0) as u16;
                                let height =
                                    (block.rect.height / f64::from(CELL.1)).ceil().max(1.0) as u16;
                                let (bound_left, bound_top, bound_right, bound_bottom) = bounds;
                                let left = x.max(i32::from(bound_left)).max(0);
                                let top = y.max(i32::from(bound_top)).max(0);
                                let right = (x + i32::from(width))
                                    .min(i32::from(columns.saturating_sub(bound_right)));
                                let bottom = (y + i32::from(height))
                                    .min(i32::from(rows.saturating_sub(bound_bottom)));
                                if right <= left || bottom <= top {
                                    return None;
                                }
                                Some((
                                    *node,
                                    left as u16,
                                    top as u16,
                                    columns - right as u16,
                                    rows - bottom as u16,
                                    width,
                                    (left - x) as usize,
                                    (top - y) as usize,
                                ))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
        },
        move |specs| {
            layers(specs.into_iter().map(
                move |(node, left, top, right, bottom, full_width, skip_x, skip)| {
                    let projection = memo(move || {
                        b.page.with(|page| {
                            page.as_ref()
                                .map(|page| {
                                    layout::project_foreground(
                                        page,
                                        node,
                                        full_width,
                                        (CELL.0 as f64, CELL.1 as f64),
                                        &b.hint_marks(),
                                    )
                                })
                                .unwrap_or_default()
                        })
                    });
                    let cells = Rc::new(pictures::Cells::with_presentation(b.presentation));
                    leaf(move |console, width, height| {
                        projection.with(|p| {
                            (skip..skip + usize::from(height))
                                .map(|i| {
                                    if i < p.lines.len() {
                                        let line = draw_row(
                                            b,
                                            console,
                                            &cells,
                                            p,
                                            i,
                                            usize::from(full_width),
                                            &Marks::default(),
                                        );
                                        segment_columns(line, skip_x, usize::from(width))
                                    } else {
                                        Vec::new()
                                    }
                                })
                                .collect()
                        })
                    })
                    .on_click_at(move |cx, column, row| {
                        if let Some(index) = projection.with_untracked(|p| {
                            p.lines.get(skip + usize::from(row)).and_then(|line| {
                                layout::control_at(line, skip_x + usize::from(column))
                            })
                        }) {
                            click_control(b, index, cx);
                        }
                        true
                    })
                    .inset(top, right, bottom, left)
                },
            ))
            .children_cover_only()
        },
    )
    .children_cover_only()
}

// Adapted text uses the terminal's foreground/surface pair. Faithful text
// and the source surface share exactly the same documented colour transform.
fn popup_fill(adapt: Adapt, source: &str, background: crate::adapt::Background) -> String {
    if adapt.on(crate::adapt::Rule::PageColors) {
        String::new()
    } else {
        crate::adapt::page_backdrop(Some(source), true, background).unwrap_or_default()
    }
}

fn source_position(b: Browser) -> f64 {
    let scroll = b.scroll.get() as usize;
    if scroll == 0 {
        0.0
    } else {
        b.layout.with(|layout| layout.y_of(scroll))
    }
}

fn document_surface(
    layer: &crate::model::Layer,
    source_y: f64,
    rows: u16,
) -> Option<(u16, u16, u64)> {
    let at = ((layer.top - source_y) / f64::from(CELL.1)).round() as i64;
    let full_height = (layer.rect.height / f64::from(CELL.1)).ceil().max(1.0) as i64;
    if at >= i64::from(rows) || at + full_height <= 0 {
        return None;
    }
    let skip = at.min(0).unsigned_abs();
    let height = (full_height - skip as i64).min(i64::from(rows) - at.max(0)) as u16;
    Some((at.max(0) as u16, height, skip))
}

fn visible_source_dialog(b: Browser) -> bool {
    let rows = b.viewport.get().1.saturating_sub(CHROME_ROWS).max(1);
    let source_y = source_position(b);
    b.page.with(|page| {
        page.as_ref().is_some_and(|page| {
            page.dialogs().any(|dialog| {
                page.layers.get(dialog.layer).is_some_and(|layer| {
                    layer.fixed || document_surface(layer, source_y, rows).is_some()
                })
            })
        })
    })
}

fn source_dialogs(b: Browser) -> Node {
    type Spec = (
        NodeId,
        u16,
        u16,
        u16,
        u16,
        bool,
        bool,
        String,
        Option<String>,
        Vec<NodeId>,
        u64,
    );
    keyed(
        move || {
            let (columns, height) = b.viewport.get();
            let rows = height.saturating_sub(CHROME_ROWS).max(1);
            let source_y = source_position(b);
            b.page.with(|page| {
                page.as_ref()
                    .map(|page| {
                        page.overlays
                            .iter()
                            .filter_map(|dialog| {
                                let layer = page.layers.get(dialog.layer)?;
                                // Fit first, then keep as much of the source origin as
                                // remains available in the actual terminal container.
                                let width = ((layer.rect.width / f64::from(CELL.0)).ceil().max(1.0)
                                    as u16)
                                    .min(columns.max(1));
                                let mut height =
                                    ((layer.rect.height / f64::from(CELL.1)).ceil().max(1.0)
                                        as u16)
                                        .min(rows);
                                let left = ((layer.rect.x / f64::from(CELL.0)).round().max(0.0)
                                    as u16)
                                    .min(columns.saturating_sub(width));
                                let desired_top = if layer.place == crate::model::Place::Docked
                                    && page.view_height > 0.0
                                {
                                    let from_bottom = ((page.view_height - layer.top).max(0.0)
                                        / f64::from(CELL.1))
                                    .round()
                                        as u16;
                                    rows.saturating_sub(from_bottom)
                                } else {
                                    (layer.top / f64::from(CELL.1)).round().max(0.0) as u16
                                };
                                let (top, skip) = if layer.fixed {
                                    (desired_top.min(rows.saturating_sub(height)), 0)
                                } else {
                                    let (top, visible_height, skip) =
                                        document_surface(layer, source_y, rows)?;
                                    height = visible_height;
                                    (top, skip)
                                };
                                Some((
                                    dialog.node,
                                    left,
                                    top,
                                    columns.saturating_sub(left + width),
                                    rows.saturating_sub(top + height),
                                    dialog.dialog,
                                    dialog.modal,
                                    dialog.name.clone(),
                                    dialog.background.clone(),
                                    dialog.foreground.clone(),
                                    skip,
                                ))
                            })
                            .collect::<Vec<Spec>>()
                    })
                    .unwrap_or_default()
            })
        },
        move |specs| {
            layers(specs.into_iter().map(
                move |(
                    node,
                    left,
                    top,
                    right,
                    bottom,
                    is_dialog,
                    modal,
                    name,
                    background,
                    foreground,
                    skip,
                )| {
                    let width = b
                        .viewport
                        .get_untracked()
                        .0
                        .saturating_sub(left + right)
                        .max(1);
                    let projection = memo(move || {
                        b.page.with(|page| {
                            page.as_ref()
                                .and_then(|page| {
                                    let index = page
                                        .overlays
                                        .iter()
                                        .position(|dialog| dialog.node == node)?;
                                    layout::project_overlay(
                                        page,
                                        index,
                                        width,
                                        (CELL.0 as f64, CELL.1 as f64),
                                        &b.hint_marks(),
                                    )
                                })
                                .unwrap_or_default()
                        })
                    });
                    let offset = signal(skip);
                    let cells = Rc::new(pictures::Cells::with_presentation(b.presentation));
                    let rows = scroll_virtual_with(
                        move || projection.with(|p| p.lines.len()),
                        |_| 1,
                        move |i| {
                            let cells = cells.clone();
                            let zone_cells = cells.clone();
                            let line = leaf(move |console, width, _| {
                                projection.with(|p| {
                                    if i >= p.lines.len() {
                                        return vec![Vec::new()];
                                    }
                                    vec![draw_row(
                                        b,
                                        console,
                                        &cells,
                                        p,
                                        i,
                                        usize::from(width.max(1)),
                                        &b.marks(),
                                    )]
                                })
                            })
                            .on_click_at(move |cx, column, _| {
                                if let Some(index) = projection.with_untracked(|p| {
                                    p.lines.get(i).and_then(|line| {
                                        layout::control_at(line, usize::from(column))
                                    })
                                }) {
                                    click_control(b, index, cx);
                                }
                                true
                            });
                            let targets = keyed(
                                move || {
                                    projection.with(|p| {
                                        b.page.with(|page| {
                                            let Some(line) = p.lines.get(i) else {
                                                return Vec::new();
                                            };
                                            let Some(page) = page.as_ref() else {
                                                return Vec::new();
                                            };
                                            let mut zones = Vec::new();
                                            let mut x = 0usize;
                                            while x < usize::from(width) {
                                                let Some(index) = layout::control_at(line, x)
                                                else {
                                                    x += 1;
                                                    continue;
                                                };
                                                let start = x;
                                                while x < usize::from(width)
                                                    && layout::control_at(line, x) == Some(index)
                                                {
                                                    x += 1;
                                                }
                                                if let Some(control) = page.controls.get(index) {
                                                    zones.push((
                                                        index,
                                                        control.node,
                                                        start as u16,
                                                        x as u16,
                                                        control.name.clone(),
                                                        control.disabled,
                                                        control.role.clone(),
                                                    ));
                                                }
                                            }
                                            zones
                                        })
                                    })
                                },
                                move |zones| {
                                    let zone_cells = zone_cells.clone();
                                    layers(zones.into_iter().map(
                                        move |(
                                            index,
                                            target,
                                            left,
                                            right,
                                            name,
                                            disabled,
                                            role,
                                        )| {
                                            use crate::model::Role as SourceRole;
                                            use rich_intuituive::a11y::Role as AccessRole;
                                            let role = match role {
                                                SourceRole::TextBox
                                                | SourceRole::SearchBox
                                                | SourceRole::Password => AccessRole::TextBox,
                                                SourceRole::CheckBox => AccessRole::CheckBox,
                                                SourceRole::Switch => AccessRole::Switch,
                                                SourceRole::Radio => AccessRole::RadioGroup,
                                                SourceRole::ComboBox | SourceRole::ListBox => {
                                                    AccessRole::ComboBox
                                                }
                                                SourceRole::Slider | SourceRole::SpinButton => {
                                                    AccessRole::Slider
                                                }
                                                SourceRole::Tab => AccessRole::Tab,
                                                SourceRole::MenuItem => AccessRole::MenuItem,
                                                _ => AccessRole::Button,
                                            };
                                            let cells = zone_cells.clone();
                                            leaf(move |console, _, _| {
                                                projection.with(|p| {
                                                    if i >= p.lines.len() {
                                                        return vec![Vec::new()];
                                                    }
                                                    let line = draw_row(
                                                        b,
                                                        console,
                                                        &cells,
                                                        p,
                                                        i,
                                                        usize::from(width),
                                                        &b.marks(),
                                                    );
                                                    vec![segment_columns(
                                                        line,
                                                        usize::from(left),
                                                        usize::from(right.saturating_sub(left)),
                                                    )]
                                                })
                                            })
                                            .focusable()
                                            .focus_style("reverse")
                                            .name(&format!("popup-control-{target}"))
                                            .label(name)
                                            .role(role)
                                            .disabled_when(move || disabled)
                                            .on_key("enter", move |cx| click_control(b, index, cx))
                                            .on_click(move |cx| click_control(b, index, cx))
                                            .inset(
                                                0,
                                                width.saturating_sub(right),
                                                0,
                                                left,
                                            )
                                        },
                                    ))
                                    .children_cover_only()
                                },
                            )
                            .children_cover_only();
                            layers([line, targets]).fixed(1)
                        },
                        offset,
                    );
                    let mut children = Vec::new();
                    if let Some(background) = background {
                        let style = popup_fill(
                            b.effective_adapt(),
                            &background,
                            crate::adapt::background(),
                        );
                        children.push(fill(" ", &style).on_click(|_| {}));
                    }
                    children.push(rows);
                    let popup = layers(children)
                        .label(&name)
                        .inset(top, right, bottom, left);
                    let popup = if is_dialog {
                        popup.role(rich_intuituive::a11y::Role::Dialog)
                    } else {
                        popup
                    };
                    let popup = if modal {
                        popup.controlled_modal()
                    } else {
                        popup
                    };
                    layers([
                        popup,
                        source_foreground(b, foreground, (left, top, right, bottom)),
                    ])
                    .children_cover_only()
                },
            ))
            .children_cover_only()
        },
    )
    .children_cover_only()
}

fn projection_view(b: Browser) -> Node {
    // The pictures' cells, made once per frame and size, shared by rows.
    let cells = Rc::new(pictures::Cells::with_presentation(b.presentation));
    // One item per row of the shared layout, built only near the view and
    // keyed by the row's node, so the row in view stays when the page
    // grows above it.
    let line = move |i: usize| {
        // The row this item is: by its key, as rows move when the page
        // grows above them.
        let key = b
            .layout
            .with_untracked(|l| l.keys.get(i).copied().unwrap_or(i as u64));
        let cells = cells.clone();
        leaf(move |console, width, _| {
            let marks = Marks {
                find: b.find.get(),
                current: b.found.with(|f| f.get(b.find_at.get()).copied()),
                ..Marks::default()
            };
            let width = usize::from(width.max(1));
            let backdrop = backdrop(b);
            let drawn = b.layout.with(|l| {
                let row = l.rows.get(&key).copied().unwrap_or(i);
                if row >= l.lines.len() {
                    return vec![Vec::new()];
                }
                vec![draw_row(b, console, &cells, l, row, width, &marks)]
            });
            match backdrop {
                Some(backdrop) => drawn
                    .into_iter()
                    .map(|line| pictures::on_backdrop(line, width, &backdrop))
                    .collect(),
                None => drawn,
            }
        })
        // A click on a picture, a canvas or a frame, not on a control, is
        // the page's at that point of it (#819).
        .on_mouse(move |cx, mouse| {
            if !mouse.is_click() {
                return false;
            }
            let (x, y) = mouse.precise();
            let column = usize::from(mouse.column);
            let target = b.layout.with_untracked(|l| {
                let row = l.rows.get(&key).copied().unwrap_or(i);
                let line = l.lines.get(row)?;
                if layout::control_at(line, column).is_some() {
                    return None;
                }
                let region = l
                    .regions
                    .iter()
                    .find(|r| r.on(row) && (r.column..r.column + r.cols).contains(&column))?;
                let at = (
                    x - region.column as f32,
                    (row - region.row) as f32 + (y - f32::from(mouse.row)),
                );
                Some((region.media, at, (region.cols, region.rows)))
            });
            let Some((media, at, cells)) = target else {
                return false;
            };
            let node = b
                .page
                .with_untracked(|p| p.as_ref()?.media.get(media).map(|m| m.node));
            node.is_some_and(|node| click_media_at(b, node, at, cells, cx))
        })
        .on_click_at(move |cx, column, _| {
            let row = b
                .layout
                .with_untracked(|l| l.rows.get(&key).copied().unwrap_or(i));
            click_row(b, row, usize::from(column), cx)
        })
        .fixed(1)
    };
    let page = scroll_virtual_with(
        move || b.layout.with(|l| l.lines.len()),
        |_| 1,
        line,
        b.scroll,
    )
    .item_key(move |i| {
        b.layout
            .with_untracked(|l| l.keys.get(i).copied().unwrap_or(i as u64))
    })
    .on_near_end(near_end(b), move |_| load_more(b))
    .autofocus()
    .name("page")
    .flex(1);
    layers([
        column([strip_view(b, false), page, strip_view(b, true)]),
        source_dialogs(b),
    ])
}

/// The rows the header and the status bar take.
pub const CHROME_ROWS: u16 = 4;

/// The byte budget moving pictures (GIFs and videos) share: the config's
/// `bandwidth`, else rs-rich-art's guess moved to the terminal's measured
/// output rate ([`OutputRate`]): lowered to it always, and on this machine
/// raised to it too, up to [`LOCAL_CEILING`] (a terminal here that takes
/// frames fast gets them; over SSH what was written may wait in a buffer
/// on the way, so the guess is only ever lowered). At most the config's
/// frame rate and never more than [`MAX_FPS`](crate::video::MAX_FPS) a
/// second. With the bandwidth rule off (the faithful projection), no
/// budget: every frame, never a step down.
pub fn byte_budget(b: Browser) -> rich_art::picture::ByteBudget {
    let adapt = b.effective_adapt();
    let measured = OUTPUT.with(|o| o.borrow_mut().rate());
    b.config.with_untracked(|c| {
        let mut budget = c.bandwidth.map_or_else(
            || {
                let mut guess = rich_art::picture::ByteBudget::guess();
                if let Some(rate) = measured {
                    guess.bytes_per_second = measured_budget(
                        guess.bytes_per_second,
                        rate,
                        rich_art::picture::over_ssh(),
                    );
                }
                guess
            },
            rich_art::picture::ByteBudget::new,
        );
        budget.max_fps = f64::from(c.fps).clamp(1.0, crate::video::MAX_FPS as f64);
        if !adapt.on(crate::adapt::Rule::Bandwidth) {
            budget.bytes_per_second = u64::MAX / 4;
            budget.min_fps = 0.0;
        }
        budget
    })
}

/// The most bytes a second the measured output rate raises the budget to
/// on this machine.
pub const LOCAL_CEILING: u64 = 32 << 20;

/// The budget from the `guess` and the `measured` output rate: the lower
/// of the two over SSH, the measured one here (at most [`LOCAL_CEILING`]).
pub fn measured_budget(guess: u64, measured: u64, over_ssh: bool) -> u64 {
    if over_ssh {
        guess.min(measured)
    } else {
        measured.min(LOCAL_CEILING.max(guess))
    }
}

thread_local! {
    /// The painter's counters, for the terminal's measured output rate.
    static OUTPUT: std::cell::RefCell<OutputRate> = std::cell::RefCell::default();
}

/// How fast the terminal takes output, from the painter's [`FrameStats`]
/// (`rich_intuituive::FrameStats`): the bytes written over the time the
/// writes took, once enough was written to tell. A terminal that keeps up
/// reads fast, so the rate only ever lowers the guessed budget.
#[derive(Default)]
pub struct OutputRate {
    stats: Option<rich_intuituive::StatsHandle>,
    last: (u64, Duration),
    rate: Option<f64>,
}

/// Written since the last sample before a rate is taken from it.
const RATE_SAMPLE: u64 = 64 * 1024;

impl OutputRate {
    /// Fold in the counters: `total_bytes` written in `total_write`.
    pub fn observe(&mut self, total_bytes: u64, total_write: Duration) {
        let bytes = total_bytes.saturating_sub(self.last.0);
        let write = total_write.saturating_sub(self.last.1);
        if bytes < RATE_SAMPLE {
            return;
        }
        self.last = (total_bytes, total_write);
        if write.is_zero() {
            return;
        }
        let rate = bytes as f64 / write.as_secs_f64();
        self.rate = Some(self.rate.map_or(rate, |before| before * 0.7 + rate * 0.3));
    }

    /// Bytes a second the terminal took, measured; `None` until known.
    pub fn rate(&mut self) -> Option<u64> {
        if let Some(stats) = self.stats.as_ref().map(rich_intuituive::StatsHandle::get) {
            self.observe(stats.total_bytes, stats.total_write);
        }
        self.rate.map(|r| r as u64)
    }
}

/// The frames' pacing: within the guessed byte budget, at most the
/// config's frame rate and never more than thirty a second, starting at
/// the protocol the user asked for, else character art over SSH, else
/// what the terminal speaks. [`step_motion`] replaces the budget with
/// [`byte_budget`]'s on its first look.
fn video_pacing(
    config: &Config,
    images: Images,
    presentation: rich_intuituive::ImagePresentation,
) -> Pacing {
    let mut budget = rich_art::picture::ByteBudget::guess();
    budget.max_fps = (config.fps as f64).clamp(1.0, crate::video::MAX_FPS as f64);
    let asked = (images != Images::Auto).then(|| pictures::pixels(images));
    Pacing::with_formats(
        budget,
        Pacing::start_for(
            asked.map(|p| presentation.with_pixels(p).pixels()),
            presentation.pixels(),
        ),
        presentation.formats(),
    )
}

/// How often the app looks at `--ssh`'s tunnel.
const TUNNEL_LOOK: Duration = Duration::from_millis(500);
/// The first wait before a dropped tunnel is reconnected, doubled after
/// each failed try up to [`TUNNEL_WAIT_MAX`].
const TUNNEL_WAIT: Duration = Duration::from_secs(2);
const TUNNEL_WAIT_MAX: Duration = Duration::from_secs(30);

/// The app's watch over `--ssh`'s tunnel: it says in the status bar what
/// the tunnel does, and reconnects a dropped one after a wait that grows
/// (without prompting: ssh's `BatchMode`), or at once on reload.
struct TunnelWatch {
    link: crate::ssh::TunnelLink,
    host: String,
    wait: Duration,
    retry_at: Option<Instant>,
}

impl TunnelWatch {
    fn new(link: crate::ssh::TunnelLink) -> TunnelWatch {
        TunnelWatch {
            host: link.host(),
            link,
            wait: TUNNEL_WAIT,
            retry_at: None,
        }
    }

    /// The status bar's words for `state`.
    fn says(&self, state: &crate::ssh::TunnelState) -> String {
        use crate::ssh::TunnelState;
        let host = markup(&self.host);
        match state {
            TunnelState::Up => format!("[dim]via ssh {host}[/]"),
            TunnelState::Connecting => format!("[yellow]ssh {host}: reconnecting…[/]"),
            TunnelState::Down(why) => {
                let left = self.retry_at.map_or(0, |at| {
                    at.saturating_duration_since(Instant::now()).as_secs()
                });
                format!(
                    "[red]ssh {host} dropped: {}[/] · retrying in {left} s (reload: now)",
                    markup(why)
                )
            }
        }
    }

    fn look(&mut self, b: Browser) {
        use crate::ssh::TunnelState;
        let mut state = self.link.check();
        if let TunnelState::Down(_) = &state {
            let now = Instant::now();
            let retry_at = *self.retry_at.get_or_insert(now + self.wait);
            if now >= retry_at || b.tunnel_now.get_untracked() {
                self.retry_at = None;
                self.wait = (self.wait * 2).min(TUNNEL_WAIT_MAX);
                state = match self.link.reconnect() {
                    Ok(()) => TunnelState::Connecting,
                    Err(error) => TunnelState::Down(error.to_string()),
                };
            }
        } else if state == TunnelState::Up {
            self.wait = TUNNEL_WAIT;
            self.retry_at = None;
        }
        if b.tunnel_now.get_untracked() {
            b.tunnel_now.set(false);
        }
        let says = self.says(&state);
        if b.tunnel
            .with_untracked(|t| t.as_deref() != Some(says.as_str()))
        {
            b.tunnel.set(Some(says));
        }
    }
}

/// The browser app over `engine`.
pub fn app(engine: Box<dyn Engine>, options: Options) -> App {
    app_with_presentation(
        engine,
        options,
        rich_intuituive::ImagePresentation::detect(),
    )
}

/// Construct with explicit independent terminal features, useful for embedded
/// clients and deterministic tests without changing the process environment.
pub fn app_with_presentation(
    engine: Box<dyn Engine>,
    options: Options,
    media_presentation: rich_intuituive::ImagePresentation,
) -> App {
    let Options {
        config,
        bookmarks,
        url,
        view,
        images,
        tunnel,
        control,
    } = options;
    let start_page = url.is_none() && config.home.is_none();
    let start = url
        .or_else(|| config.home.clone())
        .unwrap_or_else(|| "about:blank".into());
    let first_view = view.unwrap_or(config.view);
    let images = images.unwrap_or(config.images);
    let configured_adapt = config.adapt;
    let media_presentation = if images == Images::Auto {
        media_presentation
    } else {
        media_presentation.with_pixels(pictures::pixels(images))
    };
    let app = App::new(move || {
        let (keymap, warnings) = actions::keymap(config.preset, &config.keys);
        let mut warnings = [config.warnings.clone(), warnings].concat();
        warnings.truncate(3);
        let updates = Arc::new(std::sync::Mutex::new(crate::delivery::Mailbox::default()));
        let (send, receive) = mpsc::sync_channel(1);
        let inbox = updates.clone();
        let offer: UpdateSink = Arc::new(move |update| {
            let trace = {
                let mut inbox = inbox.lock().unwrap();
                inbox.push(update);
                crate::diagnostics::enabled().then(|| inbox.trace())
            };
            let _ = send.try_send(());
            if let Some(trace) = trace { trace.emit(); }
        });
        #[cfg(test)]
        TEST_OFFER.with(|slot| *slot.borrow_mut() = Some(offer.clone()));
        let tab = Tab::spawn(engine, move |update| offer(update));
        let page: Signal<Option<Arc<Page>>> = signal(None);
        let view: Signal<View> = signal(if first_view == View::Raster {
            View::Projection
        } else {
            first_view
        });
        let read_only = control.as_ref().is_some_and(|c| c.is_read_only());
        let hints: Signal<Option<Hints>> = signal(None);
        let find: Signal<Option<String>> = signal(None);
        let adapt = signal(configured_adapt);
        let faithful = signal(false);
        let logs: Signal<HashMap<NodeId, LogScroll>> = signal(HashMap::new());
        let port = viewport();
        let presentation = RefCell::new(crate::presentation::Presentation::default());
        let layout = memo(move || {
            let width = port.get().0;
            let marks = Marks {
                hints: hints
                    .with(|h| {
                        h.as_ref()
                            .map(|h| h.labels.iter().map(|(l, c)| (*c, l.clone())).collect())
                    })
                    .unwrap_or_default(),
                adapt: if faithful.get() {
                    Adapt::none()
                } else {
                    adapt.get()
                },
                logs: logs.get(),
                ..Marks::default()
            };
            let view = view.get();
            page.with(|p| match p {
                Some(p) => presentation.borrow_mut().update_shared(p, view, width, &marks),
                None => Rc::new(Projection::default()),
            })
        });
        let images_signal = signal(images);
        let found = memo(move || {
            let query = find.get().filter(|q| !q.is_empty());
            Rc::new(match query {
                Some(query) => layout.with(|l| {
                    l.lines
                        .iter()
                        .enumerate()
                        .flat_map(|(row, line)| {
                            (0..matches(&line.plain(), &query).len()).map(move |k| (row, k))
                        })
                        .collect()
                }),
                None => Vec::new(),
            })
        });
        let pacing = video_pacing(&config, images, media_presentation);
        let prepared = Arc::new(std::sync::Mutex::new(Prepared::default()));
        let completions = prepared.clone();
        let (wake_prepared, ready) = mpsc::sync_channel(1);
        let encoder = Encoder::spawn_with_presentation(media_presentation, move |job| {
            let trace = {
                let mut q = completions.lock().unwrap();
                if !q.complete(job) { return; }
                crate::diagnostics::enabled().then(|| (q.waiting.len(), q.waiting.iter().map(|job| job.picture.image().as_bytes().len()).sum::<usize>()))
            };
            let _ = wake_prepared.try_send(());
            if let Some((waiting, bytes)) = trace {
                eprintln!("RICH_MEDIA {{\"stage\":\"prepared_queue\",\"waiting\":{waiting},\"decoded_bytes\":{bytes}}}");
            }
        });
        let b = Browser {
            presentation: media_presentation,
            page,
            state: signal(PageState::default()),
            view,
            hints,
            find,
            find_at: signal(usize::MAX),
            scroll: signal(0),
            layout,
            found,
            chrome_y: signal(0.0),
            follow: signal(None),
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
            images: images_signal,
            images_default: signal(images),
            viewport: port,
            adapt,
            faithful,
            logs,
            pixels: memo(move || { let images = images_signal.get(); if images == Images::Auto { media_presentation.pixels() } else { media_presentation.with_pixels(pictures::pixels(images)).pixels() } }),
            frozen: signal(false),
            live: signal(Rc::new(HashMap::new())),
            tab: signal(tab),
            waiting: signal(HashMap::new()),
            controls: signal(HashMap::new()),
            videos: signal(Rc::new(HashMap::new())),
            pacing: signal(Rc::new(RefCell::new(pacing))),
            encoder: signal(Rc::new(encoder)),
            prepared: signal(prepared.clone()),
            regions_sent: signal((Vec::new(), Vec::new(), 1.0)),
            start: signal(false),
            tunnel: signal(None),
            tunnel_now: signal(false),
            reads: signal(0),
            read_only: signal(read_only),
            agent_pick: signal(false),
            screen: signal(crate::pixel_view::state()),
            screen_tiles: signal(Vec::new()),
            screen_stats: signal(crate::pixel_view::ScreenStats::default()),
        };
        #[cfg(test)]
        TEST_BROWSER.with(|slot| slot.set(Some(b)));
        // The pixel view needs Kitty graphics; without them it says so and
        // the projection shows.
        if b.view.get_untracked() == View::Pixels && !crate::pixel_view::available(b) {
            b.view.set(View::Projection);
            b.say(format!("[yellow]{}[/]", crate::pixel_view::NEEDS_KITTY));
        }
        watch(
            move || b.view.get() == View::Projection,
            move |projection, _| {
                if !projection { prune_media_work(b, true, &Default::default()); }
                let mut videos = (*b.videos.get_untracked()).clone();
                let mut live = (*b.live.get_untracked()).clone();
                b.pacing.with_untracked(|p| {
                    reconcile_view_media(projection, &mut p.borrow_mut(), &mut videos, &mut live)
                });
                if *b.videos.get_untracked() != videos {
                    b.videos.set(Rc::new(videos));
                }
                if *b.live.get_untracked() != live {
                    b.live.set(Rc::new(live));
                }
            },
        );
        if let Some(server) = control.as_ref().and_then(|c| c.take()) {
            serve_control(b, server);
        }
        if let Some(link) = tunnel.clone() {
            let mut watch = TunnelWatch::new(link);
            b.tunnel.set(Some(watch.says(&crate::ssh::TunnelState::Up)));
            every(TUNNEL_LOOK, move |_| watch.look(b));
        }
        spawn_stream(move |stream| forward_updates(b, receive, updates, stream.proxy().clone()));
        spawn_stream(move |stream| forward_prepared(b, ready, prepared, stream.proxy().clone()));
        // Moving pictures in view step within the byte budget.
        let motion = Rc::new(std::cell::RefCell::new(pictures::Motion::with_presentation(byte_budget(b), media_presentation)));
        every(MOTION, move |_| step_motion(b, &mut motion.borrow_mut()));
        // Queue the desired source viewport BEFORE enabling pixel forwarding.
        // The engine keeps scoped reads blocked until Chrome applies its metrics.
        watch(
            move || (source_viewport(b), b.view.get() == View::Pixels),
            move |((columns, rows), pixels), _| {
                crate::pixel_view::invalidate(b);
                b.send(Command::Resize(columns, rows));
                b.send(Command::Pixels(pixels));
            },
        );
        let (columns, rows) = source_viewport(b);
        b.send(Command::Resize(columns, rows));
        b.send(Command::Pixels(b.view.get_untracked() == View::Pixels));
        b.send(Command::Open(start.clone()));
        b.start.set(start_page);
        // The tab hears which video regions the view shows, so only those
        // get frames.
        watch(move || regions_on_screen(b), move |_, _| send_regions(b));
        // Chrome's scroll follows ours, so the page's lazy loading and
        // infinite feeds see the reader scroll.
        watch(
            move || b.scroll.get(),
            move |row, cx| follow_scroll(b, row, cx),
        );

        let address = crate::bars::header(b);
        let status = crate::bars::status(b);
        // Built again on each switch: a view kept while hidden could miss
        // the pages opened meanwhile.
        let body = keyed(
            move || b.view.get(),
            move |view| match view {
                View::Reader => reader_view(b),
                View::Pixels => crate::pixel_view::view(b),
                _ => projection_view(b),
            },
        )
        .flex(1)
        .hidden_when(move || b.start.get());
        let start = crate::start::page(crate::start::StartPage {
            keymap: keymap.clone(),
            bookmarks: b.bookmarks,
            open: Rc::new(move |url, _| {
                b.send(Command::Open(url));
            }),
            about: Rc::new(move |cx| run(b, "about", cx)),
            chrome_rows: CHROME_ROWS,
        })
        .flex(1)
        .shown_when(move || b.start.get());
        let mut root = column([address, start, body, status]);
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
                // The pixel view passes keys to the page, but for those
                // with Ctrl or Alt and the F keys.
                if b.view.with_untracked(|v| *v == View::Pixels) && crate::pixel_view::passes(key) {
                    b.send(Command::Key(key));
                    return true;
                }
                // Source-owned projection dialogs decide whether Escape closes
                // them. Hints and pushed editors/actions consume it first.
                if key.code == KeyCode::Escape
                    && !key.modifiers.ctrl && !key.modifiers.alt
                    && b.view.with_untracked(|v| *v == View::Projection)
                    && visible_source_dialog(b)
                {
                    b.send(Command::Key(key));
                    return true;
                }
                if let Some(target) = site_key(b, key) {
                    b.message.set(String::new());
                    run_target(b, &target, cx);
                    return true;
                }
                false
            })
    })
    .graphics(media_presentation.graphics());
    let stats = app.stats_handle();
    OUTPUT.with(|o| o.borrow_mut().stats = Some(stats));
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_underlay_retains_configured_semantic_twins_through_scroll_and_close() {
        use crate::adapt::Rule;
        let engine = crate::testing::FakeEngine::from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        )
        .unwrap();
        let visible = crate::model::build(
            &crate::testing::snapshot_file(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/snapshots/product-grid.json"),
            )
            .unwrap(),
            &mut crate::model::Revisions::new(),
        );
        let application = app(
            Box::new(engine),
            Options {
                config: Config::default(),
                bookmarks: Bookmarks::default(),
                url: None,
                view: Some(View::Projection),
                images: Some(Images::None),
                tunnel: None,
                control: None,
            },
        );
        let proxy = application.proxy();
        let mut driver = application.driver(160, 60);
        driver.settle();
        let b = TEST_BROWSER.with(|slot| slot.get().unwrap());
        b.start.set(false);
        assert!(!visible.overlays.is_empty());
        let mut closed = visible.clone();
        for overlay in closed.overlays.drain(..) {
            closed.layers[overlay.layer].place = crate::model::Place::Off;
        }
        for (adapt, faithful, canonical) in [
            (Adapt::default(), false, true),
            (
                Adapt::default().with(Rule::ScreenReaderTwins, false),
                false,
                false,
            ),
            (Adapt::default(), true, false),
        ] {
            b.adapt.set(adapt);
            b.faithful.set(faithful);
            for open in [true, false, true] {
                let page = if open { &visible } else { &closed }.clone();
                proxy.run_with(move |cx| apply(b, Update::Page(Arc::new(page)), cx));
                for scroll in [0, 1, 0] {
                    b.scroll.set(scroll);
                    driver.settle();
                    let text = driver.screen().plain().join("\n");
                    assert_eq!(text.contains("We use cookies"), open, "{text}");
                    for (spoken, shown) in [
                        ("$29.99", "$2999"),
                        ("$24.49", "$2449"),
                        ("$49.00", "$4900"),
                    ] {
                        let (wanted, excluded) = if canonical {
                            (spoken, shown)
                        } else {
                            (shown, spoken)
                        };
                        assert!(
                            text.contains(wanted) && !text.contains(excluded),
                            "open={open}, scroll={scroll}, faithful={faithful}, adapt={adapt:?}: {text}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn popup_underlay_keeps_nonprice_accessible_twins_without_changing_their_match() {
        use crate::{adapt::Rule, engine::Rect, testing::PageBuilder};
        let mut p = PageBuilder::new("https://example.test/", "Revision", 800.0, 640.0);
        let line = p.element(
            p.body(),
            "P",
            &[],
            &[],
            Some(Rect::new(8.0, 8.0, 240.0, 24.0)),
        );
        let spoken = p.element(
            line,
            "SPAN",
            &[],
            &[
                ("position", "absolute"),
                ("clip", "rect(0px, 0px, 0px, 0px)"),
            ],
            Some(Rect::new(8.0, 8.0, 1.0, 1.0)),
        );
        p.text(spoken, "Revision v2.03", Rect::new(8.0, 8.0, 100.0, 20.0));
        let shown = p.element(
            line,
            "SPAN",
            &[("aria-hidden", "true")],
            &[],
            Some(Rect::new(8.0, 8.0, 120.0, 24.0)),
        );
        p.text(shown, "Revision v203", Rect::new(8.0, 8.0, 120.0, 24.0));
        let popup = p.element(
            p.body(),
            "DIV",
            &[("role", "dialog")],
            &[("position", "fixed")],
            Some(Rect::new(400.0, 200.0, 300.0, 200.0)),
        );
        p.text(popup, "Popup", Rect::new(408.0, 208.0, 80.0, 20.0));
        let page = crate::model::build(&p.snapshot(), &mut crate::model::Revisions::new());
        assert!(page.text().contains("Revision v2.03"));
        assert_eq!(page.overlays.len(), 1);
        for enabled in [true, false] {
            let marks = Marks {
                adapt: Adapt::none().with(Rule::ScreenReaderTwins, enabled),
                ..Marks::default()
            };
            let text = lay_out(&page, View::Projection, 100, &marks)
                .lines
                .iter()
                .map(Line::plain)
                .collect::<Vec<_>>()
                .join("\n");
            let wanted = if enabled {
                "Revision v2.03"
            } else {
                "Revision v203"
            };
            assert!(text.contains(wanted), "enabled={enabled}: {text}");
        }
    }

    #[test]
    fn popup_underlay_retains_shelf_strip_after_image_decode() {
        let snapshot = crate::testing::snapshot_file(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/snapshots/product-grid.json"),
        )
        .unwrap();
        let page = crate::model::build(&snapshot, &mut crate::model::Revisions::new());
        let projection = lay_out(&page, View::Projection, 100, &Marks::default());
        assert!(
            projection
                .lines
                .iter()
                .any(|line| line.plain().contains("1–3 of 6")),
            "overlays={:?}, shelves={}, lines={:?}",
            page.overlays,
            page.shelves.len(),
            projection
                .lines
                .iter()
                .map(|line| line.plain())
                .collect::<Vec<_>>()
        );
        let engine = crate::testing::FakeEngine::from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        )
        .unwrap();
        let mut driver = app(
            Box::new(engine),
            Options {
                config: Config::default(),
                bookmarks: Bookmarks::default(),
                url: Some("product-grid.html".into()),
                view: Some(View::Projection),
                images: Some(Images::Ansi),
                tunnel: None,
                control: None,
            },
        )
        .driver(100, 60);
        let deadline = Instant::now() + Duration::from_secs(2);
        let b = TEST_BROWSER.with(|slot| slot.get().unwrap());
        while b.page.get_untracked().is_none() {
            driver.settle();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let projected = b.layout.get_untracked();
        let stripe = projected
            .lines
            .iter()
            .position(|line| line.plain().contains("1–3 of 6"));
        assert!(
            stripe.is_some(),
            "active={:?}, layout={:?}",
            b.adapt.get_untracked(),
            projected
                .lines
                .iter()
                .map(|line| line.plain())
                .collect::<Vec<_>>()
        );
        driver.settle();
        assert!(
            driver.find_text("1–3 of 6").is_some(),
            "stripe={stripe:?}, overlays={:?}, rows={:?}",
            b.page.get_untracked().unwrap().overlays,
            driver.screen().plain()
        );
    }

    #[test]
    fn prepared_results_are_bounded_by_current_region_incarnations() {
        let job = |node| Job {
            node,
            picture: Picture::from_rgba(1, 1, vec![255; 4]).unwrap(),
            pixels: Pixels::Iterm,
            cols: 1,
            rows: 1,
        };
        let mut queue = Prepared::default();
        let mut held = None;
        for node in 1..=64 {
            let current = job(node);
            queue.admit(&current);
            assert!(queue.complete(current));
            queue.retain(&std::collections::HashSet::from([node]));
            assert_eq!(queue.waiting.len(), 1);
            assert_eq!(queue.admitted.len(), 1);
            held = queue.waiting.pop_front();
        }
        queue.retain(&Default::default());
        assert!(
            !queue.complete(held.unwrap()),
            "removed region repopulated prepared work"
        );
        let old = job(7);
        queue.admit(&old);
        let new = Job {
            picture: old.picture.next_rgba(1, 1, vec![0; 4]).unwrap(),
            ..job(7)
        };
        queue.admit(&new);
        assert!(
            queue.complete(old.clone()),
            "executing current-incarnation frame was starved by newer waiting work"
        );
        assert!(queue.complete(new));
        assert!(
            !queue.complete(old),
            "older completion replaced a newer prepared result"
        );
        assert_eq!(queue.waiting.len(), 1);
        queue = Prepared::default();
        assert!(queue.waiting.is_empty());
        assert!(queue.admitted.is_empty());
    }

    #[test]
    fn slow_executing_encode_progresses_while_continuous_same_region_offers_coalesce() {
        let prepared = Arc::new(std::sync::Mutex::new(Prepared::default()));
        let completed = prepared.clone();
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (sent, received) = mpsc::channel();
        let first = std::cell::Cell::new(true);
        let encoder = Encoder::spawn(move |job| {
            if first.replace(false) {
                entered.send(()).unwrap();
                resume.recv().unwrap();
            }
            let version = job.picture.version();
            let accepted = completed.lock().unwrap().complete(job);
            sent.send((version, accepted)).unwrap();
        });
        let mut picture = Picture::from_rgba(1, 1, vec![255; 4]).unwrap();
        let job = |picture| Job {
            node: 7,
            picture,
            pixels: Pixels::Cells,
            cols: 1,
            rows: 1,
        };
        let initial = job(picture.clone());
        prepared.lock().unwrap().admit(&initial);
        encoder.encode(initial);
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        for value in 1..=60 {
            picture = picture.next_rgba(1, 1, vec![value; 4]).unwrap();
            let next = job(picture.clone());
            prepared.lock().unwrap().admit(&next);
            encoder.encode(next);
        }
        release.send(()).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(3)).unwrap(),
            (0, true),
            "executing current-incarnation frame was starved"
        );
        assert_eq!(
            received.recv_timeout(Duration::from_secs(3)).unwrap(),
            (60, true)
        );
        assert!(
            received.recv_timeout(Duration::from_millis(50)).is_err(),
            "more than newest waiting encode survived"
        );
        let prepared = prepared.lock().unwrap();
        assert_eq!(prepared.waiting.len(), 1);
        assert_eq!(prepared.waiting[0].picture.version(), 60);
    }

    #[test]
    fn document_churn_prunes_encoder_waiting_work_before_fresh_media() {
        let engine = crate::testing::FakeEngine::from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        )
        .unwrap();
        let app = app(
            Box::new(engine),
            Options {
                config: Config::default(),
                bookmarks: Bookmarks::default(),
                url: Some("reader.html".into()),
                view: Some(View::Projection),
                images: Some(Images::None),
                tunnel: None,
                control: None,
            },
        );
        let proxy = app.proxy();
        let mut driver = app.driver(80, 24);
        driver.settle();
        let b = TEST_BROWSER.with(|slot| slot.get().unwrap());
        let (entered, ready) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (completed, results) = mpsc::channel();
        let first = std::cell::Cell::new(true);
        let encoder = Rc::new(Encoder::spawn(move |job| {
            if first.replace(false) {
                entered.send(()).unwrap();
                held.recv().unwrap();
            }
            completed.send(job.node).unwrap();
        }));
        b.encoder.update(|value| *value = encoder.clone());
        let job = |node| Job {
            node,
            picture: Picture::from_rgba(1, 1, vec![255; 4]).unwrap(),
            pixels: Pixels::Cells,
            cols: 1,
            rows: 1,
        };
        encoder.encode(job(0));
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        for document in 1..=64 {
            encoder.encode(job(document as i64));
            proxy.run_with(move |cx| {
                apply(
                    b,
                    Update::Page(Arc::new(Page {
                        document,
                        ..Page::default()
                    })),
                    cx,
                )
            });
            driver.settle();
        }
        encoder.encode(job(999));
        encoder.encode(job(999));
        release.send(()).unwrap();
        assert_eq!(results.recv_timeout(Duration::from_secs(3)).unwrap(), 0);
        assert_eq!(
            results.recv_timeout(Duration::from_secs(3)).unwrap(),
            999,
            "historical regions delayed fresh media"
        );
        assert!(
            results.recv_timeout(Duration::from_millis(50)).is_err(),
            "superseded waiting frame survived"
        );
        let ready = job(77);
        b.prepared.with_untracked(|queue| {
            let mut queue = queue.lock().unwrap();
            queue.admit(&ready);
            assert!(queue.complete(ready));
        });
        b.view.set(View::Reader);
        driver.settle();
        b.prepared.with_untracked(|queue| {
            let queue = queue.lock().unwrap();
            assert!(queue.waiting.is_empty());
            assert!(queue.admitted.is_empty());
        });
    }

    #[test]
    fn document_dialog_scrolls_with_the_page_and_hidden_dialog_stays_absent() {
        use crate::{
            engine::Rect,
            testing::{FakeEngine, PageBuilder},
        };
        let mut p = PageBuilder::new(
            "https://example.test/dialog-scroll.html",
            "Scroll",
            960.0,
            3000.0,
        );
        p.view(0.0, 720.0);
        for row in 0..100 {
            let rect = Rect::new(0.0, f64::from(row) * 24.0, 200.0, 18.0);
            let node = p.element(p.body(), "P", &[], &[], Some(rect));
            p.text(node, &format!("Underlay row {row}"), rect);
        }
        let dialog = p.element(
            p.body(),
            "DIV",
            &[("role", "dialog")],
            &[("position", "absolute")],
            Some(Rect::new(400.0, 360.0, 300.0, 180.0)),
        );
        p.text(
            dialog,
            "Document popup",
            Rect::new(408.0, 378.0, 180.0, 18.0),
        );
        let hidden = p.element(
            p.body(),
            "DIALOG",
            &[("open", "")],
            &[("visibility", "hidden")],
            Some(Rect::new(400.0, 400.0, 300.0, 180.0)),
        );
        p.text(hidden, "Hidden popup", Rect::new(408.0, 418.0, 180.0, 18.0));
        let engine = FakeEngine::from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        )
        .unwrap();
        engine
            .state
            .lock()
            .unwrap()
            .pages
            .insert("dialog-scroll".into(), p.snapshot());
        let state = engine.state.clone();
        let app = app(
            Box::new(engine),
            Options {
                config: Config::default(),
                bookmarks: Bookmarks::default(),
                url: Some("dialog-scroll.html".into()),
                view: Some(View::Projection),
                images: Some(Images::None),
                tunnel: None,
                control: None,
            },
        );
        let mut driver = app.driver(120, 40);
        let deadline = Instant::now() + Duration::from_secs(3);
        let first = loop {
            driver.settle();
            if let Some((_, row)) = driver.find_text("Document popup") {
                break row;
            }
            assert!(Instant::now() < deadline, "popup never appeared");
            std::thread::sleep(Duration::from_millis(2));
        };
        assert!(driver.find_text("Hidden popup").is_none());
        let b = TEST_BROWSER.with(|slot| slot.get().unwrap());
        b.scroll.set(10);
        driver.settle();
        let second = driver.find_text("Document popup").unwrap().1;
        assert!(
            second < first,
            "document popup stayed fixed: {first} -> {second}"
        );
        b.scroll.set(65);
        driver.settle();
        assert!(
            driver.find_text("Document popup").is_none(),
            "offscreen popup was pinned into view"
        );
        driver.press("esc");
        std::thread::sleep(Duration::from_millis(50));
        driver.settle();
        assert!(
            !state
                .lock()
                .unwrap()
                .actions
                .iter()
                .any(|action| matches!(action, crate::engine::PageAction::Key(_))),
            "offscreen dialog intercepted owner Escape"
        );
        b.scroll.set(0);
        driver.settle();
        assert_eq!(driver.find_text("Document popup").unwrap().1, first);
    }

    #[test]
    fn popup_fill_uses_the_same_foreground_background_policy() {
        use crate::adapt::{Background, Rule};
        for terminal in [Background::Dark, Background::Light] {
            for source in ["rgb(255, 255, 255)", "rgb(17, 17, 17)"] {
                let faithful = popup_fill(Adapt::none(), source, terminal);
                assert_eq!(
                    faithful,
                    format!("on {}", crate::adapt::raw_color(source, terminal).unwrap())
                );
                assert_eq!(popup_fill(Adapt::default(), source, terminal), "");
                assert_eq!(
                    popup_fill(Adapt::none().with(Rule::PageColors, true), source, terminal),
                    ""
                );
            }
            assert_eq!(popup_fill(Adapt::none(), "rgba(0, 0, 0, 0)", terminal), "");
        }
    }

    #[test]
    fn popup_preserves_the_underlay_foreground_policy() {
        use crate::{
            engine::Rect,
            model::{Revisions, build},
            testing::PageBuilder,
        };
        let mut p = PageBuilder::new("https://example.test/", "Colours", 800.0, 640.0);
        let body = p.body();
        let text = p.element(
            body,
            "P",
            &[],
            &[("color", "rgb(17, 17, 17)")],
            Some(Rect::new(0.0, 0.0, 200.0, 20.0)),
        );
        p.text(text, "Source foreground", Rect::new(0.0, 0.0, 200.0, 20.0));
        let dialog = p.element(
            body,
            "DIV",
            &[("role", "dialog")],
            &[
                ("position", "fixed"),
                ("background-color", "rgb(255, 255, 255)"),
            ],
            Some(Rect::new(300.0, 100.0, 300.0, 300.0)),
        );
        p.text(dialog, "Popup", Rect::new(310.0, 120.0, 100.0, 20.0));
        let visible = build(&p.snapshot(), &mut Revisions::new());
        p.set_style(dialog, "opacity", "0");
        let closed = build(&p.snapshot(), &mut Revisions::new());
        for adapt in [Adapt::default(), Adapt::none()] {
            let marks = Marks {
                adapt,
                ..Marks::default()
            };
            let foreground = |page: &Page| {
                lay_out(page, View::Projection, 100, &marks)
                    .lines
                    .into_iter()
                    .flat_map(|line| line.spans)
                    .find(|span| span.text.contains("Source foreground"))
                    .unwrap()
                    .style
            };
            assert_eq!(
                foreground(&visible),
                foreground(&closed),
                "popup changed foreground without changing backdrop"
            );
        }
    }

    #[test]
    fn producer_frame_taken_before_resize_cannot_enter_new_pixel_coordinates() {
        use crate::engine::{Format, Frame, HistoryStep, PageAction, PageSnapshot, TabId};
        use rich_embed::{FrameMeta, ScreencastFrame};
        use std::io;
        struct Gated {
            inner: crate::testing::FakeEngine,
            taken: mpsc::Sender<()>,
            resume: mpsc::Receiver<()>,
            first: bool,
        }
        impl Engine for Gated {
            fn resize(&mut self, c: u16, r: u16) -> io::Result<()> {
                self.inner.resize(c, r)
            }
            fn open(&mut self, t: TabId, u: &str) -> io::Result<()> {
                self.inner.open(t, u)
            }
            fn history(&mut self, t: TabId, h: HistoryStep) -> io::Result<()> {
                self.inner.history(t, h)
            }
            fn state(&self, t: TabId) -> PageState {
                self.inner.state(t)
            }
            fn snapshot(&mut self, t: TabId) -> io::Result<PageSnapshot> {
                self.inner.snapshot(t)
            }
            fn capture(
                &mut self,
                t: TabId,
                r: crate::engine::Rect,
                f: Format,
            ) -> io::Result<Frame> {
                self.inner.capture(t, r, f)
            }
            fn act(&mut self, t: TabId, a: &PageAction) -> io::Result<()> {
                self.inner.act(t, a)
            }
            fn events(&mut self) -> Vec<crate::engine::EngineEvent> {
                self.inner.events()
            }
            fn set_screencast(&mut self, t: TabId, on: bool) -> io::Result<()> {
                self.inner.set_screencast(t, on)
            }
            fn screencast_frame(&mut self, t: TabId) -> Option<ScreencastFrame> {
                let frame = self.inner.screencast_frame(t)?;
                if std::mem::take(&mut self.first) {
                    self.taken.send(()).unwrap();
                    self.resume.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                Some(frame)
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        let inner = crate::testing::FakeEngine::from_dir(&root).unwrap();
        let state = inner.state.clone();
        let make_frame = |width, height| ScreencastFrame {
            pixels: rich_embed::Pixels::new(
                width,
                height,
                [10, 70, 200].repeat(width as usize * height as usize),
            )
            .unwrap(),
            meta: FrameMeta {
                device_width: f64::from(width),
                device_height: f64::from(height),
                page_scale: 1.0,
                ..FrameMeta::default()
            },
        };
        state.lock().unwrap().frames.push(make_frame(
            120 * crate::cdp::CELL.0,
            36 * crate::cdp::CELL.1,
        ));
        let (taken, receive) = mpsc::channel();
        let (resume, gate) = mpsc::channel();
        let engine = Gated {
            inner,
            taken,
            resume: gate,
            first: true,
        };
        let policy = rich_intuituive::ImagePresentation::new(
            Pixels::Kitty,
            rich_intuituive::KittyFeatures {
                unicode_placeholders: true,
                native_animation: false,
            },
            rich_intuituive::PixelFormats::cells().with(Pixels::Kitty),
        );
        let options = Options {
            url: Some("https://example.test/stream-with-log.html".into()),
            view: Some(View::Pixels),
            images: Some(Images::Kitty),
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            tunnel: None,
            control: None,
        };
        let app = app_with_presentation(Box::new(engine), options, policy);
        let mut driver = app.driver(120, 40);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            driver.update(Duration::ZERO);
            driver.render();
            driver.output_written();
            if receive.try_recv().is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "producer never reached the capture gate"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let b = TEST_BROWSER.with(|slot| slot.get().unwrap());
        driver.resize(40, 24);
        driver.render();
        driver.output_written();
        assert_eq!(b.viewport.get_untracked(), (40, 24));
        resume.send(()).unwrap();
        for _ in 0..40 {
            driver.update(Duration::ZERO);
            driver.render();
            driver.output_written();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            b.screen_stats.get_untracked().frames,
            0,
            "old-width producer frame was squeezed into new coordinates"
        );
        state
            .lock()
            .unwrap()
            .frames
            .push(make_frame(40 * crate::cdp::CELL.0, 20 * crate::cdp::CELL.1));
        let deadline = Instant::now() + Duration::from_secs(2);
        while b.screen_stats.get_untracked().frames == 0 {
            driver.update(Duration::ZERO);
            driver.render();
            driver.output_written();
            assert!(
                Instant::now() < deadline,
                "matching fresh frame never reached the pixel view"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(b.screen_stats.get_untracked().frames, 1);
    }

    #[test]
    fn old_encoder_completion_cannot_replace_recreated_region() {
        let mut pacing = Pacing::new(
            rich_art::picture::ByteBudget::new(10_000_000),
            Pixels::Kitty,
        );
        let (send, receive) = mpsc::channel();
        let encoder = Encoder::spawn(move |job| {
            send.send(job).unwrap();
        });
        let frame = |seq| crate::video::VideoFrame {
            node: 7,
            seq,
            pixels: rich_embed::Pixels::new(4, 4, vec![255; 48]).unwrap(),
        };
        let offer = |pacing: &mut Pacing, seq| {
            pacing
                .offer(&frame(seq), 2, 2, Instant::now(), |_| 0)
                .unwrap()
        };
        let old = offer(&mut pacing, 1);
        encoder.encode(Job {
            node: 7,
            picture: old,
            pixels: Pixels::Kitty,
            cols: 2,
            rows: 2,
        });
        let old = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        pacing.clear();
        let current = offer(&mut pacing, 1);
        encoder.encode(Job {
            node: 7,
            picture: current.clone(),
            pixels: Pixels::Kitty,
            cols: 2,
            rows: 2,
        });
        let current_job = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        let current_live = Live {
            picture: current_job.picture,
            pixels: current_job.pixels,
        };
        assert!(accepts_frame(&pacing, None, 7, &current_live));
        let old_live = Live {
            picture: old.picture,
            pixels: old.pixels,
        };
        assert!(!accepts_frame(&pacing, Some(&current_live), 7, &old_live));
        let newer = pacing
            .offer(
                &frame(2),
                2,
                2,
                Instant::now() + Duration::from_secs(1),
                |_| 0,
            )
            .unwrap();
        assert!(accepts_frame(&pacing, None, 7, &current_live));
        let newer_live = Live {
            picture: newer,
            pixels: Pixels::Kitty,
        };
        assert!(!accepts_frame(&pacing, Some(&newer_live), 7, &current_live));
        pacing.forget(7);
        assert!(!accepts_frame(&pacing, None, 7, &current_live));
    }

    #[test]
    fn queued_video_frame_does_not_recreate_pacing_outside_projection() {
        let mut pacing = Pacing::new(
            rich_art::picture::ByteBudget::new(10_000_000),
            Pixels::Kitty,
        );
        let frame = VideoFrame {
            node: 7,
            seq: 1,
            pixels: rich_embed::Pixels::new(4, 4, vec![255; 48]).unwrap(),
        };
        for view in [View::Reader, View::Pixels] {
            let (picture, _) = offer_video_frame(
                view,
                &mut pacing,
                &frame,
                2,
                2,
                Default::default(),
                Instant::now(),
            );
            assert!(
                picture.is_none(),
                "queued frame recreated pacing in {view:?}"
            );
            assert!(!pacing.has(7));
        }
        let (picture, _) = offer_video_frame(
            View::Projection,
            &mut pacing,
            &frame,
            2,
            2,
            Default::default(),
            Instant::now(),
        );
        assert!(picture.is_some());
        assert!(pacing.has(7));
    }

    #[test]
    fn old_encoder_completion_is_invalidated_by_leaving_projection() {
        let mut pacing = Pacing::new(
            rich_art::picture::ByteBudget::new(10_000_000),
            Pixels::Kitty,
        );
        let (send, receive) = mpsc::channel();
        let encoder = Encoder::spawn(move |job| {
            send.send(job).unwrap();
        });
        let frame = |seq| VideoFrame {
            node: 7,
            seq,
            pixels: rich_embed::Pixels::new(4, 4, vec![255; 48]).unwrap(),
        };
        let picture = pacing
            .offer(&frame(1), 2, 2, Instant::now(), |_| 0)
            .unwrap();
        encoder.encode(Job {
            node: 7,
            picture,
            pixels: Pixels::Kitty,
            cols: 2,
            rows: 2,
        });
        let held = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        let old = Live {
            picture: held.picture,
            pixels: held.pixels,
        };
        let mut videos = HashMap::from([(7, old.clone())]);
        let mut live = videos.clone();
        reconcile_view_media(false, &mut pacing, &mut videos, &mut live);
        reconcile_view_media(true, &mut pacing, &mut videos, &mut live);
        assert!(
            !accepts_frame(&pacing, None, 7, &old),
            "old prepared frame survived the view transition"
        );
        assert!(videos.is_empty());
        assert!(live.is_empty());
        let current = pacing
            .offer(
                &frame(2),
                2,
                2,
                Instant::now() + Duration::from_secs(1),
                |_| 0,
            )
            .unwrap();
        let current = Live {
            picture: current,
            pixels: Pixels::Kitty,
        };
        assert!(accepts_frame(&pacing, None, 7, &current));
        assert!(!accepts_frame(&pacing, None, 7, &old));
    }

    #[test]
    fn held_encoder_completion_is_rejected_after_protocol_steps_down() {
        let mut pacing = Pacing::new(
            rich_art::picture::ByteBudget::new(10_000_000),
            Pixels::Kitty,
        );
        let (send, receive) = mpsc::channel();
        let encoder = Encoder::spawn(move |job| {
            send.send(job).unwrap();
        });
        let frame = |seq| VideoFrame {
            node: 7,
            seq,
            pixels: rich_embed::Pixels::new(4, 4, vec![255; 48]).unwrap(),
        };
        let now = Instant::now();
        let picture = pacing.offer(&frame(1), 20, 10, now, |_| 0).unwrap();
        encoder.encode(Job {
            node: 7,
            picture,
            pixels: Pixels::Kitty,
            cols: 20,
            rows: 10,
        });
        let held = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        let old = Live {
            picture: held.picture,
            pixels: held.pixels,
        };
        assert!(accepts_frame(&pacing, None, 7, &old));
        pacing.budget = rich_art::picture::ByteBudget::new(1);
        let _ = pacing.offer(&frame(2), 20, 10, now + Duration::from_secs(30), |_| 0);
        assert_ne!(pacing.pixels(7), Pixels::Kitty);
        assert!(!accepts_frame(&pacing, None, 7, &old));
    }

    #[test]
    fn held_encoder_preparation_is_rebuilt_for_current_paint_context() {
        use rich_art::graphics::CellPixels;
        use rich_ext::frame::Frame;
        let (send, receive) = mpsc::channel();
        let encoder = Encoder::spawn(move |job| {
            send.send(job).unwrap();
        });
        let rgba = vec![255; 4 * 4 * 4];
        let picture = Picture::from_rgba(4, 4, rgba.clone()).unwrap();
        encoder.encode_with_metrics(
            Job {
                node: 7,
                picture: picture.clone(),
                pixels: Pixels::Iterm,
                cols: 2,
                rows: 2,
            },
            Some((8, 16)),
        );
        let held = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(held.picture.is_prepared_with_metrics(
            pictures::view(Pixels::Iterm),
            2,
            2,
            CellPixels::new(8, 16)
        ));
        let console = rich::Console::builder()
            .width(3)
            .color_system(Some(rich::ColorSystem::Truecolor))
            .build();
        let paint = |picture: &Picture, pixels: Pixels| {
            let cells = pictures::Cells::default();
            let lines = cells.of_with_metrics(&console, picture, pixels, 3, 4, Some((10, 20)));
            let mut segments = Vec::new();
            for line in lines.iter() {
                segments.extend(line.clone());
                segments.push(rich::Segment::line());
            }
            let source = image_graphics();
            let placements =
                source.placements_with_metrics(&Frame::from_segments(&segments), Some((10, 20)));
            assert_eq!(placements.len(), 1);
            placements[0].graphic.draw(0)
        };
        assert!(!held.picture.is_prepared_with_metrics(
            pictures::view(Pixels::Iterm),
            3,
            4,
            CellPixels::new(10, 20)
        ));
        let transitioned = paint(&held.picture, Pixels::Iterm);
        let clean = Picture::from_rgba(4, 4, rgba).unwrap();
        assert_eq!(transitioned, paint(&clean, Pixels::Iterm));
        assert!(held.picture.is_prepared_with_metrics(
            pictures::view(Pixels::Iterm),
            3,
            4,
            CellPixels::new(10, 20)
        ));
        assert_eq!(
            paint(&held.picture, Pixels::Sixel),
            paint(&clean, Pixels::Sixel)
        );
        let kitty = paint(&held.picture, Pixels::Kitty);
        assert!(kitty.contains("s=30,v=80"), "{kitty:?}");
    }

    #[test]
    fn page_transition_removes_old_live_pixels_immediately() {
        let mut pacing = Pacing::new(
            rich_art::picture::ByteBudget::new(10_000_000),
            Pixels::Kitty,
        );
        let picture = Picture::from_rgba(1, 1, vec![255; 4]).unwrap();
        let old = Live {
            picture,
            pixels: Pixels::Kitty,
        };
        let mut videos = HashMap::from([(7, old.clone())]);
        let mut live = HashMap::from([(7, old)]);
        reconcile_page_media(
            true,
            &std::collections::HashSet::from([7]),
            &mut pacing,
            &mut videos,
            &mut live,
        );
        assert!(videos.is_empty());
        assert!(
            live.is_empty(),
            "previous document's live image was retained"
        );
    }

    #[test]
    fn removal_then_readd_rejects_held_encoder_completion() {
        let mut pacing = Pacing::new(
            rich_art::picture::ByteBudget::new(10_000_000),
            Pixels::Kitty,
        );
        let (send, receive) = mpsc::channel();
        let encoder = Encoder::spawn(move |job| {
            send.send(job).unwrap();
        });
        let frame = |seq| crate::video::VideoFrame {
            node: 7,
            seq,
            pixels: rich_embed::Pixels::new(4, 4, vec![255; 48]).unwrap(),
        };
        let old = pacing
            .offer(&frame(1), 2, 2, Instant::now(), |_| 0)
            .unwrap();
        encoder.encode(Job {
            node: 7,
            picture: old,
            pixels: Pixels::Kitty,
            cols: 2,
            rows: 2,
        });
        let old = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        let old_live = Live {
            picture: old.picture,
            pixels: old.pixels,
        };
        let mut videos = HashMap::from([(7, old_live.clone())]);
        let mut live = HashMap::from([(7, old_live.clone())]);
        reconcile_page_media(
            false,
            &std::collections::HashSet::from([7]),
            &mut pacing,
            &mut videos,
            &mut live,
        );
        assert!(accepts_frame(&pacing, videos.get(&7), 7, &old_live));
        reconcile_page_media(
            false,
            &std::collections::HashSet::new(),
            &mut pacing,
            &mut videos,
            &mut live,
        );
        assert!(videos.is_empty());
        assert!(live.is_empty());
        reconcile_page_media(
            false,
            &std::collections::HashSet::from([7]),
            &mut pacing,
            &mut videos,
            &mut live,
        );
        let current = pacing
            .offer(
                &frame(2),
                2,
                2,
                Instant::now() + Duration::from_secs(1),
                |_| 0,
            )
            .unwrap();
        let current_live = Live {
            picture: current,
            pixels: Pixels::Kitty,
        };
        assert!(accepts_frame(&pacing, None, 7, &current_live));
        assert!(
            !accepts_frame(&pacing, None, 7, &old_live),
            "old encoder completion survived region removal"
        );
    }

    #[test]
    fn the_output_rate_is_measured_once_enough_was_written() {
        let mut rate = OutputRate::default();
        // Too little written to tell.
        rate.observe(10_000, Duration::from_millis(100));
        assert_eq!(rate.rate(), None);
        // 200 kB in a second: 200 kB a second.
        rate.observe(200_000, Duration::from_secs(1));
        assert_eq!(rate.rate(), Some(200_000));
        // Then 400 kB more in a second moves it toward 400 kB a second.
        rate.observe(600_000, Duration::from_secs(2));
        let now = rate.rate().unwrap();
        assert!((200_000..400_000).contains(&now), "{now}");
        // Writes that took no time say nothing.
        rate.observe(900_000, Duration::from_secs(2));
        assert_eq!(rate.rate(), Some(now));
    }
}

#[cfg(test)]
thread_local! {
    static TEST_OFFER: RefCell<Option<UpdateSink>> = const { RefCell::new(None) };
    static TEST_BROWSER: std::cell::Cell<Option<Browser>> = const { std::cell::Cell::new(None) };
    pub(crate) static TEST_READER_LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TEST_LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
mod update_measurements {
    use super::*;
    use crate::model::{Entry, Revisions, Run, build};
    use rich_intuituive::{Driver, Proxy};
    fn setup() -> (Driver, Browser, Page, Proxy) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        let engine = crate::testing::FakeEngine::from_dir(&root).unwrap();
        let mut page = build(
            &crate::testing::snapshot_file(&root.join("stream-with-log.json")).unwrap(),
            &mut Revisions::new(),
        );
        for run in page.blocks.iter().flat_map(|b| b.runs.iter()).chain(
            page.logs
                .iter()
                .flat_map(|l| l.entries.iter().flat_map(|e| e.runs.iter())),
        ) {
            if let Some(inline) = &run.picture
                && let Some(bytes) = engine.state.lock().unwrap().resource(&inline.src)
                && let Ok(picture) = Picture::decode(&bytes)
            {
                page.inline_pictures.insert(inline.src.clone(), picture);
            }
        }
        let images = match std::env::var("NOCTILUCA_MEASURE_PROTOCOL").as_deref() {
            Ok("kitty") => Images::Kitty,
            Ok("sixel") => Images::Sixel,
            _ => Images::Ansi,
        };
        let options = Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: None,
            view: Some(View::Projection),
            images: Some(images),
            tunnel: None,
            control: None,
        };
        let app = app(Box::new(engine), options);
        let proxy = app.proxy();
        let mut driver = app.driver(120, 40);
        driver.settle();
        let b = TEST_BROWSER.with(|slot| slot.get().unwrap());
        proxy.run_with({
            let page = page.clone();
            move |cx| {
                b.start.set(false);
                apply(b, Update::Page(Arc::new(page)), cx);
            }
        });
        driver.settle();
        (driver, b, page, proxy)
    }
    #[test]
    fn source_pair_is_atomic_and_external_legacy_replacement_cannot_reuse_it() {
        let (mut driver, b, _, proxy) = setup();
        let mut builder = crate::testing::PageBuilder::new("https://example.test/source", "source", 400.0, 400.0);
        let left = builder.element(builder.body(), "DIV", &[], &[], None);
        let right = builder.element(builder.body(), "DIV", &[], &[], None);
        let text = builder.text(left, "Source words", Rect::new(0.0, 0.0, 100.0, 18.0));
        let mut snapshot = builder.snapshot();
        let first = crate::model::build_source(&snapshot, &mut Revisions::new());
        let offer = |source: crate::model::SourcePage| {
            proxy.run_with(move |cx| apply_pair(b, Update::Page(source.page().clone()), Some(source), cx));
        };
        offer(first.clone());
        driver.settle();
        TEST_LAYOUTS.with(|n| n.set(0));
        snapshot.dom["documents"][0]["nodes"]["parentIndex"][text] = serde_json::json!(right);
        let changed = crate::model::build_source(&snapshot, &mut Revisions::new());
        assert_eq!(first.page(), changed.page());
        offer(changed.clone());
        driver.settle();
        assert_eq!(TEST_LAYOUTS.with(|n| n.get()), 1, "source-only change has one layout, no transient legacy pair");
        proxy.run_with(move |_| {
            let page = b.page.get_untracked().unwrap();
            b.source.with_untracked(|s| assert!(Arc::ptr_eq(s.as_ref().unwrap().page(), &page)));
        });
        TEST_LAYOUTS.with(|n| n.set(0));
        // Public callers can replace the legacy signal directly.
        proxy.run_with(move |_| { b.page.replace(Some(Arc::new(Page { document: 900, ..Default::default() }))); });
        driver.settle();
        assert_eq!(TEST_LAYOUTS.with(|n| n.get()), 1);
        proxy.run_with(move |cx| apply(b, Update::Page(first.page().clone()), cx));
        driver.settle();
        proxy.run_with(move |_| assert!(b.source.with_untracked(Option::is_none)));
        driver.settle();
    }

    fn put(driver: &mut Driver, proxy: &Proxy, b: Browser, page: Page) -> usize {
        TEST_LAYOUTS.with(|n| n.set(0));
        proxy.run_with(move |cx| apply(b, Update::Page(Arc::new(page)), cx));
        driver.update(Duration::ZERO);
        driver.render();
        TEST_LAYOUTS.with(|n| n.get())
    }
    #[test]
    fn playback_update_does_no_unrelated_layout() {
        let (mut d, b, mut p, proxy) = setup();
        p.media[1].playback = Some(Playback {
            time: 42.0,
            ..Playback::default()
        });
        assert_eq!(put(&mut d, &proxy, b, p), 0);
    }
    #[test]
    fn reader_playback_and_decode_reuse_reading_pieces() {
        let (mut d, b, mut p, proxy) = setup();
        proxy.run(move || b.view.set(View::Reader));
        d.settle();
        TEST_READER_LAYOUTS.with(|n| n.set(0));
        p.media[1].playback = Some(Playback {
            time: 42.0,
            ..Playback::default()
        });
        p.media[1].picture = Picture::from_rgba(1, 1, vec![10, 20, 30, 255]);
        assert_eq!(put(&mut d, &proxy, b, p), 0);
        assert_eq!(TEST_READER_LAYOUTS.with(|n| n.get()), 0);
    }
    #[test]
    fn unchanged_rectangle_decode_does_no_unrelated_layout() {
        let (mut d, b, mut p, proxy) = setup();
        p.media[1].picture = Picture::from_rgba(1, 1, vec![10, 20, 30, 255]);
        assert_eq!(put(&mut d, &proxy, b, p), 0);
    }
    #[test]
    fn side_by_side_log_append_preserves_scrollback_without_unrelated_layout() {
        let (mut d, b, mut p, proxy) = setup();
        let node = p.logs[0].node;
        p.logs[0].entries = (1..=20)
            .map(|key| Entry {
                key,
                runs: vec![Run::plain(format!("message {key}"))],
            })
            .collect();
        put(&mut d, &proxy, b, p.clone());
        proxy.run(move || {
            b.logs.update(|logs| {
                logs.insert(node, LogScroll::Paused(12));
            })
        });
        d.settle();
        p.logs[0].entries.push(Entry {
            key: 21,
            runs: vec![Run::plain("new message")],
        });
        assert_eq!(put(&mut d, &proxy, b, p.clone()), 0);
        proxy.run(move || {
            assert_eq!(
                *b.layout.get(),
                lay_out(&p, View::Projection, 120, &b.hint_marks())
            );
            assert_eq!(b.logs.get()[&node], LogScroll::Paused(12));
        });
        d.update(Duration::ZERO);
    }

    #[test]
    fn log_follow_inline_edits_and_geometry_match_full_composition() {
        let (_d, _b, mut p, _proxy) = setup();
        let mut retained = crate::presentation::Presentation::default();
        let marks = Marks::default();
        retained.update(&p, View::Projection, 120, &marks);
        let original = p.logs[0]
            .entries
            .iter()
            .find(|e| e.runs.iter().any(|r| r.picture.is_some()))
            .unwrap()
            .clone();
        for n in 0..10 {
            let mut entry = original.clone();
            entry.key = 500 + n;
            entry
                .runs
                .insert(0, Run::plain(format!("new 👩\u{200d}💻 e\u{301} {n} ")));
            p.logs[0].entries.push(entry);
            TEST_LAYOUTS.with(|n| n.set(0));
            let actual = retained.update(&p, View::Projection, 120, &marks);
            assert_eq!(TEST_LAYOUTS.with(|n| n.get()), 0);
            assert_eq!(*actual, lay_out(&p, View::Projection, 120, &marks));
        }
        p.logs[0].rect.width -= 16.0;
        TEST_LAYOUTS.with(|n| n.set(0));
        let actual = retained.update(&p, View::Projection, 120, &marks);
        assert_eq!(TEST_LAYOUTS.with(|n| n.get()), 1);
        assert_eq!(*actual, lay_out(&p, View::Projection, 120, &marks));
        p.media[1].rect.height += 18.0;
        let actual = retained.update(&p, View::Projection, 120, &marks);
        assert_eq!(*actual, lay_out(&p, View::Projection, 120, &marks));
        for playing in [false, true, false] {
            p.media[1].playback = Some(Playback {
                paused: !playing,
                ready: 4,
                ..Playback::default()
            });
            TEST_LAYOUTS.with(|n| n.set(0));
            let actual = retained.update(&p, View::Projection, 120, &marks);
            assert_eq!(TEST_LAYOUTS.with(|n| n.get()), 0);
            assert_eq!(*actual, lay_out(&p, View::Projection, 120, &marks));
        }
    }

    #[test]
    fn draft_and_focus_survive_combined_frame_and_chat_delivery() {
        let (mut d, b, mut p, proxy) = setup();
        let offer = TEST_OFFER.with(|slot| slot.borrow().as_ref().unwrap().clone());
        let index = p.controls.iter().position(|c| c.role.is_text()).unwrap();
        proxy.run_with(move |cx| activate(b, index, cx));
        d.settle();
        d.type_text("unsent draft");
        let focus = d.focused();
        assert!(focus.is_some());
        assert!(d.screen().plain().join("\n").contains("unsent draft"));
        let initial = b.reads.get_untracked();
        p.media[1].playback = Some(Playback {
            ready: 4,
            ..Playback::default()
        });
        for n in 1..=20 {
            p.logs[0].entries.push(Entry {
                key: 1000 + n,
                runs: vec![Run::plain(format!("chat {n}"))],
            });
            offer(Update::Page(Arc::new(p.clone())));
            offer(Update::Frame(Arc::new(VideoFrame {
                node: p.media[1].node,
                seq: n,
                pixels: rich_embed::Pixels::new(2, 2, vec![n as u8; 12]).unwrap(),
            })));
        }
        let until = Instant::now() + Duration::from_secs(2);
        while b.reads.get_untracked() < initial + 20 {
            d.update(Duration::ZERO);
            d.render();
            assert!(Instant::now() < until, "semantic pages were lost");
            std::thread::yield_now();
        }
        assert_eq!(d.focused(), focus);
        assert!(d.screen().plain().join("\n").contains("unsent draft"));
        let web = b
            .controls
            .with_untracked(|c| c[&p.controls[index].node].clone());
        assert!(web.draft.borrow().draft().contains("unsent draft"));
        assert_eq!(
            b.page.get_untracked().unwrap().logs[0]
                .entries
                .last()
                .unwrap()
                .key,
            1020
        );
    }

    #[test]
    fn retained_logs_match_full_projection_in_narrow_and_pinned_views() {
        let (_d, _b, original, _proxy) = setup();
        for width in [0, 8, 10, 80, 120] {
            for place in [
                crate::model::Place::Flow,
                crate::model::Place::Pinned,
                crate::model::Place::Docked,
            ] {
                let mut p = original.clone();
                if place != crate::model::Place::Flow {
                    let log = &p.logs[0];
                    p.layers.push(crate::model::Layer {
                        node: log.node,
                        fixed: true,
                        rect: log.rect,
                        top: 0.0,
                        place,
                    });
                    p.blocks
                        .iter_mut()
                        .find(|b| matches!(b.kind, crate::model::BlockKind::Log(0)))
                        .unwrap()
                        .stack
                        .layer = Some(p.layers.len() - 1);
                }
                let mut retained = crate::presentation::Presentation::default();
                let marks = Marks::default();
                retained.update(&p, View::Projection, width, &marks);
                let mut entry = p.logs[0]
                    .entries
                    .iter()
                    .find(|e| e.runs.iter().any(|r| r.picture.is_some()))
                    .unwrap()
                    .clone();
                entry.key = 991;
                p.logs[0].entries.push(entry);
                let actual = retained.update(&p, View::Projection, width, &marks);
                assert_eq!(*actual, lay_out(&p, View::Projection, width, &marks));
            }
        }
    }

    #[test]
    fn control_meaning_and_document_boundaries_invalidate_retained_layout() {
        let (_d, _b, mut p, _proxy) = setup();
        let mut retained = crate::presentation::Presentation::default();
        let marks = Marks::default();
        retained.update(&p, View::Projection, 120, &marks);
        p.controls[0].disabled = !p.controls[0].disabled;
        TEST_LAYOUTS.with(|n| n.set(0));
        let actual = retained.update(&p, View::Projection, 120, &marks);
        assert_eq!(TEST_LAYOUTS.with(|n| n.get()), 1);
        assert_eq!(*actual, lay_out(&p, View::Projection, 120, &marks));
        p.document += 1;
        TEST_LAYOUTS.with(|n| n.set(0));
        retained.update(&p, View::Projection, 120, &marks);
        assert_eq!(TEST_LAYOUTS.with(|n| n.get()), 1);
        retained.update(&p, View::Reader, 120, &marks);
        p.logs[0].entries.push(Entry {
            key: 9999,
            runs: vec![Run::plain("reader grows")],
        });
        TEST_LAYOUTS.with(|n| n.set(0));
        let actual = retained.update(&p, View::Reader, 120, &marks);
        assert_eq!(TEST_LAYOUTS.with(|n| n.get()), 1);
        assert_eq!(*actual, lay_out(&p, View::Reader, 120, &marks));
    }

    #[test]
    #[ignore = "measured update/render baseline; run with --ignored --nocapture"]
    fn combined_update_baseline() {
        let (mut d, b, mut p, proxy) = setup();
        let start = d.stats();
        eprintln!("RICH_MEDIA {{\"window\":\"begin\"}}");
        let mut latency = Vec::new();
        let mut layouts = 0;
        let mut bytes = 0;
        let mut emitted = 0;
        for n in 1..=100 {
            p.media[0].playback = Some(Playback {
                time: n as f64,
                ..Playback::default()
            });
            if n % 5 == 0 {
                p.media[0].picture = Picture::from_rgba(2, 2, vec![n as u8; 16]);
            }
            if n % 4 == 0 {
                p.logs[0].entries.push(Entry {
                    key: 1000 + n,
                    runs: vec![Run::plain(format!("message {n}"))],
                });
            }
            let page = Arc::new(p.clone());
            let at = Instant::now();
            TEST_LAYOUTS.with(|n| n.set(0));
            proxy.run_with(move |cx| apply(b, Update::Page(page), cx));
            d.update(Duration::ZERO);
            if let Some(out) = d.render() {
                bytes += out.len();
                emitted += 1;
                latency.push(at.elapsed().as_secs_f64() * 1000.0);
            }
            layouts += TEST_LAYOUTS.with(|n| n.get());
        }
        latency.sort_by(f64::total_cmp);
        let q = |p: f64| latency[((latency.len() - 1) as f64 * p).round() as usize];
        let end = d.stats();
        eprintln!("RICH_MEDIA {{\"window\":\"end\"}}");
        println!(
            "combined: config=120x40/visible-body/saved-stream-with-log/100-serial-updates reads=100 playback=100 decode=20 log=25 layouts={layouts} emitted={emitted} censored={} exact_bytes={bytes} latency_ms p50={} p95={} p99={} widget_draw_ms={} paint_encode_ms={}",
            100 - emitted,
            q(0.5),
            q(0.95),
            q(0.99),
            (end.total_draw - start.total_draw).as_secs_f64() * 1000.0,
            (end.total_paint - start.total_paint).as_secs_f64() * 1000.0
        );
    }
    #[test]
    #[ignore = "combined frame/chat pipeline; emitted bytes, not native presentation"]
    fn combined_frame_pipeline() {
        use std::sync::Mutex;
        let (mut d, b, mut p, _proxy) = setup();
        let stream = p
            .media
            .iter()
            .position(|m| m.kind == MediaKind::Video)
            .unwrap();
        let offer = TEST_OFFER.with(|slot| slot.borrow().as_ref().unwrap().clone());
        p.media[stream].playback = Some(Playback {
            ready: 4,
            volume: 1.0,
            ..Playback::default()
        });
        let template = p.logs[0]
            .entries
            .iter()
            .find(|e| e.runs.iter().any(|r| r.picture.is_some()))
            .unwrap()
            .runs
            .clone();
        p.logs[0].entries = (1..=30)
            .map(|key| {
                let mut runs = vec![Run::plain(format!("chat {key} "))];
                runs.extend(template.clone());
                Entry { key, runs }
            })
            .collect();
        offer(Update::Page(Arc::new(p.clone())));
        for _ in 0..20 {
            d.update(Duration::ZERO);
            d.render();
            std::thread::sleep(Duration::from_millis(2));
        }
        let node = p.media[stream].node;
        assert_eq!(p.media[stream].kind, MediaKind::Video);
        assert!(!b.start.get_untracked());
        let region = b
            .layout
            .get_untracked()
            .regions
            .iter()
            .find(|r| r.media == stream)
            .copied()
            .unwrap();
        assert_eq!(
            (region.cols, region.rows),
            (41, 14),
            "fixture stream cells changed"
        );
        assert!(region.row + region.rows < 40 && region.column < p.logs[0].rect.x as usize / 8);
        assert!(!b.layout.get_untracked().inline.is_empty());
        assert!(d.screen().plain().join("\n").contains("chat"));
        assert!(!p.inline_pictures.is_empty());
        let visible_tags: Vec<_> = d
            .screen()
            .lines()
            .into_iter()
            .flatten()
            .filter_map(|s| s.style.as_ref().and_then(rich_art::picture::PictureTag::of))
            .collect();
        if b.images.get_untracked() == Images::Kitty {
            assert!(
                visible_tags.iter().any(|tag| p
                    .inline_pictures
                    .values()
                    .any(|picture| picture.id() == tag.id)),
                "no inline picture in rendered body"
            );
        }
        eprintln!(
            "workload: stream_node={node} kind={:?} rect={:?} cells={}x{} inline_tags={}",
            p.media[stream].kind,
            p.media[stream].rect,
            region.cols,
            region.rows,
            visible_tags.len()
        );
        let stamps = Arc::new(Mutex::new(HashMap::<u64, Instant>::new()));
        let emitted = Arc::new(Mutex::new(std::collections::HashSet::<u64>::new()));
        let sent = stamps.clone();
        let begun = Instant::now();
        TEST_LAYOUTS.with(|n| n.set(0));
        let start = d.stats();
        let initial_reads = b.reads.get_untracked();
        eprintln!("RICH_MEDIA {{\"window\":\"begin\",\"workload\":\"frames+chat\"}}");
        let producer = std::thread::spawn(move || {
            for seq in 1..=60u64 {
                sent.lock().unwrap().insert(seq, Instant::now());
                let pixels =
                    rich_embed::Pixels::new(480, 270, [seq as u8, 77, 91].repeat(480 * 270))
                        .unwrap();
                eprintln!(
                    "RICH_MEDIA {{\"stage\":\"frame_offer\",\"seq\":{seq},\"bytes\":{}}}",
                    pixels.rgb.len()
                );
                offer(Update::Frame(Arc::new(VideoFrame { node, pixels, seq })));
                if seq % 3 == 0 {
                    let mut runs = vec![Run::plain(format!("new chat {seq} "))];
                    runs.extend(template.clone());
                    p.logs[0].entries.push(Entry {
                        key: 1000 + seq,
                        runs,
                    });
                    p.media[stream].playback.as_mut().unwrap().time = seq as f64 / 30.0;
                    offer(Update::Page(Arc::new(p.clone())));
                }
                std::thread::sleep(Duration::from_millis(33));
            }
        });
        let mut bytes = 0usize;
        let mut nonempty = 0;
        let mut empty = 0;
        let mut latency = Vec::new();
        let mut updates = Vec::new();
        let mut inputs = Vec::new();
        let mut last_frame = 0;
        let mut next_input = Duration::from_millis(200);
        let mut input = None;
        let mut input_count = 0;
        while begun.elapsed() < Duration::from_millis(2400) {
            let elapsed = begun.elapsed();
            if elapsed >= next_input && input_count < 8 {
                input = Some(Instant::now());
                let key = if input_count % 2 == 0 { "{" } else { "}" };
                d.event(rich_intuituive::interact::Event::Key(
                    Key::parse(key).unwrap(),
                ));
                input_count += 1;
                next_input += Duration::from_millis(200);
            }
            let at = Instant::now();
            d.update(elapsed);
            updates.push(at.elapsed().as_secs_f64() * 1000.0);
            if let Some(out) = d.render() {
                if out.is_empty() {
                    empty += 1;
                } else {
                    nonempty += 1;
                    bytes += out.len();
                    if let Some(at) = input.take() {
                        inputs.push(at.elapsed().as_secs_f64() * 1000.0);
                    }
                    if let Some(live) = b.videos.get_untracked().get(&node) {
                        let seq = live.picture.image().to_rgb8().get_pixel(0, 0).0[0] as u64;
                        let visible = if live.pixels == Pixels::Cells {
                            out.contains(&format!(";2;{seq};77;91"))
                        } else {
                            let tag = rich_art::picture::PictureTag {
                                id: live.picture.id(),
                                version: live.picture.version(),
                                cols: region.cols,
                                rows: region.rows,
                                row: 0,
                                pixels: live.pixels,
                                fit: Some(rich_art::ImageFit::Cover),
                                anchor: rich_art::ImageAnchor::Center,
                            };
                            live.picture
                                .encode_tag(&tag, 0, region.rows)
                                .is_some_and(|encoded| out.contains(&encoded))
                        };
                        if visible && seq != last_frame && stamps.lock().unwrap().contains_key(&seq)
                        {
                            last_frame = seq;
                            emitted.lock().unwrap().insert(seq);
                            latency.push(
                                stamps.lock().unwrap()[&seq].elapsed().as_secs_f64() * 1000.0,
                            );
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        producer.join().unwrap();
        eprintln!("RICH_MEDIA {{\"window\":\"end\"}}");
        let q = |values: &mut Vec<f64>| {
            values.sort_by(f64::total_cmp);
            [0.5, 0.95, 0.99].map(|p| {
                values
                    .get(((values.len().saturating_sub(1)) as f64 * p).round() as usize)
                    .copied()
                    .unwrap_or(0.0)
            })
        };
        let end = d.stats();
        let update_total_ms: f64 = updates.iter().sum();
        println!(
            "pipeline: config=120x40/480x270-RGB/30fps/2s-source/2.4s-drain applied_pages={} actual_layouts={} offered_frames=60 emitted_frame_ids={} dropped_or_censored={} nonempty_outputs={nonempty} empty_outputs={empty} exact_bytes={bytes} frame_latency_ms={:?} update_latency_ms={:?} input_population={} input_censored={} input_latency_ms={:?} update_total_ms={update_total_ms} widget_draw_ms={} paint_encode_ms={}",
            b.reads.get_untracked() - initial_reads,
            TEST_LAYOUTS.with(|n| n.get()),
            latency.len(),
            60 - latency.len(),
            q(&mut latency),
            q(&mut updates),
            inputs.len(),
            input_count - inputs.len(),
            q(&mut inputs),
            (end.total_draw - start.total_draw).as_secs_f64() * 1000.0,
            (end.total_paint - start.total_paint).as_secs_f64() * 1000.0
        );
        assert!(nonempty > 0);
        assert!(!latency.is_empty(), "no real frame emitted");
    }
}
