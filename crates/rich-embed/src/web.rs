//! The web view: a web page in a pane, over a [`WebEngine`].
//!
//! The view keeps the page's state in signals (its address, title, whether
//! it is loading, and whether it can go back or forward), draws a small
//! address bar over the page, forwards keys and the mouse, and tells the
//! engine the page's size. Engines hand back frames of cells (a text
//! browser) or of pixels (a graphical one); pixels are drawn as coloured
//! half blocks.

use std::cell::RefCell;
use std::io;
use std::rc::{Rc, Weak};

use rich::{Segment, Style};
use rich_intuituive::interact::{Button, Key, KeyCode, Mouse, MouseKind};
use rich_intuituive::widget::{widget, Canvas, DrawCx, EventCx, Used, Widget, WidgetEvent};
use rich_intuituive::{signal, watch, Ctx, Node, Signal};

use crate::host::Notify;
use crate::pixels::{half_blocks, Pixels};
use crate::program::ProgramEngine;
use crate::wake::{connect, Fed};

/// Input for the page, in the page's own cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebInput {
    Key(Key),
    /// The mouse, at a cell of the page (the address bar is not counted).
    Mouse(Mouse),
    Paste(String),
}

/// What an engine draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebFrame {
    /// Text: one line of rich segments per row, and the text cursor.
    Cells {
        lines: Vec<Vec<Segment>>,
        cursor: Option<(u16, u16)>,
    },
    /// An image of the page, scaled into the pane.
    Pixels(Pixels),
}

/// Where the page is and what it is doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageState {
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// Why the page could not be shown, if it could not.
    pub error: Option<String>,
}

/// A browser behind a [`web_view`]: opens pages, takes input, and hands
/// back frames. Every method is called on the app's thread; work that takes
/// time happens on the engine's own threads, which call the [`Notify`]
/// when there is a new frame or a new state.
///
/// The view calls [`resize`](Self::resize) before the first
/// [`open`](Self::open), and again whenever the page's area changes.
pub trait WebEngine {
    /// Go to `url`.
    fn open(&mut self, url: &str) -> io::Result<()>;
    /// The page's area is now `columns` x `rows` cells.
    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()>;
    fn input(&mut self, input: WebInput) -> io::Result<()>;
    fn back(&mut self) -> io::Result<()>;
    fn forward(&mut self) -> io::Result<()>;
    fn reload(&mut self) -> io::Result<()>;
    /// The newest frame since the last call, if there is one.
    fn poll(&mut self) -> Option<WebFrame>;
    /// Where the page is and what it is doing now.
    fn state(&self) -> PageState;
    /// Call `notify` when there is a new frame or state. Given once.
    fn set_notify(&mut self, notify: Notify);
}

/// The page's state as signals, and what the app can ask of it. `Copy`,
/// like a signal: move it into handlers.
pub struct WebHandle {
    address: Signal<String>,
    title: Signal<String>,
    loading: Signal<bool>,
    can_go_back: Signal<bool>,
    can_go_forward: Signal<bool>,
    error: Signal<Option<String>>,
    view: Signal<Weak<RefCell<ViewState>>>,
}

impl Clone for WebHandle {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for WebHandle {}

impl WebHandle {
    /// The page's address. Setting it opens that address.
    pub fn address(&self) -> Signal<String> {
        self.address
    }

    pub fn title(&self) -> Signal<String> {
        self.title
    }

    pub fn loading(&self) -> Signal<bool> {
        self.loading
    }

    pub fn can_go_back(&self) -> Signal<bool> {
        self.can_go_back
    }

    pub fn can_go_forward(&self) -> Signal<bool> {
        self.can_go_forward
    }

    /// Why the page could not be shown, if it could not.
    pub fn error(&self) -> Signal<Option<String>> {
        self.error
    }

    fn with_view(&self, f: impl FnOnce(&mut ViewState)) {
        if let Some(view) = self.view.with_untracked(Weak::upgrade) {
            let mut view = view.borrow_mut();
            f(&mut view);
            view.sync();
        }
    }

    /// Open `url`.
    pub fn open(&self, url: impl Into<String>) {
        self.address.set(url.into());
    }

    pub fn back(&self) {
        self.with_view(|view| view.act(|engine| engine.back()));
    }

    pub fn forward(&self) {
        self.with_view(|view| view.act(|engine| engine.forward()));
    }

