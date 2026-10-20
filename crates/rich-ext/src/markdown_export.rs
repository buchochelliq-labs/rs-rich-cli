//! GitHub-flavoured Markdown from renderables, for pull request comments,
//! issue bodies and job summaries.
//!
//! A terminal table pasted into a PR comment is a block of box-drawing
//! characters; this module turns renderables into the Markdown GitHub
//! renders itself:
//!
//! | renderable | Markdown |
//! |---|---|
//! | [`Table`] | a GFM table: a header row, an alignment row from each column's justify, `\|` escaped |
//! | [`Tree`] | nested lists |
//! | [`Panel`] | a blockquote with the title in bold, or `<details><summary>` ([`PanelStyle`]) |
//! | [`Rule`] | `---`, its title after it in bold |
//! | [`Text`] and markup | inline Markdown: bold `**`, italic `*`, strike `~~`, code `` ` ``, links `[text](url)` |
//! | `Syntax` (feature `syntax`) | a fenced code block with its language |
//! | `Markdown` (feature `markdown`) and Markdown source | the source as it is ([`GfmDocument::push_markdown`] for a string) |
//! | anything else | its plain rendering at [`GfmOptions::width`] in a ```` ```text ```` fence |
//!
//! Text styles map where Markdown has an equivalent: bold, italic, strike and
//! links. Colour, dim, underline and reverse have none and are dropped. Code
//! comes from style *names* ([`GfmOptions::code_styles`], `code` and
//! `markdown.code` by default), in [`Text`], markup, table cells, panel text
//! and tree labels.
//!
//! # How content is read
//!
//! Through the core's read-only accessors, which mirror upstream's public
//! attributes:
//!
//! - **Text**, **Tree**: `plain`, `spans`, `label`, `children`.
//! - **Table**: `columns` (header, footer, justify, style), `rows` (cells,
//!   style), `get_title`, `get_caption`, `get_show_header`,
//!   `get_show_footer`, `get_footer_style` and `get_row_style`, so every
//!   table is written exactly, a grid or a cell of several lines included.
//! - **Panel**: `get_title`, `get_subtitle` and `renderable`; a body that is
//!   not text is rendered wide and read back (a drawing is fenced).
//! - **Rule**: `get_title`. **Markdown**: `markup`, its source.
//! - **Syntax**: a copy of it is highlighted with a highlighter that records
//!   the code and language it is given (`Syntax::highlighter` and
//!   `Syntax::highlight`), so line ranges and themes do not matter.
//! - **Columns** expose nothing of their items and are rendered plain.
//!
//! # Example
//!
//! ```
//! use rich::{Justify, Rule, Table, Tree};
//! use rich_ext::markdown_export::{GfmDocument, fit_github_comment};
//!
//! let mut table = Table::new();
//! table.add_column("check");
//! table.add_column_justify("ms", Justify::Right);
//! table.add_row(&["[bold]lint[/]", "1200"]);
//! table.add_row(&["a|b", "35"]);
//!
//! let mut tree = Tree::new("crates");
//! tree.add("rich").add("table.rs");
//!
//! let mut doc = GfmDocument::new();
//! doc.push_heading(2, "Report");
//! doc.push(&table);
//! doc.push(&Rule::line());
//! doc.push(&tree);
//! let markdown = fit_github_comment(&doc.finish());
//! assert_eq!(
//!     markdown,
//!     "## Report\n\n\
//!      | check | ms |\n\
//!      | --- | ---: |\n\
//!      | **lint** | 1200 |\n\
//!      | a\\|b | 35 |\n\n\
//!      ---\n\n\
//!      - crates\n  - rich\n    - table.rs\n"
//! );
//! ```

use rich::containers::Renderables;
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::{Style, StyleType};
use rich::table::Cell;
use rich::{Columns, Console, Justify, Panel, Rule, Table, Text, Tree};

/// The most characters a GitHub issue or pull request comment may hold.
pub const GITHUB_COMMENT_LIMIT: usize = 65_536;

/// How a [`Panel`] is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelStyle {
    /// A blockquote, the title in bold on its first line (the default).
    #[default]
    Blockquote,
    /// A collapsible `<details>` block with the title as its summary.
    Details {
        /// Whether the block starts open.
        open: bool,
    },
}

/// Options for [`GfmExporter`] and [`GfmDocument`].
#[derive(Clone, Debug)]
pub struct GfmOptions {
    width: usize,
    panel: PanelStyle,
    code_styles: Vec<String>,
    summary: String,
}

impl Default for GfmOptions {
    fn default() -> Self {
        GfmOptions {
            width: 100,
            panel: PanelStyle::Blockquote,
            code_styles: vec!["code".into(), "markdown.code".into(), "inline_code".into()],
            summary: "Details".into(),
        }
    }
}

impl GfmOptions {
    /// The default options: plain renderings 100 cells wide, panels as
    /// blockquotes.
    pub fn new() -> Self {
        Self::default()
    }

