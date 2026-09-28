//! HTML and SVG export from a frame (#226), with links and regions kept.
//!
//! Both follow core's exporters (`rich::export`, `rich::svg`, ports of
//! upstream's `Console.export_html` and `export_svg`) and use their
//! templates, so a frame without regions exports as core does, with these
//! differences (docs/DIVERGENCES.md, "Frame export"):
//!
//! - links are kept: `<a href>` in HTML, as core writes them, and `<a>`
//!   around the text in SVG, which core drops; the URL is escaped for the
//!   attribute, where core writes it as it is;
//! - an empty segment is not part of a frame, so it neither takes a class
//!   number nor draws a zero-width background;
//! - SVG text is stretched over the cells it covers, where core counts
//!   characters (the two agree unless a character is wide);
//! - SVG can leave out the window frame, and add a cursor and a caption.

use std::fmt::Write as _;

use rich::export::{format_template, ExportFormatError, CONSOLE_HTML_FORMAT};
use rich::protocol::RegionRole;
use rich::svg::CONSOLE_SVG_FORMAT;
use rich::terminal_theme::{TerminalTheme, DEFAULT_TERMINAL_THEME, SVG_EXPORT_THEME};
use rich::Segment;

use super::regions::{role_name, Region};
use super::{Frame, StyleId, NEWLINE};

/// Options for [`Frame::to_html`].
#[derive(Clone, Debug)]
pub struct HtmlOptions<'a> {
    /// Resolves colours, as core's `export_html(theme=…)`. Default: core's
    /// `DEFAULT_TERMINAL_THEME`, as `Console::export_html` uses.
    pub theme: &'a TerminalTheme,
    /// `style="…"` on each span (true, the default, as
    /// `Console::export_html`), or classes and a stylesheet (false).
    pub inline_styles: bool,
    /// Wrap each region's cells in a `<span>` carrying its role (default
    /// true). A frame without regions writes none either way.
    pub regions: bool,
}

impl Default for HtmlOptions<'_> {
    fn default() -> Self {
        HtmlOptions {
            theme: &DEFAULT_TERMINAL_THEME,
            inline_styles: true,
            regions: true,
        }
    }
}

/// Options for [`Frame::to_svg`].
#[derive(Clone, Debug)]
pub struct SvgOptions<'a> {
    /// Default: core's `SVG_EXPORT_THEME`, as `Console::export_svg` uses.
    pub theme: &'a TerminalTheme,
    /// The window title; empty for none.
    pub title: &'a str,
    /// Prefixes every id and class, as core's `unique_id`.
    pub unique_id: &'a str,
    /// The terminal width in cells; `None` is the frame's width.
    pub width: Option<usize>,
    /// Draw the window: rounded frame, title and buttons (default true).
    /// Without it, only the terminal's background, with a small margin.
    pub window: bool,
    /// Draw a block cursor at (column, row).
    pub cursor: Option<(usize, usize)>,
    /// A line of text under the terminal.
    pub caption: Option<&'a str>,
    /// Cell width over height, as core's `font_aspect_ratio` (default 0.61).
    pub font_aspect_ratio: f64,
}

impl Default for SvgOptions<'_> {
    fn default() -> Self {
        SvgOptions {
            theme: &SVG_EXPORT_THEME,
            title: "Rich",
            unique_id: "terminal",
            width: None,
            window: true,
            cursor: None,
            caption: None,
            font_aspect_ratio: 0.61,
        }
    }
}

/// HTML-escape, as Python's `html.escape` (`quote=True`).
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// A stretch of text in one style and, in region mode, one region.
struct Piece {
    text: String,
    style: StyleId,
}

/// Class numbers for style rules, in first-seen order (core's
/// `styles.setdefault`).
#[derive(Default)]
struct Classes(Vec<(String, usize)>);

impl Classes {
    fn number(&mut self, rule: &str) -> usize {
        match self.0.iter().find(|(existing, _)| existing == rule) {
            Some((_, n)) => *n,
            None => {
                let n = self.0.len() + 1;
                self.0.push((rule.to_string(), n));
                n
            }
        }
    }
}

impl Frame {
    /// A self-contained HTML document, with core's template
    /// (`rich::export::CONSOLE_HTML_FORMAT`). See [`Frame::to_html_with`].
    pub fn to_html(&self, options: &HtmlOptions<'_>) -> String {
        self.to_html_with(options, CONSOLE_HTML_FORMAT)
            .expect("the built-in template is valid")
    }

