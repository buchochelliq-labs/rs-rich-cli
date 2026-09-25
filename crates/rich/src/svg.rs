//! Exporting rendered output to SVG.
//!
//! Port of `rich/console.py`'s `export_svg` + the `CONSOLE_SVG_FORMAT` template.
//! Turns a recorded stream of [`Segment`]s (captured via
//! [`Console::export_svg`](crate::console::Console::export_svg)) into a
//! self-contained SVG image of a terminal window, using a [`TerminalTheme`].
//!
//! **Divergence:** upstream's *default* `unique_id` is `adler32` over Python's
//! `repr()` of each segment, which Rust can't reproduce. So byte-parity holds
//! only when an explicit `unique_id` is passed (see docs/DIVERGENCES.md #15) —
//! the same shape as the OSC 8 `id=` deviation (#20).

use crate::cells::cell_len;
use crate::segment::Segment;
use crate::terminal_theme::TerminalTheme;

const CHAR_HEIGHT: f64 = 20.0;
const FONT_ASPECT_RATIO: f64 = 0.61;
const MARGIN: i64 = 1;
const PADDING_TOP: i64 = 40;
const PADDING_SIDE: i64 = 8;

/// The SVG document template. Port of `_export_format.CONSOLE_SVG_FORMAT`,
/// verbatim: a Python format string, so literal braces are doubled. Pass it
/// (or your own, as upstream's `code_format=`) to [`export_svg_with`].
pub const CONSOLE_SVG_FORMAT: &str = r#"<svg class="rich-terminal" viewBox="0 0 {width} {height}" xmlns="http://www.w3.org/2000/svg">
    <!-- Generated with Rich https://www.textualize.io -->
    <style>

    @font-face {{
        font-family: "Fira Code";
        src: local("FiraCode-Regular"),
                url("https://cdnjs.cloudflare.com/ajax/libs/firacode/6.2.0/woff2/FiraCode-Regular.woff2") format("woff2"),
                url("https://cdnjs.cloudflare.com/ajax/libs/firacode/6.2.0/woff/FiraCode-Regular.woff") format("woff");
        font-style: normal;
        font-weight: 400;
    }}
    @font-face {{
        font-family: "Fira Code";
        src: local("FiraCode-Bold"),
                url("https://cdnjs.cloudflare.com/ajax/libs/firacode/6.2.0/woff2/FiraCode-Bold.woff2") format("woff2"),
                url("https://cdnjs.cloudflare.com/ajax/libs/firacode/6.2.0/woff/FiraCode-Bold.woff") format("woff");
        font-style: bold;
        font-weight: 700;
    }}

    .{unique_id}-matrix {{
        font-family: Fira Code, monospace;
        font-size: {char_height}px;
        line-height: {line_height}px;
        font-variant-east-asian: full-width;
    }}

    .{unique_id}-title {{
        font-size: 18px;
        font-weight: bold;
        font-family: arial;
    }}

    {styles}
    </style>

    <defs>
    <clipPath id="{unique_id}-clip-terminal">
      <rect x="0" y="0" width="{terminal_width}" height="{terminal_height}" />
    </clipPath>
    {lines}
    </defs>

    {chrome}
    <g transform="translate({terminal_x}, {terminal_y})" clip-path="url(#{unique_id}-clip-terminal)">
    {backgrounds}
    <g class="{unique_id}-matrix">
    {matrix}
    </g>
    </g>
</svg>
"#;

/// Python `format(v, "g")`: 6 significant figures, trailing zeros (and a
/// trailing `.`) stripped, scientific below `1e-4` or from `1e6`. Used for
/// the coordinates inside SVG tags.
fn fmt_g(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    if !v.is_finite() {
        return py_non_finite(v);
    }
    // The exponent after rounding to 6 significant figures.
    let sci = format!("{v:.5e}");
    let (mantissa, exp) = split_exp(&sci);
    if (-4..6).contains(&exp) {
        let decimals = (5 - exp) as usize;
        strip_fraction_zeros(format!("{v:.decimals$}"))
    } else {
        py_exponent(&strip_fraction_zeros(mantissa.to_string()), exp)
    }
}

