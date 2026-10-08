//! A program's screen: a [`PtyHost`] followed by rs-rich-record's VT
//! emulator, read back as rich segments. Shared by the terminal pane and
//! [`ProgramEngine`](crate::ProgramEngine).

use std::collections::HashMap;

use rich::{Color, Segment, Style};
use rich_intuituive::interact::{Key, Mouse, MouseKind};
use rich_record::terminal::Terminal;

use crate::host::{ExitStatus, Notify, PtyHost};
use crate::keys;

/// What changed when the host was read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pumped {
    /// New output reached the screen.
    pub output: bool,
    /// The program exited, now.
    pub exit: Option<ExitStatus>,
}

pub(crate) struct TermCore {
    pub host: Box<dyn PtyHost>,
    terminal: Terminal,
    scrollback: usize,
    /// The size the program was started at or last resized to.
    size: Option<(u16, u16)>,
    /// Rows scrolled back into the scrollback; 0 is the live screen.
    scroll: usize,
    pub exit: Option<ExitStatus>,
    /// Why the program could not start, or the emulator failed.
    pub error: Option<String>,
    styles: HashMap<(u32, u32, u8), Style>,
}

impl TermCore {
    pub fn new(host: Box<dyn PtyHost>, scrollback: usize) -> TermCore {
        TermCore {
            host,
            terminal: Terminal::with_scrollback(24, 80, scrollback),
            scrollback,
            size: None,
            scroll: 0,
            exit: None,
            error: None,
            styles: HashMap::new(),
        }
    }

    pub fn set_notify(&mut self, notify: Notify) {
        self.host.set_notify(notify);
    }

    pub fn started(&self) -> bool {
        self.size.is_some()
    }

    /// Start the program at `columns` x `rows`, or tell it the pane's new
    /// size.
    pub fn fit(&mut self, columns: u16, rows: u16) {
        if columns == 0 || rows == 0 || self.size == Some((columns, rows)) {
            return;
        }
        if self.size.is_none() {
            self.resize_screen(columns, rows);
            self.size = Some((columns, rows));
            if let Err(error) = self.host.start(columns, rows) {
                self.error = Some(format!("could not start the program: {error}"));
            }
            return;
        }
        self.size = Some((columns, rows));
        self.resize_screen(columns, rows);
        if self.exit.is_none() {
            let _ = self.host.resize(columns, rows);
        }
    }

    fn resize_screen(&mut self, columns: u16, rows: u16) {
        if self.terminal.set_size(rows, columns).is_err() {
            // vt100 cannot cut some screens (a wide character at the new
            // edge): start again blank; the program redraws on the resize.
            self.terminal = Terminal::with_scrollback(rows, columns, self.scrollback);
        }
        self.set_scroll(self.scroll);
    }

    /// Read what the host has: output onto the screen, and the exit.
    pub fn pump(&mut self) -> Pumped {
        let mut pumped = Pumped::default();
        let bytes = self.host.read();
        if !bytes.is_empty() {
            pumped.output = true;
            if self.terminal.process(&bytes).is_err() {
                // What vt100 could not take is lost; the screen goes on.
                let (rows, columns) = self.terminal.screen().size();
                self.terminal = Terminal::with_scrollback(rows, columns, self.scrollback);
            }
            if self.scroll > 0 {
                // Keep the same lines in view while output pushes them up.
                self.set_scroll(self.scroll);
            }
        }
        if self.exit.is_none() && self.started() {
            if let Some(status) = self.host.exit_status() {
                self.exit = Some(status.clone());
                pumped.exit = Some(status);
            }
        }
        pumped
    }

    pub fn write(&mut self, bytes: &[u8]) {
        if self.exit.is_none() && self.started() && !bytes.is_empty() {
            let _ = self.host.write(bytes);
        }
    }

    /// Send a key, back at the live screen.
    pub fn key(&mut self, key: Key) {
        self.set_scroll(0);
        let application = self.terminal.screen().application_cursor();
        self.write(&keys::key_bytes(key, application));
    }

