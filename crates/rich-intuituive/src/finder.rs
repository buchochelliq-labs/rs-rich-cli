//! A fuzzy list over any source of items, whose results stream in:
//! [`fuzzy_list`], and [`finder_of`] on it over an iterator.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::clock::Instant;

use rich::markup::escape;
use rich_interact::fuzzy::fuzzy;

use crate::app::Ctx;
use crate::border::Border;
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

/// Where a [`fuzzy_list`]'s items come from: an iterator, or a producer
/// that feeds items as they arrive. Either runs on a thread of its own and
/// stops when the list's screen closes.
pub struct FuzzySource<T> {
    run: Produce<T>,
}

/// What feeds a [`FuzzySource`]'s items, on its thread.
type Produce<T> = Box<dyn FnOnce(&mut Feed<T>) + Send>;

/// What Esc runs on a [`fuzzy_list`], instead of closing its screen.
type OnCancel = Box<dyn FnMut(&mut Ctx)>;

impl<T: Send + 'static> FuzzySource<T> {
    /// The items `items` gives, read on a thread: a directory walk, a
    /// file's lines, a long list.
    pub fn iter<I>(items: I) -> FuzzySource<T>
    where
        I: IntoIterator<Item = T> + Send + 'static,
    {
        FuzzySource::feed(move |feed| {
            for item in items {
                if !feed.push(item) {
                    break;
                }
            }
        })
    }

    /// The items `produce` pushes into its [`Feed`], on a thread: a
    /// search's results as a program prints them, a socket's messages.
    /// It should stop once [`push`](Feed::push) says false.
    ///
    /// ```
    /// use intuituive::finder::{fuzzy_list, FuzzySource};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let source = FuzzySource::feed(|feed| {
    ///         for n in 0..3 {
    ///             feed.push(format!("result {n}"));
    ///         }
    ///     });
    ///     fuzzy_list(source, String::clone, |_, _| {})
    /// })
    /// .wait_for_tasks(true);
    /// let mut driver = app.driver(20, 5);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[1].trim(), "3/3");
    /// ```
    pub fn feed(produce: impl FnOnce(&mut Feed<T>) + Send + 'static) -> FuzzySource<T> {
        FuzzySource {
            run: Box::new(produce),
        }
    }
}

/// What a [`FuzzySource::feed`] producer pushes its items into: they reach
/// the list in batches, every 50 ms while it goes on.
pub struct Feed<T> {
    batch: Vec<T>,
    last: Instant,
    /// Sends a batch to the list; false once the list is gone.
    send: Box<dyn FnMut(Vec<T>) -> bool + Send>,
    stream: crate::task::Stream,
    open: bool,
}

impl<T> Feed<T> {
    /// Add `item`; false once the list's screen has closed (stop then).
    pub fn push(&mut self, item: T) -> bool {
        if !self.open || self.stream.cancelled() {
            self.open = false;
            return false;
        }
        self.batch.push(item);
        if self.last.elapsed() >= EVERY {
            self.last = Instant::now();
            self.open = (self.send)(std::mem::take(&mut self.batch));
        }
        self.open
    }

    /// Whether the list's screen has closed.
    pub fn cancelled(&self) -> bool {
        !self.open || self.stream.cancelled()
    }
}

/// A [`fuzzy_list`]'s options: a title, a border, what Esc does, the
/// prompt.
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct FuzzyOptions {
    title: Option<String>,
    border: Option<Border>,
    prompt: String,
    on_cancel: Option<OnCancel>,
}

impl Default for FuzzyOptions {
    fn default() -> FuzzyOptions {
        FuzzyOptions {
            title: None,
            border: None,
            prompt: "[bold]>[/] ".into(),
            on_cancel: None,
        }
    }
}

impl FuzzyOptions {
    /// No title or border, `>` as the prompt, Esc closing the modal.
    pub fn new() -> FuzzyOptions {
        FuzzyOptions::default()
    }

    /// A panel round the list, with `markup` as its title.
    pub fn title(mut self, markup: &str) -> FuzzyOptions {
        self.title = Some(markup.to_string());
        self
    }

    /// A panel round the list in `border`'s lines.
    pub fn border(mut self, border: Border) -> FuzzyOptions {
        self.border = Some(border);
        self
    }

    /// The markup before what is typed, instead of `> `.
    pub fn prompt(mut self, markup: &str) -> FuzzyOptions {
        self.prompt = markup.to_string();
        self
    }

    /// Run `handler` on Esc, instead of closing the screen or modal the
    /// list is on.
    pub fn on_cancel(mut self, handler: impl FnMut(&mut Ctx) + 'static) -> FuzzyOptions {
        self.on_cancel = Some(Box::new(handler));
        self
    }
}

