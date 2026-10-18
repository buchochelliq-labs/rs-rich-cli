//! A port of ratatui's `async-github` example
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps/async-github>),
//! rebuilt on intuiTUIve. It reuses none of ratatui's code: it rebuilds the
//! behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs. The pull
//! requests are our own, made up.
//!
//!     cargo run -p rs-rich-intuituive --example pulls
//!     cargo run -p rs-rich-intuituive --example pulls -- --delay 3000
//!     cargo run -p rs-rich-intuituive --example pulls -- --fail
//!
//! A table of pull requests loaded in the background: the panel's
//! right-hand title says Loading, Loaded or Failed while it is. j/k (or
//! ↓/↑) move, a click picks a row, r loads them again, ? lists the keys,
//! Ctrl+P finds a command, q quits.
//!
//! ratatui's fetches from GitHub. This crate has no HTTP client, so the
//! example makes no requests: its pull requests are a fixture behind a
//! delay (`--delay MS`, 800 by default), and `--fail` makes the load fail.
//!
//! What intuiTUIve adds: the load is a `resource`, a value fetched on a
//! thread of its own that the table reads like a signal, so no channel or
//! state machine is the app's to keep; reloading drops a result still on
//! its way. The tests wait for it with `App::wait_for_tasks`, so they see
//! the result on the next frame however fast the machine is.

use std::sync::Arc;
use std::time::Duration;

use intuituive::prelude::*;
use intuituive::widgets::{table_optional, Column, TableOptions};

/// A pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    pub number: u32,
    pub title: &'static str,
    pub author: &'static str,
    pub state: State,
    pub comments: u32,
    /// How long ago it was opened, as the table shows it.
    pub age: &'static str,
}

/// Where a pull request is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Open,
    Draft,
    Merged,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Draft => "draft",
            State::Merged => "merged",
        }
    }

    fn style(self) -> &'static str {
        match self {
            State::Open => "green",
            State::Draft => "dim",
            State::Merged => "magenta",
        }
    }
}

/// The fixture: pull requests to a made-up project, by made-up people.
#[rustfmt::skip]
pub const PULLS: [Pull; 12] = [
    pull(212, "Add a dark theme to the settings page", "mira-k", State::Open, 4, "2h"),
    pull(211, "Cache the parsed config between runs", "tobias", State::Open, 9, "5h"),
    pull(210, "Fix the off-by-one in the pager's last line", "anaïs", State::Merged, 2, "1d"),
    pull(209, "Draft: stream the log instead of reading it whole", "jun-w", State::Draft, 0, "1d"),
    pull(208, "Document the plugin hooks", "olu", State::Open, 1, "2d"),
    pull(207, "Retry the upload once on a timeout", "mira-k", State::Merged, 6, "3d"),
    pull(206, "Bump the minimum toolchain", "pavel", State::Merged, 3, "4d"),
    pull(205, "Draft: a second look at the keymap", "sana", State::Draft, 12, "5d"),
    pull(204, "Speed up the table's sort", "tobias", State::Open, 7, "1w"),
    pull(203, "Test the importer on empty files", "olu", State::Merged, 0, "1w"),
    pull(202, "Show the version in the about box", "jun-w", State::Open, 2, "2w"),
    pull(201, "Remove the unused colour helpers", "pavel", State::Merged, 1, "3w"),
];

const fn pull(
    number: u32,
    title: &'static str,
    author: &'static str,
    state: State,
    comments: u32,
    age: &'static str,
) -> Pull {
    Pull {
        number,
        title,
        author,
        state,
        comments,
        age,
    }
}

/// Where the pull requests come from: called on a thread of its own, each
/// load.
pub type Fetch = Arc<dyn Fn() -> Result<Vec<Pull>, String> + Send + Sync>;

/// The fixture, after `delay`; or, if `fail`, an error after it.
pub fn fixture(delay: Duration, fail: bool) -> Fetch {
    Arc::new(move || {
        std::thread::sleep(delay);
        if fail {
            Err("the server said 503: try again later".into())
        } else {
            Ok(PULLS.to_vec())
        }
    })
}

/// The command line: `--delay MS` and `--fail`.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Fetch, String> {
    let (mut delay, mut fail) = (Duration::from_millis(800), false);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--delay" => {
                let ms: u64 = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--delay takes milliseconds")?;
                delay = Duration::from_millis(ms);
            }
            "--fail" => fail = true,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    Ok(fixture(delay, fail))
}

/// How the panel's right-hand title reads in each state.
fn status(load: &Load<Vec<Pull>>) -> String {
    match load {
        Load::Loading => "[yellow]Loading…[/]".into(),
        Load::Ready(pulls) => format!("[green]Loaded {}[/]", pulls.len()),
        Load::Failed(_) => "[red]Failed[/]".into(),
    }
}

/// The app, loading its pull requests with `fetch`.
pub fn pulls_app(fetch: Fetch) -> App {
    App::new(move || {
        let fetch = fetch.clone();
        let pulls = resource(move || fetch());
        let selected = signal(None::<usize>);
        let columns = vec![
            Column::new("#", Size::Fixed(5)),
            Column::new("Title", Size::Flex(1)),
            Column::new("Author", Size::Fixed(8)),
            Column::new("State", Size::Fixed(7)),
            Column::new("Replies", Size::Fixed(7)),
            Column::new("Age", Size::Fixed(4)),
        ];
        let rows = move || {
            pulls.with(|load| match load {
                Load::Ready(list) => list
                    .iter()
                    .map(|p| {
                        vec![
                            format!("[dim]#[/]{}", p.number),
                            intuituive::rich::markup::escape(p.title),
                            p.author.to_string(),
                            format!("[{}]{}[/]", p.state.style(), p.state.name()),
                            p.comments.to_string(),
                            p.age.to_string(),
                        ]
                    })
                    .collect(),
                _ => Vec::new(),
            })
        };
        let table = table_optional(
            columns,
            rows,
            selected,
            TableOptions::default().symbol("> ").header_gap(1),
        )
        .name("pulls")
        .label("Pull requests")
        .busy_when(move || pulls.with(Load::is_loading));
        let failure = text(move || match pulls.get() {
            Load::Failed(error) => format!(
                "[red]Could not load them:[/] {} · [dim]r tries again[/]",
                intuituive::rich::markup::escape(&error)
            ),
            _ => String::new(),
        })
        .name("failure")
        .live()
        .fixed(1);
        // A panel's titles are fixed when it is made: the one that changes
        // is a layer over the panel's top edge, at the right, where
        // `.title_right` puts one.
        let state = text(move || format!(" {} ", pulls.with(status)))
            .name("state")
            .live()
            .place(Align::End, Align::Start)
            .inset(0, 2, 0, 0);
        let panel = column([table.flex(1), failure])
            .panel("Pull requests")
            .title_bottom("[dim]j/k move · r reloads · q quits[/]");
        layers([panel, state])
            .bind("down j", "the next pull request", move |_| {
                let count = pulls.with(|l| l.ready().map_or(0, Vec::len));
                if count > 0 {
                    selected.update(|s| *s = Some(s.map_or(0, |s| (s + 1).min(count - 1))));
                }
            })
            .bind("up k", "the pull request before", move |_| {
                let count = pulls.with(|l| l.ready().map_or(0, Vec::len));
                if count > 0 {
                    selected.update(|s| *s = Some(s.map_or(count - 1, |s| s.saturating_sub(1))));
                }
            })
            .bind("r", "load them again", move |_| {
                selected.set(None);
                pulls.reload();
            })
            .bind("q", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p")
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let fetch = parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    pulls_app(fetch).run()
}
