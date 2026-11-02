//! The header and the status bar, as the PoC drew them:
//!
//! - the header's first row, on a violet band: the browser, the view and
//!   how pictures reach the terminal, then the page's title; its second
//!   row, on a dark band, the address;
//! - the status bar's first row: a message, else the media state
//!   (`media PLAYING 0:03 / 0:05`), the row in view and the sound; its
//!   second row the main keys.
//!
//! Each row is an intuiTUIve `text` node over the app's signals, so it
//! follows the page, the view and the media as they change. The mouse
//! uses them as a toolbar: `◀` `▶` `↻` in the header go back, forward and
//! reload, the address row edits the address, and each key in the status
//! bar's second row runs its action.

use rich_intuituive::prelude::*;

use rich::cells::cell_len;

use crate::actions::keys_label;
use crate::app::{Browser, current_media, run};
use crate::clean::{markup, truncate};
use crate::config::Images;
use crate::engine::Playback;

/// The header's band.
const BAND: &str = "white on rgb(53,31,83)";
/// The address's and the status bar's band.
const DARK: &str = "rgb(200,203,215) on rgb(24,26,35)";

/// How the view's pictures reach the terminal, as the header says it:
/// the protocol in use (`auto` resolved to what the terminal speaks), or
/// `NO PICTURES`.
pub fn pixels_name(images: Images, pixels: rich_intuituive::Pixels) -> String {
    match images {
        Images::None => "NO PICTURES".into(),
        _ => pixels.name().to_uppercase(),
    }
}

/// The media state as the status bar says it: `media PLAYING 0:03 / 0:05`,
/// with `muted` when it is.
pub fn media_state(playback: &Playback) -> String {
    let time = format!(
        "{} / {}",
        clock(playback.time),
        playback.duration.map_or("live".into(), clock)
    );
    let muted = if playback.muted { " muted" } else { "" };
    format!("media {} {time}{muted}", playback.label())
}

/// Seconds as `m:ss`.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The header's band before its buttons, as plain text: where they start.
fn band_prefix(b: Browser) -> String {
    let view = b.view.get().name().to_uppercase();
    let pixels = pixels_name(b.images.get(), b.pixels.get());
    format!(" NOCTILUCA   {view} + {pixels}   ")
}

/// The header's buttons, `◀ ▶ ↻`: each one's column after the band's
/// prefix, and the action it runs.
const BUTTONS: [(usize, &str); 3] = [(0, "back"), (2, "forward"), (4, "reload")];

/// The action of the header's button at `column`, if one is there.
pub fn header_button(b: Browser, column: usize) -> Option<&'static str> {
    let start = cell_len(&band_prefix(b));
    BUTTONS
        .iter()
        .find(|(at, _)| column == start + at)
        .map(|(_, id)| *id)
}

/// The header: two rows.
pub fn header(b: Browser) -> Node {
    let band = text(move || {
        let state = b.state.get();
        let back = if state.can_go_back {
            "◀"
        } else {
            "[dim]◀[/]"
        };
        let forward = if state.can_go_forward {
            "▶"
        } else {
            "[dim]▶[/]"
        };
        let loading = if state.loading { " [yellow]…[/]" } else { "" };
        let title = b
            .page
            .with(|p| p.as_ref().map(|p| p.title.clone()))
            .unwrap_or_default();
        let view = b.view.get().name().to_uppercase();
        let pixels = pixels_name(b.images.get(), b.pixels.get());
        format!(
            " [bold]NOCTILUCA[/]   {view} + {pixels}   {back} {forward} ↻{loading}   [bold]{}[/]",
            markup(&truncate(&title, 80))
        )
    })
    .no_wrap()
    .style(BAND)
    .fixed(1)
    .on_click_at(move |cx, column, _| {
        header_button(b, usize::from(column)).is_some_and(|id| {
            run(b, id, cx);
            true
        })
    })
    .name("header");
    let address = text(move || {
        let url = b
            .page
            .with(|p| p.as_ref().map(|p| p.url.clone()))
            .unwrap_or_default();
        format!(" {}", markup(&truncate(&url, 200)))
    })
    .no_wrap()
    .style(DARK)
    .fixed(1)
    // A click edits the address, as Ctrl+L does.
    .on_click_at(move |cx, _, _| {
        run(b, "address", cx);
        true
    })
    .name("address");
    column([band, address]).fixed(2)
}

