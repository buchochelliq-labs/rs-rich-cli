//! Drawing the document model with core's renderables, in the look of core's
//! Markdown: the same `markdown.*` theme styles, list markers, quote bar,
//! code panels, table boxes and blank-line spacing, so an Org file, an
//! AsciiDoc file and a man page read like a Markdown file does.

use std::sync::Arc;

use rich::cells::cell_len;
use rich::protocol::{enter_region, RegionInfo, RegionRole};
use rich::r#box::{ROUNDED, SIMPLE};
use rich::table::Table;
use rich::{
    Console, ConsoleOptions, FenceRenderer, Justify, Overflow, Panel, Renderable, Segment, Style,
    StyleType, Syntax, Text,
};

use super::model::{
    inlines_text, Align, Block, Checkbox, CodeBlock, Definition, Document, Heading, Inline, List,
    ListItem, ListKind, TableBlock,
};
use super::{Diagnostic, Parsed, STYLES};

/// The placeholder core's Markdown puts in front of an image.
const IMAGE_MARKER: &str = "\u{1f306} ";
/// Core Markdown's bullet.
const BULLET: &str = " \u{2022} ";
/// Core Markdown's block-quote bar.
const QUOTE_PREFIX: &str = "\u{258c} ";
/// How far a definition's body is indented under its term.
const DEFINITION_INDENT: usize = 4;
/// Containers nest at most this deep on screen; deeper content keeps the
/// last indentation, so a hostile document cannot squeeze text to nothing.
///
/// Every container re-processes each line of what it holds, so the cost of
/// a nest grows with its depth squared. Depth is therefore weighted by what
/// a level costs: a list item or indent counts 1, a quote (which restyles
/// its content) 2, and a panel 4, so at most six panels nest.
const MAX_DEPTH: usize = 24;
const QUOTE_DEPTH: usize = 2;
const PANEL_DEPTH: usize = 4;
/// Nor does nesting indent content narrower than this many cells.
const MIN_CONTENT: usize = 12;

/// What a document's code, links and hyperlinks look like when drawn.
///
/// The defaults match core's `Markdown`: hyperlinks on (a link's text is an
/// OSC 8 link), code in `Syntax`'s default theme (or the console's code
/// highlighting), and no fence renderers.
#[derive(Clone)]
pub struct RenderOptions {
    /// `true`: a link's text carries an OSC 8 hyperlink. `false`: the target
    /// is written out after the text, `text (https://…)`, as `rich -m` does
    /// without `-y`.
    pub hyperlinks: bool,
    /// The `Syntax` theme for code blocks; `None` keeps the console's.
    pub code_theme: Option<String>,
    /// The language of code blocks that name none (a man page's examples,
    /// an AsciiDoc listing). `None` draws them as plain text.
    pub default_language: Option<String>,
    /// Renderers asked first for code blocks with a language (Mermaid, DOT),
    /// as `Markdown::fence_renderer` asks them for fences.
    pub fences: Vec<Arc<dyn FenceRenderer>>,
    /// The justification of paragraphs; `None` is left.
    pub justify: Option<Justify>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            hyperlinks: true,
            code_theme: None,
            default_language: None,
            fences: Vec::new(),
            justify: None,
        }
    }
}

impl std::fmt::Debug for RenderOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderOptions")
            .field("hyperlinks", &self.hyperlinks)
            .field("code_theme", &self.code_theme)
            .field("default_language", &self.default_language)
            .field("fences", &self.fences.len())
            .field("justify", &self.justify)
            .finish()
    }
}

impl RenderOptions {
    /// The defaults: hyperlinks on, the console's code theme.
    pub fn new() -> Self {
        RenderOptions::default()
    }

    /// Whether links carry OSC 8 hyperlinks (see [`hyperlinks`](Self::hyperlinks)).
    pub fn hyperlinks(mut self, hyperlinks: bool) -> Self {
        self.hyperlinks = hyperlinks;
        self
    }

    /// The `Syntax` theme for code blocks.
    pub fn code_theme(mut self, theme: impl Into<String>) -> Self {
        self.code_theme = Some(theme.into());
        self
    }

    /// The language of code blocks that name none.
    pub fn default_language(mut self, language: impl Into<String>) -> Self {
        self.default_language = Some(language.into());
        self
    }

    /// Ask `renderer` first for code blocks with a language.
    pub fn fence_renderer(mut self, renderer: Arc<dyn FenceRenderer>) -> Self {
        self.fences.push(renderer);
        self
    }

    /// Justify paragraphs.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = Some(justify);
        self
    }
}