/// Python `str(float)`: the shortest round-tripping form, keeping a `.0` for
/// integer-valued floats, scientific below `1e-4` or from `1e16`. Used for
/// the template's float placeholders.
fn fmt_str(v: f64) -> String {
    if !v.is_finite() {
        return py_non_finite(v);
    }
    let sci = format!("{v:e}");
    let (mantissa, exp) = split_exp(&sci);
    if v != 0.0 && !(-4..16).contains(&exp) {
        return py_exponent(mantissa, exp);
    }
    let s = format!("{v}");
    if s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}

/// `inf`, `-inf` or `nan`, as Python formats them.
fn py_non_finite(v: f64) -> String {
    if v.is_nan() {
        "nan".to_string()
    } else if v > 0.0 {
        "inf".to_string()
    } else {
        "-inf".to_string()
    }
}

/// Split Rust's `1.5e-7` form into its mantissa and exponent.
fn split_exp(sci: &str) -> (&str, i32) {
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci, "0"));
    (mantissa, exp.parse().unwrap_or(0))
}

/// Python's exponent suffix: signed, at least two digits (`1.5e-07`).
fn py_exponent(mantissa: &str, exp: i32) -> String {
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exp.abs())
}

/// Drop trailing fractional zeros and a trailing `.`.
fn strip_fraction_zeros(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// HTML-escape (`quote=True`) then replace spaces with `&#160;`. Port of the
/// `escape_text` closure.
fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
        .replace(' ', "&#160;")
}

/// Split `segments` into lines, padding each to `length` cells and appending a
/// trailing `\n` segment. Port of `Segment.split_and_crop_lines` (`pad=True`,
/// `include_new_lines=True`); a final line with no trailing newline gets no
/// `\n` segment.
fn split_and_crop_lines(segments: &[Segment], length: usize) -> Vec<Vec<Segment>> {
    let mut result: Vec<Vec<Segment>> = Vec::new();
    let mut line: Vec<Segment> = Vec::new();
    for seg in segments {
        if !seg.control && seg.text.contains('\n') {
            let mut text = seg.text.as_str();
            loop {
                match text.find('\n') {
                    Some(idx) => {
                        let before = &text[..idx];
                        if !before.is_empty() {
                            line.push(Segment::new(before, seg.style.clone()));
                        }
                        let mut cropped = Segment::adjust_line_length(&line, length, None);
                        cropped.push(Segment::new("\n", None));
                        result.push(cropped);
                        line = Vec::new();
                        text = &text[idx + 1..];
                    }
                    None => {
                        if !text.is_empty() {
                            line.push(Segment::new(text, seg.style.clone()));
                        }
                        break;
                    }
                }
            }
        } else {
            line.push(seg.clone());
        }
    }
    if !line.is_empty() {
        result.push(Segment::adjust_line_length(&line, length, None));
    }
    result
}

/// Look up (or insert) the class number for a CSS rule string, numbering from 1
/// in first-seen order.
fn class_number(classes: &mut Vec<(String, usize)>, rules: &str) -> usize {
    match classes.iter().find(|(existing, _)| existing == rules) {
        Some((_, n)) => *n,
        None => {
            let n = classes.len() + 1;
            classes.push((rules.to_string(), n));
            n
        }
    }
}

/// Render `segments` to a self-contained SVG document. Port of
/// `Console.export_svg`. `unique_id` prefixes every generated id/class; passing
/// a fixed value makes the output deterministic (and byte-parity — see the
/// module divergence note).
pub fn export_svg(
    segments: &[Segment],
    theme: &TerminalTheme,
    title: &str,
    unique_id: &str,
    width: usize,
) -> String {
    export_svg_with(
        segments,
        theme,
        title,
        unique_id,
        width,
        CONSOLE_SVG_FORMAT,
        FONT_ASPECT_RATIO,
    )
    .expect("the built-in template is valid")
}

