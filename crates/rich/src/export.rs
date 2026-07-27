//! Exporting rendered output to HTML.
//!
//! Port of `rich/console.py`'s `export_html` + the `_export_format.py` template.
//! Turns a recorded stream of [`Segment`]s (captured via
//! [`Console::export_html`](crate::console::Console::export_html)) into a
//! self-contained HTML document, using a [`TerminalTheme`] to resolve colors.
//!
//! Scope: `inline_styles` (each span carries its own `style="…"`). The CSS-class
//! variant is a follow-up (see docs/DIVERGENCES.md).

use crate::segment::Segment;
use crate::terminal_theme::TerminalTheme;

/// The HTML document template. Port of `_export_format.CONSOLE_HTML_FORMAT`
/// (placeholders are substituted, not `format!`-ed, to avoid brace escaping).
const CONSOLE_HTML_FORMAT: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
{stylesheet}
body {
    color: {foreground};
    background-color: {background};
}
</style>
</head>
<body>
    <pre style="font-family:Menlo,'DejaVu Sans Mono',consolas,'Courier New',monospace"><code style="font-family:inherit">{code}</code></pre>
</body>
</html>
"#;

/// HTML-escape `text` (matching Python's `html.escape`, `quote=True`).
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Render `segments` to a self-contained HTML document with inline styles.
/// Port of `Console.export_html(inline_styles=True)`.
pub fn export_html_inline(segments: &[Segment], theme: &TerminalTheme) -> String {
    let simplified = Segment::simplify(segments);
    let mut code = String::new();
    for segment in &simplified {
        if segment.control {
            continue;
        }
        let text = escape(&segment.text);
        match &segment.style {
            Some(style) if !style.is_null() => {
                let rule = style.get_html_style(theme);
                if rule.is_empty() {
                    code.push_str(&text);
                } else {
                    code.push_str(&format!("<span style=\"{rule}\">{text}</span>"));
                }
            }
            _ => code.push_str(&text),
        }
    }

    CONSOLE_HTML_FORMAT
        .replace("{stylesheet}", "")
        .replace("{foreground}", &theme.foreground.hex())
        .replace("{background}", &theme.background.hex())
        .replace("{code}", &code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;

    #[test]
    fn escapes_html_special_chars() {
        assert_eq!(escape("a<b>&\"'c"), "a&lt;b&gt;&amp;&quot;&#x27;c");
    }

    #[test]
    fn bold_red_html_style() {
        // Captured from real rich 15.0.0 Style.get_html_style(DEFAULT_TERMINAL_THEME).
        let style = Style::parse("bold red").unwrap();
        assert_eq!(
            style.get_html_style(&crate::terminal_theme::DEFAULT_TERMINAL_THEME),
            "color: #800000; text-decoration-color: #800000; font-weight: bold"
        );
    }
}