    /// The width plain renderings (the ```` ```text ```` fallback) are made at.
    pub fn width(mut self, width: usize) -> Self {
        self.width = width.max(1);
        self
    }

    /// How panels are written.
    pub fn panel(mut self, panel: PanelStyle) -> Self {
        self.panel = panel;
        self
    }

    /// Style names written as inline code (`code`, `markdown.code` and
    /// `inline_code` by default). A span whose style is one of these names, or
    /// a style definition with one as a word, becomes `` `code` ``.
    pub fn code_styles<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.code_styles = names.into_iter().map(Into::into).collect();
        self
    }

    /// The summary of a [`PanelStyle::Details`] block for a panel with no
    /// title (`"Details"` by default).
    pub fn untitled_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }
}

/// A value that can be written as GitHub-flavoured Markdown.
///
/// Implemented for the core renderables this module reads (see the
/// [module documentation](self)); [`Plain`] wraps any other renderable.
pub trait ToGfm {
    /// The Markdown block for `self`, without a trailing newline.
    fn to_gfm(&self, exporter: &GfmExporter) -> String;
}

/// Writes renderables as GitHub-flavoured Markdown blocks.
///
/// ```
/// use rich::Text;
/// use rich_ext::markdown_export::GfmExporter;
///
/// let gfm = GfmExporter::new();
/// assert_eq!(gfm.markup("[bold]ok[/] and [link=https://example.com]docs[/]"),
///            "**ok** and [docs](https://example.com)");
/// assert_eq!(gfm.export(&Text::new("2 * 3")), "2 \\* 3");
/// ```
pub struct GfmExporter {
    console: Console,
    options: GfmOptions,
}

impl Default for GfmExporter {
    fn default() -> Self {
        Self::new()
    }
}

/// A panel body that is read back from its rendering is rendered this wide,
/// so that nothing wraps that would not wrap anyway.
const WIDE: usize = 2000;

impl GfmExporter {
    /// An exporter with the default [`GfmOptions`].
    pub fn new() -> Self {
        Self::with_options(GfmOptions::default())
    }

    /// An exporter with `options`.
    pub fn with_options(options: GfmOptions) -> Self {
        let console = Console::builder()
            .width(options.width)
            .color_system(None)
            .highlight(false)
            .build();
        GfmExporter { console, options }
    }

    /// The options in use.
    pub fn options(&self) -> &GfmOptions {
        &self.options
    }

    /// The console renderings are made with: no colour, no highlighting,
    /// [`GfmOptions::width`] wide, the default theme.
    pub fn console(&self) -> &Console {
        &self.console
    }

    /// `value` as a Markdown block.
    pub fn export(&self, value: &(impl ToGfm + ?Sized)) -> String {
        value.to_gfm(self)
    }

    /// Console markup as inline Markdown. Malformed markup is taken literally.
    pub fn markup(&self, markup: &str) -> String {
        self.text(&self.markup_text(markup))
    }

    /// A [`Text`] as Markdown paragraphs: styles become inline Markdown, line
    /// breaks hard breaks and blank lines paragraph breaks.
    pub fn text(&self, text: &Text) -> String {
        paragraphs(&self.text_chars(text, None))
    }

    /// A fenced code block. `language` is its info string (empty for none).
    pub fn code(&self, code: &str, language: &str) -> String {
        fenced(code.trim_end_matches('\n'), language, "")
    }

    /// `renderable`'s plain rendering at [`GfmOptions::width`], in a
    /// ```` ```text ```` fence.
    pub fn plain(&self, renderable: &dyn Renderable) -> String {
        let rendered = self.console.render_export(renderable);
        let lines: Vec<&str> = rendered.lines().map(str::trim_end).collect();
        fenced(lines.join("\n").trim_end_matches('\n'), "text", "")
    }

    fn markup_text(&self, markup: &str) -> Text {
        self.console.render_str(markup, Some(false))
    }

    fn is_code_name(&self, name: &str) -> bool {
        self.options
            .code_styles
            .iter()
            .any(|code| name == code || name.split_whitespace().any(|word| word == code))
    }

    /// The characters of `text` with their inline flags, over `base`.
    fn text_chars(&self, text: &Text, base: Option<&StyleType>) -> Vec<StyledChar> {
        let plain = text.plain();
        let mut layers: Vec<(usize, usize, &StyleType)> = Vec::new();
        if let Some(base) = base {
            layers.push((0, plain.len(), base));
        }
        layers.push((0, plain.len(), text.base_style()));
        for span in text.spans() {
            layers.push((span.start, span.end, &span.style));
        }
        let resolved: Vec<(usize, usize, Style, bool)> = layers
            .into_iter()
            .map(|(start, end, style)| {
                let code = matches!(style, StyleType::Name(name) if self.is_code_name(name));
                // A code style's own look (colours, bold) is the code span.
                let style = if code {
                    Style::new()
                } else {
                    self.console.get_style(style).unwrap_or_default()
                };
                (start, end, style, code)
            })
            .collect();
        plain
            .char_indices()
            .map(|(at, ch)| {
                let mut style = Style::new();
                let mut code = false;
                for (start, end, layer, is_code) in &resolved {
                    if *start <= at && at < *end {
                        style = style.combine(layer);
                        code |= *is_code;
                    }
                }
                StyledChar {
                    ch,
                    flags: Flags::from_style(&style, code),
                }
            })
            .collect()
    }

