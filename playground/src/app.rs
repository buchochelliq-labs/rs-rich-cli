//! Tier 3: interactive components and intuiTUIve apps, driven by the page.
//!
//! A browser cannot block, so nothing here waits: the page pushes what
//! xterm.js gives it (`onData` bytes, resizes) and the time from
//! `performance.now()`, and writes back what comes out. Two kinds of
//! session share that shape:
//!
//! - **Components** (`choose`, `filter`, `choose-multi`, `pager`,
//!   `explore`): rs-rich-interact's [`EventLoop`] on a [`PushBackend`],
//!   stepped with [`EventLoop::step`] and [`EventLoop::pump`], as
//!   `rich choose` and the others run it with `run`.
//! - **Apps** (`todo`, `form`, `meters`, `sprites`): an intuiTUIve app's
//!   [`Driver`], the loop rs-rich-web's server runs for a browser tab, here
//!   in the tab itself.
//!
//! Keys and the mouse are decoded by rs-rich-web's `input::decode` (its
//! `server` feature off), the decoder its own page's input goes through.

use std::time::Duration;

use intuituive::interact::{
    Event, EventLoop, Key, LoopOptions, MultiSelect, Outcome, Pager, PushBackend, PushPort, Select,
};
use intuituive::Driver;
use rich::Text;
use rich_ext::data::{parse, Format};
use wasm_bindgen::prelude::*;

// The intuiTUIve examples the page offers, unchanged: the tutorial's to-do
// list, a form, level meters and sprites.
#[path = "../../crates/rich-intuituive/examples/todo.rs"]
#[allow(dead_code)]
mod todo;

#[path = "../../crates/rich-intuituive/examples/form.rs"]
#[allow(dead_code)]
mod form;

#[path = "../../crates/rich-intuituive/examples/meters.rs"]
#[allow(dead_code)]
mod meters;

#[path = "../../crates/rich-intuituive/examples/sprites.rs"]
#[allow(dead_code)]
mod sprites;

/// The sessions there are, as `(name, what it is, its input)`.
pub const APPS: &[(&str, &str, &str)] = &[
    ("choose", "rich choose: pick one line", "lines"),
    (
        "filter",
        "rich filter: type to narrow the lines, pick one",
        "lines",
    ),
    (
        "choose-multi",
        "rich choose --multi: mark lines with Tab",
        "lines",
    ),
    ("pager", "rich pager: scroll the text", "text"),
    (
        "explore",
        "rich explore: browse JSON or YAML, Enter picks a path",
        "json or yaml",
    ),
    (
        "todo",
        "intuiTUIve's tutorial app: a to-do list",
        "lines (the to-dos)",
    ),
    ("form", "intuiTUIve: a form with validation", "none"),
    ("meters", "intuiTUIve: animated level meters", "none"),
    ("sprites", "intuiTUIve: sprites on a stage", "none"),
];

/// Mouse reporting on (clicks, drags and the wheel, in SGR form), as
/// rs-rich-web's page asks for it; and off again.
const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1006h";
const MOUSE_OFF: &str = "\x1b[?1000l\x1b[?1002l\x1b[?1006l";
/// The alternate screen with the cursor hidden, for an app; and back.
const APP_ENTER: &str = "\x1b[?1049h\x1b[?25l";
const APP_LEAVE: &str = "\x1b[?25h\x1b[?1049l";

/// An outcome, as text for the page.
fn outcome<T>(outcome: Option<Outcome<T>>, show: impl FnOnce(T) -> String) -> String {
    match outcome {
        Some(Outcome::Done(value)) => show(value),
        Some(Outcome::Cancelled) => "cancelled".to_string(),
        Some(Outcome::Interrupted) => "interrupted".to_string(),
        None => String::new(),
    }
}

enum Inner {
    Component {
        event_loop: Box<EventLoop<'static>>,
        port: PushPort,
        /// Takes the outcome, as text, once the component is done.
        result: Box<dyn FnMut() -> String>,
    },
    App {
        driver: Box<Driver>,
    },
}

/// One running component or app: the page's side of a tier-3 block.
#[wasm_bindgen]
pub struct Session {
    inner: Option<Inner>,
    /// What it finished with ("Done: …", "cancelled", …), once done.
    result: String,
    /// Text it copied, for `navigator.clipboard`.
    copies: Vec<String>,
}

fn at(now_ms: f64) -> Duration {
    Duration::from_secs_f64(now_ms.max(0.0) / 1000.0)
}