/// The status bar: two rows.
pub fn status(b: Browser) -> Node {
    let keys_for = move |id: &str| b.keymap.with(|k| keys_label(k, id));
    let state = text(move || {
        if let Some(hints) = b.hints.get() {
            return format!(
                " [bold black on yellow] hints [/] type a label: {}  [dim]Esc cancels[/]",
                markup(&hints.typed)
            );
        }
        let message = b.message.get();
        if !message.is_empty() {
            return format!(" {message}");
        }
        if let Some(find) = b.find.get() {
            return format!(
                " find: {}  [dim]{} next · {} previous[/]",
                markup(&find),
                markup(&keys_for("find.next")),
                markup(&keys_for("find.previous"))
            );
        }
        let mut parts = vec![b.view.get().name().to_string()];
        // A page with more than the model reads says so.
        if b.page.with(|p| p.as_ref().is_some_and(|p| p.truncated)) {
            parts.push("[yellow]page cut: too long to read whole[/]".into());
        }
        if let Some((_, playback)) = current_media(b) {
            parts.push(markup(&media_state(&playback)));
            if !b.config.with(|c| c.audio) {
                parts.push("sound off (--audio)".into());
            }
        }
        let rows = b.layout.with(|l| l.lines.len());
        let row = b.scroll.get() + 1;
        parts.push(format!("row {row}/{}", rows.max(1)));
        format!(" {}", parts.join(" | "))
    })
    .no_wrap()
    .style(DARK)
    .fixed(1)
    .name("status");
    let keys = text(move || {
        let parts: Vec<String> = key_parts(b).into_iter().map(|(text, _)| text).collect();
        format!(" {}", markup(&parts.join(" | ")))
    })
    .no_wrap()
    .style("dim")
    .fixed(1)
    // A click on a key runs its action.
    .on_click_at(move |cx, column, _| {
        key_at(b, usize::from(column)).is_some_and(|id| {
            b.message.set(String::new());
            run(b, id, cx);
            true
        })
    })
    .name("keys");
    column([state, keys]).fixed(2)
}

/// The status bar's keys, as it shows them, each with the action it runs
/// when clicked. The seek pair runs the forward seek.
fn key_parts(b: Browser) -> Vec<(String, &'static str)> {
    let keys_for = move |id: &str| b.keymap.with(|k| keys_label(k, id));
    let mut parts = vec![
        (format!("{} actions", keys_for("actions")), "actions"),
        (format!("{} hints", keys_for("hints")), "hints"),
        (format!("{} open", keys_for("open")), "open"),
        (format!("{} reader", keys_for("reader")), "reader"),
    ];
    if current_media(b).is_some() {
        parts.push((
            format!("{} play/pause", keys_for("media.play_pause")),
            "media.play_pause",
        ));
        parts.push((
            format!(
                "{}/{} seek",
                keys_for("media.seek_back"),
                keys_for("media.seek_forward")
            ),
            "media.seek_forward",
        ));
        parts.push((format!("{} mute", keys_for("media.mute")), "media.mute"));
    }
    parts.push((format!("{} quit", keys_for("quit")), "quit"));
    parts
}

/// The action of the key at `column` of the status bar's second row.
fn key_at(b: Browser, column: usize) -> Option<&'static str> {
    // After the leading space, the parts joined by " | ".
    let mut at = 1;
    key_parts(b).into_iter().find_map(|(text, id)| {
        let width = cell_len(&text);
        let hit = (at..at + width).contains(&column);
        at += width + 3;
        hit.then_some(id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_media_state_says_what_the_element_does() {
        let mut p = Playback {
            paused: false,
            volume: 1.0,
            time: 3.4,
            duration: Some(5.0),
            ready: 4,
            ..Playback::default()
        };
        assert_eq!(media_state(&p), "media PLAYING 0:03 / 0:05");
        p.paused = true;
        p.muted = true;
        assert_eq!(media_state(&p), "media PAUSED 0:03 / 0:05 muted");
        p.paused = false;
        p.muted = false;
        p.ready = 2;
        p.duration = None;
        assert_eq!(media_state(&p), "media BUFFERING 0:03 / live");
        p.ended = true;
        assert!(media_state(&p).starts_with("media ENDED"));
    }
}