/// [`export_svg`] with upstream's `code_format` and `font_aspect_ratio`
/// arguments. `code_format` is a Python format string (literal braces
/// doubled) with the fields `{unique_id}`, `{char_width}`, `{char_height}`,
/// `{line_height}`, `{terminal_width}`, `{terminal_height}`, `{width}`,
/// `{height}`, `{terminal_x}`, `{terminal_y}`, `{styles}`, `{chrome}`,
/// `{backgrounds}`, `{matrix}` and `{lines}`; `font_aspect_ratio` (upstream's
/// default 0.61) sets the character cell width.
#[allow(clippy::too_many_arguments)]
pub fn export_svg_with(
    segments: &[Segment],
    theme: &TerminalTheme,
    title: &str,
    unique_id: &str,
    width: usize,
    code_format: &str,
    font_aspect_ratio: f64,
) -> Result<String, crate::export::ExportFormatError> {
    let char_width = CHAR_HEIGHT * font_aspect_ratio;
    let line_height = CHAR_HEIGHT * 1.22;
    let padding_width = PADDING_SIDE + PADDING_SIDE;
    let padding_height = PADDING_TOP + PADDING_SIDE;
    let margin_width = MARGIN + MARGIN;
    let margin_height = MARGIN + MARGIN;

    // Upstream's `ceil(width * char_width + padding_width)` raises for a
    // non-finite result (`OverflowError` for infinity, `ValueError` for NaN).
    let terminal_width_float = (width as f64 * char_width + padding_width as f64).ceil();
    if terminal_width_float.is_nan() {
        return Err(crate::export::ExportFormatError(
            "cannot convert float NaN to integer".to_string(),
        ));
    }
    if terminal_width_float.is_infinite() {
        return Err(crate::export::ExportFormatError(
            "cannot convert float infinity to integer".to_string(),
        ));
    }

    let filtered: Vec<Segment> = segments.iter().filter(|s| !s.control).cloned().collect();

    let mut classes: Vec<(String, usize)> = Vec::new();
    let mut text_backgrounds: Vec<String> = Vec::new();
    let mut text_group: Vec<String> = Vec::new();

    let lines = split_and_crop_lines(&filtered, width);
    let mut last_y = 0usize;
    for (y, line) in lines.iter().enumerate() {
        last_y = y;
        let mut x = 0usize;
        for seg in line {
            let style = seg.style.clone().unwrap_or_default();
            let rules = style.get_svg_style(theme);
            let class_no = class_number(&mut classes, &rules);

            let (has_background, background) = if style.attr(6) == Some(true) {
                // reverse: the (foreground) colour becomes the background rect.
                let hex = style
                    .color()
                    .map_or(theme.foreground, |c| theme.resolve(c, true))
                    .hex();
                (true, hex)
            } else {
                let has_bg = style.bgcolor().is_some_and(|c| !c.is_default());
                let hex = style
                    .bgcolor()
                    .map_or(theme.background, |c| theme.resolve(c, false))
                    .hex();
                (has_bg, hex)
            };

            let text_length = cell_len(&seg.text);
            if has_background {
                text_backgrounds.push(format!(
                    r#"<rect fill="{background}" x="{x}" y="{y2}" width="{w}" height="{h}" shape-rendering="crispEdges"/>"#,
                    x = fmt_g(x as f64 * char_width),
                    y2 = fmt_g(y as f64 * line_height + 1.5),
                    w = fmt_g(char_width * text_length as f64),
                    h = fmt_g(line_height + 0.25),
                ));
            }

            if !seg.text.chars().all(|c| c == ' ') {
                let char_count = seg.text.chars().count();
                text_group.push(format!(
                    r#"<text class="{unique_id}-r{class_no}" x="{x}" y="{y2}" textLength="{tl}" clip-path="url(#{unique_id}-line-{y})">{content}</text>"#,
                    x = fmt_g(x as f64 * char_width),
                    y2 = fmt_g(y as f64 * line_height + CHAR_HEIGHT),
                    tl = fmt_g(char_width * char_count as f64),
                    content = escape_text(&seg.text),
                ));
            }
            x += text_length;
        }
    }

    // Per-line clip-paths for every line but the last (upstream's `range(y)`).
    let lines_svg: Vec<String> = (0..last_y)
        .map(|line_no| {
            let offset = line_no as f64 * line_height + 1.5;
            format!(
                "<clipPath id=\"{unique_id}-line-{line_no}\">\n    <rect x=\"0\" y=\"{y}\" width=\"{w}\" height=\"{h}\"/>\n            </clipPath>",
                y = fmt_g(offset),
                w = fmt_g(char_width * width as f64),
                h = fmt_g(line_height + 0.25),
            )
        })
        .collect();
    let lines_str = lines_svg.join("\n");

    let styles = classes
        .iter()
        .map(|(css, n)| format!(".{unique_id}-r{n} {{ {css} }}"))
        .collect::<Vec<_>>()
        .join("\n");

    let backgrounds = text_backgrounds.concat();
    let matrix = text_group.concat();

    // Python's unbounded `int` from `ceil`, so a huge (finite) aspect ratio
    // prints exactly rather than saturating.
    let terminal_width_local = py_int_plus(terminal_width_float, 0);
    let terminal_width_half = py_int_plus((terminal_width_float / 2.0).floor(), 0);
    let terminal_height_local = (last_y as f64 + 1.0) * line_height + padding_height as f64;

    let mut chrome = format!(
        r#"<rect fill="{bg}" stroke="rgba(255,255,255,0.35)" stroke-width="1" x="{ml}" y="{mt}" width="{tw}" height="{th}" rx="8"/>"#,
        bg = theme.background.hex(),
        ml = MARGIN,
        mt = MARGIN,
        tw = terminal_width_local,
        th = fmt_g(terminal_height_local),
    );
    if !title.is_empty() {
        chrome.push_str(&format!(
            r#"<text class="{unique_id}-title" fill="{fg}" text-anchor="middle" x="{x}" y="{y}">{title}</text>"#,
            fg = theme.foreground.hex(),
            x = terminal_width_half,
            y = MARGIN + CHAR_HEIGHT as i64 + 6,
            title = escape_text(title),
        ));
    }
    chrome.push_str(
        "\n            <g transform=\"translate(26,22)\">\n            <circle cx=\"0\" cy=\"0\" r=\"7\" fill=\"#ff5f57\"/>\n            <circle cx=\"22\" cy=\"0\" r=\"7\" fill=\"#febc2e\"/>\n            <circle cx=\"44\" cy=\"0\" r=\"7\" fill=\"#28c840\"/>\n            </g>\n        ",
    );

    let int = |value: i64| value.to_string();
    crate::export::format_template(
        code_format,
        &[
            ("unique_id", unique_id),
            ("char_width", &fmt_str(char_width)),
            ("char_height", &int(CHAR_HEIGHT as i64)),
            ("line_height", &fmt_str(line_height)),
            ("terminal_width", &fmt_str(char_width * width as f64 - 1.0)),
            (
                "terminal_height",
                &fmt_str((last_y as f64 + 1.0) * line_height - 1.0),
            ),
            (
                "width",
                &py_int_plus(terminal_width_float, margin_width as i64),
            ),
            (
                "height",
                &fmt_str(terminal_height_local + margin_height as f64),
            ),
            ("terminal_x", &int(MARGIN + PADDING_SIDE)),
            ("terminal_y", &int(MARGIN + PADDING_TOP)),
            ("styles", &styles),
            ("chrome", &chrome),
            ("backgrounds", &backgrounds),
            ("matrix", &matrix),
            ("lines", &lines_str),
        ],
    )
}

