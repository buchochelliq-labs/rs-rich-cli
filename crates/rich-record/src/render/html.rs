//! A self-contained HTML page per tape: a player and the screenshots.
//!
//! Everything is inline: the screens are HTML from rich-ext's frame exporter
//! ([`rich_ext::frame::Frame::to_html_with`]), with selectable text, and the
//! player is a few lines of script that swaps recorded screens in time. It
//! needs no terminal emulator and nothing from a CDN. Each distinct row is
//! stored once, and each screen lists its rows, so a typing demo costs a row
//! per keystroke rather than a screen.
//!
//! Nothing plays until the play button is pressed, so the page is still for
//! readers who prefer reduced motion.

use std::collections::HashMap;
use std::fmt::Write as _;

use rich_ext::frame::HtmlOptions;
use serde_json::json;

use crate::render::Look;
use crate::screen::{Cell, Snapshot, Theme};
use crate::session::Timeline;

/// How long a key stays in the player's overlay, in seconds, as in video.
const KEY_SHOWN: f64 = 0.8;
/// The longest pause the player makes between two screens, in seconds.
const IDLE: f64 = 2.0;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `snapshot`'s screen as HTML to put inside a `<pre>`.
fn screen_html(snapshot: &Snapshot, theme: &Theme) -> String {
    let terminal = theme.terminal_theme();
    let options = HtmlOptions {
        theme: &terminal,
        inline_styles: true,
        regions: false,
    };
    snapshot
        .export_frame(theme)
        .to_html_with(&options, "{code}")
        .expect("a template of one field is valid")
}

