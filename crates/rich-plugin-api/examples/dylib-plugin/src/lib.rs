//! An example native plugin for `rich`.
//!
//! Everything crosses the boundary as UTF-8 text through the C ABI that
//! `export_dylib_plugin!` writes; this crate has no unsafe code of its own.

use rich_plugin_api::abi::Exports;

/// `reverse`: a transform that reverses each line.
fn reverse(input: &str, _width: u32) -> Result<String, String> {
    Ok(input
        .lines()
        .map(|line| line.chars().rev().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n"))
}

/// `digits`: a highlighter that makes every ASCII digit bold magenta.
fn digits(input: &str, _width: u32) -> Result<String, String> {
    Ok(input
        .char_indices()
        .filter(|(_, c)| c.is_ascii_digit())
        .map(|(i, _)| format!("{i} {} bold magenta\n", i + 1))
        .collect())
}

/// `banner` fences: the code in a markup banner as wide as the width given.
fn banner(input: &str, width: u32) -> Result<String, String> {
    if input.trim().is_empty() {
        return Err("an empty banner".into());
    }
    let rule = "=".repeat(width.clamp(1, 200) as usize);
    Ok(format!("[bold green]{rule}[/]\n{}\n[bold green]{rule}[/]", input.trim_end()))
}

/// `red` fences: the code in red, as ANSI. The window-title escape shows
/// that a host strips everything but SGR.
fn red(input: &str, _width: u32) -> Result<String, String> {
    Ok(format!(
        "\u{1b}]0;owned\u{7}\u{1b}[31m{}\u{1b}[0m",
        input.trim_end()
    ))
}

fn exports() -> Exports {
    Exports::new("example-dylib", env!("CARGO_PKG_VERSION"))
        .description("An example native plugin")
        .transform("reverse", reverse)
        .highlighter("digits", digits)
        .fence_markup("banner", banner)
        .fence_ansi("red", red)
}

#[cfg(not(feature = "future-abi"))]
rich_plugin_api::export_dylib_plugin!(exports);

/// A descriptor from a future ABI: only the version words, which is all a
/// host may read before refusing it.
#[cfg(feature = "future-abi")]
#[unsafe(no_mangle)]
pub extern "C" fn rich_plugin_entry() -> *const u32 {
    let _ = exports;
    static VERSION: [u32; 2] = [rich_plugin_api::abi::ABI_MAJOR + 1, 0];
    VERSION.as_ptr()
}