    pub fn paste(&mut self, text: &str) {
        self.set_scroll(0);
        let bracketed = self.terminal.screen().bracketed_paste();
        self.write(&keys::paste_bytes(text, bracketed));
    }

    /// Whether the program asked for the mouse.
    pub fn wants_mouse(&self) -> bool {
        self.terminal.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None
    }

    /// Give the program `mouse`; whether it was used. The wheel goes to
    /// the program when it asked for the mouse, as arrow keys on the
    /// alternate screen (as terminals do for pagers), and otherwise scrolls
    /// the scrollback.
    pub fn mouse(&mut self, mouse: Mouse) -> bool {
        let screen = self.terminal.screen();
        let alternate = screen.alternate_screen();
        if let Some(bytes) = keys::mouse_bytes(
            mouse,
            screen.mouse_protocol_mode(),
            screen.mouse_protocol_encoding(),
        ) {
            self.set_scroll(0);
            self.write(&bytes);
            return true;
        }
        let wheel = match mouse.kind {
            MouseKind::ScrollUp => -1,
            MouseKind::ScrollDown => 1,
            _ => return false,
        };
        if alternate {
            let key = if wheel < 0 { "up" } else { "down" };
            let key = Key::parse(key).expect("a key name");
            for _ in 0..3 {
                self.key(key);
            }
        } else {
            self.scroll_by(-3 * wheel);
        }
        true
    }

    /// Scroll the view `rows` back (positive) or forward (negative).
    pub fn scroll_by(&mut self, rows: isize) {
        let target = (self.scroll as isize + rows).max(0) as usize;
        self.set_scroll(target);
    }

    fn set_scroll(&mut self, rows: usize) {
        self.terminal.set_scrollback(rows);
        self.scroll = self.terminal.screen().scrollback();
    }

    /// Rows scrolled back; 0 at the live screen.
    pub fn scrolled(&self) -> usize {
        self.scroll
    }

    /// The text cursor, when the program shows it and the live screen is
    /// in view.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        let screen = self.terminal.screen();
        if self.scroll > 0 || screen.hide_cursor() || self.exit.is_some() {
            return None;
        }
        let (row, column) = screen.cursor_position();
        Some((column, row))
    }

    /// The screen in view, as rich segments, one line per row.
    pub fn lines(&mut self) -> Vec<Vec<Segment>> {
        let screen = self.terminal.screen();
        let (rows, columns) = screen.size();
        let mut runs: Vec<Vec<(String, (u32, u32, u8))>> = Vec::with_capacity(rows as usize);
        for y in 0..rows {
            let mut line = Vec::new();
            let mut run = String::new();
            let mut style_key: Option<(u32, u32, u8)> = None;
            // Filler cells still owed to the cluster before them.
            let mut owed = 0usize;
            for x in 0..columns {
                let Some(cell) = screen.cell(y, x) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let native = if cell.is_wide() { 2 } else { 1 };
                let text = if cell.has_contents() {
                    self.terminal.cell_text(cell.contents())
                } else {
                    Some(" ".into())
                };
                let text = match text {
                    None if owed > 0 => {
                        owed -= 1;
                        continue;
                    }
                    None => std::borrow::Cow::Borrowed(" "),
                    Some(text) => text,
                };
                owed = 0;
                let width = rich::cells::cell_len(&text);
                // Only a cluster's cell (read back whole, so owned) is
                // followed by filler cells that make up its width.
                let cluster = matches!(text, std::borrow::Cow::Owned(_));
                let text = if width < native || (width > native && !cluster) {
                    // Keep the columns: what rich would measure otherwise
                    // is drawn as blanks.
                    std::borrow::Cow::Owned(" ".repeat(native))
                } else {
                    owed = width - native;
                    text
                };
                let key = style_key_of(cell);
                if let Some(previous) = style_key.filter(|k| *k != key) {
                    line.push((std::mem::take(&mut run), previous));
                }
                style_key = Some(key);
                run.push_str(&text);
            }
            if let Some(key) = style_key {
                line.push((run, key));
            }
            runs.push(line);
        }
        runs.into_iter()
            .map(|line| {
                line.into_iter()
                    .map(|(text, key)| Segment::new(text, Some(self.style(key))))
                    .collect()
            })
            .collect()
    }

    fn style(&mut self, key: (u32, u32, u8)) -> Style {
        self.styles
            .entry(key)
            .or_insert_with(|| make_style(key))
            .clone()
    }
}

