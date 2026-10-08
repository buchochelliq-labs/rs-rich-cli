//! A small intuiTUIve app, served to a browser.
//!
//!     cargo run -p rs-rich-web --example serve [-- ADDRESS]
//!
//! Open the address it prints: it carries the token the server checks.
//! Every tab gets its own copy of the app, on its own thread: its own
//! clock, counter and list. The address is `127.0.0.1:8080` unless given;
//! keep it a loopback one unless a proxy with authentication is in front
//! (see the crate's documentation).
//!
//! In the page: type in the box and press Enter to add a line · Tab moves
//! to the counter, where + and - change it · a click on the counter adds
//! one · Ctrl+Q ends the tab's session (reload for a new one).

use std::time::Duration;

use rich_web::intuituive::interact::Input;
use rich_web::intuituive::prelude::*;
use rich_web::intuituive::rich::markup::escape;

/// The app each tab runs.
fn app() -> App {
    App::new(|| {
        let seconds = signal(0u64);
        every(Duration::from_secs(1), move |_| seconds.update(|s| *s += 1));
        let count = signal(0i64);
        let said = signal(Vec::<String>::new());

        let entry = repeating(
            || Input::new("Say"),
            move |line: String, _| {
                let line = line.trim().to_string();
                if !line.is_empty() {
                    said.update(|s| s.push(line));
                }
            },
        );
        let lines = text(move || {
            said.with(|s| {
                if s.is_empty() {
                    "[muted]Nothing yet: type above and press Enter.".to_string()
                } else {
                    s.iter()
                        .rev()
                        .map(|line| format!("[accent]•[/] {}", escape(line)))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            })
        });
        let counter = text!("[b]{count}[/]  [muted]+ and - change it; a click adds one")
            .focus_style("reverse")
            .on_key("+", move |_| count.update(|c| *c += 1))
            .on_key("-", move |_| count.update(|c| *c -= 1))
            .on_click(move |_| count.update(|c| *c += 1));

        column([
            text!("Up for [b]{seconds}[/] s in this tab")
                .panel("Clock")
                .fixed(3),
            entry.panel("Say something").fixed(3),
            counter.panel("Counter").fixed(3),
            lines.panel("Said"),
            label("[muted]Tab moves · Ctrl+Q ends this tab's session").auto(),
        ])
        .on_key("ctrl+q", |cx| cx.quit())
    })
}

fn main() -> std::io::Result<()> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());
    rich_web::Server::bind(addr.as_str(), app)?
        .title("rs-rich-web example")
        .run()
}
