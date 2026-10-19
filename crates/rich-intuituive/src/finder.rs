//! A fuzzy finder over any stream of items, whose results stream in.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rich::markup::escape;
use rich_interact::fuzzy::fuzzy;

use crate::app::Ctx;
use crate::input::input;
use crate::list::list_lazy;
use crate::node::{column, row, text, Node};
use crate::reactive::{memo, signal, watch, Memo, Signal};
use crate::task::spawn_stream;

/// The most matches a finder lists (the count says how many there are).
const SHOWN: usize = 1_000;
/// How often found items reach the app while the stream goes on.
const EVERY: Duration = Duration::from_millis(50);

/// The state of a [`finder_of`]: what is typed, what the stream has given
/// so far, and whether it is still going.
pub struct Finder<T: 'static> {
    pub query: Signal<String>,
    /// The items found, in the order found.
    pub found: Signal<Vec<T>>,
    /// The stream has not finished.
    pub streaming: Signal<bool>,
    /// The hovered match.
    pub cursor: Signal<usize>,
    /// Each found item's text, matched and shown.
    labels: Signal<Vec<String>>,
    ranked: Memo<Ranked>,
}

impl<T> Clone for Finder<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Finder<T> {}

/// The matches to list: indices into `found` with the matched characters,
/// best first, and how many match in all.
#[derive(Clone, Debug, Default, PartialEq)]
struct Ranked {
    total: usize,
    shown: Vec<(usize, Vec<usize>)>,
}

/// What has been matched so far, so a batch of new items is matched alone.
#[derive(Default)]
struct Cache {
    query: String,
    scanned: usize,
    /// (score, length, index, positions).
    hits: Vec<(i64, usize, usize, Vec<usize>)>,
}

impl<T: Clone + 'static> Finder<T> {
    /// The hovered match.
    pub fn hovered(&self) -> Option<T> {
        let at = self.cursor.get_untracked();
        let index = self
            .ranked
            .with_untracked(|r| r.shown.get(at).map(|(i, _)| *i))?;
        self.found.with_untracked(|f| f.get(index).cloned())
    }
}

/// A fuzzy finder over the items `items` gives, each matched and shown as
/// the text `display` makes of it: a file walk, a command list, the lines
/// of a log, a database's rows. `items` is read on a thread of its own and
/// its items reach the list in batches while it goes on, so the first
/// results show at once and typing never waits for a long stream; a new
/// batch is matched alone, not the whole list again. Matching is
/// rs-rich-interact's [`fuzzy`] (smart case, words in any order), best
/// first; the stream stops when the finder's screen closes. Up and Down
/// (Ctrl+P, Ctrl+N) move, Enter calls `on_pick` with the hovered item.
///
/// ```
/// use intuituive::finder_of;
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let picked = signal(String::new());
///     let commands = vec!["open file", "save all", "close window"];
///     column([
///         finder_of(commands, |c| c.to_string(), move |c, _| picked.set(c.to_string())).flex(1),
///         text!("{picked}").fixed(1),
///     ])
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(30, 6);
/// driver.type_text("sva");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[5].trim_end(), "save all");
/// ```
pub fn finder_of<I, T>(
    items: I,
    display: impl Fn(&T) -> String + Send + 'static,
    on_pick: impl Fn(T, &mut Ctx) + 'static,
) -> Node
where
    I: IntoIterator<Item = T> + Send + 'static,
    T: Clone + Send + 'static,
{
    let query = signal(String::new());
    let found = signal(Vec::<T>::new());
    let labels = signal(Vec::<String>::new());
    let streaming = signal(true);
    let cursor = signal(0usize);
    let cache = Rc::new(RefCell::new(Cache::default()));
    let ranked = memo(move || {
        let query = query.get();
        let mut cache = cache.borrow_mut();
        labels.with(|items| {
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
        streaming,
        cursor,
        labels,
        ranked,
    };
    stream(items, display, state);
    // A new query starts at the best match.
    watch(move || query.get(), move |_, _| cursor.set(0));
    let status = text(move || {
        let (total, all) = (ranked.with(|r| r.total), labels.with(Vec::len));
        let going = if streaming.get() { " (loading…)" } else { "" };
        format!("[bright_black]  {total}/{all}{going}")
    })
    .no_wrap()
    .fixed(1);
    let item = move |i: usize| {
        ranked.with(|r| {
            let Some((index, positions)) = r.shown.get(i) else {
                return String::new();
            };
            labels.with(|f| {
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
            if let Some(item) = state.hovered() {
                on_pick(item, cx);
            }
        })
        .name("finder")
}

/// `text` with the characters at `positions` picked out, in markup.
fn highlight(text: &str, positions: &[usize]) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        let c = escape(&c.to_string());
        if positions.binary_search(&i).is_ok() {
            out.push_str(&format!("[bold yellow]{c}[/]"));
        } else {
            out.push_str(&c);
        }
    }
    out
}

/// Read `items` on a thread, sending them (and their labels) to the
/// finder in batches; stop when the screen the finder is on closes.
fn stream<I, T>(items: I, display: impl Fn(&T) -> String + Send + 'static, state: Finder<T>)
where
    I: IntoIterator<Item = T> + Send + 'static,
    T: Send + 'static,
{
    let (found, labels, streaming) = (state.found, state.labels, state.streaming);
    let task = spawn_stream(move |stream| {
        let proxy = stream.proxy();
        let mut batch = Vec::new();
        let mut last = Instant::now();
        let send = |batch: Vec<T>| {
            let shown: Vec<String> = batch.iter().map(&display).collect();
            proxy.run(move || {
                labels.update(|l| l.extend(shown));
                found.update(|f| f.extend(batch));
            })
        };
        for item in items {
            if stream.cancelled() {
                break;
            }
            batch.push(item);
            if last.elapsed() >= EVERY {
                last = Instant::now();
                if !send(std::mem::take(&mut batch)) {
                    return;
                }
            }
        }
        send(batch);
        proxy.run(move || streaming.set(false));
    });
    // A watch made with the screen is dropped when it closes, and its
    // closure with it: the guard then stops the stream.
    struct Guard(crate::task::Task);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
    let guard = Guard(task);
    watch(
        || (),
        move |_, _| {
            let _ = &guard;
        },
    );
}
