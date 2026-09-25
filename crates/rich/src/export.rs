//! Exporting rendered output to HTML.
//!
//! Port of `rich/console.py`'s `export_html` + the `_export_format.py` template.
//! Turns a recorded stream of [`Segment`]s (captured via
//! [`Console::export_html`](crate::console::Console::export_html)) into a
//! self-contained HTML document, using a [`TerminalTheme`] to resolve colors.
//!
//! Both variants are ported: `inline_styles` (each span carries its own
//! `style="…"`) and the CSS-class stylesheet, with links and `code_format`.

use crate::segment::Segment;
use crate::terminal_theme::TerminalTheme;

/// The HTML document template. Port of `_export_format.CONSOLE_HTML_FORMAT`,
/// verbatim: a Python format string, so literal braces are doubled. Pass it
/// (or your own, as upstream's `code_format=`) to [`export_html_with`].
pub const CONSOLE_HTML_FORMAT: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
{stylesheet}
body {{
    color: {foreground};
    background-color: {background};
}}
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

/// A `code_format` template that Python's `str.format` would reject: an
/// unknown field (`KeyError`), a positional or formatted field, or an
/// unmatched brace (`ValueError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFormatError(pub String);

impl std::fmt::Display for ExportFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid code_format: {}", self.0)
    }
}

impl std::error::Error for ExportFormatError {}

/// Substitute `{name}` fields in a Python format string, with `{{` and `}}`
/// standing for literal braces: the subset of `str.format` upstream's export
/// templates use. Conversions (`!r`) and format specs (`:>10`) are refused.
pub fn format_template(
    template: &str,
    fields: &[(&str, &str)],
) -> Result<String, ExportFormatError> {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '{' if chars.peek().map(|&(_, next)| next) == Some('{') => {
                chars.next();
                out.push('{');
            }
            '{' => {
                let rest = &template[index + 1..];
                let Some(close) = rest.find('}') else {
                    return Err(ExportFormatError(
                        "expected '}' before end of string".to_string(),
                    ));
                };
                let name = &rest[..close];
                if name.contains(['{', '!', ':', '[', '.']) || name.is_empty() {
                    return Err(ExportFormatError(format!(
                        "unsupported replacement field {{{name}}}"
                    )));
                }
                let Some((_, value)) = fields.iter().find(|(field, _)| *field == name) else {
                    return Err(ExportFormatError(format!("unknown field {name:?}")));
                };
                out.push_str(value);
                // Skip the field name and its closing brace.
                for _ in 0..name.chars().count() + 1 {
                    chars.next();
                }
            }
            '}' if chars.peek().map(|&(_, next)| next) == Some('}') => {
                chars.next();
                out.push('}');
            }
            '}' => {
                return Err(ExportFormatError(
                    "Single '}' encountered in format string".to_string(),
                ))
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Render `segments` to a self-contained HTML document with inline styles.
/// Port of `Console.export_html(inline_styles=True)`.
pub fn export_html_inline(segments: &[Segment], theme: &TerminalTheme) -> String {
    export_html_with(segments, theme, None, true).expect("the built-in template is valid")
}

/// Render `segments` to a self-contained HTML document using CSS classes and a
/// generated stylesheet. Port of `Console.export_html(inline_styles=False)`
/// (upstream's default). Distinct styles are numbered `.r1`, `.r2`, … in the
/// order first seen.
pub fn export_html_classes(segments: &[Segment], theme: &TerminalTheme) -> String {
    export_html_with(segments, theme, None, false).expect("the built-in template is valid")
}

/// Render `segments` to HTML with every option of upstream's
/// `Console.export_html`: `code_format` replaces [`CONSOLE_HTML_FORMAT`]
/// (fields `{code}`, `{stylesheet}`, `{foreground}`, `{background}`), and
/// `inline_styles` chooses `style="…"` attributes over a class stylesheet.
///
/// A styled segment with a link becomes an `<a href="…">` (the URL is not
/// escaped, as upstream does not escape it).
pub fn export_html_with(
    segments: &[Segment],
    theme: &TerminalTheme,
    code_format: Option<&str>,
    inline_styles: bool,
) -> Result<String, ExportFormatError> {
    let simplified = Segment::simplify(segments);
    let mut code = String::new();
    // (rule → class number), in insertion order: `styles.setdefault(...)`.
    let mut styles: Vec<(String, usize)> = Vec::new();
    for segment in &simplified {
        if segment.control {
            continue;
        }
        let mut text = escape(&segment.text);
        // `if style:` — a null style is falsy.
        if let Some(style) = segment.style.as_ref().filter(|style| !style.is_null()) {
            let rule = style.get_html_style(theme);
            if inline_styles {
                if let Some(link) = style.link() {
                    text = format!("<a href=\"{link}\">{text}</a>");
                }
                if !rule.is_empty() {
                    text = format!("<span style=\"{rule}\">{text}</span>");
                }
            } else {
                // Upstream numbers every truthy style, even one whose rule is
                // empty; only the stylesheet skips empty rules.
                let number = match styles.iter().find(|(existing, _)| *existing == rule) {
                    Some((_, n)) => *n,
                    None => {
                        let n = styles.len() + 1;
                        styles.push((rule, n));
                        n
                    }
                };
                text = match style.link() {
                    Some(link) => format!("<a class=\"r{number}\" href=\"{link}\">{text}</a>"),
                    None => format!("<span class=\"r{number}\">{text}</span>"),
                };
            }
        }
        code.push_str(&text);
    }
    let stylesheet = styles
        .iter()
        .filter(|(rule, _)| !rule.is_empty())
        .map(|(rule, number)| format!(".r{number} {{{rule}}}"))
        .collect::<Vec<_>>()
        .join("\n");
    format_template(
        code_format.unwrap_or(CONSOLE_HTML_FORMAT),
        &[
            ("code", &code),
            ("stylesheet", &stylesheet),
            ("foreground", &theme.foreground.hex()),
            ("background", &theme.background.hex()),
        ],
    )
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
    fn format_template_follows_python_str_format() {
        let fields = [("a", "1"), ("b", "2")];
        assert_eq!(
            format_template("{{x}} {a}-{b}", &fields).unwrap(),
            "{x} 1-2"
        );
        assert!(format_template("{c}", &fields).is_err());
        assert!(format_template("{}", &fields).is_err());
        assert!(format_template("{a!r}", &fields).is_err());
        assert!(format_template("a } b", &fields).is_err());
        assert!(format_template("a { b", &fields).is_err());
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