    /// HTML in `code_format`, a Python format string with the fields
    /// `{code}`, `{stylesheet}`, `{foreground}` and `{background}`, as core's
    /// `export_html_with`. `"{code}"` alone gives the markup to embed in a
    /// page of one's own (with inline styles, it needs no stylesheet).
    ///
    /// A linked run becomes `<a href>`. With regions (and
    /// [`HtmlOptions::regions`]), each row's cells of a region are wrapped in
    /// `<span class="rich-region rich-ROLE" data-region="N">`, nested as the
    /// regions are, so the text keeps its place in the grid; the first
    /// wrapper of a region carries its ARIA role and label.
    pub fn to_html_with(
        &self,
        options: &HtmlOptions<'_>,
        code_format: &str,
    ) -> Result<String, ExportFormatError> {
        let theme = options.theme;
        let mut classes = Classes::default();
        let mut code = String::new();
        let regions = if options.regions && self.has_regions() {
            self.regions()
        } else {
            Vec::new()
        };
        if regions.is_empty() {
            for piece in self.pieces() {
                code.push_str(&self.piece_html(&piece, options, &mut classes));
            }
        } else {
            self.region_html(&regions, options, &mut classes, &mut code);
        }
        let stylesheet = classes
            .0
            .iter()
            .filter(|(rule, _)| !rule.is_empty())
            .map(|(rule, number)| format!(".r{number} {{{rule}}}"))
            .collect::<Vec<_>>()
            .join("\n");
        format_template(
            code_format,
            &[
                ("code", &code),
                ("stylesheet", &stylesheet),
                ("foreground", &theme.foreground.hex()),
                ("background", &theme.background.hex()),
            ],
        )
    }

    /// The frame as core's HTML export sees its stream after
    /// `Segment::simplify`: pieces of one style merged, line breaks inside
    /// the piece of the segment they belonged to.
    fn pieces(&self) -> Vec<Piece> {
        let mut pieces: Vec<Piece> = Vec::new();
        let mut push = |text: &str, style: StyleId| match pieces.last_mut() {
            Some(last) if last.style == style => last.text.push_str(text),
            _ => pieces.push(Piece {
                text: text.to_string(),
                style,
            }),
        };
        for row in 0..self.height() {
            let runs = self.row(row);
            for run in runs {
                push(self.run_text(run), run.style);
                if run.flags & NEWLINE != 0 {
                    push("\n", run.style);
                }
            }
            let breaks = row + 1 < self.height() || self.ends_with_newline();
            if breaks && runs.last().is_none_or(|run| run.flags & NEWLINE == 0) {
                push("\n", StyleId::NONE);
            }
        }
        pieces
    }

    /// One piece as core writes a segment: escaped, in a styled `<span>`
    /// (or `<a>`) unless its style is null.
    fn piece_html(
        &self,
        piece: &Piece,
        options: &HtmlOptions<'_>,
        classes: &mut Classes,
    ) -> String {
        let mut text = escape(&piece.text);
        let Some(style) = self.styles.get(piece.style).filter(|s| !s.is_null()) else {
            return text;
        };
        let rule = style.get_html_style(options.theme);
        let link = style.link().map(escape);
        if options.inline_styles {
            if let Some(link) = link {
                text = format!("<a href=\"{link}\">{text}</a>");
            }
            if !rule.is_empty() {
                text = format!("<span style=\"{rule}\">{text}</span>");
            }
            text
        } else {
            let number = classes.number(&rule);
            match link {
                Some(link) => format!("<a class=\"r{number}\" href=\"{link}\">{text}</a>"),
                None => format!("<span class=\"r{number}\">{text}</span>"),
            }
        }
    }

