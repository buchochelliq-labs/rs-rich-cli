//! Markdown rendering.
//!
//! Port of upstream `rich/markdown.py` (core block/inline elements). Parses
//! CommonMark with `pulldown-cmark` and renders each block as justified,
//! full-width lines separated by blank lines.
//!
//! Scope: paragraphs, ATX headings (h1–h6), bullet + ordered lists, block quotes,
//! thematic breaks, fenced/indented **code blocks** (syntax-highlighted via
//! [`Syntax`]), **links** (OSC 8 hyperlinks), inline strong/emphasis/code, and
//! **GFM tables** (rendered via [`Table`]). Inline styling *within* a table cell
//! is a documented follow-up (see the Markdown issue).

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions, Justify};
use crate::protocol::Renderable;
use crate::r#box::SIMPLE;
use crate::segment::Segment;
use crate::style::Style;
use crate::syntax::Syntax;
use crate::table::Table;
use crate::text::Text;

const CODE_STYLE: &str = "bold cyan on black"; // markdown.code
const BULLET: &str = " \u{2022} "; // " • ", markdown.item.bullet = bold
const QUOTE_PREFIX: &str = "\u{258c} "; // "▌ ", markdown.block_quote = magenta
const LINK_STYLE: &str = "underline blue"; // markdown.link_url
const TABLE_BORDER_STYLE: &str = "cyan"; // markdown.table.border
const TABLE_HEADER_STYLE: &str = "not bold cyan"; // markdown.table.header

/// One item of a list. An item is a **container**: it holds whatever blocks it
/// contains — paragraphs, code, tables, quotes, further lists — not a single
/// line of text.
///
/// `number` is `Some` for an ordered list and carries the value to print.
struct ListEntry {
    number: Option<u64>,
    blocks: Vec<Block>,
}

/// An open container while parsing.
///
/// Markdown nests, so parsing it needs a stack. Tracking the open list, quote
/// and paragraph in flat `Option`s meant any nested block overwrote its
/// parent's pending content: a heading inside a list item deleted the item's
/// own text, a nested quote deleted the outer quote, and a code block inside an
/// item was hoisted above the whole list.
enum Frame {
    List {
        ordered: bool,
        start: u64,
        entries: Vec<ListEntry>,
    },
    Item {
        blocks: Vec<Block>,
    },
    Quote {
        blocks: Vec<Block>,
    },
}

