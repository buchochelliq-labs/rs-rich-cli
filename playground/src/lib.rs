//! The docs playground: rs-rich rendering in the reader's browser tab.
//!
//! Tier 1 of docs/design/docs-playground.md. A `.rich-playground` block's
//! input (console markup, CSV, JSON, Markdown, …) and options go in; ANSI
//! text for xterm.js, or an HTML document from `Console::export_html_with`,
//! comes out. Everything goes through rs-rich's and rs-rich-ext's public
//! APIs, as a program would use them; nothing here changes how they render.
//!
//! The same functions run natively (`cargo test`) and under Node
//! (`scripts/check_playground.mjs`, which renders every block in the docs).

pub mod options;
pub mod render;

use rich::{ColorSystem, Console, SVG_EXPORT_THEME};
use wasm_bindgen::prelude::*;

use crate::options::{Options, MAX_INPUT};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(message: &str);
}

/// Panics go to the browser's console. They abort (`panic = "abort"`), so
/// the page reloads the module after one.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
fn start() {
    std::panic::set_hook(Box::new(|info| error(&info.to_string())));
}

/// A console as xterm.js is: a terminal, truecolor, `width` cells wide.
/// The clock is pinned: `std::time::Instant` panics on
/// `wasm32-unknown-unknown`, and nothing tier 1 draws needs the time.
fn console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .get_time(|| 0.0)
        .build()
}

fn prepare(kind: &str, input: &str, options: &str) -> Result<(Console, Options), String> {
    if input.len() > MAX_INPUT {
        return Err(format!("the input is over {} KB", MAX_INPUT / 1024));
    }
    if render::kind(kind).is_none() {
        return Err(format!("unknown kind {kind:?}"));
    }
    let options = Options::parse(options)?;
    Ok((console(options.width()), options))
}

/// Render `input` as a block of `kind` with `options` (a JSON object of the
/// block's `data-*` attributes), to ANSI text.
#[wasm_bindgen]
pub fn render(kind: &str, input: &str, options: &str) -> Result<String, String> {
    let (console, options) = prepare(kind, input, options)?;
    let mut result = Ok(());
    let out = console.capture(|c| result = render::draw(c, kind, input, &options));
    result.map(|()| out)
}

/// The HTML document the page shows in a sandboxed `srcdoc` iframe when it
/// has not loaded xterm.js: lines of exactly [`HTML_LINE_HEIGHT`] pixels,
/// so the page can size the frame from the line count.
pub const HTML_FORMAT: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
{stylesheet}
html, body {{
    margin: 0;
}}
body {{
    color: {foreground};
    background-color: {background};
}}
a {{
    color: inherit;
}}
pre {{
    margin: 0;
    padding: 8px 12px;
    font: 13px/17px "Fira Code", "DejaVu Sans Mono", Menlo, Consolas, monospace;
}}
</style>
</head>
<body><pre><code style="font-family:inherit">{code}</code></pre></body>
</html>
"#;

/// The height of one line of [`HTML_FORMAT`], in CSS pixels.
#[wasm_bindgen]
pub fn html_line_height() -> u32 {
    17
}

/// [`render`], exported as an HTML document with inline styles.
#[wasm_bindgen]
pub fn render_html(kind: &str, input: &str, options: &str) -> Result<String, String> {
    let (console, options) = prepare(kind, input, options)?;
    let mut result = Ok(());
    let html = console
        .export_html_with(&SVG_EXPORT_THEME, Some(HTML_FORMAT), true, |c| {
            result = render::draw(c, kind, input, &options)
        })
        .map_err(|e| e.to_string())?;
    result.map(|()| html)
}

/// The kinds this module renders, as JSON: `[{"name", "full", "input"}]`.
#[wasm_bindgen]
pub fn kinds() -> String {
    let kinds: Vec<_> = render::KINDS
        .iter()
        .filter(|kind| cfg!(feature = "full") || !kind.full)
        .map(|kind| serde_json::json!({"name": kind.name, "full": kind.full, "input": kind.input}))
        .collect();
    serde_json::Value::Array(kinds).to_string()
}

/// Which module this is, for the page's footer and the stamp check.
#[wasm_bindgen]
pub fn info() -> String {
    serde_json::json!({
        "module": if cfg!(feature = "full") { "full" } else { "lean" },
        "rs-rich": env!("RS_RICH_VERSION"),
        "rs-rich-ext": env!("RS_RICH_EXT_VERSION"),
    })
    .to_string()
}

#[cfg(test)]
mod tests;