    /// Rows with region wrappers: per row, runs of one style and region are
    /// merged, wrappers open and close around them following the region
    /// tree, and every wrapper closes before the line break.
    fn region_html(
        &self,
        regions: &[Region],
        options: &HtmlOptions<'_>,
        classes: &mut Classes,
        code: &mut String,
    ) {
        let chain = |innermost: Option<usize>| {
            let mut chain = Vec::new();
            let mut at = innermost;
            while let Some(index) = at {
                chain.push(index);
                at = regions[index].parent;
            }
            chain.reverse();
            chain
        };
        let mut started = vec![false; regions.len()];
        for row in 0..self.height() {
            let mut open: Vec<usize> = Vec::new();
            let range = self.row_range(row);
            let mut index = range.start;
            while index < range.end {
                let run = self.runs[index];
                let innermost = self.innermost(index);
                let mut piece = Piece {
                    text: self.run_text(&run).to_string(),
                    style: run.style,
                };
                index += 1;
                while index < range.end
                    && self.runs[index].style == run.style
                    && self.innermost(index) == innermost
                {
                    piece.text.push_str(self.run_text(&self.runs[index]));
                    index += 1;
                }
                if piece.text.is_empty() {
                    continue;
                }
                let wanted = chain(innermost);
                let keep = open.iter().zip(&wanted).take_while(|(a, b)| a == b).count();
                for _ in keep..open.len() {
                    code.push_str("</span>");
                }
                open.truncate(keep);
                for &region in &wanted[keep..] {
                    code.push_str(&open_tag(&regions[region], region, !started[region]));
                    started[region] = true;
                    open.push(region);
                }
                code.push_str(&self.piece_html(&piece, options, classes));
            }
            for _ in 0..open.len() {
                code.push_str("</span>");
            }
            if row + 1 < self.height() || self.ends_with_newline() {
                code.push('\n');
            }
        }
    }