/// A parsed document, ready to print. Build one from source with
/// [`org`](Self::org), [`asciidoc`](Self::asciidoc) or [`man`](Self::man),
/// or from a [`Document`] built by hand with [`new`](Self::new).
///
/// ```
/// use rich::Console;
/// use rich_ext::document::DocumentView;
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let view = DocumentView::org("* TODO Write the report :work:\nSome /italic/ text.");
/// let out = console.render_to_string(&view);
/// assert!(out.contains("TODO Write the report"));
/// assert!(out.contains("Some italic text."));
/// ```
#[derive(Clone, Debug)]
pub struct DocumentView {
    document: Document,
    diagnostics: Vec<Diagnostic>,
    options: RenderOptions,
}

impl DocumentView {
    /// Draw `document` with the default [`RenderOptions`].
    pub fn new(document: Document) -> Self {
        DocumentView {
            document,
            diagnostics: Vec::new(),
            options: RenderOptions::default(),
        }
    }

    /// Draw what a parser returned, keeping its diagnostics.
    pub fn from_parsed(parsed: Parsed) -> Self {
        DocumentView {
            document: parsed.document,
            diagnostics: parsed.diagnostics,
            options: RenderOptions::default(),
        }
    }

    /// Parse and draw an Org document.
    pub fn org(source: &str) -> Self {
        DocumentView::from_parsed(super::org::parse(source))
    }

    /// Parse and draw an AsciiDoc document.
    pub fn asciidoc(source: &str) -> Self {
        DocumentView::from_parsed(super::asciidoc::parse(source))
    }

    /// Parse and draw a man page (man or mdoc macros).
    pub fn man(source: &str) -> Self {
        DocumentView::from_parsed(super::man::parse(source))
    }

    /// Replace every render option.
    pub fn options(mut self, options: RenderOptions) -> Self {
        self.options = options;
        self
    }

    /// Whether links carry OSC 8 hyperlinks (see [`RenderOptions::hyperlinks`]).
    pub fn hyperlinks(mut self, hyperlinks: bool) -> Self {
        self.options.hyperlinks = hyperlinks;
        self
    }

    /// The `Syntax` theme for code blocks.
    pub fn code_theme(mut self, theme: impl Into<String>) -> Self {
        self.options.code_theme = Some(theme.into());
        self
    }

    /// The language of code blocks that name none.
    pub fn default_language(mut self, language: impl Into<String>) -> Self {
        self.options.default_language = Some(language.into());
        self
    }

    /// Ask `renderer` first for code blocks with a language.
    pub fn fence_renderer(mut self, renderer: Arc<dyn FenceRenderer>) -> Self {
        self.options.fences.push(renderer);
        self
    }

    /// The parsed document.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// What the parser could not show as written.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The render options.
    pub fn render_options(&self) -> &RenderOptions {
        &self.options
    }
}

impl Renderable for DocumentView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let painter = Painter {
            console,
            options,
            doc: &self.options,
        };
        let lines = painter.blocks(&self.document.blocks, options.max_width, Container::Top, 0);
        join_lines(lines)
    }
}

/// Blocks inside a panel, which needs an owned renderable for its body.
struct Nested {
    blocks: Vec<Block>,
    options: RenderOptions,
    depth: usize,
}

impl Renderable for Nested {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let painter = Painter {
            console,
            options,
            doc: &self.options,
        };
        let lines = painter.blocks(
            &self.blocks,
            options.max_width,
            Container::Boxed,
            self.depth,
        );
        join_lines(lines)
    }
}

/// Rows joined with line breaks; lines are separated, not terminated.
fn join_lines(lines: Vec<Vec<Segment>>) -> Vec<Segment> {
    let mut segments = Vec::new();
    let last = lines.len().saturating_sub(1);
    for (index, line) in lines.into_iter().enumerate() {
        segments.extend(line);
        if index != last {
            segments.push(Segment::line());
        }
    }
    segments
}

/// Pad every row out to `width`, as core's Markdown pads list items and
/// quotes, so nested content keeps its column.
fn pad_lines(lines: &mut [Vec<Segment>], width: usize) {
    for line in lines.iter_mut() {
        let len: usize = line.iter().map(Segment::cell_length).sum();
        if len < width {
            line.push(Segment::new(" ".repeat(width - len), None));
        }
    }
}

/// Where a run of blocks sits, which decides the blank lines between them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    /// The document, a quote, an indent or a definition's body: a blank
    /// line between blocks.
    Top,
    /// Inside a panel.
    Boxed,
    /// A list item: tight, except between two paragraphs.
    Item,
}

struct Painter<'a> {
    console: &'a Console,
    options: &'a ConsoleOptions,
    doc: &'a RenderOptions,
}