/// `int(value) + add` as Python prints it, for an integral finite `value`
/// of any magnitude: exact past `i64`, where a cast would saturate.
fn py_int_plus(value: f64, add: i64) -> String {
    if value.abs() < 1e36 {
        return (value as i128 + i128::from(add)).to_string();
    }
    // `{:.0}` prints the float's exact integer value; apply the small `add`
    // to its decimal magnitude (it cannot flip the sign at this size).
    let text = format!("{:.0}", value.abs());
    let mut digits: Vec<u8> = text.bytes().map(|b| b - b'0').collect();
    let (negative, mut carry) = (value < 0.0, i128::from(add));
    if negative {
        carry = -carry;
    }
    for digit in digits.iter_mut().rev() {
        if carry == 0 {
            break;
        }
        let sum = i128::from(*digit) + carry;
        *digit = sum.rem_euclid(10) as u8;
        carry = sum.div_euclid(10);
    }
    let magnitude: String = digits.iter().map(|d| char::from(b'0' + d)).collect();
    if negative {
        format!("-{magnitude}")
    } else {
        magnitude
    }
}

#[cfg(test)]
mod tests {
    use crate::color::ColorSystem;
    use crate::console::Console;

    #[test]
    fn export_svg_matches_upstream() {
        // Byte-parity with real rich 15.0.0 `export_svg(title="X", unique_id="test")`
        // at width 10 (the fixture was captured from it). A fixed unique_id makes
        // the output deterministic — see the module's divergence note (#15).
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(10)
            .no_color(false)
            .build();
        let svg = console.export_svg("X", "test", |c| c.print_str("[bold red]Hi[/] ok"));
        // The fixture is stored with the same LF newlines we emit; normalise in
        // case git checked it out with CRLF on Windows.
        let expected = include_str!("../tests/golden/svg_export.svg").replace("\r\n", "\n");
        assert_eq!(svg, expected);
    }
}