    /// A self-contained SVG image of a terminal, with core's template
    /// (`rich::svg::CONSOLE_SVG_FORMAT`). Linked text is wrapped in `<a>`.
    pub fn to_svg(&self, options: &SvgOptions<'_>) -> String {
        let theme = options.theme;
        let unique_id = options.unique_id;
        let width = options.width.unwrap_or_else(|| self.width());
        let char_height = CHAR_HEIGHT;
        let char_width = char_height * options.font_aspect_ratio;
        let line_height = char_height * 1.22;
        let padding_top = if options.window {
            PADDING_TOP
        } else {
            PADDING_SIDE
        };
        let caption_height = if options.caption.is_some() {
            CAPTION
        } else {
            0
        };
        let padding_width = PADDING_SIDE + PADDING_SIDE;
        let padding_height = padding_top + PADDING_SIDE + caption_height;

        let mut classes = Classes::default();
        let mut backgrounds = String::new();
        let mut matrix = String::new();
        let height = self.height().max(1);
        for y in 0..self.height() {
            let line: Vec<Segment> = self
                .row(y)
                .iter()
                .filter(|run| run.len > 0)
                .map(|run| Segment::new(self.run_text(run), self.styles.get(run.style).cloned()))
                .collect();
            let mut line = Segment::adjust_line_length(&line, width, None);
            // Core keeps each line's break as a segment of its own, and so
            // writes it as text past the right edge (clipped away).
            if y + 1 < self.height() || self.ends_with_newline() {
                line.push(Segment::line());
            }
            let mut x = 0usize;
            for segment in line {
                let style = segment.style.clone().unwrap_or_default();
                let class = classes.number(&style.get_svg_style(theme));
                let (has_background, background) = if style.attr(6) == Some(true) {
                    let hex = style
                        .color()
                        .map_or(theme.foreground, |c| theme.resolve(c, true))
                        .hex();
                    (true, hex)
                } else {
                    let has = style.bgcolor().is_some_and(|c| !c.is_default());
                    let hex = style
                        .bgcolor()
                        .map_or(theme.background, |c| theme.resolve(c, false))
                        .hex();
                    (has, hex)
                };
                let cells = rich::cells::cell_len(&segment.text);
                if has_background {
                    let _ = write!(
                        backgrounds,
                        r#"<rect fill="{background}" x="{}" y="{}" width="{}" height="{}" shape-rendering="crispEdges"/>"#,
                        fmt_g(x as f64 * char_width),
                        fmt_g(y as f64 * line_height + 1.5),
                        fmt_g(char_width * cells as f64),
                        fmt_g(line_height + 0.25),
                    );
                }
                if !segment.text.chars().all(|c| c == ' ') {
                    // Stretched over its cells; a line break, as core has it,
                    // over one.
                    let length = if segment.text == "\n" { 1 } else { cells };
                    let text = format!(
                        r#"<text class="{unique_id}-r{class}" x="{}" y="{}" textLength="{}" clip-path="url(#{unique_id}-line-{y})">{}</text>"#,
                        fmt_g(x as f64 * char_width),
                        fmt_g(y as f64 * line_height + char_height),
                        fmt_g(char_width * length as f64),
                        escape_text(&segment.text),
                    );
                    match style.link() {
                        Some(link) => {
                            let _ = write!(matrix, r#"<a href="{}">{text}</a>"#, escape(link));
                        }
                        None => matrix.push_str(&text),
                    }
                }
                x += cells;
            }
        }
        if let Some((column, row)) = options.cursor {
            let _ = write!(
                matrix,
                r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" opacity="0.7"/>"#,
                fmt_g(column as f64 * char_width),
                fmt_g(row as f64 * line_height + 1.5),
                fmt_g(char_width),
                fmt_g(line_height + 0.25),
                theme.foreground.hex()
            );
        }
        let last_y = height - 1;
        let lines = (0..last_y)
            .map(|line_no| {
                format!(
                    "<clipPath id=\"{unique_id}-line-{line_no}\">\n    <rect x=\"0\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n            </clipPath>",
                    fmt_g(line_no as f64 * line_height + 1.5),
                    fmt_g(char_width * width as f64),
                    fmt_g(line_height + 0.25),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let styles = classes
            .0
            .iter()
            .map(|(css, n)| format!(".{unique_id}-r{n} {{ {css} }}"))
            .collect::<Vec<_>>()
            .join("\n");
        let terminal_width = (width as f64 * char_width + padding_width as f64).ceil();
        let terminal_height = (last_y as f64 + 1.0) * line_height + padding_height as f64;
        let mut chrome = if options.window {
            format!(
                r#"<rect fill="{}" stroke="rgba(255,255,255,0.35)" stroke-width="1" x="{MARGIN}" y="{MARGIN}" width="{}" height="{}" rx="8"/>"#,
                theme.background.hex(),
                terminal_width as i64,
                fmt_g(terminal_height),
            )
        } else {
            format!(
                r#"<rect fill="{}" x="{MARGIN}" y="{MARGIN}" width="{}" height="{}"/>"#,
                theme.background.hex(),
                terminal_width as i64,
                fmt_g(terminal_height),
            )
        };
        if options.window {
            if !options.title.is_empty() {
                let _ = write!(
                    chrome,
                    r#"<text class="{unique_id}-title" fill="{}" text-anchor="middle" x="{}" y="{}">{}</text>"#,
                    theme.foreground.hex(),
                    (terminal_width / 2.0).floor() as i64,
                    MARGIN + CHAR_HEIGHT as i64 + 6,
                    escape_text(options.title),
                );
            }
            chrome.push_str(
                "\n            <g transform=\"translate(26,22)\">\n            <circle cx=\"0\" cy=\"0\" r=\"7\" fill=\"#ff5f57\"/>\n            <circle cx=\"22\" cy=\"0\" r=\"7\" fill=\"#febc2e\"/>\n            <circle cx=\"44\" cy=\"0\" r=\"7\" fill=\"#28c840\"/>\n            </g>\n        ",
            );
        }
        if let Some(caption) = options.caption {
            let _ = write!(
                chrome,
                r#"<text class="{unique_id}-title" fill="{}" text-anchor="middle" x="{}" y="{}" font-weight="normal">{}</text>"#,
                theme.foreground.hex(),
                (terminal_width / 2.0).floor() as i64,
                fmt_g(MARGIN as f64 + terminal_height - PADDING_SIDE as f64 - 10.0),
                escape_text(caption),
            );
        }
        let int = |value: i64| value.to_string();
        format_template(
            CONSOLE_SVG_FORMAT,
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
                ("width", &py_int_plus(terminal_width, MARGIN * 2)),
                ("height", &fmt_str(terminal_height + (MARGIN * 2) as f64)),
                ("terminal_x", &int(MARGIN + PADDING_SIDE)),
                ("terminal_y", &int(MARGIN + padding_top)),
                ("styles", &styles),
                ("chrome", &chrome),
                ("backgrounds", &backgrounds),
                ("matrix", &matrix),
                ("lines", &lines),
            ],
        )
        .expect("the built-in template is valid")
    }
}

/// A region wrapper's opening tag. The first wrapper of a region carries its
/// ARIA role and label; later ones (its cells on other rows) only its number.
fn open_tag(region: &Region, index: usize, first: bool) -> String {
    let mut tag = format!(
        "<span class=\"rich-region rich-{}\" data-region=\"{index}\"",
        escape(&role_name(&region.role))
    );
    if first {
        let label = region.label.as_deref().map(escape);
        let aria_label = |tag: &mut String| {
            if let Some(label) = &label {
                let _ = write!(tag, " aria-label=\"{label}\"");
            }
        };
        match &region.role {
            RegionRole::Panel => {
                tag.push_str(" role=\"group\"");
                aria_label(&mut tag);
            }
            RegionRole::Table => {
                tag.push_str(" role=\"group\" aria-roledescription=\"table\"");
                aria_label(&mut tag);
            }
            RegionRole::TableHeader { column } => {
                let _ = write!(
                    tag,
                    " role=\"group\" aria-roledescription=\"column header\" data-column=\"{column}\""
                );
            }
            RegionRole::TableFooter { column } => {
                let _ = write!(
                    tag,
                    " role=\"group\" aria-roledescription=\"column footer\" data-column=\"{column}\""
                );
            }
            RegionRole::TableCell { row, column } => {
                let _ = write!(
                    tag,
                    " role=\"group\" aria-roledescription=\"cell\" data-row=\"{row}\" data-column=\"{column}\""
                );
            }
            RegionRole::Rule => {
                tag.push_str(" role=\"separator\"");
                aria_label(&mut tag);
            }
            RegionRole::Heading { level } => {
                let _ = write!(tag, " role=\"heading\" aria-level=\"{level}\"");
                aria_label(&mut tag);
            }
            RegionRole::Code => {
                tag.push_str(" role=\"code\"");
                if let Some(label) = &label {
                    let _ = write!(tag, " data-language=\"{label}\"");
                }
            }
            _ => aria_label(&mut tag),
        }
    }
    tag.push('>');
    tag
}

const CHAR_HEIGHT: f64 = 20.0;
const MARGIN: i64 = 1;
const PADDING_TOP: i64 = 40;
const PADDING_SIDE: i64 = 8;
/// The height of the caption line under the terminal.
const CAPTION: i64 = 32;

/// HTML-escape, then spaces as `&#160;`: core's SVG `escape_text`.
fn escape_text(text: &str) -> String {
    escape(text).replace(' ', "&#160;")
}

// The number formatting below is core's (`rich::svg`, private there), so the
// SVG's numbers print as upstream's Python does.

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
/// integer-valued floats, scientific below `1e-4` or from `1e16`.
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

fn py_non_finite(v: f64) -> String {
    if v.is_nan() {
        "nan".to_string()
    } else if v > 0.0 {
        "inf".to_string()
    } else {
        "-inf".to_string()
    }
}

fn split_exp(sci: &str) -> (&str, i32) {
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci, "0"));
    (mantissa, exp.parse().unwrap_or(0))
}

fn py_exponent(mantissa: &str, exp: i32) -> String {
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exp.abs())
}