    pub fn reload(&self) {
        self.with_view(|view| view.act(|engine| engine.reload()));
    }
}

struct ViewState {
    engine: Box<dyn WebEngine>,
    handle: Option<WebHandle>,
    /// Bumped when there is something new to draw.
    wake: Signal<u64>,
    frame: Option<WebFrame>,
    /// The page's area it was last given; `None` before the first.
    size: Option<(u16, u16)>,
    /// The address to open once the page has a size.
    first: Option<String>,
    /// The address the engine was last asked for, or reported.
    current: String,
    /// The last call to the engine that failed, and why.
    failure: Option<String>,
}

impl ViewState {
    /// Ask the engine for something; a failure is kept for the next
    /// [`sync`](Self::sync) (this may run while the view draws, which
    /// writes no signals).
    fn act(&mut self, f: impl FnOnce(&mut dyn WebEngine) -> io::Result<()>) {
        if self.size.is_none() {
            return;
        }
        self.failure = f(self.engine.as_mut()).err().map(|e| e.to_string());
    }

    /// Copy the engine's state into the signals.
    fn sync(&mut self) {
        let Some(handle) = self.handle else {
            return;
        };
        let state = self.engine.state();
        if !state.url.is_empty() {
            self.current = state.url.clone();
            handle.address.set(state.url);
        }
        handle.title.set(state.title);
        handle.loading.set(state.loading);
        handle.can_go_back.set(state.can_go_back);
        handle.can_go_forward.set(state.can_go_forward);
        handle.error.set(state.error.or_else(|| self.failure.clone()));
    }

    /// The page's area changed to `columns` x `rows`: tell the engine, and
    /// open the first page once there is one.
    fn fit(&mut self, columns: u16, rows: u16) {
        if columns == 0 || rows == 0 || self.size == Some((columns, rows)) {
            return;
        }
        self.size = Some((columns, rows));
        self.act(|engine| engine.resize(columns, rows));
        if let Some(url) = self.first.take() {
            self.current = url.clone();
            self.act(|engine| engine.open(&url));
        }
    }
}

impl Fed for ViewState {
    fn install(&mut self, notify: Notify) {
        self.engine.set_notify(notify);
    }

    fn pump(this: &Rc<RefCell<Self>>, _cx: &mut Ctx) {
        let mut view = this.borrow_mut();
        if let Some(frame) = view.engine.poll() {
            view.frame = Some(frame);
            view.wake.update(|n| *n = n.wrapping_add(1));
        }
        view.sync();
    }
}

/// A web view being built: [`web_view`] or [`web_view_with`], then
/// [`node`](Self::node) to place it; [`handle`](Self::handle) for its
/// signals and navigation.
///
/// The top row is an address bar: `←` back, `→` forward, `↻` reload, the
/// address, and `…` while the page loads. Click the address or press
/// Ctrl+L to type one (Enter opens it, Esc gives up); Alt+Left, Alt+Right
/// and F5 go back, forward and reload. Every other key, the mouse and
/// pastes go to the page while the view has the focus, except the keys
/// [`release_keys`](Self::release_keys) names.
pub struct WebView {
    state: Rc<RefCell<ViewState>>,
    handle: WebHandle,
    bar: bool,
    release: Vec<Key>,
}

/// A web page at `url`, shown by a terminal browser named by the
/// environment or found on `PATH` ([`ProgramEngine::detect`]). Name one
/// with [`web_view_with`] and [`ProgramEngine::new`].
///
/// # Panics
/// Outside a running or building app, like [`signal`].
pub fn web_view(url: impl Into<String>) -> WebView {
    web_view_with(ProgramEngine::detect(), url)
}

/// A web page at `url`, shown by `engine`.
///
/// ```no_run
/// # extern crate rich_intuituive as intuituive;
/// use intuituive::prelude::*;
/// use rich_embed::{web_view_with, ProgramEngine};
///
/// App::new(|| {
///     let page = web_view_with(ProgramEngine::new("w3m"), "https://example.com");
///     let address = page.handle().address();
///     column([page.node(), text!("{address}").fixed(1)])
/// })
/// .run()
/// # ; Ok::<(), std::io::Error>(())
/// ```
///
/// # Panics
/// Outside a running or building app, like [`signal`].
pub fn web_view_with(engine: impl WebEngine + 'static, url: impl Into<String>) -> WebView {
    let url = url.into();
    let state = Rc::new(RefCell::new(ViewState {
        engine: Box::new(engine),
        handle: None,
        wake: signal(0),
        frame: None,
        size: None,
        first: Some(url.clone()),
        current: url.clone(),
        failure: None,
    }));
    let handle = WebHandle {
        address: signal(url),
        title: signal(String::new()),
        loading: signal(false),
        can_go_back: signal(false),
        can_go_forward: signal(false),
        error: signal(None),
        view: signal(Rc::downgrade(&state)),
    };
    state.borrow_mut().handle = Some(handle);
    connect(&state);
    // The app setting the address opens it; the engine reporting a new one
    // (a link followed, a redirect) changes nothing.
    let weak = Rc::downgrade(&state);
    watch(
        move || handle.address.get(),
        move |url, _| {
            let Some(view) = weak.upgrade() else {
                return;
            };
            let mut view = view.borrow_mut();
            if url == view.current {
                return;
            }
            view.current = url.clone();
            if view.size.is_none() {
                view.first = Some(url);
                return;
            }
            view.act(|engine| engine.open(&url));
        },
    );
    WebView {
        state,
        handle,
        bar: true,
        release: Vec::new(),
    }
}

impl WebView {
    /// The page's signals, and back, forward, reload and open.
    pub fn handle(&self) -> WebHandle {
        self.handle
    }