    fn cell_chars(&self, cell: &Cell, base: Option<&StyleType>) -> Option<Vec<StyledChar>> {
        match cell {
            Cell::Text(text) => Some(self.text_chars(text, base)),
            Cell::Markup(markup) => Some(self.text_chars(&self.markup_text(markup), base)),
            Cell::Renderable(renderable) => renderable
                .printed_text()
                .map(|text| self.text_chars(&text, base)),
        }
    }

    /// Render `renderable` wide and return its lines as styled cells.
    fn wide_lines(&self, renderable: &dyn Renderable) -> Vec<Vec<CellChar>> {
        let options = self.console.options().update_width(WIDE);
        self.console
            .render_lines(renderable, &options, false)
            .iter()
            .map(|line| cell_chars(line))
            .collect()
    }
}

/// Inline Markdown flags of one character.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Flags {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: Option<String>,
}

impl Flags {
    fn from_style(style: &Style, code: bool) -> Self {
        Flags {
            bold: style.attr(0) == Some(true),
            italic: style.attr(2) == Some(true),
            strike: style.attr(8) == Some(true),
            code,
            link: style.link().map(str::to_string),
        }
    }

    fn emphasis(&self) -> [bool; 3] {
        [self.strike, self.bold, self.italic]
    }
}

#[derive(Clone, Debug)]
struct StyledChar {
    ch: char,
    flags: Flags,
}

/// A character of a rendered line, with its style.
#[derive(Clone, Debug)]
struct CellChar {
    ch: char,
    style: Style,
}

fn cell_chars(line: &[Segment]) -> Vec<CellChar> {
    line.iter()
        .filter(|segment| !segment.control)
        .flat_map(|segment| {
            let style = segment.style.clone().unwrap_or_default();
            segment.text.chars().map(move |ch| CellChar {
                ch,
                style: style.clone(),
            })
        })
        .collect()
}