fn strip_fraction_zeros(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// `int(value) + add`, for the integral, finite widths an SVG has.
fn py_int_plus(value: f64, add: i64) -> String {
    (value as i128 + i128::from(add)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::render_frame;
    use rich::markdown::Markdown;
    use rich::{ColorSystem, Console, Panel, Renderable, Table, Text};

    fn console(width: usize) -> Console {
        Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build()
    }

    fn segments(console: &Console, renderable: &dyn Renderable) -> Vec<Segment> {
        let mut segments = renderable.rich_render(console, &console.options());
        segments.push(Segment::line());
        segments
    }

    #[test]
    fn html_matches_core_without_regions() {
        let console = console(30);
        let mut table = Table::new();
        table.add_column("Name");
        table.add_column("Size");
        table.add_row(&["[bold red]a[/]", "1"]);
        table.add_row(&["b", "[on blue]22[/]"]);
        let panel = Panel::new(Box::new(Text::from_markup("[italic]hi[/] there").unwrap()));
        for renderable in [&table as &dyn Renderable, &panel] {
            let segments = segments(&console, renderable);
            let frame = Frame::from_segments(&segments);
            let theme = &DEFAULT_TERMINAL_THEME;
            assert_eq!(
                frame.to_html(&HtmlOptions::default()),
                rich::export::export_html_inline(&segments, theme)
            );
            let classes = HtmlOptions {
                inline_styles: false,
                ..HtmlOptions::default()
            };
            assert_eq!(
                frame.to_html(&classes),
                rich::export::export_html_classes(&segments, theme)
            );
        }
    }

    #[test]
    fn svg_matches_core_without_regions() {
        let wide = console(24);
        let mut table = Table::new().title("T");
        table.add_column("Name");
        table.add_row(&["[bold red on green]x[/] y"]);
        let drawn = segments(&wide, &table);
        let frame = Frame::from_segments(&drawn);
        let options = SvgOptions {
            title: "X",
            unique_id: "test",
            width: Some(24),
            ..SvgOptions::default()
        };
        assert_eq!(
            frame.to_svg(&options),
            rich::svg::export_svg(&drawn, &SVG_EXPORT_THEME, "X", "test", 24)
        );
        // The fixture captured from upstream, through a frame. It lives in
        // the core crate, so a packaged crate skips this.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../rich/tests/golden/svg_export.svg"
        );
        let Ok(expected) = std::fs::read_to_string(path) else {
            return;
        };
        let expected = expected.replace("\r\n", "\n");
        let small = console(10);
        let hi = segments(&small, &Text::from_markup("[bold red]Hi[/] ok").unwrap());
        let frame = Frame::from_segments(&hi);
        let options = SvgOptions {
            title: "X",
            unique_id: "test",
            width: Some(10),
            ..SvgOptions::default()
        };
        assert_eq!(frame.to_svg(&options), expected);
    }

    #[test]
    fn links_survive_in_html_and_svg() {
        let console = console(30);
        let text = Text::from_markup("see [link=https://x.test/?a=1&b=2]docs[/link] now").unwrap();
        let frame = Frame::from_segments(&segments(&console, &text));
        let html = frame.to_html(&HtmlOptions::default());
        assert!(
            html.contains(r#"<a href="https://x.test/?a=1&amp;b=2">docs</a>"#),
            "{html}"
        );
        let svg = frame.to_svg(&SvgOptions::default());
        assert!(
            svg.contains(r#"<a href="https://x.test/?a=1&amp;b=2"><text"#),
            "{svg}"
        );
        let regions = frame.regions();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].role, RegionRole::Link);
        assert_eq!(regions[0].spans[0].columns, 4..8);
    }

    #[test]
    fn regions_become_nested_wrappers() {
        let console = console(40);
        let mut table = Table::new().title("Crates");
        table.add_column("Name");
        table.add_row(&["rich"]);
        let panel = Panel::new(Box::new(table)).title("Box");
        let frame = render_frame(&console, &console.options(), &panel);
        // Without regions the bytes are the plain frame's.
        let plain = Frame::from_segments(&panel.rich_render(&console, &console.options()));
        assert_eq!(frame.to_ansi(&console), plain.to_ansi(&console));
        let off = HtmlOptions {
            regions: false,
            ..HtmlOptions::default()
        };
        assert_eq!(frame.to_html(&off), plain.to_html(&off));
        let html = frame.to_html(&HtmlOptions::default());
        assert!(
            html.contains(r#"<span class="rich-region rich-panel" data-region="0" role="group" aria-label="Box">"#),
            "{html}"
        );
        assert!(html.contains(r#"aria-roledescription="table" aria-label="Crates""#));
        assert!(html.contains(r#"aria-roledescription="cell" data-row="0" data-column="0""#));
        // Wrappers open and close on each row, so the grid is intact.
        let code = html.split("<code").nth(1).unwrap();
        for line in code.lines().take(frame.height()) {
            assert_eq!(
                line.matches("<span class=\"rich-region").count(),
                line.matches("</span>").count() - line.matches("<span style").count(),
                "{line}"
            );
        }
        let text: String = frame.plain();
        let stripped = strip_tags(code.split_once('>').unwrap().1);
        assert!(stripped.starts_with(text.trim_end()), "{stripped}");
    }

    #[test]
    fn headings_and_code_get_roles() {
        let console = console(40);
        let markdown = Markdown::new("## Install\n\n```sh\ncargo add rs-rich\n```\n");
        let frame = render_frame(&console, &console.options(), &markdown);
        let html = frame.to_html(&HtmlOptions::default());
        assert!(
            html.contains(r#"role="heading" aria-level="2" aria-label="Install""#),
            "{html}"
        );
        assert!(html.contains(r#"role="code" data-language="sh""#), "{html}");
    }

    #[test]
    fn svg_without_window_with_cursor_and_caption() {
        let console = console(10);
        let frame = Frame::from_segments(&segments(&console, &Text::new("ab")));
        let options = SvgOptions {
            window: false,
            cursor: Some((2, 0)),
            caption: Some("A <caption>"),
            ..SvgOptions::default()
        };
        let svg = frame.to_svg(&options);
        assert!(!svg.contains("#ff5f57"));
        assert!(svg.contains("A&#160;&lt;caption&gt;"));
        assert!(svg.contains(r#"opacity="0.7""#));
        assert!(svg.contains("translate(9, 9)"));
    }

    fn strip_tags(html: &str) -> String {
        let mut out = String::new();
        let mut inside = false;
        for c in html.chars() {
            match c {
                '<' => inside = true,
                '>' => inside = false,
                c if !inside => out.push(c),
                _ => {}
            }
        }
        out.replace("&#160;", " ")
    }
}