/// `outer` with `inner` layered over it.
fn layer(outer: Option<&Style>, inner: Style) -> Style {
    match outer {
        Some(outer) => outer.combine(&inner),
        None => inner,
    }
}

impl Painter<'_> {
    /// A theme style: the console theme's, else this module's default, else
    /// core Markdown's default, else `name` parsed as a style definition.
    fn style(&self, name: &str) -> Style {
        if let Some(style) = self.console.theme().get(name) {
            return style.clone();
        }
        STYLES
            .iter()
            .chain(MARKDOWN_STYLES)
            .find(|(key, _)| *key == name)
            .and_then(|(_, spec)| Style::parse(spec).ok())
            .or_else(|| {
                self.console
                    .get_style(&StyleType::Name(name.to_string()))
                    .ok()
            })
            .unwrap_or_default()
    }

    fn blocks(
        &self,
        blocks: &[Block],
        width: usize,
        container: Container,
        depth: usize,
    ) -> Vec<Vec<Segment>> {
        let width = width.max(1);
        let mut out: Vec<Vec<Segment>> = Vec::new();
        let mut previous: Option<&Block> = None;
        for block in blocks {
            let lines = self.block(block, width, depth);
            if lines.is_empty() {
                continue;
            }
            if let Some(previous) = previous {
                if !self.joins(previous, block, container) {
                    out.push(Vec::new());
                }
            }
            out.extend(lines);
            previous = Some(block);
        }
        out
    }

    /// Whether `block` follows `previous` with no blank line between.
    fn joins(&self, previous: &Block, block: &Block, container: Container) -> bool {
        // A byline and a heading's drawers and planning lines stay with the
        // heading, and a caption with what it names.
        let attached = match block {
            Block::Byline(_) => matches!(previous, Block::Heading(_) | Block::Byline(_)),
            Block::Properties { .. } => {
                matches!(previous, Block::Heading(_) | Block::Properties { .. })
            }
            _ => false,
        };
        if attached || matches!(previous, Block::Caption(_)) {
            return true;
        }
        container == Container::Item
            && !(matches!(previous, Block::Paragraph(_)) && matches!(block, Block::Paragraph(_)))
    }

    fn block(&self, block: &Block, width: usize, depth: usize) -> Vec<Vec<Segment>> {
        match block {
            Block::Heading(heading) => self.heading(heading, width),
            Block::Byline(inlines) => {
                let style = self.style("document.byline");
                let mut text = self.inlines(inlines, Some(&style));
                text.set_justify(Justify::Center);
                self.text_lines(&text, width)
            }
            Block::Paragraph(inlines) => {
                let mut text = self.inlines(inlines, None);
                text.set_justify(self.doc.justify.unwrap_or(Justify::Left));
                self.text_lines(&text, width)
            }
            Block::Caption(inlines) => {
                let style = self.style("document.caption");
                let text = self.inlines(inlines, Some(&style));
                self.text_lines(&text, width)
            }
            Block::Lines(lines) => {
                let mut out = Vec::new();
                for line in lines {
                    let text = self.inlines(line, None);
                    out.extend(self.text_lines(&text, width));
                }
                out
            }
            Block::List(list) => self.list(list, width, depth),
            Block::Definitions(definitions) => self.definitions(definitions, width, depth),
            Block::Code(code) => self.code(code, width),
            Block::Table(table) => self.table(table, width),
            Block::Quote {
                blocks,
                attribution,
            } => self.quote(blocks, attribution.as_deref(), width, depth),
            Block::Admonition {
                kind,
                title,
                blocks,
            } => {
                let style = self.style(kind.style_key());
                // The panel pads its title with a space each side.
                let mut label = Text::styled(kind.label(), style.clone());
                if let Some(title) = title {
                    label.append(": ", Some(style.clone().into()));
                    label = label.append_text(&self.inlines(title, Some(&style)));
                }
                self.panel(blocks, Some(label), style, width, depth)
            }
            Block::Aside { title, blocks } => {
                let style = self.style("document.aside");
                let label = title.as_ref().map(|title| self.inlines(title, None));
                self.panel(blocks, label, style, width, depth)
            }
            Block::Indent { amount, blocks } => {
                self.indented(blocks, *amount, width, depth, Container::Top)
            }
            Block::Rule => {
                let style = self.style("markdown.hr");
                vec![vec![Segment::new("-".repeat(width), Some(style))]]
            }
            Block::Image { target, alt } => {
                let text = self.image(target, alt, None);
                self.text_lines(&text, width)
            }
            Block::Properties { name, entries } => self.properties(name.as_deref(), entries, width),
            Block::Banner {
                left,
                center,
                right,
            } => self.banner(left, center, right, width),
        }
    }

    /// Wrap `text` to `width`, as `Text.__rich_console__` does: its own
    /// justify, overflow and no-wrap win, then the options'.
    fn text_lines(&self, text: &Text, width: usize) -> Vec<Vec<Segment>> {
        let justify = text.get_justify_option().unwrap_or(self.options.justify);
        text.render_lines_wrapped_tabs(
            self.console.theme(),
            self.console.base_style(),
            Some(width.max(1)),
            justify,
            text.get_overflow()
                .or(self.options.overflow)
                .unwrap_or(Overflow::Fold),
            text.get_no_wrap().or(self.options.no_wrap).unwrap_or(false),
            text.console_tab_size(self.console),
        )
    }

    fn heading(&self, heading: &Heading, width: usize) -> Vec<Vec<Segment>> {
        let level = heading.level.clamp(1, 6);
        let style = self.style(&format!("markdown.h{level}"));
        let mut text = Text::new("");
        if let Some(keyword) = &heading.keyword {
            let key = if keyword.done {
                "document.done"
            } else {
                "document.todo"
            };
            text.append(&keyword.text, Some(style.combine(&self.style(key)).into()));
            text.append(" ", Some(style.clone().into()));
        }
        if let Some(priority) = &heading.priority {
            let priority_style = style.combine(&self.style("document.priority"));
            text.append(&format!("[#{priority}]"), Some(priority_style.into()));
            text.append(" ", Some(style.clone().into()));
        }
        self.append_inlines(&mut text, &heading.content, Some(&style));
        if !heading.tags.is_empty() {
            text.append("  ", None);
            let tags = format!(":{}:", heading.tags.join(":"));
            text.append(&tags, Some(self.style("document.tag").into()));
        }
        text.set_justify(if level == 1 {
            Justify::Center
        } else {
            Justify::Left
        });
        let region = enter_region(self.console, || {
            RegionInfo::new(RegionRole::Heading { level }).label(inlines_text(&heading.content))
        });
        let mut lines = self.text_lines(&text, width);
        if let Some(region) = region {
            for line in lines.iter_mut() {
                region.tag(line);
            }
        }
        lines
    }

    /// `inlines` as one `Text`, every run layered over `base`.
    fn inlines(&self, inlines: &[Inline], base: Option<&Style>) -> Text {
        let mut text = Text::new("");
        self.append_inlines(&mut text, inlines, base);
        text
    }

    fn append_inlines(&self, text: &mut Text, inlines: &[Inline], style: Option<&Style>) {
        for inline in inlines {
            match inline {
                Inline::Text(content) => text.append(content, style.cloned().map(Into::into)),
                Inline::Emphasis(content) => {
                    let inner = layer(style, self.style("markdown.em"));
                    self.append_inlines(text, content, Some(&inner));
                }
                Inline::Strong(content) => {
                    let inner = layer(style, self.style("markdown.strong"));
                    self.append_inlines(text, content, Some(&inner));
                }
                Inline::Underline(content) => {
                    let inner = layer(style, self.style("document.underline"));
                    self.append_inlines(text, content, Some(&inner));
                }
                Inline::Strike(content) => {
                    let inner = layer(style, self.style("markdown.s"));
                    self.append_inlines(text, content, Some(&inner));
                }
                Inline::Code(content) => {
                    let inner = layer(style, self.style("markdown.code"));
                    text.append(content, Some(inner.into()));
                }
                Inline::Link { target, content } => self.link(text, target, content, style),
                Inline::Image { target, alt } => {
                    let image = self.image(target, alt, style);
                    *text = std::mem::take(text).append_text(&image);
                }
                Inline::LineBreak => text.append("\n", None),
                Inline::Styled {
                    style: name,
                    content,
                } => {
                    let inner = layer(style, self.style(name));
                    self.append_inlines(text, content, Some(&inner));
                }
            }
        }
    }

    /// A link as core's Markdown draws one: with hyperlinks, the text in
    /// `markdown.link_url` carrying the OSC 8 target; without, the text in
    /// `markdown.link` and the target written out after it. A link whose
    /// text is its target (a bare URL) shows the target once.
    fn link(&self, text: &mut Text, target: &str, content: &[Inline], style: Option<&Style>) {
        let url_style = layer(style, self.style("markdown.link_url"));
        let shown = inlines_text(content);
        let bare = content.is_empty() || shown == target;
        if self.doc.hyperlinks {
            let linked = url_style.with_link(target.to_string());
            if bare {
                text.append(target, Some(linked.into()));
            } else {
                self.append_inlines(text, content, Some(&linked));
            }
            return;
        }
        if bare {
            text.append(target, Some(url_style.into()));
            return;
        }
        let label_style = layer(style, self.style("markdown.link"));
        self.append_inlines(text, content, Some(&label_style));
        text.append(" (", style.cloned().map(Into::into));
        text.append(target, Some(url_style.into()));
        text.append(")", style.cloned().map(Into::into));
    }

    /// Core Markdown's image placeholder: the marker, then the alt text
    /// (or the target's last path component), linked to the target when
    /// hyperlinks are on.
    fn image(&self, target: &str, alt: &str, style: Option<&Style>) -> Text {
        let title = if alt.trim().is_empty() {
            let trimmed = target.trim_matches('/');
            trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
        } else {
            alt.to_string()
        };
        let mut title_style = style.cloned().unwrap_or_default();
        if self.doc.hyperlinks && !target.is_empty() {
            title_style = title_style.with_link(target.to_string());
        }
        let mut text = Text::new(IMAGE_MARKER);
        text.append(&title, Some(title_style.into()));
        text.append(" ", None);
        text
    }

    fn list(&self, list: &List, width: usize, depth: usize) -> Vec<Vec<Segment>> {
        let markers = list_markers(list);
        let marker_width = markers.iter().map(|m| cell_len(m)).max().unwrap_or(0);
        let marker_style = self.style(match list.kind {
            ListKind::Bullet => "markdown.item.bullet",
            ListKind::Ordered { .. } => "markdown.item.number",
            ListKind::Callout => "document.callout",
        });
        let mut out = Vec::new();
        for (item, marker) in list.items.iter().zip(markers) {
            // Bullets sit left in their column, numbers right, as columns
            // of numbers do.
            let marker = match list.kind {
                ListKind::Bullet => format!("{marker:<marker_width$}"),
                _ => format!("{marker:>marker_width$}"),
            };
            let mut prefix = vec![Segment::new(marker, Some(marker_style.clone()))];
            let mut prefix_width = marker_width;
            if let Some(checkbox) = item.checkbox {
                let (glyph, key) = match checkbox {
                    Checkbox::Unchecked => ("[ ] ", "document.checkbox"),
                    Checkbox::Checked => ("[x] ", "document.checkbox.checked"),
                    Checkbox::Partial => ("[-] ", "document.checkbox.partial"),
                };
                prefix.push(Segment::new(glyph, Some(self.style(key))));
                prefix_width += 4;
            }
            out.extend(self.item(item, prefix, prefix_width, width, depth));
        }
        out
    }

    /// One list item: its blocks beside its marker, continuation rows
    /// indented to the text.
    fn item(
        &self,
        item: &ListItem,
        prefix: Vec<Segment>,
        prefix_width: usize,
        width: usize,
        depth: usize,
    ) -> Vec<Vec<Segment>> {
        let indent = depth < MAX_DEPTH && prefix_width + MIN_CONTENT <= width;
        let content_width = if indent { width - prefix_width } else { width };
        let mut lines: Vec<Vec<Segment>> = self
            .blocks(&item.blocks, content_width, Container::Item, depth + 1)
            .into_iter()
            .skip_while(|line| line.is_empty())
            .collect();
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        if !indent {
            let mut out = vec![prefix];
            out.extend(lines);
            return out;
        }
        pad_lines(&mut lines, content_width);
        lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let mut row = if index == 0 {
                    prefix.clone()
                } else {
                    vec![Segment::new(" ".repeat(prefix_width), None)]
                };
                row.extend(line);
                row
            })
            .collect()
    }

    fn definitions(
        &self,
        definitions: &[Definition],
        width: usize,
        depth: usize,
    ) -> Vec<Vec<Segment>> {
        let term_style = self.style("document.term");
        let mut out = Vec::new();
        for (index, definition) in definitions.iter().enumerate() {
            if index > 0 {
                out.push(Vec::new());
            }
            let term = self.inlines(&definition.term, Some(&term_style));
            out.extend(self.text_lines(&term, width));
            if !definition.blocks.is_empty() {
                out.extend(self.indented(
                    &definition.blocks,
                    DEFINITION_INDENT,
                    width,
                    depth,
                    Container::Top,
                ));
            }
        }
        out
    }

    fn indented(
        &self,
        blocks: &[Block],
        amount: usize,
        width: usize,
        depth: usize,
        container: Container,
    ) -> Vec<Vec<Segment>> {
        let indent = depth < MAX_DEPTH && amount + MIN_CONTENT <= width;
        if !indent {
            return self.blocks(blocks, width, container, depth + 1);
        }
        let inner = width - amount;
        let mut lines = self.blocks(blocks, inner, container, depth + 1);
        pad_lines(&mut lines, inner);
        lines
            .into_iter()
            .map(|line| {
                let mut row = vec![Segment::new(" ".repeat(amount), None)];
                row.extend(line);
                row
            })
            .collect()
    }

    fn quote(
        &self,
        blocks: &[Block],
        attribution: Option<&[Inline]>,
        width: usize,
        depth: usize,
    ) -> Vec<Vec<Segment>> {
        let style = self.style("markdown.block_quote");
        // Core's Markdown draws quote content four cells narrower.
        let indent = depth + QUOTE_DEPTH <= MAX_DEPTH && 4 + MIN_CONTENT <= width;
        let inner = if indent { width - 4 } else { width };
        let mut lines: Vec<Vec<Segment>> = self
            .blocks(blocks, inner, Container::Top, depth + QUOTE_DEPTH)
            .into_iter()
            .skip_while(|line| line.is_empty())
            .collect();
        if let Some(attribution) = attribution {
            let attribution_style = self.style("document.attribution");
            let mut text = Text::styled("\u{2014} ", attribution_style.clone());
            text = text.append_text(&self.inlines(attribution, Some(&attribution_style)));
            lines.extend(self.text_lines(&text, inner));
        }
        if !indent {
            return lines;
        }
        pad_lines(&mut lines, inner);
        lines
            .into_iter()
            .map(|line| {
                let mut row = vec![Segment::new(QUOTE_PREFIX, Some(style.clone()))];
                row.extend(Segment::apply_style(&line, &style));
                row
            })
            .collect()
    }

    fn panel(
        &self,
        blocks: &[Block],
        title: Option<Text>,
        border: Style,
        width: usize,
        depth: usize,
    ) -> Vec<Vec<Segment>> {
        if depth + PANEL_DEPTH > MAX_DEPTH || width < 4 + MIN_CONTENT {
            return self.blocks(blocks, width, Container::Boxed, depth + 1);
        }
        let body = Nested {
            blocks: blocks.to_vec(),
            options: self.doc.clone(),
            depth: depth + PANEL_DEPTH,
        };
        let mut panel = Panel::new(Box::new(body))
            .box_set(ROUNDED)
            .border_style(border);
        if let Some(title) = title {
            panel = panel
                .title_as_text(title)
                .title_align(rich::HorizontalAlign::Left);
        }
        let inner = self.options.update_width(width);
        Segment::split_lines(&panel.rich_render(self.console, &inner))
    }

    fn code(&self, code: &CodeBlock, width: usize) -> Vec<Vec<Segment>> {
        let inner = self.options.update_width(width);
        let language = code
            .language
            .as_deref()
            .or(self.doc.default_language.as_deref())
            .unwrap_or("");
        let region = enter_region(self.console, || {
            RegionInfo::new(RegionRole::Code).label(language)
        });
        let drawn = if language.is_empty() {
            None
        } else {
            self.doc
                .fences
                .iter()
                .find_map(|fence| fence.render_fence(language, &code.code, self.console, &inner))
        };
        let mut segments = match drawn {
            Some(segments) => segments,
            None => {
                // Callout columns are counted in the tab-expanded code.
                let source = if code.callouts.is_empty() {
                    code.code.clone()
                } else {
                    super::scan::expand_tabs(&code.code)
                };
                let mut syntax = Syntax::new(source, lexer_name(language))
                    .word_wrap(true)
                    .padding(1);
                if let Some(theme) = &self.doc.code_theme {
                    syntax = syntax.theme(theme.as_str());
                }
                let callout = self.style("document.callout");
                for marker in &code.callouts {
                    let line = marker.line as i64 + 1;
                    syntax.stylize_range(
                        callout.clone(),
                        (line, marker.start as i64),
                        (line, marker.end as i64),
                        false,
                    );
                }
                syntax.rich_render(self.console, &inner)
            }
        };
        if let Some(region) = region {
            region.tag(&mut segments);
        }
        Segment::split_lines(&segments)
    }

    fn table(&self, table: &TableBlock, width: usize) -> Vec<Vec<Segment>> {
        let columns = table
            .header
            .iter()
            .chain(&table.rows)
            .map(Vec::len)
            .max()
            .unwrap_or(0);
        if columns == 0 {
            return Vec::new();
        }
        let justify = |column: usize| match table.align.get(column) {
            Some(Align::Center) => Justify::Center,
            Some(Align::Right) => Justify::Right,
            _ => Justify::Left,
        };
        // As core's Markdown builds its tables.
        let mut grid = Table::new()
            .box_set(SIMPLE)
            .pad_edge(false)
            .collapse_padding(true)
            .style(self.style("markdown.table.border"))
            .show_header(table.header.is_some());
        if let Some(title) = &table.title {
            grid = grid.title_text(self.inlines(title, Some(&self.style("document.caption"))));
        }
        let header_style = self.style("markdown.table.header");
        for column in 0..columns {
            let header = table
                .header
                .as_ref()
                .and_then(|header| header.get(column))
                .map(|cell| self.inlines(cell, None))
                .unwrap_or_else(|| Text::new(""));
            grid.add_column_text(header, justify(column));
            grid.column_header_style(header_style.clone());
        }
        for (index, row) in table.rows.iter().enumerate() {
            if index > 0 && table.sections.contains(&index) {
                grid.add_section();
            }
            let cells = (0..columns)
                .map(|column| {
                    row.get(column)
                        .map(|cell| self.inlines(cell, None))
                        .unwrap_or_else(|| Text::new(""))
                })
                .collect();
            grid.add_row_text(cells);
        }
        let inner = self.options.update_width(width);
        Segment::split_lines(&grid.rich_render(self.console, &inner))
    }

    /// Drawers, planning lines and keywords: one dim, wrapped line.
    fn properties(
        &self,
        name: Option<&str>,
        entries: &[(String, String)],
        width: usize,
    ) -> Vec<Vec<Segment>> {
        let style = self.style("document.properties");
        let key_style = style.combine(&self.style("document.properties.key"));
        let mut text = Text::new("");
        if let Some(name) = name {
            text.append(name, Some(key_style.clone().into()));
            if !entries.is_empty() {
                text.append("  ", Some(style.clone().into()));
            }
        }
        for (index, (key, value)) in entries.iter().enumerate() {
            if index > 0 {
                text.append(" \u{b7} ", Some(style.clone().into()));
            }
            if !key.is_empty() {
                text.append(key, Some(key_style.clone().into()));
                if !value.is_empty() {
                    text.append(": ", Some(style.clone().into()));
                }
            }
            text.append(value, Some(style.clone().into()));
        }
        self.text_lines(&text, width)
    }

    /// A man page's running header: left, centre and right on one row; the
    /// centre part is dropped first, then the right, when the row is narrow.
    fn banner(&self, left: &str, center: &str, right: &str, width: usize) -> Vec<Vec<Segment>> {
        let style = self.style("document.banner");
        let (l, c, r) = (cell_len(left), cell_len(center), cell_len(right));
        let line = if l + c + r + 2 <= width {
            // Centre the middle part on the row, as far as the sides allow.
            let start = ((width - c) / 2).clamp(l + 1, width - r - 1 - c);
            format!(
                "{left}{}{center}{}{right}",
                " ".repeat(start - l),
                " ".repeat(width - r - start - c)
            )
        } else if l + r + 2 <= width {
            format!("{left}{}{right}", " ".repeat(width - l - r))
        } else {
            left.to_string()
        };
        let text = Text::styled(line, style);
        self.text_lines(&text, width)
    }
}

