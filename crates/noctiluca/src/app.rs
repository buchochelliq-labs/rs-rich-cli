//! The browser as an intuiTUIve app: the address line, the page (the
//! projection by default, reader mode one key away), the status line, and
//! the action registry's keys, hints, find, F1 overlay and site keys.
//!
//! The tab runs on a thread of its own ([`crate::tab`]); its updates come
//! to the app's thread through a proxy, into signals the nodes read.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rich_embed::PageState;
use rich_intuituive::interact::keymap::{try_keys, Keymap};
use rich_intuituive::interact::{Key, KeyCode};
use rich_intuituive::prelude::*;
use rich_intuituive::{
    button_of, image_graphics, picture_bytes, picture_of, scroll_virtual_with, slider,
    spawn_stream, Action, ImageView, Picture, Pixels,
};

use crate::actions::{self, ACTIONS};
use crate::bookmarks::{Bookmark, Bookmarks};
use crate::cdp::CELL;
use crate::clean::{markup, truncate};
use crate::config::{remove_site_key, save_site_key, Config, Images, Site, Target, View};
use crate::engine::{Engine, HistoryStep, MediaOp, NodeId, Playback};
use crate::layout::{self, hint_labels, matches, to_text, Line, Marks, Piece, Projection};
use crate::model::{Binding, MediaKind, Page, Role};
use crate::pictures::{self, Live};
use crate::tab::{Command, Intent, Refusal, Tab, Update};
use crate::video::{Pacing, VideoFrame};
use crate::widgets::{web_widget, WebControl};

/// The video regions on screen and the pacing's intervals, as told to the
/// tab.
type RegionsSent = (Vec<NodeId>, Vec<(NodeId, Duration)>);

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
    /// The regions on screen and the pacing's intervals, as last told to
    /// the tab.
    regions_sent: Signal<RegionsSent>,
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

    /// What the views lay out with: hint labels only (find marks are drawn
    /// over the layout, not part of it).
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
        }
    }

    fn say(&self, markup: impl Into<String>) {
        self.message.set(markup.into());
    }
}

/// The page's lines in the current view at `width`, as find counts rows:
/// every piece of reader mode in turn (a media region or a control as its
/// rows of nothing), or the projection, with each row's key and place on
/// the page.
fn lay_out(page: &Page, view: View, width: u16, marks: &Marks) -> Projection {
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
        _ => layout::project(page, width, (CELL.0 as f64, CELL.1 as f64), marks),
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
    let page = b.page.get_untracked();
    let images = b.images.get_untracked();
    let next = match page {
        Some(page) if images != Images::None && b.view.get_untracked() != View::Reader => {
            let first = b.scroll.get_untracked() as usize;
            let rows = first..first + usize::from(b.viewport.get_untracked().1);
            b.layout.with_untracked(|l| {
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
        _ => HashMap::new(),
    };
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
        "back" => drop(b.send(Command::History(HistoryStep::Back))),
        "forward" => drop(b.send(Command::History(HistoryStep::Forward))),
        "reload" => drop(b.send(Command::History(HistoryStep::Reload))),
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
    let (picture, pixels) = b.pacing.with_untracked(|p| {
        let mut pacing = p.borrow_mut();
        let picture = pacing.offer(frame, cols, rows, Instant::now(), picture_bytes);
        (picture, pacing.pixels(frame.node))
    });
    if let Some(picture) = picture {
        let live = Live { picture, pixels };
        let mut videos = (*b.videos.get_untracked()).clone();
        videos.insert(frame.node, live.clone());
        b.videos.set(Rc::new(videos));
        // Shown at once, with the moving GIFs.
        let mut shown = (*b.live.get_untracked()).clone();
        shown.insert(frame.node, live);
        b.live.set(Rc::new(shown));
    }
    send_regions(b);
}

/// The video regions the view shows: those whose box meets the page's
/// rows in view.
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
                    .filter(|m| m.kind == MediaKind::Video)
                    .filter(|m| m.rect.bottom() > top && m.rect.y < bottom)
                    .map(|m| m.node)
                    .collect()
            })
            .unwrap_or_default()
    })
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
            // In steps of 25 ms, so a small change in the estimate does
            // not send again.
            .map(|(node, d)| (node, Duration::from_millis(d.as_millis() as u64 / 25 * 25)))
            .collect()
    });
    let next = (on_screen, intervals);
    if b.regions_sent.with_untracked(|sent| *sent != next) {
        b.regions_sent.set(next.clone());
        b.send(Command::Regions {
            on_screen: next.0,
            intervals: next.1,
        });
    }
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
            if let Some(v) = v {
                if volume.get_untracked() != v as f64 {
                    volume.set(v as f64);
                }
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
    {
        b.view.set(view);
    }
    let (columns, rows) = b.viewport.get_untracked();
    let width = site.as_ref().and_then(|s| s.width).unwrap_or(columns);
    b.send(Command::Resize(width, rows.saturating_sub(CHROME_ROWS)));
}

