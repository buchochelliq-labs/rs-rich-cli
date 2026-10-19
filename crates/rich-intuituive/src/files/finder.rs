//! A fuzzy finder over a directory walk, whose results stream in.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rich::markup::escape;
use rich_ext::listing::Walk;
use rich_interact::fuzzy::fuzzy;

use crate::app::Ctx;
use crate::input::input;
use crate::list::list_lazy;
use crate::node::{column, row, text, Node};
use crate::reactive::{memo, signal, watch, Memo, Signal};

/// The most paths a finder collects from its walk.
const LIMIT: usize = 100_000;
/// The most matches it lists (the count says how many there are).
const SHOWN: usize = 1_000;
/// How often found paths reach the app while the walk goes on.
const EVERY: Duration = Duration::from_millis(50);

/// The state of a [`finder`]: what is typed, what the walk has found so
/// far, and whether it is still going.
#[derive(Clone, Copy)]
pub struct Finder {
    pub query: Signal<String>,
    /// The paths found, relative to the walk's root, in the order found.
    pub found: Signal<Vec<String>>,
    /// The walk has not finished.
    pub walking: Signal<bool>,
    /// The hovered match.
    pub cursor: Signal<usize>,
    ranked: Memo<Ranked>,
}

/// The matches to list: indices into `found` with the matched characters,
/// best first, and how many match in all.
#[derive(Clone, Debug, Default, PartialEq)]
struct Ranked {
    total: usize,
    shown: Vec<(usize, Vec<usize>)>,
}

/// What has been matched so far, so a batch of new paths is matched alone.
#[derive(Default)]
struct Cache {
    query: String,
    scanned: usize,
    /// (score, length, index, positions).
    hits: Vec<(i64, usize, usize, Vec<usize>)>,
}

impl Finder {
    /// The path of the hovered match, relative to the root.
    pub fn hovered(&self) -> Option<String> {
        let at = self.cursor.get_untracked();
        let index = self
            .ranked
            .with_untracked(|r| r.shown.get(at).map(|(i, _)| *i))?;
        self.found.with_untracked(|f| f.get(index).cloned())
    }
}