fn lines(input: &str) -> Vec<String> {
    input
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

#[wasm_bindgen]
impl Session {
    /// Start `app` (one of [`APPS`]) on `input` in a `columns` x `rows`
    /// terminal at `now_ms`. [`output`](Self::output) is its first frame.
    #[wasm_bindgen(constructor)]
    pub fn new(app: &str, input: &str, columns: u16, rows: u16) -> Result<Session, String> {
        if input.len() > crate::options::MAX_INPUT {
            return Err(format!(
                "the input is over {} KB",
                crate::options::MAX_INPUT / 1024
            ));
        }
        let columns = columns.clamp(20, 300);
        let rows = rows.clamp(4, 200);
        let inner = match app {
            "choose" | "filter" | "choose-multi" => {
                let items = lines(input);
                if items.is_empty() {
                    return Err("give it some lines to choose from".to_string());
                }
                component(columns, rows, |event_loop| match app {
                    "choose-multi" => {
                        let handle =
                            event_loop.mount(MultiSelect::new("Choose", items).with_mouse(true));
                        Box::new(move || outcome(handle.take(), |values| values.join("\n")))
                    }
                    _ => {
                        let prompt = if app == "filter" { "Filter" } else { "Choose" };
                        let handle = event_loop.mount(Select::new(prompt, items).with_mouse(true));
                        Box::new(move || outcome(handle.take(), |value| value))
                    }
                })
            }
            "pager" => component(columns, rows, |event_loop| {
                let handle = event_loop.mount(Pager::new(Text::new(input.to_string())));
                Box::new(move || outcome(handle.take(), |()| String::new()))
            }),
            "explore" => {
                let node = parse(Format::Json, input)
                    .or_else(|json| parse(Format::Yaml, input).map_err(|_| json))
                    .map_err(|e| e.to_string())?;
                component(columns, rows, |event_loop| {
                    let explorer = intuituive::interact::DataExplorer::new("data", node);
                    let handle = event_loop.mount(explorer);
                    Box::new(move || outcome(handle.take(), |path| path.to_string()))
                })
            }
            "todo" | "form" | "meters" | "sprites" => {
                let app = match app {
                    "todo" => {
                        let mut items = lines(input);
                        if items.is_empty() {
                            items = vec![
                                "Read the intuiTUIve tutorial".into(),
                                "Run it in a browser".into(),
                            ];
                        }
                        let todos = (1..)
                            .zip(items)
                            .map(|(id, text)| todo::Todo::new(id, &text))
                            .collect();
                        todo::todo_app(todos)
                    }
                    "form" => form::form_app(),
                    "meters" => meters::meters_app(false),
                    _ => sprites::app(),
                };
                let mut driver = app.driver(columns, rows);
                driver.set_clipboard(true);
                Inner::App {
                    driver: Box::new(driver),
                }
            }
            _ => return Err(format!("unknown app {app:?}")),
        };
        Ok(Session {
            inner: Some(inner),
            result: String::new(),
            copies: Vec::new(),
        })
    }

    /// What to write first: the setup and the first frame.
    pub fn output(&mut self, now_ms: f64) -> String {
        let start = match &self.inner {
            Some(Inner::App { .. }) => format!("{APP_ENTER}{MOUSE_ON}"),
            Some(Inner::Component { .. }) => MOUSE_ON.to_string(),
            None => String::new(),
        };
        start + &self.turn(now_ms, None)
    }

    /// Bytes xterm.js produced (`onData`) at `now_ms`; returns what to
    /// write. A paste arrives as one string, as it does from a terminal.
    pub fn input(&mut self, data: &str, now_ms: f64) -> String {
        let events = rich_web::input::decode(data);
        self.turn(now_ms, Some(events))
    }

    /// The terminal is now `columns` x `rows`.
    pub fn resize(&mut self, columns: u16, rows: u16, now_ms: f64) -> String {
        if let Some(Inner::Component { port, .. }) = &self.inner {
            port.set_size(columns, rows);
        }
        self.turn(now_ms, Some(vec![Event::Resize { columns, rows }]))
    }

    /// Ticks, timers and animations up to `now_ms`; returns what to write.
    pub fn tick(&mut self, now_ms: f64) -> String {
        self.turn(now_ms, Some(Vec::new()))
    }

    /// Milliseconds from `now_ms` until it next needs a [`tick`](Self::tick);
    /// -1 when only input changes anything, or once it is done.
    pub fn timeout(&self, now_ms: f64) -> f64 {
        let wait = match &self.inner {
            Some(Inner::App { driver }) => Some(driver.timeout(at(now_ms))),
            Some(Inner::Component {
                event_loop, port, ..
            }) => {
                // Its timeout counts from the time it was last given.
                let behind = at(now_ms).saturating_sub(port.elapsed());
                event_loop.timeout().map(|wait| wait.saturating_sub(behind))
            }
            None => None,
        };
        wait.filter(|wait| *wait < Duration::from_secs(3600))
            .map_or(-1.0, |wait| wait.as_secs_f64() * 1000.0)
    }

    /// Whether it has finished.
    pub fn done(&self) -> bool {
        self.inner.is_none()
    }

    /// What it finished with: the line or lines chosen, the path explored
    /// to, `cancelled` or `interrupted`; empty while it runs.
    pub fn result(&self) -> String {
        self.result.clone()
    }

    /// Text copied since the last call, as a JSON array of strings.
    pub fn copies(&mut self) -> String {
        serde_json::to_string(&std::mem::take(&mut self.copies)).unwrap_or_default()
    }

    /// The sessions there are, as JSON: `[{"name", "about", "input"}]`.
    pub fn apps() -> String {
        let apps: Vec<_> = APPS
            .iter()
            .map(|(name, about, input)| serde_json::json!({"name": name, "about": about, "input": input}))
            .collect();
        serde_json::Value::Array(apps).to_string()
    }
}

impl Session {
    /// One turn at `now_ms`: `events` in (`None` for the first frame).
    fn turn(&mut self, now_ms: f64, events: Option<Vec<Event>>) -> String {
        let now = at(now_ms);
        let Some(inner) = self.inner.as_mut() else {
            return String::new();
        };
        let (mut out, done) = match inner {
            Inner::App { driver } => {
                driver.update(now);
                for event in events.into_iter().flatten() {
                    driver.event(event);
                }
                driver.update(now);
                let out = driver.render().unwrap_or_default();
                self.copies.extend(driver.take_copies());
                (out, driver.is_done())
            }
            Inner::Component {
                event_loop, port, ..
            } => {
                // The ticks due before `now`, each at its own time, then the
                // events at `now`: as `run` would have seen them.
                while let Some(wait) = event_loop.timeout() {
                    let due = port.elapsed() + wait;
                    if due > now || !event_loop.is_running() {
                        break;
                    }
                    port.set_elapsed(due);
                    let _ = event_loop.pump();
                }
                port.set_elapsed(now);
                let result = match events {
                    None => event_loop.pump().map(|_| ()),
                    Some(events) if events.is_empty() => event_loop.pump().map(|_| ()),
                    Some(events) => events
                        .into_iter()
                        .try_for_each(|event| event_loop.step(legacy(event)).map(|_| ())),
                };
                if let Err(error) = result {
                    self.result = format!("error: {error}");
                }
                self.copies.extend(port.take_copies());
                (port.take_output(), !event_loop.is_running())
            }
        };
        if done {
            match self.inner.take() {
                Some(Inner::App { mut driver }) => {
                    let exit = driver.take_exit::<String>();
                    out += &(*driver).finish();
                    out += MOUSE_OFF;
                    out += APP_LEAVE;
                    self.result = exit.unwrap_or_else(|| "done".to_string());
                }
                Some(Inner::Component {
                    mut event_loop,
                    port,
                    mut result,
                }) => {
                    let _ = event_loop.finish();
                    out += &port.take_output();
                    out += MOUSE_OFF;
                    if self.result.is_empty() {
                        self.result = result();
                    }
                }
                None => {}
            }
        }
        out
    }
}

/// Keys as a terminal without the kitty protocol sends them, which is what
/// xterm.js is.
fn legacy(event: Event) -> Event {
    match event {
        Event::Key(key) => Event::Key(Key::legacy(key)),
        event => event,
    }
}

fn component(
    columns: u16,
    rows: u16,
    mount: impl FnOnce(&mut EventLoop<'static>) -> Box<dyn FnMut() -> String>,
) -> Inner {
    // Inline, as `rich choose` draws in a shell: the page's terminal is the
    // block's own.
    let backend = PushBackend::new(columns, rows).inline();
    let port = backend.port();
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let result = mount(&mut event_loop);
    Inner::Component {
        event_loop: Box::new(event_loop),
        port,
        result,
    }
}