/// Apply one of the tab's updates, on the app's thread.
fn apply(b: Browser, update: Update, cx: &mut Ctx) {
    match update {
        Update::Page(page) => {
            let new_document =
                b.page.with_untracked(|p| p.as_ref().map(|p| p.document)) != Some(page.document);
            if new_document {
                b.controls.update(HashMap::clear);
                b.videos.set(Rc::new(HashMap::new()));
                b.pacing.with_untracked(|p| p.borrow_mut().clear());
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
        Update::Frame(frame) => video_frame(b, &frame),
        Update::Still(node) => {
            b.pacing.with_untracked(|p| p.borrow_mut().still(node));
            send_regions(b);
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
        return Some((live.picture, live.pixels));
    }
    let picture = b.page.with(|p| {
        p.as_ref()?
            .media
            .iter()
            .find(|m| m.node == node)?
            .picture
            .clone()
    })?;
    Some((picture, b.pixels.get()))
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
    // The pieces and their keys, made together so they always agree.
    let reading = memo(move || {
        let width = b.viewport.get().0;
        let marks = b.hint_marks();
        let images = b.images.get();
        b.page.with(|p| {
            p.as_ref()
                .map(|p| {
                    let pieces = layout::reader(p, width.saturating_sub(2), &marks);
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
                let picture = picture_of(move || shown_picture(b, node).map(|(p, _)| p), view);
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
/// the view asks again only once it has more rows.
fn load_more(b: Browser) {
    if b.page.with_untracked(Option::is_some) {
        b.send(Command::LoadMore);
    }
}

fn projection_view(b: Browser) -> Node {
    // The pictures' cells, made once per frame and size, shared by rows.
    let cells = Rc::new(pictures::Cells::default());
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
            b.layout.with(|l| {
                let row = l.rows.get(&key).copied().unwrap_or(i);
                let Some(line) = l.lines.get(row) else {
                    return vec![Vec::new()];
                };
                let text = to_text(std::slice::from_ref(line), &marks, row);
                let options = console.options().update_width(width);
                let segments = console
                    .render_lines(&text, &options, false)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                // Only a row a picture covers reads the pictures.
                if b.images.get() == Images::None || !l.regions.iter().any(|r| r.on(row)) {
                    return vec![segments];
                }
                let nodes: Vec<Option<NodeId>> = b.page.with(|p| {
                    l.regions
                        .iter()
                        .map(|r| p.as_ref()?.media.get(r.media).map(|m| m.node))
                        .collect()
                });
                let spliced = pictures::splice(
                    console,
                    &cells,
                    segments,
                    width,
                    row,
                    &l.regions,
                    |region| {
                        let at = l.regions.iter().position(|r| r == region)?;
                        shown_picture(b, nodes.get(at).copied().flatten()?)
                    },
                );
                vec![b.page.with(|p| match p {
                    Some(page) => play_marks(spliced, width, row, &l.regions, page),
                    None => spliced,
                })]
            })
        })
        .fixed(1)
    };
    scroll_virtual_with(
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
}

/// The rows the header and the status bar take.
pub const CHROME_ROWS: u16 = 4;

/// The frames' pacing: within the guessed byte budget, at most the
/// config's frame rate and never more than eight a second, starting at
/// the protocol the user asked for, else character art over SSH, else
/// what the terminal speaks. Where the painter measures the terminal's
/// output rate (its frame budget), the budget should come from that.
fn video_pacing(config: &Config, images: Images) -> Pacing {
    let mut budget = rich_art::picture::ByteBudget::guess();
    budget.max_fps = (config.fps as f64).clamp(1.0, crate::video::MAX_FPS as f64);
    let asked = (images != Images::Auto).then(|| pictures::pixels(images));
    Pacing::new(budget, Pacing::start_for(asked, Pixels::detect()))
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
        let page: Signal<Option<Arc<Page>>> = signal(None);
        let view: Signal<View> = signal(if first_view == View::Raster {
            View::Projection
        } else {
            first_view
        });
        let hints: Signal<Option<Hints>> = signal(None);
        let find: Signal<Option<String>> = signal(None);
        let port = viewport();
        let layout = memo(move || {
            let width = port.get().0;
            let marks = Marks {
                hints: hints
                    .with(|h| {
                        h.as_ref()
                            .map(|h| h.labels.iter().map(|(l, c)| (*c, l.clone())).collect())
                    })
                    .unwrap_or_default(),
                ..Marks::default()
            };
            let view = view.get();
            Rc::new(page.with(|p| {
                p.as_ref()
                    .map(|p| lay_out(p, view, width, &marks))
                    .unwrap_or_default()
            }))
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
        let pacing = video_pacing(&config, images);
        let b = Browser {
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
            pixels: memo(move || pictures::pixels(images_signal.get())),
            frozen: signal(false),
            live: signal(Rc::new(HashMap::new())),
            tab: signal(tab),
            waiting: signal(HashMap::new()),
            controls: signal(HashMap::new()),
            videos: signal(Rc::new(HashMap::new())),
            pacing: signal(Rc::new(RefCell::new(pacing))),
            regions_sent: signal((Vec::new(), Vec::new())),
        };
        spawn_stream(move |stream| {
            let proxy = stream.proxy().clone();
            while let Ok(update) = receive.recv() {
                if !proxy.run_with(move |cx| apply(b, update, cx)) {
                    break;
                }
            }
        });
        // Moving pictures in view step within the byte budget.
        let motion = Rc::new(std::cell::RefCell::new(pictures::Motion::default()));
        every(MOTION, move |_| step_motion(b, &mut motion.borrow_mut()));
        // Chrome lays the page out at the terminal's size.
        watch(
            move || b.viewport.get(),
            move |(columns, rows), _| {
                b.send(Command::Resize(columns, rows.saturating_sub(CHROME_ROWS)));
            },
        );
        b.send(Command::Open(start.clone()));
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