/// A fuzzy finder over the paths `walk` finds (rs-rich-ext's
/// [`walk`](rich_ext::listing::walk): `walk(root).hidden(false)`), as
/// Yazi's `z` is over fd and fzf. The walk runs on a thread of its own
/// and its paths reach the list in batches while it goes on, so the first
/// results show at once and typing never waits for a big tree; a new batch
/// is matched alone, not the whole list again. Matching is rs-rich-interact's
/// [`fuzzy`] (smart case, words in any order), best first; the walk stops
/// when the finder's screen closes. Up and Down (Ctrl+P, Ctrl+N) move,
/// Enter calls `on_pick` with the hovered path (the root joined to it).
///
/// ```
/// use intuituive::files::finder;
/// use intuituive::prelude::*;
/// use rich_ext::listing::walk;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-finder-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("src")).unwrap();
/// std::fs::write(dir.join("src/main.rs"), "").unwrap();
/// std::fs::write(dir.join("README.md"), "").unwrap();
/// let root = dir.clone();
/// let app = App::new(move || {
///     let root = root.clone();
///     let picked = signal(String::new());
///     column([
///         finder(walk(&root), move |path, _| {
///             picked.set(path.strip_prefix(&root).unwrap().display().to_string())
///         })
///         .flex(1),
///         text!("{picked}").fixed(1),
///     ])
///     .on_key("ctrl+q", |cx| cx.quit())
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(40, 6);
/// driver.type_text("mrs");
/// driver.press("enter");
/// let shown = driver.screen().plain()[5].trim_end().replace('\\', "/");
/// assert_eq!(shown, "src/main.rs");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn finder(walk: Walk, on_pick: impl Fn(PathBuf, &mut Ctx) + 'static) -> Node {
    let root = walk.root().to_path_buf();
    let query = signal(String::new());
    let found = signal(Vec::<String>::new());
    let walking = signal(true);
    let cursor = signal(0usize);
    let cache = Rc::new(RefCell::new(Cache::default()));
    let ranked = memo(move || {
        let query = query.get();
        let mut cache = cache.borrow_mut();
        found.with(|items| {
            if cache.query != query || items.len() < cache.scanned {
                *cache = Cache {
                    query: query.clone(),
                    ..Cache::default()
                };
            }
            let start = cache.scanned;
            for (index, item) in items.iter().enumerate().skip(start) {
                if let Some(m) = fuzzy(&query, item) {
                    cache
                        .hits
                        .push((m.score, item.chars().count(), index, m.positions));
                }
            }
            cache.scanned = items.len();
            if !query.trim().is_empty() && start < items.len() {
                cache
                    .hits
                    .sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
            }
            Ranked {
                total: cache.hits.len(),
                shown: cache
                    .hits
                    .iter()
                    .take(SHOWN)
                    .map(|(_, _, i, p)| (*i, p.clone()))
                    .collect(),
            }
        })
    });
    let state = Finder {
        query,
        found,
        walking,
        cursor,
        ranked,
    };
    stream(walk, state);
    // A new query starts at the best match.
    watch(move || query.get(), move |_, _| cursor.set(0));
    let status = text(move || {
        let (total, all) = (ranked.with(|r| r.total), found.with(Vec::len));
        let going = if walking.get() { " (walking…)" } else { "" };
        format!("[bright_black]  {total}/{all}{going}")
    })
    .no_wrap()
    .fixed(1);
    let item = move |i: usize| {
        ranked.with(|r| {
            let Some((index, positions)) = r.shown.get(i) else {
                return String::new();
            };
            found.with(|f| {
                f.get(*index)
                    .map(|p| highlight(p, positions))
                    .unwrap_or_default()
            })
        })
    };
    let results = list_lazy(move || ranked.with(|r| r.shown.len()), item, cursor)
        .no_focus()
        .flex(1);
    let field = row([
        text(|| "[bold]>[/] ".into()).fixed(2),
        input(query).flex(1).autofocus(),
    ])
    .fixed(1);
    column([field, status, results])
        .bind("up ctrl+p", "Previous match", move |_| {
            cursor.update(|c| *c = c.saturating_sub(1))
        })
        .bind("down ctrl+n", "Next match", move |_| {
            let len = ranked.with_untracked(|r| r.shown.len());
            cursor.update(|c| *c = (*c + 1).min(len.saturating_sub(1)))
        })
        .bind("enter", "Go to the match", move |cx| {
            if let Some(path) = state.hovered() {
                on_pick(root.join(path), cx);
            }
        })
        .name("finder")
}

/// `path` with the characters at `positions` picked out, in markup.
fn highlight(path: &str, positions: &[usize]) -> String {
    let mut out = String::new();
    for (i, c) in path.chars().enumerate() {
        let c = escape(&c.to_string());
        if positions.binary_search(&i).is_ok() {
            out.push_str(&format!("[bold yellow]{c}[/]"));
        } else {
            out.push_str(&c);
        }
    }
    out
}

/// Run `walk` on a thread, sending what it finds to `state.found` in
/// batches; stop when the screen the finder is on closes.
fn stream(walk: Walk, state: Finder) {
    let stop = Arc::new(AtomicBool::new(false));
    // A watch made with the screen is dropped when it closes, and its
    // closure with it: the guard then stops the walk.
    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let guard = Guard(stop.clone());
    watch(
        || (),
        move |_, _| {
            let _ = &guard;
        },
    );
    let (found, walking) = (state.found, state.walking);
    super::background("finder()", move |proxy| {
        let root = walk.root().to_path_buf();
        let mut batch = Vec::new();
        let mut last = Instant::now();
        let send = |batch: Vec<String>| proxy.run(move || found.update(|f| f.extend(batch)));
        for path in walk.take(LIMIT) {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let rel = path.strip_prefix(&root).unwrap_or(&path);
            batch.push(rel.to_string_lossy().into_owned());
            if last.elapsed() >= EVERY {
                last = Instant::now();
                if !send(std::mem::take(&mut batch)) {
                    return;
                }
            }
        }
        send(batch);
        proxy.run(move || walking.set(false));
    });
}
