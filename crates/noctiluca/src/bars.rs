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
//! follows the page, the view and the media as they change.

use rich_intuituive::prelude::*;

use crate::actions::keys_label;
use crate::app::{current_media, Browser};
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
            " [bold]NOCTILUCA[/]   {view} + {pixels}   {back}{forward}{loading}   [bold]{}[/]",
            markup(&truncate(&title, 80))
        )
    })
    .no_wrap()
    .style(BAND)
    .fixed(1)
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
        let has_media = current_media(b).is_some();
        let mut parts = vec![
            format!("{} actions", keys_for("actions")),
            format!("{} hints", keys_for("hints")),
            format!("{} open", keys_for("open")),
            format!("{} reader", keys_for("reader")),
        ];
        if has_media {
            parts.push(format!("{} play/pause", keys_for("media.play_pause")));
            parts.push(format!(
                "{}/{} seek",
                keys_for("media.seek_back"),
                keys_for("media.seek_forward")
            ));
            parts.push(format!("{} mute", keys_for("media.mute")));
        }
        parts.push(format!("{} quit", keys_for("quit")));
        format!(" {}", markup(&parts.join(" | ")))
    })
    .no_wrap()
    .style("dim")
    .fixed(1)
    .name("keys");
    column([state, keys]).fixed(2)
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
