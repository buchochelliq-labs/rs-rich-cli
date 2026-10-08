//! A shell beside a web page: a terminal pane running `$SHELL` (or `sh`)
//! and a web view showing a page in w3m, with a status line built from the
//! page's signals.
//!
//!     cargo run -p rs-rich-embed --example embed
//!     cargo run -p rs-rich-embed --example embed -- https://example.org
//!
//! Click a pane (or press F6) to move between them; each takes every key
//! while it has the focus. In the web view, Ctrl+L types an address,
//! Alt+Left and Alt+Right go back and forward, F5 reloads. F10 quits.
//! w3m is not part of this crate: install it (`apt install w3m`,
//! `brew install w3m`) or name another browser in `RICH_EMBED_BROWSER`.

use rich_embed::{terminal, web_view_with, ProgramEngine, BROWSER_VARIABLE};
use rich_intuituive as intuituive;

use intuituive::prelude::*;

fn main() -> std::io::Result<()> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://example.com".to_string());
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    App::new(move || {
        let browser = match std::env::var_os(BROWSER_VARIABLE) {
            Some(_) => ProgramEngine::detect(),
            None => ProgramEngine::new("w3m"),
        };
        let page = web_view_with(browser, url).release_keys("f6 f10");
        let web = page.handle();
        let shell = terminal(shell.as_str())
            .release_keys("f6 f10")
            .on_exit(|status, cx| cx.toast(format!("the shell {status}")));
        let status = shell.status();
        column([
            row([shell.node().panel("shell"), page.node().panel("web")]).gap(1),
            text(move || {
                let shell = match status.get() {
                    Some(status) => format!("shell {status}"),
                    None => "shell running".to_string(),
                };
                let page = if web.loading().get() {
                    "loading…".to_string()
                } else {
                    web.address().get()
                };
                format!("[dim]{shell} · {page} · F6 switches pane · F10 quits")
            })
            .fixed(1),
        ])
        .on_key("f6", |cx| cx.focus_next())
        .on_key("f10", |cx| cx.quit())
    })
    .run()
}
