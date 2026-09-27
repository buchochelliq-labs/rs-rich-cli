//! asciinema v2 casts.

use serde_json::json;

use crate::screen::{Rgb, Snapshot, Theme};
use crate::session::{Event, Timeline};

fn hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// A cast of `timeline`. The header carries the theme, so players (and
/// asciinema.org) show it in the screenshots' colours.
pub fn cast(timeline: &Timeline, columns: u16, rows: u16, title: &str, theme: &Theme) -> String {
    let palette: Vec<String> = theme.ansi.iter().copied().map(hex).collect();
    let header = json!({
        "version": 2,
        "width": columns,
        "height": rows,
        "title": title,
        "env": {"TERM": "xterm-256color", "SHELL": "bash"},
        "theme": {
            "fg": hex(theme.foreground),
            "bg": hex(theme.background),
            "palette": palette.join(":"),
        },
    });
    let mut out = header.to_string();
    out.push('\n');
    for (t, event) in &timeline.events {
        let t = (t * 1000.0).round() / 1000.0;
        let line = match event {
            Event::Output(data) if !data.is_empty() => json!([t, "o", data]),
            Event::Input(data) if !data.is_empty() => json!([t, "i", data]),
            Event::Resize { columns, rows } => json!([t, "r", format!("{columns}x{rows}")]),
            _ => continue,
        };
        out.push_str(&line.to_string());
        out.push('\n');
    }
    out
}

/// Escape codes that draw `snapshot` from scratch: used when a hidden
/// stretch ends. Default colours are written as defaults, so a player's own
/// background shows through.
pub fn repaint(snapshot: &Snapshot, theme: &Theme) -> String {
    let mut out = String::from("\x1b[0m\x1b[2J\x1b[H");
    for (y, row) in snapshot.rows.iter().enumerate() {
        out.push_str(&format!("\x1b[{};1H", y + 1));
        for cell in row.iter().filter(|cell| !cell.is_continuation()) {
            let mut codes = vec!["0".to_string()];
            if cell.fg != theme.foreground {
                let (r, g, b) = cell.fg;
                codes.push(format!("38;2;{r};{g};{b}"));
            }
            if cell.bg != theme.background {
                let (r, g, b) = cell.bg;
                codes.push(format!("48;2;{r};{g};{b}"));
            }
            for (on, code) in [(cell.bold, "1"), (cell.italic, "3"), (cell.underline, "4")] {
                if on {
                    codes.push(code.into());
                }
            }
            out.push_str(&format!("\x1b[{}m{}", codes.join(";"), cell.text));
        }
    }
    out.push_str("\x1b[0m");
    match snapshot.cursor {
        Some((x, y)) => out.push_str(&format!("\x1b[{};{}H\x1b[?25h", y + 1, x + 1)),
        None => out.push_str("\x1b[?25l"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_events_and_resize() {
        let timeline = Timeline {
            events: vec![
                (0.0, Event::Output("hi".into())),
                (0.5, Event::Input("\r".into())),
                (
                    1.23456,
                    Event::Resize {
                        columns: 40,
                        rows: 8,
                    },
                ),
            ],
            ..Timeline::default()
        };
        let cast = cast(&timeline, 80, 24, "Demo", &Theme::default());
        let lines: Vec<&str> = cast.lines().collect();
        let header: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(header["version"], 2);
        assert_eq!(header["theme"]["bg"], "#292929");
        assert_eq!(lines[1], r#"[0.0,"o","hi"]"#);
        assert_eq!(lines[2], r#"[0.5,"i","\r"]"#);
        assert_eq!(lines[3], r#"[1.235,"r","40x8"]"#);
    }
}