/// The marker text of each item of `list`, before alignment.
fn list_markers(list: &List) -> Vec<String> {
    let mut next = match list.kind {
        ListKind::Ordered { start } => start,
        _ => 1,
    };
    list.items
        .iter()
        .map(|item| match list.kind {
            ListKind::Bullet => BULLET.to_string(),
            ListKind::Ordered { .. } => {
                let number = item.number.unwrap_or(next);
                next = number.saturating_add(1);
                format!(" {number} ")
            }
            ListKind::Callout => {
                let number = item.number.unwrap_or(next);
                next = number.saturating_add(1);
                format!(" <{number}> ")
            }
        })
        .collect()
}

/// The `Syntax` name for a language as Org and AsciiDoc spell it.
fn lexer_name(language: &str) -> &str {
    match language.to_ascii_lowercase().as_str() {
        "emacs-lisp" | "elisp" | "lisp-interaction" => "lisp",
        "shell" | "sh" | "zsh" | "console" | "shell-session" | "bash" => "bash",
        "js" | "node" => "javascript",
        "ts" => "typescript",
        "py" | "python3" => "python",
        "rb" => "ruby",
        "c++" | "cxx" => "cpp",
        "rs" => "rust",
        "yml" => "yaml",
        _ => language,
    }
}

/// Core Markdown's defaults for the `markdown.*` keys this module reads,
/// for a console whose theme lacks them.
const MARKDOWN_STYLES: &[(&str, &str)] = &[
    ("markdown.em", "italic"),
    ("markdown.strong", "bold"),
    ("markdown.code", "bold cyan on black"),
    ("markdown.block_quote", "magenta"),
    ("markdown.item.bullet", "bold"),
    ("markdown.item.number", "cyan"),
    ("markdown.hr", "dim"),
    ("markdown.h1", "bold underline"),
    ("markdown.h2", "underline magenta"),
    ("markdown.h3", "bold magenta"),
    ("markdown.h4", "italic magenta"),
    ("markdown.h5", "italic"),
    ("markdown.h6", "dim"),
    ("markdown.link", "bright_blue"),
    ("markdown.link_url", "underline blue"),
    ("markdown.s", "strike"),
    ("markdown.table.border", "cyan"),
    ("markdown.table.header", "not bold cyan"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::{AdmonitionKind, Keyword};

    fn plain(document: Document, width: usize) -> String {
        let console = Console::builder().width(width).color_system(None).build();
        console.render_to_string(&DocumentView::new(document))
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(vec![Inline::text(text)])
    }

    #[test]
    fn headings_paragraphs_and_spacing() {
        let document = Document {
            blocks: vec![
                Block::Heading(Heading {
                    level: 1,
                    content: vec![Inline::text("Title")],
                    ..Heading::default()
                }),
                para("One."),
                Block::Heading(Heading {
                    level: 2,
                    content: vec![Inline::text("Part")],
                    keyword: Some(Keyword {
                        text: "TODO".into(),
                        done: false,
                    }),
                    priority: Some("A".into()),
                    tags: vec!["work".into()],
                }),
            ],
        };
        let out = plain(document, 30);
        let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
        assert_eq!(
            lines,
            [
                "            Title",
                "",
                "One.",
                "",
                "TODO [#A] Part  :work:"
            ]
        );
    }

    #[test]
    fn lists_nest_and_number() {
        let nested = Block::List(List {
            kind: ListKind::Ordered { start: 9 },
            items: vec![
                ListItem {
                    blocks: vec![para("nine")],
                    ..ListItem::default()
                },
                ListItem {
                    checkbox: Some(Checkbox::Checked),
                    blocks: vec![para("ten")],
                    ..ListItem::default()
                },
            ],
        });
        let document = Document {
            blocks: vec![Block::List(List {
                kind: ListKind::Bullet,
                items: vec![ListItem {
                    blocks: vec![para("a"), nested],
                    ..ListItem::default()
                }],
            })],
        };
        let out = plain(document, 30);
        let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
        // Numbers right-align in their column, as core's Markdown aligns
        // them.
        assert_eq!(lines, [" • a", "     9 nine", "    10 [x] ten"]);
    }

    #[test]
    fn links_write_their_target_without_hyperlinks() {
        let document = Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::Link {
                    target: "https://example.com".into(),
                    content: vec![Inline::text("site")],
                },
                Inline::text(" and "),
                Inline::Link {
                    target: "https://x.org".into(),
                    content: vec![],
                },
            ])],
        };
        let console = Console::builder().width(60).color_system(None).build();
        let out = console.render_to_string(&DocumentView::new(document).hyperlinks(false));
        assert_eq!(
            out.trim_end(),
            "site (https://example.com) and https://x.org"
        );
    }

    #[test]
    fn admonitions_are_titled_panels() {
        let document = Document {
            blocks: vec![Block::Admonition {
                kind: AdmonitionKind::Warning,
                title: None,
                blocks: vec![para("Careful.")],
            }],
        };
        let out = plain(document, 20);
        assert_eq!(
            out,
            "╭─ Warning ────────╮\n│ Careful.         │\n╰──────────────────╯"
        );
    }

    #[test]
    fn a_banner_spreads_over_the_row() {
        let document = Document {
            blocks: vec![Block::Banner {
                left: "LS(1)".into(),
                center: "User Commands".into(),
                right: "LS(1)".into(),
            }],
        };
        let out = plain(document, 40);
        assert_eq!(out.len(), 40, "{out:?}");
        assert!(out.starts_with("LS(1)") && out.ends_with("LS(1)"));
        assert!(out.contains("User Commands"));
    }

    #[test]
    fn deep_nesting_stays_within_the_width() {
        let mut block = para("deep");
        for _ in 0..40 {
            block = Block::Quote {
                blocks: vec![block],
                attribution: None,
            };
        }
        let out = plain(
            Document {
                blocks: vec![block],
            },
            30,
        );
        assert!(out.contains("deep"), "{out}");
        assert!(out.lines().all(|line| rich::cells::cell_len(line) <= 30));
    }
}