impl Drop for TermCore {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.host.kill();
        }
    }
}

fn color_key(color: vt100::Color) -> u32 {
    match color {
        vt100::Color::Default => 0,
        vt100::Color::Idx(index) => 0x100 | index as u32,
        vt100::Color::Rgb(r, g, b) => 0x0100_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32,
    }
}

fn color_of(key: u32) -> Option<Color> {
    if key == 0 {
        None
    } else if key & 0x0100_0000 != 0 {
        Some(Color::from_rgb((key >> 16) as u8, (key >> 8) as u8, key as u8))
    } else {
        Some(Color::from_ansi(key as u8))
    }
}

const BOLD: u8 = 1;
const DIM: u8 = 2;
const ITALIC: u8 = 4;
const UNDERLINE: u8 = 8;
const REVERSE: u8 = 16;

fn style_key_of(cell: &vt100::Cell) -> (u32, u32, u8) {
    let attrs = BOLD * cell.bold() as u8
        + DIM * cell.dim() as u8
        + ITALIC * cell.italic() as u8
        + UNDERLINE * cell.underline() as u8
        + REVERSE * cell.inverse() as u8;
    (color_key(cell.fgcolor()), color_key(cell.bgcolor()), attrs)
}

fn make_style((fg, bg, attrs): (u32, u32, u8)) -> Style {
    let names: Vec<&str> = [
        (BOLD, "bold"),
        (DIM, "dim"),
        (ITALIC, "italic"),
        (UNDERLINE, "underline"),
        (REVERSE, "reverse"),
    ]
    .into_iter()
    .filter(|(bit, _)| attrs & bit != 0)
    .map(|(_, name)| name)
    .collect();
    let base = if names.is_empty() {
        Style::new()
    } else {
        Style::parse(&names.join(" ")).unwrap_or_default()
    };
    base.combine(&Style::from_color(color_of(fg), color_of(bg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::ReplayHost;

    fn core(output: &str, columns: u16, rows: u16) -> TermCore {
        let mut core = TermCore::new(Box::new(ReplayHost::new().output(output)), 100);
        core.fit(columns, rows);
        core.pump();
        core
    }

    fn plain(lines: &[Vec<Segment>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect()
    }

    #[test]
    fn styles_and_wide_text_keep_their_columns() {
        let mut core = core("\x1b[1;31mred\x1b[0m 漢字 👩\u{200d}👧|", 20, 2);
        let lines = core.lines();
        let text = plain(&lines);
        assert_eq!(text[0], "red 漢字 👩\u{200d}👧|        ");
        assert_eq!(rich::cells::cell_len(&text[0]), 20);
        let red = &lines[0][0];
        assert_eq!(red.text, "red");
        let style = red.style.as_ref().unwrap();
        assert_eq!(style.color(), Some(&Color::from_ansi(1)));
        assert_eq!(style.attr(0), Some(true));
    }

    #[test]
    fn the_view_scrolls_back() {
        let mut core = core("1\r\n2\r\n3\r\n4", 5, 2);
        assert_eq!(plain(&core.lines())[0].trim_end(), "3");
        core.scroll_by(2);
        assert_eq!(core.scrolled(), 2);
        assert_eq!(plain(&core.lines())[0].trim_end(), "1");
        assert_eq!(core.cursor(), None);
        core.scroll_by(-10);
        assert_eq!(core.scrolled(), 0);
        assert_eq!(core.cursor(), Some((1, 1)));
    }
}