/// A parsed Markdown block.
enum Block {
    /// A paragraph or heading (its `Text` carries justify + any heading span).
    Text(Text),
    /// A bullet or ordered list. Each item holds its own blocks, so a nested
    /// list, code block or quote inside an item is simply part of that item.
    List { items: Vec<ListEntry> },
    /// A block quote, holding whatever blocks it contains.
    Quote(Vec<Block>),
    /// A fenced/indented code block, syntax-highlighted via [`Syntax`].
    Code { language: String, code: String },
    /// A thematic break (horizontal rule).
    Rule,
    /// A GFM table: per-column justify (from the alignment row), header cells,
    /// and body rows. Rendered via [`Table`], matching upstream's construction.
    Table {
        alignments: Vec<Justify>,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

/// Accumulates a GFM table across `pulldown-cmark`'s table events.
#[derive(Default)]
struct TableAccum {
    alignments: Vec<Justify>,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    in_head: bool,
    in_cell: bool,
    cur_row: Vec<String>,
    cur_cell: String,
}

fn alignment_justify(alignment: Alignment) -> Justify {
    match alignment {
        Alignment::Right => Justify::Right,
        Alignment::Center => Justify::Center,
        // `None` has no explicit marker; upstream leaves it default (left).
        Alignment::Left | Alignment::None => Justify::Left,
    }
}

/// A rendered Markdown document. Mirrors `rich.markdown.Markdown`.
pub struct Markdown {
    blocks: Vec<Block>,
}

impl Markdown {
    /// Parse CommonMark `source` into renderable blocks.
    pub fn new(source: &str) -> Self {
        Markdown {
            blocks: parse(source),
        }
    }
}

fn heading_level(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// `(base style, justify)` for a heading level (`default_styles.py` +
/// `Heading.LEVEL_ALIGN`).
fn heading_format(level: usize) -> (Style, Justify) {
    let (spec, justify) = match level {
        1 => ("bold underline", Justify::Center),
        2 => ("underline magenta", Justify::Left),
        3 => ("bold magenta", Justify::Left),
        4 => ("italic magenta", Justify::Left),
        5 => ("italic", Justify::Left),
        _ => ("dim", Justify::Left),
    };
    (Style::parse(spec).unwrap_or_default(), justify)
}

fn inline_style(strong: usize, emphasis: usize, strike: usize) -> Option<Style> {
    if strong == 0 && emphasis == 0 && strike == 0 {
        return None;
    }
    let mut style = Style::new();
    if strong > 0 {
        style = style.combine(&Style::parse("bold").expect("valid style"));
    }
    if emphasis > 0 {
        style = style.combine(&Style::parse("italic").expect("valid style"));
    }
    if strike > 0 {
        // `markdown.s` in upstream's default theme.
        style = style.combine(&Style::parse("strike").expect("valid style"));
    }
    Some(style)
}

/// Where a finished block belongs: the innermost open item or quote, else the
/// document. A `List` frame holds entries rather than blocks, so content passes
/// straight through it to the item that owns it.
fn sink<'a>(document: &'a mut Vec<Block>, stack: &'a mut [Frame]) -> &'a mut Vec<Block> {
    match stack
        .iter()
        .rposition(|frame| matches!(frame, Frame::Item { .. } | Frame::Quote { .. }))
    {
        Some(index) => match &mut stack[index] {
            Frame::Item { blocks } | Frame::Quote { blocks } => blocks,
            Frame::List { .. } => unreachable!("rposition matched Item or Quote"),
        },
        None => document,
    }
}

/// How deep containers may nest before further nesting is flattened.
///
/// Rendering recurses once per level, so an unbounded document overflows the
/// stack and takes the process with it: 400 nested block quotes aborted with
/// STATUS_STACK_OVERFLOW, no output, after burning four seconds of CPU.
///
/// Upstream caps this too — markdown-it's `maxNesting` defaults to 20, which is
/// why it renders such a document rather than dying. Content past the cap is
/// kept; it simply stops indenting.
const MAX_NESTING: usize = 20;

/// Commit any pending inline text to the innermost open container.
///
/// A *tight* list item's text arrives as bare `Text` events with no enclosing
/// paragraph, so it sits in `current` until something closes it. Every
/// block-level start must call this first, or it overwrites that text — which
/// silently deleted the item's own content and reordered code blocks ahead of
/// the paragraph introducing them.
fn flush_pending(current: &mut Option<Text>, blocks: &mut Vec<Block>, stack: &mut [Frame]) {
    let Some(mut text) = current.take() else {
        return;
    };
    // A freshly opened item holds an empty buffer; committing it would emit a
    // blank block.
    if text.plain().is_empty() {
        return;
    }
    text.set_justify(Justify::Left);
    sink(blocks, stack).push(Block::Text(text));
}

fn parse(source: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut current: Option<Text> = None;
    let mut heading_style: Option<Style> = None;
    let mut justify = Justify::Left;
    let mut strong = 0usize;
    let mut emphasis = 0usize;
    let mut strike = 0usize;
    // Depth of single-tilde spans currently open; their delimiters are re-emitted
    // as literal text so the run is not styled.
    let mut single_tilde = 0usize;
    // Open containers, innermost last. Markdown nests, so this has to be a
    // stack: with flat slots, any nested block overwrote its parent's pending
    // content and the parent then emitted nothing.
    let mut stack: Vec<Frame> = Vec::new();
    // Containers past MAX_NESTING are not pushed; these count them so the
    // matching End events unwind symmetrically and the stack stays balanced.
    let mut suppressed = 0usize;
    let mut item_suppressed = 0usize;
    // (language, accumulated source) while inside a code block.
    let mut code: Option<(String, String)> = None;
    // The destination URL while inside a link.
    let mut link: Option<String> = None;
    // The table being assembled while inside a GFM table.
    let mut table: Option<TableAccum> = None;

    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    // Offsets, not just events: pulldown-cmark accepts a *single* tilde as a
    // strikethrough delimiter, while upstream's markdown-it requires two. Prose
    // like `costs ~5~10` was silently restyled and its tildes deleted. The
    // source range is the only way to tell `~x~` from `~~x~~` after parsing.
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Rule => {
                flush_pending(&mut current, &mut blocks, &mut stack);
                sink(&mut blocks, &mut stack).push(Block::Rule);
            }
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => link = None,
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_pending(&mut current, &mut blocks, &mut stack);
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        // The info string is `lang` (possibly with extra tokens).
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((language, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, mut source)) = code.take() {
                    // Drop the single trailing newline the parser appends.
                    if source.ends_with('\n') {
                        source.pop();
                    }
                    sink(&mut blocks, &mut stack).push(Block::Code {
                        language,
                        code: source,
                    });
                }
            }
            Event::Start(Tag::Table(aligns)) => {
                flush_pending(&mut current, &mut blocks, &mut stack);
                table = Some(TableAccum {
                    alignments: aligns.into_iter().map(alignment_justify).collect(),
                    ..TableAccum::default()
                });
            }
            Event::End(TagEnd::Table) => {
                if let Some(acc) = table.take() {
                    sink(&mut blocks, &mut stack).push(Block::Table {
                        alignments: acc.alignments,
                        headers: acc.headers,
                        rows: acc.rows,
                    });
                }
            }
            Event::Start(Tag::TableHead) => {
                if let Some(acc) = table.as_mut() {
                    acc.in_head = true;
                    acc.cur_row = Vec::new();
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(acc) = table.as_mut() {
                    acc.headers = std::mem::take(&mut acc.cur_row);
                    acc.in_head = false;
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(acc) = table.as_mut() {
                    acc.cur_row = Vec::new();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(acc) = table.as_mut() {
                    let row = std::mem::take(&mut acc.cur_row);
                    acc.rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(acc) = table.as_mut() {
                    acc.in_cell = true;
                    acc.cur_cell = String::new();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(acc) = table.as_mut() {
                    let cell = std::mem::take(&mut acc.cur_cell);
                    acc.cur_row.push(cell);
                    acc.in_cell = false;
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush_pending(&mut current, &mut blocks, &mut stack);
                if stack.len() >= MAX_NESTING {
                    suppressed += 1;
                } else {
                    stack.push(Frame::Quote { blocks: Vec::new() });
                }
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                if suppressed > 0 {
                    suppressed -= 1;
                } else if let Some(Frame::Quote { blocks: quoted }) = stack.pop() {
                    sink(&mut blocks, &mut stack).push(Block::Quote(quoted));
                }
            }
            Event::Start(Tag::List(first)) => {
                flush_pending(&mut current, &mut blocks, &mut stack);
                if stack.len() >= MAX_NESTING {
                    suppressed += 1;
                } else {
                    stack.push(Frame::List {
                        ordered: first.is_some(),
                        start: first.unwrap_or(1),
                        entries: Vec::new(),
                    });
                }
            }
            Event::End(TagEnd::List(_)) => {
                if suppressed > 0 {
                    suppressed -= 1;
                } else if let Some(Frame::List { entries, .. }) = stack.pop() {
                    sink(&mut blocks, &mut stack).push(Block::List { items: entries });
                }
            }
            Event::Start(Tag::Item) => {
                if stack.len() >= MAX_NESTING {
                    item_suppressed += 1;
                } else {
                    stack.push(Frame::Item { blocks: Vec::new() });
                }
                // A *tight* list emits its item text as bare `Text` events with
                // no enclosing Paragraph, so open a buffer here for it to land
                // in. A loose item simply resets this at its Start(Paragraph).
                current = Some(Text::new(""));
                heading_style = None;
                justify = Justify::Left;
            }
            Event::End(TagEnd::Item) => {
                // A *tight* list emits its item text without a Paragraph, so
                // anything still pending belongs to this item.
                if let Some(mut text) = current.take() {
                    text.set_justify(Justify::Left);
                    sink(&mut blocks, &mut stack).push(Block::Text(text));
                }
                if item_suppressed > 0 {
                    item_suppressed -= 1;
                } else if let Some(Frame::Item {
                    blocks: item_blocks,
                }) = stack.pop()
                {
                    if let Some(Frame::List {
                        ordered,
                        start,
                        entries,
                    }) = stack.last_mut()
                    {
                        let number = ordered.then(|| *start + entries.len() as u64);
                        entries.push(ListEntry {
                            number,
                            blocks: item_blocks,
                        });
                    }
                }
            }
            Event::Start(Tag::Paragraph) => {
                flush_pending(&mut current, &mut blocks, &mut stack);
                current = Some(Text::new(""));
                heading_style = None;
                justify = Justify::Left;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush_pending(&mut current, &mut blocks, &mut stack);
                let (style, heading_justify) = heading_format(heading_level(level));
                current = Some(Text::new(""));
                heading_style = Some(style);
                justify = heading_justify;
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Heading(_)) => {
                if let Some(mut text) = current.take() {
                    let in_quote = stack
                        .iter()
                        .rposition(|f| matches!(f, Frame::Item { .. } | Frame::Quote { .. }))
                        .is_some_and(|i| matches!(stack[i], Frame::Quote { .. }));
                    if in_quote {
                        // Quote paragraph: magenta base so its padding is magenta too.
                        text.set_base_style(Style::parse("magenta").expect("valid style"));
                        // A heading inside a quote keeps its own style and
                        // alignment on top of that base; treating everything in
                        // a quote as body text flattened h1 to plain magenta and
                        // left-aligned it.
                        if let Some(style) = &heading_style {
                            let end = text.plain().len();
                            text.stylize(style.clone(), 0, end);
                            text.set_justify(justify);
                        } else {
                            text.set_justify(Justify::Left);
                        }
                    } else {
                        // A heading's style is a SPAN over the text, not a base
                        // style: a base style would paint the centring padding
                        // too, which upstream leaves unstyled.
                        if let Some(style) = &heading_style {
                            let end = text.plain().len();
                            text.stylize(style.clone(), 0, end);
                        }
                        text.set_justify(justify);
                    }
                    sink(&mut blocks, &mut stack).push(Block::Text(text));
                }
                heading_style = None;
                justify = Justify::Left;
                strong = 0;
                emphasis = 0;
            }
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
            Event::Start(Tag::Strikethrough) => {
                if source[range.clone()].starts_with("~~") {
                    strike += 1;
                } else {
                    // Single-tilde: not a delimiter upstream. Keep the literal
                    // text, tildes and all.
                    single_tilde += 1;
                    let block = current.get_or_insert_with(|| Text::new(""));
                    block.append("~", None);
                }
            }
            Event::End(TagEnd::Strikethrough) => {
                if single_tilde > 0 {
                    single_tilde -= 1;
                    let block = current.get_or_insert_with(|| Text::new(""));
                    block.append("~", None);
                } else {
                    strike = strike.saturating_sub(1);
                }
            }
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis = emphasis.saturating_sub(1),
            Event::Text(text) => {
                if let Some(acc) = table.as_mut().filter(|a| a.in_cell) {
                    // Table cells collect plain text; inline styling within a cell
                    // is a documented follow-up (see the Markdown issue).
                    acc.cur_cell.push_str(&text);
                } else if let Some((_, source)) = code.as_mut() {
                    source.push_str(&text);
                } else {
                    // Open a buffer if none is active. In a tight list item the
                    // text after a nested block arrives bare, with the previous
                    // buffer already flushed by that block's start — matching
                    // on `as_mut()` here silently dropped it.
                    let block = current.get_or_insert_with(|| Text::new(""));
                    // Inside a link, use the markdown.link_url style + an OSC 8
                    // hyperlink; otherwise the inline strong/emphasis style.
                    let style = match &link {
                        Some(url) => Style::parse(LINK_STYLE)
                            .ok()
                            .map(|s| s.with_link(url.clone())),
                        None => inline_style(strong, emphasis, strike),
                    };
                    block.append(&text, style.map(Into::into));
                }
            }
            Event::Code(text) => {
                if let Some(acc) = table.as_mut().filter(|a| a.in_cell) {
                    acc.cur_cell.push_str(&text);
                } else {
                    // Open a buffer if none is active. In a tight list item the
                    // text after a nested block arrives bare, with the previous
                    // buffer already flushed by that block's start — matching
                    // on `as_mut()` here silently dropped it.
                    let block = current.get_or_insert_with(|| Text::new(""));
                    block.append(&text, Style::parse(CODE_STYLE).ok().map(Into::into));
                }
            }
            Event::SoftBreak => {
                if let Some(block) = current.as_mut() {
                    block.append(" ", None);
                }
            }
            Event::HardBreak => {
                if let Some(block) = current.as_mut() {
                    block.append("\n", None);
                }
            }
            _ => {}
        }
    }
    blocks
}

impl Renderable for Markdown {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut lines = render_blocks(&self.blocks, console, options, options.max_width, true);

        // Upstream's thematic-break element emits a trailing line break, which is
        // only observable when the rule is the document's last block: it adds one
        // extra blank line there (a mid-document rule merges with the normal block
        // separator). Match that.
        if matches!(self.blocks.last(), Some(Block::Rule)) {
            lines.push(Vec::new());
        }

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
}

/// Pad every row out to `width`, as upstream's `console.render_lines` does —
/// `pad=True` is its default, and both the list-item and block-quote handlers
/// rely on it.
///
/// Without this a child rendered in a narrower box hands back short rows and
/// every enclosing level inherits the shortfall, so nesting lost two cells per
/// level: quotes measured 68, 66, 64, 62 at depths 1–4 where upstream holds a
/// flat 68.
fn pad_lines(lines: &mut [Vec<Segment>], width: usize) {
    for line in lines.iter_mut() {
        let len: usize = line.iter().map(Segment::cell_length).sum();
        if len < width {
            line.push(Segment::new(" ".repeat(width - len), None));
        }
    }
}

/// Render a run of blocks into rows of segments at `width`.
///
/// Recursive, because a list item and a quote are containers: whatever they
/// hold is rendered by this same function at a reduced width and then prefixed.
fn render_blocks(
    blocks: &[Block],
    console: &Console,
    options: &ConsoleOptions,
    width: usize,
    top_level: bool,
) -> Vec<Vec<Segment>> {
    let base = console.base_style();
    let mut lines: Vec<Vec<Segment>> = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        // A blank line precedes every non-first block, and every
        // list/quote/table (which upstream renders with a leading gap).
        // Blank lines between blocks are a *document* convention. Upstream puts
        // none inside a list item or a quote — neither before a nested list nor
        // between two paragraphs of one item — so applying the rule there added
        // a stray row per block, and one per level of nesting.
        // A rule brings its own trailing blank, so the usual gap after it would
        // double up (upstream sets `HorizontalRule.new_line = False` for exactly
        // this reason).
        let after_rule = index > 0 && matches!(blocks[index - 1], Block::Rule);
        // A list, quote or table carries its own leading gap, which survives even
        // after a rule; only the generic inter-block separator is suppressed.
        let own_gap = matches!(
            block,
            Block::List { .. } | Block::Quote(_) | Block::Table { .. }
        );
        if top_level && (own_gap || (index > 0 && !after_rule)) {
            lines.push(Vec::new());
        }
        match block {
            Block::Text(text) => {
                lines.extend(text.render_lines(console.theme(), base, Some(width)))
            }
            Block::List { items } => {
                for item in items {
                    let (prefix, prefix_style) = match item.number {
                        Some(number) => (
                            format!(" {number} "),
                            Style::parse("cyan").expect("valid style"),
                        ),
                        None => (
                            BULLET.to_string(),
                            Style::parse("bold").expect("valid style"),
                        ),
                    };
                    let prefix_width = cell_len(&prefix);
                    // The item's own blocks, rendered in the space left beside
                    // its marker. A nested list is just one of those blocks, so
                    // indentation compounds naturally.
                    let item_lines = render_blocks(
                        &item.blocks,
                        console,
                        options,
                        width.saturating_sub(prefix_width),
                        false,
                    );
                    // A leading blank row would push the marker off its content.
                    let mut item_lines: Vec<Vec<Segment>> = item_lines
                        .into_iter()
                        .skip_while(|line| line.is_empty())
                        .collect();
                    pad_lines(&mut item_lines, width.saturating_sub(prefix_width));
                    for (line_index, line) in item_lines.into_iter().enumerate() {
                        let mut row = Vec::new();
                        if line_index == 0 {
                            row.push(Segment::new(prefix.clone(), Some(prefix_style.clone())));
                        } else {
                            row.push(Segment::new(" ".repeat(prefix_width), None));
                        }
                        row.extend(line);
                        lines.push(row);
                    }
                }
            }
            Block::Quote(quoted) => {
                let prefix_style = Style::parse("magenta").expect("valid style");
                // Upstream renders quote content at `max_width - 4`.
                let content_width = width.saturating_sub(4);
                let quoted_lines = render_blocks(quoted, console, options, content_width, false);
                let mut quoted_lines: Vec<Vec<Segment>> = quoted_lines
                    .into_iter()
                    .skip_while(|line| line.is_empty())
                    .collect();
                pad_lines(&mut quoted_lines, content_width);
                for line in quoted_lines {
                    let mut row = vec![Segment::new(
                        QUOTE_PREFIX.to_string(),
                        Some(prefix_style.clone()),
                    )];
                    // Upstream passes `style=self.style` to `render_lines`, so
                    // the quote colour reaches *every* child — including a list
                    // or table, which set their own styles and so previously
                    // rendered inside a quote with no magenta at all.
                    row.extend(Segment::apply_style(&line, &prefix_style));
                    lines.push(row);
                }
            }
            Block::Code { language, code } => {
                // Render the code block via the Syntax renderable (functional,
                // not byte-parity — see DIVERGENCES). Split its segment stream
                // back into per-line rows for the shared join below.
                // Upstream: `Syntax(code, lexer, theme=..., word_wrap=True, padding=1)`.
                // Upstream: `Syntax(code, lexer, theme=..., word_wrap=True, padding=1)`.
                // Without word_wrap a long line was cropped dead at the console
                // width and its tail discarded entirely — a README's install
                // command lost half its flags, with no marker that anything went.
                let syntax = Syntax::new(code.as_str(), language.as_str())
                    .word_wrap(true)
                    .padding(1);
                let inner = options.update_width(width);
                let segments = syntax.rich_render(console, &inner);
                lines.extend(Segment::split_lines(&segments));
            }
            Block::Rule => {
                let style = Style::parse("dim").expect("valid style");
                lines.push(vec![Segment::new("-".repeat(width), Some(style))]);
                // Upstream's rule carries a trailing blank row of its own, in
                // place of the usual inter-block gap (`HorizontalRule.new_line
                // = False`). Inside a quote that row picks up the quote prefix,
                // which is why upstream shows a bare `▌` line under a quoted
                // rule and we showed none.
                //
                // At the very end of a document the trailing break already
                // arrives from the join below — the `markdown_hr_end` golden
                // pins it — so adding one here would double it.
                if index + 1 < blocks.len() || !top_level {
                    lines.push(Vec::new());
                }
            }
            Block::Table {
                alignments,
                headers,
                rows,
            } => {
                // Build the Table exactly as upstream's TableElement does:
                // box=SIMPLE, pad_edge=False, collapse_padding=True, and the
                // markdown.table.border/header styles. Per-column justify comes
                // from the alignment row.
                let mut table = Table::new()
                    .box_set(SIMPLE)
                    .pad_edge(false)
                    .collapse_padding(true)
                    .style(Style::parse(TABLE_BORDER_STYLE).expect("valid style"));
                let header_style = Style::parse(TABLE_HEADER_STYLE).expect("valid style");
                for (col, header) in headers.iter().enumerate() {
                    let justify = alignments.get(col).copied().unwrap_or(Justify::Left);
                    table.add_column_justify(header.as_str(), justify);
                    table.column_header_style(header_style.clone());
                }
                for row in rows {
                    let refs: Vec<&str> = row.iter().map(String::as_str).collect();
                    table.add_row(&refs);
                }
                let inner = options.update_width(width);
                lines.extend(Segment::split_lines(&table.rich_render(console, &inner)));
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(source: &str) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        console.render_to_string(&Markdown::new(source))
    }

    #[test]
    fn paragraph_inline_styles() {
        assert_eq!(
            render("a `x` b"),
            "a \x1b[1;36;40mx\x1b[0m b               "
        );
    }

    #[test]
    fn link_renders_osc8_hyperlink() {
        // Matches real rich 15.0.0 exactly except upstream's random `id=` field,
        // which we omit for determinism (DIVERGENCES). markdown.link_url styling
        // is "underline blue" (4;34).
        let out = render("See [the site](https://example.com) now.");
        assert!(
            out.contains(
                "\x1b]8;;https://example.com\x1b\\\x1b[4;34mthe site\x1b[0m\x1b]8;;\x1b\\"
            ),
            "got {out:?}"
        );
        assert!(!out.contains("id="), "we omit the random link id");
    }

    #[test]
    fn fenced_code_block_is_highlighted() {
        // Functional (not byte-parity): the fenced code renders via Syntax, so
        // its text survives and it's colored.
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(24)
            .no_color(false)
            .build();
        let out = console.render_to_string(&Markdown::new("```rust\nfn main() {}\n```"));
        assert!(out.contains("fn"), "got {out:?}");
        assert!(out.contains("main"));
        assert!(out.contains('\x1b'), "code block should be colored");
    }

    #[test]
    fn headings() {
        assert_eq!(render("# Head"), "        \x1b[1;4mHead\x1b[0m        ");
        assert_eq!(render("## Sub"), "\x1b[4;35mSub\x1b[0m                 ");
    }

    #[test]
    fn two_paragraphs_separated_by_blank_line() {
        assert_eq!(
            render("First para.\n\nSecond para."),
            "First para.         \n\nSecond para.        "
        );
    }

    #[test]
    fn bullet_list() {
        assert_eq!(
            render("- one\n- two"),
            "\n\x1b[1m \u{2022} \x1b[0mone              \n\x1b[1m \u{2022} \x1b[0mtwo              "
        );
    }

    #[test]
    fn ordered_list() {
        assert_eq!(
            render("1. first\n2. second"),
            "\n\x1b[36m 1 \x1b[0mfirst            \n\x1b[36m 2 \x1b[0msecond           "
        );
    }

    #[test]
    fn block_quote() {
        assert_eq!(
            render("> quoted text"),
            "\n\x1b[35m\u{258c} \x1b[0m\x1b[35mquoted text\x1b[0m\x1b[35m     \x1b[0m"
        );
    }

    #[test]
    fn gfm_table() {
        // Byte-parity is guaranteed by the `markdown_table` golden; this guards
        // the parser wiring (tables enabled, cells + alignment collected).
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .no_color(false)
            .build();
        let md = "| Name | Age |\n| :--- | ---: |\n| Alice | 30 |\n| Bob | 7 |\n";
        let out = console.render_to_string(&Markdown::new(md));
        assert!(out.contains("Name"), "header present: {out:?}");
        assert!(out.contains("Alice"), "body cell present");
        assert!(out.contains('\u{2500}'), "SIMPLE box head rule present");
        // Right-justified Age column: "30" padded on the left, "7" further.
        assert!(out.contains(" 30"), "right-justified 30");
        assert!(out.contains("  7"), "right-justified 7");
    }

    #[test]
    fn thematic_break() {
        assert_eq!(
            render("a\n\n---\n\nb"),
            "a                   \n\n\x1b[2m--------------------\x1b[0m\n\nb                   "
        );
    }

    #[test]
    fn thematic_break_at_end_adds_trailing_blank() {
        // A document ending with a rule emits one extra trailing blank line
        // (upstream's hr element yields a trailing break). Byte-parity is
        // guaranteed by the `markdown_hr_end` golden; here we assert the shape.
        assert_eq!(
            render("a\n\n---"),
            "a                   \n\n\x1b[2m--------------------\x1b[0m\n"
        );
    }
}

#[cfg(test)]
mod container_tests {
    use super::*;

    fn plain(source: &str, width: usize) -> String {
        let console = Console::builder().width(width).no_color(true).build();
        console.render_to_string(&Markdown::new(source))
    }

    /// Every case here lost content before parsing used a container stack: the
    /// open list, quote and paragraph lived in flat `Option`s, so a nested block
    /// overwrote its parent's pending text and the parent emitted nothing.
    fn assert_all_present(source: &str, expected: &[&str]) {
        let out = plain(source, 44);
        for item in expected {
            assert!(out.contains(item), "{item:?} missing from:\n{out}");
        }
    }

    #[test]
    fn a_nested_list_keeps_every_item() {
        assert_all_present("- one\n- two\n  - nested\n", &["one", "two", "nested"]);
    }

    #[test]
    fn nesting_three_deep_keeps_every_item() {
        assert_all_present("- top\n  - mid\n    - deep\n", &["top", "mid", "deep"]);
    }

    #[test]
    fn an_item_following_a_sublist_keeps_its_place() {
        let out = plain("- one\n  - nested\n- two\n", 44);
        let (a, b, c) = (
            out.find("one").expect("one"),
            out.find("nested").expect("nested"),
            out.find("two").expect("two"),
        );
        assert!(a < b && b < c, "order was wrong:\n{out}");
    }

    #[test]
    fn each_level_of_an_ordered_list_numbers_independently() {
        let out = plain("1. first\n2. second\n   1. sub\n", 44);
        for expected in ["1 first", "2 second", "1 sub"] {
            assert!(out.contains(expected), "expected {expected:?} in:\n{out}");
        }
    }

    #[test]
    fn nested_items_are_indented_under_their_parent() {
        let out = plain("- top\n  - child\n", 44);
        let indent = |needle: &str| {
            let line = out.lines().find(|l| l.contains(needle)).expect(needle);
            line.len() - line.trim_start().len()
        };
        assert!(indent("child") > indent("top"), "not indented:\n{out}");
    }

    /// A heading inside a list item used to delete the item's own text and take
    /// its place in the list.
    #[test]
    fn a_heading_inside_an_item_keeps_the_item_text() {
        assert_all_present(
            "- ITEMTEXT\n\n  ## HEADTEXT\n\n- NEXTTEXT\n",
            &["ITEMTEXT", "HEADTEXT", "NEXTTEXT"],
        );
    }

    /// A code block inside an item used to be hoisted above the whole list, so
    /// the code appeared before the text introducing it.
    #[test]
    fn a_code_block_inside_an_item_stays_in_the_item() {
        let out = plain("- FIRSTITEM\n\n  ```\n  CODETEXT\n  ```\n", 44);
        let (item, code) = (
            out.find("FIRSTITEM").expect("item"),
            out.find("CODETEXT").expect("code"),
        );
        assert!(item < code, "the code was hoisted above its item:\n{out}");
    }

    /// A second paragraph used to be fused onto the first with no separator.
    #[test]
    fn two_paragraphs_in_one_item_stay_separate() {
        let out = plain("- AAA\n\n  BBB\n", 44);
        assert!(!out.contains("AAABBB"), "paragraphs were fused:\n{out}");
        assert!(out.contains("AAA") && out.contains("BBB"), "{out}");
    }

    /// A nested quote used to delete the outer quote's text entirely.
    #[test]
    fn a_nested_quote_keeps_the_outer_text() {
        assert_all_present(
            "> OUTERTEXT\n>\n> > INNERTEXT\n",
            &["OUTERTEXT", "INNERTEXT"],
        );
    }

    /// A list inside a quote used to be reordered ahead of the quote's own text
    /// and to lose the quote bar.
    #[test]
    fn a_list_inside_a_quote_stays_quoted_and_in_order() {
        let out = plain("> intro\n>\n> - item one\n> - item two\n", 44);
        for line in out
            .lines()
            .filter(|l| l.contains("item one") || l.contains("intro"))
        {
            assert!(
                line.trim_start().starts_with(QUOTE_PREFIX.trim_end()),
                "lost the quote bar: {line:?}\n{out}"
            );
        }
        let (intro, one) = (
            out.find("intro").expect("intro"),
            out.find("item one").expect("item one"),
        );
        assert!(intro < one, "quote content was reordered:\n{out}");
    }

    #[test]
    fn a_quote_inside_an_item_stays_inside_it() {
        let out = plain("- alpha\n\n  > quoted\n", 44);
        assert!(!out.contains("alphaquoted"), "fused:\n{out}");
        let quoted = out.lines().find(|l| l.contains("quoted")).expect("quoted");
        assert!(
            quoted.contains(QUOTE_PREFIX.trim_end()),
            "lost the quote bar:\n{out}"
        );
    }

    /// In a *tight* list the item's text arrives as bare `Text` events, so any
    /// block-level start used to overwrite it: the item's own content vanished
    /// and the block took its place.
    #[test]
    fn a_tight_item_keeps_its_text_before_a_heading() {
        assert_all_present(
            "- P1_text\n  ## H1_head\n- P2_text\n",
            &["P1_text", "H1_head", "P2_text"],
        );
    }

    #[test]
    fn a_tight_item_keeps_its_text_before_a_quote() {
        assert_all_present("- Q1_text\n  > Q1_quote\n", &["Q1_text", "Q1_quote"]);
    }

    #[test]
    fn a_tight_ordered_item_keeps_its_text_before_a_quote() {
        assert_all_present("1. C_num_text\n   > C_quote\n", &["C_num_text", "C_quote"]);
    }

    #[test]
    fn a_nested_tight_item_keeps_its_text_before_a_heading() {
        assert_all_present(
            "- A\n  - B_inner\n    ## B_head\n",
            &["A", "B_inner", "B_head"],
        );
    }

    /// A fenced block tight after the item's text used to render *before* it —
    /// #69 stopped hoisting it above the whole list, but it still overtook the
    /// paragraph that introduced it.
    #[test]
    fn a_tight_code_block_renders_after_the_text_that_introduces_it() {
        let out = plain("- F1_text\n  ```\n  F1_code\n  ```\n- F2_text\n", 55);
        let (text, code) = (
            out.find("F1_text").expect("F1_text"),
            out.find("F1_code").expect("F1_code"),
        );
        assert!(text < code, "the code block overtook its paragraph:\n{out}");
    }

    /// Rendering recurses once per nesting level, so an unbounded document
    /// overflowed the stack and killed the process: 400 nested quotes aborted
    /// with STATUS_STACK_OVERFLOW after four seconds, no output at all.
    #[test]
    fn deeply_nested_input_does_not_overflow_the_stack() {
        for depth in [50usize, 400, 2000] {
            let quotes = ">".repeat(depth) + " x\n";
            let _ = plain(&quotes, 80);

            let list: String = (0..depth)
                .map(|i| format!("{}- L{i}\n", "  ".repeat(i)))
                .collect();
            let _ = plain(&list, 80);
        }
        // Reaching here without aborting is the assertion.
    }

    /// Text after a nested block inside a tight item arrives as a bare `Text`
    /// event with no buffer open — the previous one having been flushed by that
    /// block's start — and was silently dropped at exit 0.
    #[test]
    fn a_tight_item_keeps_text_that_follows_a_nested_block() {
        assert_all_present(
            "- ITEM\n  ```\n  FIRST code\n  ```\n  SECOND para\n",
            &["ITEM", "FIRST code", "SECOND para"],
        );
        assert_all_present(
            "- ITEM\n  ## HEAD\n  TAIL para\n",
            &["ITEM", "HEAD", "TAIL para"],
        );
        assert_all_present("- ITEM\n  ---\n  TAIL para\n", &["ITEM", "TAIL para"]);
    }

    /// A heading inside a quote was flattened to body text: it lost its own
    /// style and its centring, keeping only the quote's magenta.
    #[test]
    fn a_heading_inside_a_quote_keeps_its_alignment() {
        let out = plain("> # Heading in quote\n", 50);
        let line = out
            .lines()
            .find(|l| l.contains("Heading in quote"))
            .expect("heading line");
        // Centred: the text does not start immediately after the quote bar.
        let after_bar = line.split(QUOTE_PREFIX.trim_end()).nth(1).expect("bar");
        assert!(
            after_bar.starts_with("  "),
            "heading was left-aligned inside the quote: {line:?}"
        );
    }

    /// Upstream enables strikethrough explicitly; without the parser option the
    /// tilde markers leaked into the output and widened table columns.
    #[test]
    fn strikethrough_is_rendered_rather_than_leaked() {
        let out = plain("~~Deprecated~~ text\n", 50);
        assert!(!out.contains("~~"), "tildes leaked into output: {out:?}");
        assert!(out.contains("Deprecated"), "content lost: {out:?}");
    }

    /// Blank lines between blocks are a document convention. Applying them
    /// inside a container added a stray row per block and per nesting level —
    /// upstream emits none there.
    #[test]
    fn nested_blocks_gain_no_phantom_blank_row() {
        let out = plain("- a\n  - b\n  - c\n- d\n", 50);
        let rows: Vec<&str> = out
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(
            rows.len(),
            4,
            "expected exactly four content rows, got {rows:?}"
        );
    }

    /// Upstream's `render_lines` pads a child back to the width it was handed
    /// (`pad=True`). We never padded, so every nesting level inherited the
    /// shortfall: quote rows measured 68, 66, 64, 62 at depths 1–4 where
    /// upstream holds a flat 68.
    #[test]
    fn nesting_does_not_narrow_each_level() {
        let source = "> d1\n\n>> d2\n\n>>> d3\n\n>>>> d4\n";
        let out = plain(source, 70);
        let widths: Vec<usize> = out
            .lines()
            .filter(|l| {
                l.contains("d1") || l.contains("d2") || l.contains("d3") || l.contains("d4")
            })
            .map(|l| l.chars().count())
            .collect();
        assert_eq!(widths.len(), 4, "expected one row per depth: {widths:?}");
        assert!(
            widths.iter().all(|w| *w == widths[0]),
            "each nesting level lost width: {widths:?}"
        );
    }

    /// pulldown-cmark accepts a single tilde as a strikethrough delimiter;
    /// upstream's markdown-it requires two, so `~struck~` had its tildes deleted
    /// and its content restyled where upstream leaves the text alone.
    #[test]
    fn a_single_tilde_is_literal_text() {
        let out = plain("a ~struck~ b and ~~gone~~ here", 60);
        assert!(
            out.contains("~struck~"),
            "single tildes were eaten: {out:?}"
        );
        assert!(!out.contains("~~gone~~"), "double tildes leaked: {out:?}");
        assert!(out.contains("gone"), "struck content lost: {out:?}");
    }

    /// Upstream renders a fenced block as `Syntax(..., padding=1)`: a blank
    /// inset row above and below and a one-column gutter. Without it the code
    /// sat flush against the surrounding text.
    #[test]
    fn a_code_block_is_inset_by_one_cell() {
        let out = plain("intro para\n\n```\nCODEWORD\n```\n", 40);
        let rows: Vec<&str> = out.lines().collect();
        let index = rows
            .iter()
            .position(|r| r.contains("CODEWORD"))
            .expect("code row present");
        assert!(
            rows[index].starts_with(' '),
            "no left gutter on the code row: {:?}",
            rows[index]
        );
        assert!(
            rows[index - 1].trim().is_empty(),
            "no blank inset row above the code: {:?}",
            rows[index - 1]
        );
        assert!(
            rows.get(index + 1).is_some_and(|r| r.trim().is_empty()),
            "no blank inset row below the code"
        );
    }

    /// A rule carries its own trailing blank in place of the usual inter-block
    /// gap, so a block after it is separated by exactly one blank row — not two,
    /// and not none.
    #[test]
    fn a_rule_is_followed_by_exactly_one_blank_row() {
        let out = plain("before\n\n---\n\nafter\n", 40);
        let rows: Vec<&str> = out.lines().collect();
        let rule = rows
            .iter()
            .position(|r| r.trim_end().ends_with('-') && r.trim().len() > 3)
            .expect("rule row present");
        let after = rows
            .iter()
            .position(|r| r.contains("after"))
            .expect("following row present");
        assert_eq!(
            after - rule,
            2,
            "expected one blank row between rule and next block: {rows:?}"
        );
    }

    /// Markdown code blocks are `Syntax(..., word_wrap=True)` upstream. Without
    /// it a long line was cropped dead at the console width and its tail
    /// discarded — a README's install command lost half its flags, silently.
    #[test]
    fn a_long_code_line_keeps_its_tail() {
        let source = "```bash\npip install some-package another-package \
yet-another-package --upgrade --no-cache-dir\n```\n";
        let out = plain(source, 80);
        assert!(
            out.contains("no-cache-dir"),
            "the tail of the code line was discarded: {out:?}"
        );
    }
}