/// The page for a tape titled `look.title`, with its `shots` and a player
/// over `timeline`'s screens (and its keys, when `keys`).
pub fn page(
    look: &Look<'_>,
    shots: &[(String, Snapshot)],
    timeline: &Timeline,
    theme: &Theme,
    keys: bool,
) -> String {
    let hex = |(r, g, b): (u8, u8, u8)| format!("#{r:02x}{g:02x}{b:02x}");
    let (background, foreground) = (hex(theme.background), hex(theme.foreground));
    let title = escape(look.title);
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light dark; --screen-bg: {background}; --screen-fg: {foreground}; }}
body {{ margin: 0 auto; max-width: 72rem; padding: 1.5rem 1rem 3rem; font: 16px/1.5 system-ui, sans-serif; }}
h1 {{ font-size: 1.5rem; }}
h2 {{ font-size: 1.15rem; margin-top: 2.5rem; }}
figure {{ margin: 1.5rem 0; }}
figcaption {{ margin-top: .5rem; text-align: center; opacity: .85; }}
.screen {{ position: relative; overflow-x: auto; border-radius: 8px; background: var(--screen-bg); box-shadow: 0 4px 18px rgba(0, 0, 0, .3); }}
.screen pre {{ margin: 0; padding: 12px 14px; color: var(--screen-fg); background: var(--screen-bg); font: 14px/1.25 "Fira Code", "DejaVu Sans Mono", Menlo, Consolas, monospace; }}
.key {{ position: absolute; right: 12px; bottom: 12px; padding: 2px 10px; border-radius: 6px; background: #141414; color: #f0f0f0; border: 1px solid #787878; font: bold 14px/1.6 system-ui, sans-serif; }}
.key[hidden] {{ display: none; }}
.controls {{ display: flex; gap: .75rem; align-items: center; margin-top: .5rem; }}
.controls input {{ flex: 1; }}
.controls output {{ min-width: 4.5rem; font-variant-numeric: tabular-nums; text-align: right; }}
button {{ font: inherit; padding: .25rem .9rem; }}
</style>
</head>
<body>
<main>
<h1>{title}</h1>
"#
    );
    if !timeline.frames.is_empty() {
        let _ = write!(
            out,
            r#"<figure class="player" aria-label="Recording">
<div class="screen"><pre id="tape-screen"></pre><div class="key" id="tape-key" hidden></div></div>
<div class="controls">
<button type="button" id="tape-play" aria-pressed="false">Play</button>
<input type="range" id="tape-position" min="0" max="0" value="0" aria-label="Position in the recording">
<output id="tape-time" for="tape-position">0.0 s</output>
</div>
"#
        );
        if let Some(caption) = look.caption {
            let _ = writeln!(out, "<figcaption>{}</figcaption>", escape(caption));
        }
        out.push_str("</figure>\n");
    } else if let Some(caption) = look.caption {
        let _ = writeln!(out, "<p>{}</p>", escape(caption));
    }
    if !shots.is_empty() {
        out.push_str("<h2>Screenshots</h2>\n");
        for (name, snapshot) in shots {
            let name = escape(name);
            let _ = write!(
                out,
                "<figure id=\"shot-{name}\">\n<div class=\"screen\"><pre>{}</pre></div>\n<figcaption>{name}</figcaption>\n</figure>\n",
                screen_html(snapshot, theme)
            );
        }
    }
    out.push_str("</main>\n");
    if !timeline.frames.is_empty() {
        let data = player_data(timeline, theme, keys);
        let _ = write!(
            out,
            "<script type=\"application/json\" id=\"tape-data\">{data}</script>\n<script>{PLAYER}</script>\n"
        );
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// The screens as JSON: distinct rows once, each screen as its time and row
/// numbers, and the keys. `<` is escaped, so the JSON can sit in a script.
fn player_data(timeline: &Timeline, theme: &Theme, keys: bool) -> String {
    let mut rows: Vec<String> = Vec::new();
    let mut seen: HashMap<&[Cell], usize> = HashMap::new();
    let mut frames = Vec::with_capacity(timeline.frames.len());
    for (t, snapshot) in &timeline.frames {
        let numbers: Vec<usize> = snapshot
            .rows
            .iter()
            .map(|row| {
                *seen.entry(row.as_slice()).or_insert_with(|| {
                    let one = Snapshot {
                        rows: vec![row.clone()],
                        cursor: None,
                    };
                    rows.push(screen_html(&one, theme));
                    rows.len() - 1
                })
            })
            .collect();
        frames.push(json!([(t * 1000.0).round() / 1000.0, numbers]));
    }
    let keys: Vec<_> = if keys {
        timeline
            .keys
            .iter()
            .map(|(t, label)| json!([(t * 1000.0).round() / 1000.0, label]))
            .collect()
    } else {
        Vec::new()
    };
    json!({
        "rows": rows,
        "frames": frames,
        "keys": keys,
        "keyShown": KEY_SHOWN,
        "idle": IDLE,
    })
    .to_string()
    .replace('<', "\\u003c")
}

/// The player: show a screen, play from it with the recorded timing (pauses
/// capped at `idle`), scrub with the range. It starts on the last screen.
const PLAYER: &str = r#"(() => {
  const data = JSON.parse(document.getElementById("tape-data").textContent);
  const screen = document.getElementById("tape-screen");
  const key = document.getElementById("tape-key");
  const play = document.getElementById("tape-play");
  const position = document.getElementById("tape-position");
  const time = document.getElementById("tape-time");
  const last = data.frames.length - 1;
  let index = 0, timer = null;
  position.max = String(last);
  function show(i) {
    index = i;
    const [t, rows] = data.frames[i];
    screen.innerHTML = rows.map((r) => data.rows[r]).join("\n");
    position.value = String(i);
    time.value = t.toFixed(1) + " s";
    const pressed = data.keys.filter(([k]) => k <= t && t - k <= data.keyShown).pop();
    key.hidden = !pressed;
    key.textContent = pressed ? pressed[1] : "";
  }
  function stop() {
    clearTimeout(timer);
    timer = null;
    play.textContent = "Play";
    play.setAttribute("aria-pressed", "false");
  }
  function step() {
    if (index >= last) { stop(); return; }
    const wait = Math.min(data.frames[index + 1][0] - data.frames[index][0], data.idle);
    timer = setTimeout(() => { show(index + 1); step(); }, Math.max(wait, 0) * 1000);
  }
  play.addEventListener("click", () => {
    if (timer) { stop(); return; }
    if (index >= last) show(0);
    play.textContent = "Pause";
    play.setAttribute("aria-pressed", "true");
    step();
  });
  position.addEventListener("input", () => { stop(); show(Number(position.value)); });
  show(last);
})();"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(bytes: &[u8]) -> Snapshot {
        let mut parser = vt100::Parser::new(2, 12, 0);
        parser.process(bytes);
        Snapshot::from_screen(parser.screen(), &Theme::default())
    }

    #[test]
    fn a_page_holds_the_player_and_the_screenshots() {
        let timeline = Timeline {
            frames: vec![
                (0.0, snapshot(b"$ ")),
                (0.5, snapshot(b"$ l")),
                (1.0, snapshot(b"$ ls\r\n\x1b[31m<a>\x1b[0m")),
            ],
            keys: vec![(1.0, "⏎".into())],
            ..Timeline::default()
        };
        let shots = vec![("done".to_string(), timeline.frames[2].1.clone())];
        let look = Look {
            title: "A & B",
            window: true,
            caption: Some("Listing"),
        };
        let page = page(&look, &shots, &timeline, &Theme::default(), true);
        assert!(page.starts_with("<!DOCTYPE html>"));
        assert!(page.contains("<title>A &amp; B</title>"));
        assert!(page.contains("<figcaption>Listing</figcaption>"));
        assert!(page.contains("id=\"shot-done\""));
        // The screenshot is text, with its colour inline.
        assert!(page.contains("&lt;a&gt;</span>"), "{page}");
        // Nothing is fetched: no src or href attributes, no CDN.
        assert!(!page.contains(" src=") && !page.contains("href=") && !page.contains("http"));
        // The data sits in a script without closing it early.
        let data = page.split("id=\"tape-data\">").nth(1).unwrap();
        let data = data.split("</script>").next().unwrap();
        let json: serde_json::Value = serde_json::from_str(data).unwrap();
        assert_eq!(json["frames"].as_array().unwrap().len(), 3);
        // The blank second row is stored once and shared.
        assert_eq!(json["frames"][0][1][1], json["frames"][1][1][1]);
        assert_eq!(json["keys"][0][1], "⏎");
        let quiet = page_without_keys(&timeline);
        assert!(quiet.contains("\"keys\":[]"));
    }

    fn page_without_keys(timeline: &Timeline) -> String {
        page(&Look::window("t"), &[], timeline, &Theme::default(), false)
    }
}