    /// Show the address bar (default) or not: an app may draw its own from
    /// the [handle](Self::handle)'s signals.
    pub fn address_bar(mut self, on: bool) -> WebView {
        self.bar = on;
        self
    }

    /// Keys (space-separated names, as [`Node::on_key`] takes them) that
    /// are not sent to the page but go on to the app's bindings.
    pub fn release_keys(mut self, keys: &str) -> WebView {
        self.release.extend(keys.split_whitespace().filter_map(Key::parse));
        self
    }

    /// The view as a node: focusable, and flexible until sized.
    pub fn node(self) -> Node {
        widget(ViewWidget {
            state: self.state,
            handle: self.handle,
            bar: self.bar,
            release: self.release,
            editing: None,
            caret: None,
        })
    }
}

impl From<WebView> for Node {
    fn from(view: WebView) -> Node {
        view.node()
    }
}

/// Where the address bar's parts are.
const BACK: u16 = 1;
const FORWARD: u16 = 3;
const RELOAD: u16 = 5;
const ADDRESS: u16 = 7;

struct ViewWidget {
    state: Rc<RefCell<ViewState>>,
    handle: WebHandle,
    bar: bool,
    release: Vec<Key>,
    /// The address being typed, while it is.
    editing: Option<String>,
    caret: Option<(u16, u16)>,
}

impl ViewWidget {
    fn top(&self) -> u16 {
        self.bar as u16
    }

    fn draw_bar(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let width = canvas.width();
        let bar = cx.style("web.bar", "on grey15");
        let off = bar.combine(&Style::parse("dim").unwrap_or_default());
        canvas.fill(0, 0, width, 1, Some(&bar));
        let back = self.handle.can_go_back.get();
        let forward = self.handle.can_go_forward.get();
        canvas.print(BACK, 0, "←", Some(if back { &bar } else { &off }));
        canvas.print(FORWARD, 0, "→", Some(if forward { &bar } else { &off }));
        canvas.print(RELOAD, 0, "↻", Some(&bar));
        let loading = self.handle.loading.get();
        let room = width.saturating_sub(ADDRESS + 2 * loading as u16);
        match &self.editing {
            Some(text) => {
                let field = cx.style("web.address.edit", "on grey30");
                canvas.fill(ADDRESS, 0, room, 1, Some(&field));
                // The end of what is typed stays in view.
                let shown = tail(text, room.saturating_sub(1));
                canvas.print(ADDRESS, 0, &shown, Some(&field));
                let at = ADDRESS + rich::cells::cell_len(&shown) as u16;
                self.caret = Some((at.min(width.saturating_sub(1)), 0));
            }
            None => {
                let address = self.handle.address.get();
                let line = crate::web::fit(&address, room);
                canvas.print(ADDRESS, 0, &line, Some(&bar));
            }
        }
        if loading {
            canvas.print(width.saturating_sub(2), 0, "…", Some(&bar));
        }
    }

    fn input(&self, input: WebInput) {
        let mut view = self.state.borrow_mut();
        view.act(|engine| engine.input(input));
    }

    fn edit(&mut self, cx: &mut EventCx, key: Key) -> Used {
        let Some(text) = &mut self.editing else {
            return Used::No;
        };
        match key.code {
            KeyCode::Enter => {
                let url = std::mem::take(text).trim().to_string();
                self.editing = None;
                if !url.is_empty() {
                    self.handle.open(with_scheme(&url));
                }
            }
            KeyCode::Escape => self.editing = None,
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => text.push(c),
            _ => {}
        }
        cx.redraw();
        Used::Yes
    }
}

impl Widget for ViewWidget {
    fn name(&self) -> &'static str {
        "web view"
    }