/// A fuzzy list over the items `source` gives (an iterator, or a producer
/// feeding them: [`FuzzySource`]), each matched and shown as the text
/// `display` makes of it: a file walk, a command list, a search's results,
/// the lines of a log, a database's rows. The source is read on a thread
/// of its own and its items reach the list in batches while it goes on,
/// so the first results show at once and typing never waits for a long
/// stream; a new batch is matched alone, not the whole list again.
/// Matching is rs-rich-interact's [`fuzzy`] (smart case, words in any
/// order), best first, the matched characters picked out; a count says
/// how many match, and `(loading…)` while the source goes on. The source
/// stops when the list's screen closes. Up and Down (Ctrl+P, Ctrl+N)
/// move, Enter calls `on_pick` with the hovered item, and Esc closes the
/// screen or modal it is on (on the first screen Esc is left to the
/// app). For a title, a border, another prompt or
/// another Esc, see [`fuzzy_list_with`]. [`finder_of`] is one over an
/// iterator.
///
/// ```
/// use intuituive::finder::{fuzzy_list, FuzzySource};
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let picked = signal(String::new());
///     let commands = vec!["open file", "save all", "close window"];
///     column([
///         fuzzy_list(
///             FuzzySource::iter(commands),
///             |c| c.to_string(),
///             move |c, _| picked.set(c.to_string()),
///         )
///         .flex(1),
///         text!("{picked}").fixed(1),
///     ])
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(30, 6);
/// driver.type_text("sva");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[5].trim_end(), "save all");
/// ```
pub fn fuzzy_list<T: Clone + Send + 'static>(
    source: FuzzySource<T>,
    display: impl Fn(&T) -> String + Send + 'static,
    on_pick: impl Fn(T, &mut Ctx) + 'static,
) -> Node {
    fuzzy_list_with(source, display, on_pick, FuzzyOptions::new())
}

/// A [`fuzzy_list`] with a title, a border, a prompt or an Esc of its own,
/// as `options` says.
///
/// ```
/// use intuituive::finder::{fuzzy_list_with, FuzzyOptions, FuzzySource};
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let closed = signal(false);
///     column([
///         fuzzy_list_with(
///             FuzzySource::iter(vec!["alpha", "beta"]),
///             |s| s.to_string(),
///             |_, _| {},
///             FuzzyOptions::new()
///                 .title("Jump")
///                 .border(Border::Rounded)
///                 .on_cancel(move |_| closed.set(true)),
///         )
///         .flex(1),
///         text!("closed: {closed}").fixed(1),
///     ])
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(20, 7);
/// driver.settle();
/// assert!(driver.screen().plain()[0].starts_with("╭─ Jump "));
/// driver.press("esc");
/// assert_eq!(driver.screen().plain()[6].trim_end(), "closed: true");
/// ```
pub fn fuzzy_list_with<T: Clone + Send + 'static>(
    source: FuzzySource<T>,
    display: impl Fn(&T) -> String + Send + 'static,
    on_pick: impl Fn(T, &mut Ctx) + 'static,
    options: FuzzyOptions,
) -> Node {
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
    stream(source, display, state);
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
    let FuzzyOptions {
        title,
        border,
        prompt,
        on_cancel,
    } = options;
    let prompt_width = crate::widget::markup_width(&prompt);
    let field = row([
        text(move || prompt.clone()).fixed(prompt_width),
        input(query).flex(1).autofocus(),
    ])
    .fixed(1);
    let node = column([field, status, results])
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
        .name("finder");
    // Esc runs `on_cancel`, else closes the screen the list is on: on the
    // first screen there is none to close, and Esc goes on to the app.
    let node = match on_cancel {
        Some(mut cancel) => node.bind("esc", "Close", move |cx| cancel(cx)),
        None => node.bind_when(crate::app::over_first, "esc", "Close", |cx| cx.pop()),
    };
    match (title, border) {
        (None, None) => node,
        (title, border) => {
            let node = node.panel(title.as_deref().unwrap_or(""));
            match border {
                Some(border) => node.border(border),
                None => node,
            }
        }
    }
}

/// A fuzzy finder over the items `items` gives, each matched and shown as
/// the text `display` makes of it: a [`fuzzy_list`] over an iterator
/// ([`FuzzySource::iter`]), which looks and works as every fuzzy list
/// does. `items` is read on a thread of its own, its items reaching the
/// list in batches while it goes on; it stops when the finder's screen
/// closes. Up and Down (Ctrl+P, Ctrl+N) move, Enter calls `on_pick` with
/// the hovered item, Esc closes the screen or modal.
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
    fuzzy_list(FuzzySource::iter(items), display, on_pick)
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

/// Run `source` on a thread, sending its items (and their labels) to the
/// list in batches; stop when the screen the list is on closes.
fn stream<T>(
    source: FuzzySource<T>,
    display: impl Fn(&T) -> String + Send + 'static,
    state: Finder<T>,
) where
    T: Send + 'static,
{
    let (found, labels, streaming) = (state.found, state.labels, state.streaming);
    let task = spawn_stream(move |stream| {
        let proxy = stream.proxy().clone();
        let to_app = proxy.clone();
        let send = move |batch: Vec<T>| {
            let shown: Vec<String> = batch.iter().map(&display).collect();
            to_app.run(move || {
                labels.update(|l| l.extend(shown));
                found.update(|f| f.extend(batch));
            })
        };
        let mut feed = Feed {
            batch: Vec::new(),
            last: Instant::now(),
            send: Box::new(send),
            stream,
            open: true,
        };
        (source.run)(&mut feed);
        if feed.open && !feed.stream.cancelled() {
            let rest = std::mem::take(&mut feed.batch);
            (feed.send)(rest);
            proxy.run(move || streaming.set(false));
        }
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