fn styled(chars: &[CellChar]) -> Vec<StyledChar> {
    chars
        .iter()
        .map(|c| StyledChar {
            ch: c.ch,
            flags: Flags::from_style(&c.style, false),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Inline Markdown
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Paragraph,
    TableCell,
}

const MARKERS: [&str; 3] = ["~~", "**", "*"];

/// One line (no `\n`) of styled characters as inline Markdown.
fn inline(chars: &[StyledChar], context: Context) -> String {
    let escape_at = match context {
        Context::Paragraph => line_start_escape(chars),
        Context::TableCell => None,
    };
    let mut out = String::new();
    let mut start = 0;
    while start < chars.len() {
        let link = &chars[start].flags.link;
        let mut end = start + 1;
        while end < chars.len() && chars[end].flags.link == *link {
            end += 1;
        }
        let body = emphasis(&chars[start..end], start, escape_at, context);
        match link {
            Some(url) if !body.trim().is_empty() => {
                out.push('[');
                out.push_str(&body);
                out.push_str("](");
                out.push_str(&escape_url(url));
                out.push(')');
            }
            _ => out.push_str(&body),
        }
        start = end;
    }
    out
}

/// The index of a character to escape because the line would otherwise
/// start a block: a heading (`# `), a list item (`- `, `+ `, `1. `), a
/// thematic break or a setext underline (`---`, `===`).
fn line_start_escape(chars: &[StyledChar]) -> Option<usize> {
    let first = chars.iter().position(|c| !c.ch.is_whitespace())?;
    if chars[first].flags.code {
        return None;
    }
    let rest: String = chars[first..].iter().map(|c| c.ch).collect();
    let rest = rest.trim_end();
    let spaced = |at: usize| rest[at..].chars().next().is_none_or(char::is_whitespace);
    let hashes = rest.chars().take_while(|ch| *ch == '#').count();
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let lone = |mark: char| rest.chars().all(|ch| ch == mark || ch == ' ');
    match rest.chars().next()? {
        '#' if hashes <= 6 && spaced(hashes) => Some(first),
        '-' | '+' if spaced(1) => Some(first),
        '-' | '=' if lone(rest.chars().next()?) => Some(first),
        '0'..='9'
            if digits <= 9
                && matches!(rest[digits..].chars().next(), Some('.' | ')'))
                && spaced(digits + 1) =>
        {
            Some(first + digits)
        }
        _ => None,
    }
}

fn emphasis(
    chars: &[StyledChar],
    offset: usize,
    escape_at: Option<usize>,
    context: Context,
) -> String {
    let mut out = String::new();
    let mut open: Vec<usize> = Vec::new();
    let mut pending = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = &chars[i];
        if c.flags.code {
            let mut end = i;
            while end < chars.len() && chars[end].flags.code {
                end += 1;
            }
            let code: String = chars[i..end].iter().map(|c| c.ch).collect();
            if code.trim().is_empty() {
                pending.push_str(&code);
            } else {
                transition(&mut out, &mut open, c.flags.emphasis(), &mut pending);
                out.push_str(&code_span(&code, context));
            }
            i = end;
            continue;
        }
        if c.ch.is_whitespace() {
            pending.push(if c.ch == '\t' { ' ' } else { c.ch });
            i += 1;
            continue;
        }
        transition(&mut out, &mut open, c.flags.emphasis(), &mut pending);
        if escape_at == Some(offset + i) {
            out.push('\\');
        }
        escape_char(&mut out, c.ch);
        i += 1;
    }
    transition(&mut out, &mut open, [false; 3], &mut String::new());
    out.push_str(&pending);
    out
}

/// Close the open markers `wanted` does not keep, then write the pending
/// whitespace, then open what `wanted` adds, so markers hug the text.
fn transition(out: &mut String, open: &mut Vec<usize>, wanted: [bool; 3], pending: &mut String) {
    let keep = open.iter().take_while(|&&marker| wanted[marker]).count();
    for &marker in open[keep..].iter().rev() {
        out.push_str(MARKERS[marker]);
    }
    open.truncate(keep);
    out.push_str(pending);
    pending.clear();
    for (marker, &on) in wanted.iter().enumerate() {
        if on && !open.contains(&marker) {
            out.push_str(MARKERS[marker]);
            open.push(marker);
        }
    }
}

fn escape_char(out: &mut String, ch: char) {
    if matches!(
        ch,
        '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '~' | '|' | '&'
    ) {
        out.push('\\');
    }
    out.push(ch);
}

fn code_span(code: &str, context: Context) -> String {
    let code = if context == Context::TableCell {
        code.replace('|', "\\|")
    } else {
        code.to_string()
    };
    let ticks = "`".repeat(longest_run(&code, '`') + 1);
    let pad = if code.starts_with('`') || code.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{ticks}{pad}{code}{pad}{ticks}")
}

fn longest_run(text: &str, ch: char) -> usize {
    let (mut best, mut run) = (0, 0);
    for c in text.chars() {
        run = if c == ch { run + 1 } else { 0 };
        best = best.max(run);
    }
    best
}

fn escape_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for ch in url.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '|' => out.push_str("%7C"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Styled characters (with `\n`) as Markdown paragraphs.
fn paragraphs(chars: &[StyledChar]) -> String {
    let lines: Vec<String> = chars
        .split(|c| c.ch == '\n')
        .map(|line| {
            let line = trim_end(line);
            // Leading spaces would make an indented code block: keep the
            // indent with no-break spaces.
            let indent = line.iter().take_while(|c| c.ch == ' ').count();
            let mut out = "\u{a0}".repeat(indent);
            out.push_str(&inline(&line[indent..], Context::Paragraph));
            out
        })
        .collect();
    join_paragraph_lines(&lines)
}

fn trim_end(line: &[StyledChar]) -> &[StyledChar] {
    let end = line
        .iter()
        .rposition(|c| !c.ch.is_whitespace())
        .map_or(0, |at| at + 1);
    &line[..end]
}

fn trim(line: &[StyledChar]) -> &[StyledChar] {
    let line = trim_end(line);
    let start = line
        .iter()
        .position(|c| !c.ch.is_whitespace())
        .unwrap_or(line.len());
    &line[start..]
}

/// Lines of one block: consecutive lines joined by hard breaks, blank lines
/// separating paragraphs.
fn join_paragraph_lines(lines: &[String]) -> String {
    let mut out = String::new();
    let mut previous_blank = true;
    for line in lines {
        if line.is_empty() {
            if !previous_blank {
                out.push_str("\n\n");
            }
            previous_blank = true;
            continue;
        }
        if !previous_blank {
            out.push_str("\\\n");
        }
        out.push_str(line);
        previous_blank = false;
    }
    out.trim_end().to_string()
}

/// A fenced block of `body`, each line prefixed with `prefix` (`"> "` inside
/// a blockquote).
fn fenced(body: &str, language: &str, prefix: &str) -> String {
    let ticks = "`".repeat(longest_run(body, '`').max(2) + 1);
    let mut out = format!("{prefix}{ticks}{language}\n");
    for line in body.split('\n') {
        out.push_str(prefix);
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(prefix);
    out.push_str(&ticks);
    out
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

impl ToGfm for Table {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        table_gfm(self, exporter).unwrap_or_else(|| exporter.plain(self))
    }
}

/// `chars` with only their links kept: a header or caption is styled as a
/// whole by Markdown (bold, italic), so its own emphasis is dropped.
fn links_only(chars: Vec<StyledChar>) -> Vec<StyledChar> {
    chars
        .into_iter()
        .map(|c| StyledChar {
            ch: c.ch,
            flags: Flags {
                link: c.flags.link,
                ..Flags::default()
            },
        })
        .collect()
}

/// A cell's lines as one table cell, joined with `<br>`.
fn table_cell(chars: &[StyledChar]) -> String {
    let parts: Vec<String> = chars
        .split(|c| c.ch == '\n')
        .map(|line| inline(trim(line), Context::TableCell))
        .collect();
    let mut parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    while parts.last().is_some_and(|part| part.is_empty()) {
        parts.pop();
    }
    while parts.first().is_some_and(|part| part.is_empty()) {
        parts.remove(0);
    }
    parts.join("<br>")
}

impl GfmExporter {
    /// A cell's characters over `base`; a renderable with no text of its own
    /// is its plain rendering at [`GfmOptions::width`].
    fn any_cell_chars(&self, cell: &Cell, base: Option<&StyleType>) -> Vec<StyledChar> {
        self.cell_chars(cell, base).unwrap_or_else(|| match cell {
            Cell::Renderable(renderable) => self
                .console
                .render_export(renderable.as_ref())
                .chars()
                .map(|ch| StyledChar {
                    ch,
                    flags: Flags::default(),
                })
                .collect(),
            _ => Vec::new(),
        })
    }

    /// A title, caption or label as one line of inline Markdown.
    fn label(&self, cell: &Cell) -> String {
        let chars = links_only(self.any_cell_chars(cell, None));
        chars
            .split(|c| c.ch == '\n')
            .map(|line| inline(trim(line), Context::Paragraph))
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The table read through its accessors (`Table::columns`, `Table::rows`,
/// the title, caption and header and footer settings).
fn table_gfm(table: &Table, exporter: &GfmExporter) -> Option<String> {
    let columns = table.columns();
    if columns.is_empty() {
        return None;
    }
    let console = &exporter.console;
    let row_line = |cells: Vec<String>| format!("| {} |", cells.join(" | "));
    let mut out = String::new();
    if let Some(title) = table.get_title().map(|title| exporter.label(title)) {
        if !title.is_empty() {
            out.push_str(&format!("**{title}**\n\n"));
        }
    }
    out.push_str(&row_line(if table.get_show_header() {
        columns
            .iter()
            .map(|column| table_cell(&links_only(exporter.any_cell_chars(column.header(), None))))
            .collect()
    } else {
        vec![String::new(); columns.len()]
    }));
    out.push('\n');
    out.push_str(&row_line(
        columns
            .iter()
            .map(|column| match column.justify() {
                Justify::Center => ":---:".to_string(),
                Justify::Right => "---:".to_string(),
                _ => "---".to_string(),
            })
            .collect(),
    ));
    let empty = Cell::Text(Text::new(""));
    for (index, row) in table.rows().iter().enumerate() {
        let row_style = table.get_row_style(console, index);
        out.push('\n');
        out.push_str(&row_line(
            columns
                .iter()
                .enumerate()
                .map(|(at, column)| {
                    let base = StyleType::Style(column.style().combine(&row_style));
                    let cell = row.cells().get(at).unwrap_or(&empty);
                    table_cell(&exporter.any_cell_chars(cell, Some(&base)))
                })
                .collect(),
        ));
    }
    if table.get_show_footer() {
        let footer = console
            .get_style(table.get_footer_style())
            .unwrap_or_default();
        out.push('\n');
        out.push_str(&row_line(
            columns
                .iter()
                .map(|column| {
                    let style = match column.footer_style() {
                        Some(fill) => footer.combine(fill),
                        None => footer.clone(),
                    };
                    let base = StyleType::Style(style);
                    table_cell(&exporter.any_cell_chars(column.footer(), Some(&base)))
                })
                .collect(),
        ));
    }
    if let Some(caption) = table.get_caption().map(|caption| exporter.label(caption)) {
        if !caption.is_empty() {
            out.push_str(&format!("\n\n*{caption}*"));
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Tree
// ---------------------------------------------------------------------------

impl ToGfm for Tree {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        let mut out = String::new();
        if self.is_root_hidden() {
            if self.is_expanded() {
                for child in self.children() {
                    tree_item(child, 0, exporter, &mut out);
                }
            }
        } else {
            tree_item(self, 0, exporter, &mut out);
        }
        out.trim_end().to_string()
    }
}

fn tree_item(tree: &Tree, depth: usize, exporter: &GfmExporter, out: &mut String) {
    let indent = "  ".repeat(depth);
    let style = tree.get_style();
    let label = match exporter.cell_chars(tree.label(), Some(style)) {
        Some(chars) => chars
            .split(|c| c.ch == '\n')
            .map(|line| inline(trim(line), Context::Paragraph))
            .collect::<Vec<_>>(),
        None => match tree.label() {
            Cell::Renderable(renderable) => {
                let rendered = exporter.console.render_export(renderable.as_ref());
                rendered
                    .lines()
                    .map(|line| {
                        let chars: Vec<StyledChar> = line
                            .trim()
                            .chars()
                            .map(|ch| StyledChar {
                                ch,
                                flags: Flags::default(),
                            })
                            .collect();
                        inline(&chars, Context::Paragraph)
                    })
                    .collect()
            }
            _ => Vec::new(),
        },
    };
    let label: Vec<&String> = label.iter().filter(|line| !line.is_empty()).collect();
    out.push_str(&indent);
    out.push_str("- ");
    let continuation = format!("\\\n{indent}  ");
    out.push_str(
        &label
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join(&continuation),
    );
    out.push('\n');
    if tree.is_expanded() {
        for child in tree.children() {
            tree_item(child, depth + 1, exporter, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Panel and Rule
// ---------------------------------------------------------------------------

impl ToGfm for Panel {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        panel_gfm(self, exporter)
    }
}

/// The body of a panel: a [`Text`] (or anything that prints as one) as
/// paragraphs. Anything else exposes nothing, so it is rendered wide and read
/// back: a drawing (box characters) is fenced, the rest is paragraphs.
fn panel_body(body: &dyn Renderable, exporter: &GfmExporter) -> String {
    if let Some(text) = body.printed_text() {
        return exporter.text(&text);
    }
    let lines = exporter.wide_lines(body);
    let indent = lines
        .iter()
        .filter(|line| line.iter().any(|c| c.ch != ' '))
        .map(|line| line.iter().take_while(|c| c.ch == ' ').count())
        .min()
        .unwrap_or(0);
    let mut body: Vec<Vec<StyledChar>> = lines
        .iter()
        .map(|line| {
            let line = styled(&line[indent.min(line.len())..]);
            trim_end(&line).to_vec()
        })
        .collect();
    while body.first().is_some_and(Vec::is_empty) {
        body.remove(0);
    }
    while body.last().is_some_and(Vec::is_empty) {
        body.pop();
    }
    let preformatted = body.iter().any(|line| {
        line.iter()
            .any(|c| ('\u{2500}'..='\u{259f}').contains(&c.ch))
    });
    if preformatted {
        let plain: Vec<String> = body
            .iter()
            .map(|line| line.iter().map(|c| c.ch).collect())
            .collect();
        return fenced(&plain.join("\n"), "text", "");
    }
    let mut joined = Vec::new();
    for (i, line) in body.iter().enumerate() {
        if i > 0 {
            joined.push(StyledChar {
                ch: '\n',
                flags: Flags::default(),
            });
        }
        joined.extend(line.iter().cloned());
    }
    paragraphs(&joined)
}

/// The panel read through its accessors (`Panel::get_title`,
/// `Panel::get_subtitle`, `Panel::renderable`).
fn panel_gfm(panel: &Panel, exporter: &GfmExporter) -> String {
    let label = |cell: Option<Cell>| cell.map(|cell| exporter.label(&cell)).unwrap_or_default();
    let title = label(panel.get_title());
    let subtitle = label(panel.get_subtitle());
    let content = panel_body(panel.renderable(), exporter);
    let mut block = Vec::new();
    match exporter.options.panel {
        PanelStyle::Blockquote => {
            if !title.is_empty() {
                block.push(format!("**{title}**"));
                block.push(String::new());
            }
            block.extend(content.lines().map(str::to_string));
            if !subtitle.is_empty() {
                block.push(String::new());
                block.push(format!("*{subtitle}*"));
            }
            block
                .iter()
                .map(|line| {
                    if line.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        PanelStyle::Details { open } => {
            let summary = match panel.get_title() {
                Some(cell) if !title.is_empty() => exporter
                    .any_cell_chars(&cell, None)
                    .iter()
                    .map(|c| if c.ch == '\n' { ' ' } else { c.ch })
                    .collect::<String>()
                    .trim()
                    .to_string(),
                _ => exporter.options.summary.clone(),
            };
            let mut out = format!(
                "<details{}><summary>{}</summary>\n\n{content}",
                if open { " open" } else { "" },
                escape_html(&summary)
            );
            if !subtitle.is_empty() {
                out.push_str(&format!("\n\n*{subtitle}*"));
            }
            out.push_str("\n\n</details>");
            out
        }
    }
}

/// `---`, and the rule's title (`Rule::get_title`) in bold after it.
impl ToGfm for Rule {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        let title = self
            .get_title()
            .map(|cell| exporter.label(&cell))
            .unwrap_or_default();
        if title.is_empty() {
            "---".to_string()
        } else {
            format!("---\n\n**{title}**")
        }
    }
}

/// A core `Markdown` is written as its source (`Markdown::markup`).
#[cfg(feature = "markdown")]
impl ToGfm for rich::markdown::Markdown {
    fn to_gfm(&self, _exporter: &GfmExporter) -> String {
        self.markup().trim_end().to_string()
    }
}

// ---------------------------------------------------------------------------
// Text, groups, the fallback
// ---------------------------------------------------------------------------

impl ToGfm for Text {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        exporter.text(self)
    }
}

impl ToGfm for str {
    /// A string is console markup, as everywhere in `rich`.
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        exporter.markup(self)
    }
}

impl ToGfm for String {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        exporter.markup(self)
    }
}

/// `Columns` expose nothing of their items: written as their plain rendering.
impl ToGfm for Columns {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        exporter.plain(self)
    }
}

/// A group writes each item as its own block: a `Text` as paragraphs, any
/// other renderable as its plain rendering.
impl ToGfm for Renderables {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        self.renderables()
            .iter()
            .map(|item| match item.printed_text() {
                Some(text) => exporter.text(&text),
                None => exporter.plain(item.as_ref()),
            })
            .filter(|block| !block.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Any renderable, written as its plain rendering in a ```` ```text ```` fence.
///
/// ```
/// use rich::Text;
/// use rich_ext::markdown_export::{GfmExporter, Plain};
///
/// let out = GfmExporter::new().export(&Plain(&Text::new("**")));
/// assert_eq!(out, "```text\n**\n```");
/// ```
pub struct Plain<'a>(pub &'a dyn Renderable);

impl ToGfm for Plain<'_> {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        exporter.plain(self.0)
    }
}

/// Fenced code with an info string, for [`GfmDocument::push`].
pub struct Code<'a> {
    /// The code.
    pub code: &'a str,
    /// The fence's language (empty for none).
    pub language: &'a str,
}

impl ToGfm for Code<'_> {
    fn to_gfm(&self, exporter: &GfmExporter) -> String {
        exporter.code(self.code, self.language)
    }
}

/// Markdown written as it is, for [`GfmDocument::push`].
pub struct MarkdownSource<'a>(pub &'a str);

impl ToGfm for MarkdownSource<'_> {
    fn to_gfm(&self, _exporter: &GfmExporter) -> String {
        self.0.trim_end().to_string()
    }
}

#[cfg(feature = "syntax")]
mod syntax_export {
    use super::*;
    use rich::protocol::{
        CodeHighlighter, CodeHighlighting, ConsoleCodeHighlighting, HighlightError, HighlightedCode,
    };
    use rich::Syntax;
    use std::sync::{Arc, Mutex};

    /// Records the code and language `Syntax` highlights.
    #[derive(Default)]
    struct Probe(Mutex<Option<(String, Option<String>)>>);

    impl CodeHighlighter for Probe {
        fn highlight(
            &self,
            code: &str,
            language: Option<&str>,
            _theme: &str,
        ) -> Result<HighlightedCode, HighlightError> {
            if let Ok(mut seen) = self.0.lock() {
                *seen = Some((code.to_string(), language.map(str::to_string)));
            }
            Ok(HighlightedCode::default())
        }
        fn default_theme(&self) -> &str {
            "probe"
        }
        fn themes(&self) -> Vec<String> {
            vec!["probe".into()]
        }
        fn languages(&self) -> Vec<String> {
            Vec::new()
        }
    }

    /// A fenced block with the code and the language `Syntax` was given. A
    /// `Syntax` given a highlighter of its own (`Syntax::highlighter`) does
    /// not show its language: the fence has none.
    impl ToGfm for Syntax {
        fn to_gfm(&self, exporter: &GfmExporter) -> String {
            // A `Syntax` without a highlighter of its own uses the console's,
            // so a console whose highlighter is the probe sees its code and
            // language.
            let probe = Arc::new(Probe::default());
            let mut console = Console::builder().width(exporter.options.width).build();
            console.set_code_highlighting(Some(CodeHighlighting {
                highlighter: probe.clone(),
                theme: None,
            }));
            let text = self.highlight_for(&console);
            let seen = probe.0.lock().ok().and_then(|mut seen| seen.take());
            match seen {
                Some((code, language)) => exporter.code(&code, language.as_deref().unwrap_or("")),
                // It has its own highlighter: the code, without its language.
                None => exporter.code(text.plain(), ""),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Documents and GitHub's limit
// ---------------------------------------------------------------------------

/// A Markdown document built block by block.
///
/// ```
/// use rich::{Panel, Text};
/// use rich_ext::markdown_export::{GfmDocument, GfmOptions, PanelStyle};
///
/// let mut doc = GfmDocument::with_options(
///     GfmOptions::new().panel(PanelStyle::Details { open: false }),
/// );
/// doc.push(&Panel::new(Box::new(Text::new("all green"))).title("CI"));
/// doc.push_code("SELECT 1", "sql");
/// assert_eq!(
///     doc.finish(),
///     "<details><summary>CI</summary>\n\nall green\n\n</details>\n\n```sql\nSELECT 1\n```\n"
/// );
/// ```
#[derive(Default)]
pub struct GfmDocument {
    exporter: GfmExporter,
    blocks: Vec<String>,
}

impl GfmDocument {
    /// An empty document with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty document with `options`.
    pub fn with_options(options: GfmOptions) -> Self {
        GfmDocument {
            exporter: GfmExporter::with_options(options),
            blocks: Vec::new(),
        }
    }

    /// The exporter blocks are written with.
    pub fn exporter(&self) -> &GfmExporter {
        &self.exporter
    }

    /// Append `value` as a block.
    pub fn push(&mut self, value: &(impl ToGfm + ?Sized)) -> &mut Self {
        let block = value.to_gfm(&self.exporter);
        if !block.is_empty() {
            self.blocks.push(block);
        }
        self
    }

    /// Append any renderable's plain rendering in a ```` ```text ```` fence.
    pub fn push_plain(&mut self, renderable: &dyn Renderable) -> &mut Self {
        self.push(&Plain(renderable))
    }

    /// Append Markdown as it is (the source of a `rich::markdown::Markdown`).
    pub fn push_markdown(&mut self, source: &str) -> &mut Self {
        self.push(&MarkdownSource(source))
    }

    /// Append a fenced code block.
    pub fn push_code(&mut self, code: &str, language: &str) -> &mut Self {
        self.push(&Code { code, language })
    }

    /// Append an ATX heading (`level` 1 to 6) whose text is console markup.
    pub fn push_heading(&mut self, level: usize, markup: &str) -> &mut Self {
        let text = self.exporter.markup(markup).replace('\n', " ");
        let block = format!("{} {text}", "#".repeat(level.clamp(1, 6)));
        self.blocks.push(block);
        self
    }

    /// The blocks written so far.
    pub fn blocks(&self) -> &[String] {
        &self.blocks
    }

    /// The document: blocks separated by blank lines, ending in a newline.
    pub fn finish(&self) -> String {
        if self.blocks.is_empty() {
            return String::new();
        }
        let mut out = self.blocks.join("\n\n");
        out.push('\n');
        out
    }
}

/// `markdown` cut to fit a GitHub comment ([`GITHUB_COMMENT_LIMIT`]
/// characters), with a note saying so; see [`truncate_markdown`].
pub fn fit_github_comment(markdown: &str) -> String {
    truncate_markdown(markdown, GITHUB_COMMENT_LIMIT)
}

/// `markdown` cut to at most `limit` characters (Unicode scalar values).
///
/// Text that fits is returned unchanged. Otherwise it is cut at a line break,
/// a code fence or `<details>` block left open is closed, and a note giving
/// how much was kept is appended; the result, note included, fits `limit`.
///
/// ```
/// use rich_ext::markdown_export::truncate_markdown;
///
/// let long = format!("```text\n{}```\n", "line\n".repeat(100));
/// let cut = truncate_markdown(&long, 200);
/// assert!(cut.chars().count() <= 200);
/// assert!(cut.contains("\n```\n"), "the fence is closed");
/// assert!(cut.ends_with("characters shown.*\n"));
/// ```
pub fn truncate_markdown(markdown: &str, limit: usize) -> String {
    let total = markdown.chars().count();
    if total <= limit {
        return markdown.to_string();
    }
    let note = |kept: usize| format!("\n\n*Truncated: {kept} of {total} characters shown.*\n");
    let mut budget = limit.saturating_sub(note(total).chars().count());
    loop {
        let cut: String = markdown.chars().take(budget).collect();
        let kept = match cut.rfind('\n') {
            Some(at) if at > 0 => &cut[..at],
            _ => cut.as_str(),
        };
        let closers = closers(kept);
        let kept_chars = kept.chars().count();
        let out = format!("{kept}{closers}{}", note(kept_chars));
        let size = out.chars().count();
        if size <= limit || budget == 0 {
            if size <= limit {
                return out;
            }
            // Not even the note fits.
            return markdown.chars().take(limit).collect();
        }
        budget = budget.saturating_sub(size - limit);
    }
}

/// What closes the fences and `<details>` blocks `markdown` leaves open.
fn closers(markdown: &str) -> String {
    let mut fence: Option<(String, char, usize)> = None;
    let mut details = 0usize;
    for line in markdown.lines() {
        let prefix_len = line.len() - line.trim_start_matches(['>', ' ']).len();
        let (prefix, rest) = line.split_at(prefix_len);
        let marker = rest.chars().next().filter(|ch| *ch == '`' || *ch == '~');
        let run = marker.map_or(0, |ch| rest.chars().take_while(|c| *c == ch).count());
        match (&fence, marker) {
            (None, Some(ch)) if run >= 3 => fence = Some((prefix.to_string(), ch, run)),
            (Some((_, open, len)), Some(ch))
                if ch == *open && run >= *len && rest.trim_end().chars().all(|c| c == ch) =>
            {
                fence = None
            }
            (None, _) => {
                details += line.matches("<details").count();
                details = details.saturating_sub(line.matches("</details>").count());
            }
            _ => {}
        }
    }
    let mut out = String::new();
    if let Some((prefix, ch, len)) = fence {
        out.push('\n');
        out.push_str(&prefix);
        out.extend(std::iter::repeat_n(ch, len));
    }
    for _ in 0..details {
        out.push_str("\n\n</details>");
    }
    out
}

#[cfg(test)]
mod tests;