    fn describe(&self) -> Option<String> {
        Some(self.state.borrow().current.clone())
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let wake = self.state.borrow().wake;
        wake.get();
        self.caret = None;
        let (width, height) = (canvas.width(), canvas.height());
        let top = self.top();
        if self.bar && height > 0 {
            self.draw_bar(cx, canvas);
        }
        let rows = height.saturating_sub(top);
        let mut view = self.state.borrow_mut();
        view.fit(width, rows);
        if rows == 0 {
            return;
        }
        let error = self.handle.error.get().or_else(|| view.failure.clone());
        match &view.frame {
            Some(WebFrame::Cells { lines, cursor }) => {
                canvas.lines_at(0, top, width, rows, lines);
                if self.editing.is_none() {
                    self.caret = cursor.map(|(x, y)| (x, y + top));
                }
            }
            Some(WebFrame::Pixels(pixels)) => {
                canvas.lines_at(0, top, width, rows, &half_blocks(pixels, width, rows));
            }
            None => {
                canvas.clear(0, top, width, rows);
                let note = match &error {
                    Some(error) => format!("[red]{}", error.replace('[', "\\[")),
                    None => "[dim]loading…".to_string(),
                };
                canvas.markup(cx.console(), 1, top, width.saturating_sub(1), &note, None);
            }
        }
        if let (Some(error), Some(_)) = (error, &view.frame) {
            let line = format!("[reverse red] {} ", error.replace('[', "\\["));
            canvas.markup(cx.console(), 0, height - 1, width, &line, None);
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let top = self.top();
        match event {
            WidgetEvent::Key(key) if self.release.contains(key) && self.editing.is_none() => {
                Used::No
            }
            WidgetEvent::Key(key) => {
                if self.editing.is_some() {
                    return self.edit(cx, *key);
                }
                let alt = key.modifiers.alt && !key.modifiers.ctrl;
                match key.code {
                    KeyCode::Char('l') if key.modifiers.ctrl && self.bar => {
                        self.editing = Some(self.handle.address.get_untracked());
                    }
                    KeyCode::Left if alt => self.handle.back(),
                    KeyCode::Right if alt => self.handle.forward(),
                    KeyCode::F(5) => self.handle.reload(),
                    _ => self.input(WebInput::Key(*key)),
                }
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Paste(text) => {
                match &mut self.editing {
                    Some(editing) => editing.push_str(text.trim()),
                    None => self.input(WebInput::Paste(text.clone())),
                }
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Mouse(mouse) if self.bar && mouse.row == 0 => {
                if mouse.kind != MouseKind::Down(Button::Left) {
                    return Used::Yes;
                }
                match mouse.column {
                    c if c == BACK => self.handle.back(),
                    c if c == FORWARD => self.handle.forward(),
                    c if c == RELOAD => self.handle.reload(),
                    c if c >= ADDRESS && self.editing.is_none() => {
                        self.editing = Some(self.handle.address.get_untracked());
                    }
                    _ => {}
                }
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Mouse(mouse) => {
                let mut page = *mouse;
                page.row -= top;
                self.input(WebInput::Mouse(page));
                Used::Yes
            }
            WidgetEvent::Focus(false) if self.editing.is_some() => {
                self.editing = None;
                cx.redraw();
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn caret(&self) -> Option<(u16, u16)> {
        self.caret
    }
}

/// `text` cut to `width` cells, with an ellipsis where it was cut.
pub(crate) fn fit(text: &str, width: u16) -> String {
    let width = width as usize;
    if rich::cells::cell_len(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = rich::cells::cell_len(c.encode_utf8(&mut [0u8; 4]));
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// The last `width` cells of `text`.
fn tail(text: &str, width: u16) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut used = 0;
    for c in text.chars().rev() {
        let w = rich::cells::cell_len(c.encode_utf8(&mut [0u8; 4]));
        if used + w > width as usize {
            break;
        }
        out.push(c);
        used += w;
    }
    out.into_iter().rev().collect()
}

/// An address as typed, with `https://` when it names no scheme.
fn with_scheme(url: &str) -> String {
    if url.contains("://") || url.starts_with("about:") || url.starts_with("file:") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_fit_and_get_a_scheme() {
        assert_eq!(fit("https://example.com", 10), "https://e…");
        assert_eq!(fit("short", 10), "short");
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(with_scheme("example.com"), "https://example.com");
        assert_eq!(with_scheme("http://x"), "http://x");
        assert_eq!(with_scheme("about:blank"), "about:blank");
    }
}
