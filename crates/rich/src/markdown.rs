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

/// One entry in a (possibly nested) list.
///
/// `depth` is the nesting level, 0 for the outermost. `number` is `Some` for an
/// ordered list and carries the value to print, resolved when the item is
/// collected so each nesting level keeps its own sequence.
struct ListItem {
    depth: usize,
    number: Option<u64>,
    text: Text,
}

/// A list under construction: its own ordered-ness and start, plus the items
/// collected so far (including those merged up from nested lists).
struct ListFrame {
    ordered: bool,
    start: u64,
    items: Vec<ListItem>,
}

impl ListFrame {
    /// Collect `text` as an item of this frame at `depth`.
    ///
    /// The item's number counts only items at this depth, so entries merged up
    /// from a nested list do not advance the parent's numbering.
    fn push_item(&mut self, depth: usize, text: Text) {
        let index = self.items.iter().filter(|item| item.depth == depth).count() as u64;
        let number = self.ordered.then(|| self.start + index);
        self.items.push(ListItem {
            depth,
            number,
            text,
        });
    }
}

/// A parsed Markdown block.
enum Block {
    /// A paragraph or heading (its `Text` carries justify + any heading span).
    Text(Text),
    /// A bullet or ordered list, flattened: every item carries its own nesting
    /// depth and marker, so a nested list is not a separate block.
    List { items: Vec<ListItem> },
    /// A block quote; each paragraph is a magenta, left-justified `Text`.
    Quote(Vec<Text>),
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

fn inline_style(strong: usize, emphasis: usize) -> Option<Style> {
    if strong == 0 && emphasis == 0 {
        return None;
    }
    let mut style = Style::new();
    if strong > 0 {
        style = style.combine(&Style::parse("bold").expect("valid style"));
    }
    if emphasis > 0 {
        style = style.combine(&Style::parse("italic").expect("valid style"));
    }
    Some(style)
}

fn parse(source: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut current: Option<Text> = None;
    let mut heading_style: Option<Style> = None;
    let mut justify = Justify::Left;
    let mut strong = 0usize;
    let mut emphasis = 0usize;
    // (ordered, start_number, items) while inside a list.
    // A STACK, not a single slot. A nested list starts while its parent is
    // still open, so one Option was overwritten by the child: the parent's
    // collected items were discarded and the parent's End(List) then found
    // nothing to emit. A three-item list with one nested entry rendered as
    // just the nested entry, silently deleting the other two and promoting
    // the survivor to top level.
    let mut lists: Vec<ListFrame> = Vec::new();
    // Collected quote paragraphs while inside a block quote.
    let mut quote: Option<Vec<Text>> = None;
    // (language, accumulated source) while inside a code block.
    let mut code: Option<(String, String)> = None;
    // The destination URL while inside a link.
    let mut link: Option<String> = None;
    // The table being assembled while inside a GFM table.
    let mut table: Option<TableAccum> = None;

    for event in Parser::new_ext(source, Options::ENABLE_TABLES) {
        match event {
            Event::Rule => blocks.push(Block::Rule),
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => link = None,
            Event::Start(Tag::CodeBlock(kind)) => {
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
                    blocks.push(Block::Code {
                        language,
                        code: source,
                    });
                }
            }
            Event::Start(Tag::Table(aligns)) => {
                table = Some(TableAccum {
                    alignments: aligns.into_iter().map(alignment_justify).collect(),
                    ..TableAccum::default()
                });
            }
            Event::End(TagEnd::Table) => {
                if let Some(acc) = table.take() {
                    blocks.push(Block::Table {
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
            Event::Start(Tag::BlockQuote(_)) => quote = Some(Vec::new()),
            Event::End(TagEnd::BlockQuote(_)) => {
                if let Some(paragraphs) = quote.take() {
                    blocks.push(Block::Quote(paragraphs));
                }
            }
            Event::Start(Tag::List(first)) => {
                // The parent item's text is complete before its sublist starts,
                // but its End(Item) does not arrive until after the sublist
                // closes. Flush it now so ordering survives.
                if let (Some(text), Some(_)) = (current.take(), lists.last()) {
                    let depth = lists.len() - 1;
                    if let Some(frame) = lists.last_mut() {
                        frame.push_item(depth, text);
                    }
                }
                lists.push(ListFrame {
                    ordered: first.is_some(),
                    start: first.unwrap_or(1),
                    items: Vec::new(),
                });
            }
            Event::End(TagEnd::List(_)) => {
                if let Some(frame) = lists.pop() {
                    match lists.last_mut() {
                        // A nested list belongs to its parent, not to the
                        // document: merge its items up rather than emitting a
                        // separate block.
                        Some(parent) => parent.items.extend(frame.items),
                        None => blocks.push(Block::List { items: frame.items }),
                    }
                }
            }
            Event::Start(Tag::Item) => {
                current = Some(Text::new(""));
                heading_style = None;
                justify = Justify::Left;
            }
            Event::End(TagEnd::Item) => {
                if let (Some(mut text), Some(_)) = (current.take(), lists.last()) {
                    text.set_justify(Justify::Left);
                    let depth = lists.len() - 1;
                    if let Some(frame) = lists.last_mut() {
                        frame.push_item(depth, text);
                    }
                }
            }
            // Don't reset the active text if we're inside a list item.
            Event::Start(Tag::Paragraph) if current.is_none() => {
                current = Some(Text::new(""));
                heading_style = None;
                justify = Justify::Left;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let (style, heading_justify) = heading_format(heading_level(level));
                current = Some(Text::new(""));
                heading_style = Some(style);
                justify = heading_justify;
            }
            // In a list, the item text is finalized at End(Item) instead.
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Heading(_)) if lists.is_empty() => {
                if let Some(mut text) = current.take() {
                    if let Some(paragraphs) = quote.as_mut() {
                        // Quote paragraph: magenta base so its padding is magenta too.
                        text.set_base_style(Style::parse("magenta").expect("valid style"));
                        text.set_justify(Justify::Left);
                        paragraphs.push(text);
                    } else {
                        if let Some(style) = &heading_style {
                            let end = text.plain().len();
                            text.stylize(style.clone(), 0, end);
                        }
                        text.set_justify(justify);
                        blocks.push(Block::Text(text));
                    }
                }
                strong = 0;
                emphasis = 0;
            }
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis = emphasis.saturating_sub(1),
            Event::Text(text) => {
                if let Some(acc) = table.as_mut().filter(|a| a.in_cell) {
                    // Table cells collect plain text; inline styling within a cell
                    // is a documented follow-up (see the Markdown issue).
                    acc.cur_cell.push_str(&text);
                } else if let Some((_, source)) = code.as_mut() {
                    source.push_str(&text);
                } else if let Some(block) = current.as_mut() {
                    // Inside a link, use the markdown.link_url style + an OSC 8
                    // hyperlink; otherwise the inline strong/emphasis style.
                    let style = match &link {
                        Some(url) => Style::parse(LINK_STYLE)
                            .ok()
                            .map(|s| s.with_link(url.clone())),
                        None => inline_style(strong, emphasis),
                    };
                    block.append(&text, style.map(Into::into));
                }
            }
            Event::Code(text) => {
                if let Some(acc) = table.as_mut().filter(|a| a.in_cell) {
                    acc.cur_cell.push_str(&text);
                } else if let Some(block) = current.as_mut() {
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
        let width = options.max_width;
        let base = console.base_style();
        let mut lines: Vec<Vec<Segment>> = Vec::new();

        for (index, block) in self.blocks.iter().enumerate() {
            // A blank line precedes every non-first block, and every
            // list/quote/table (which upstream renders with a leading gap).
            if index > 0
                || matches!(
                    block,
                    Block::List { .. } | Block::Quote(_) | Block::Table { .. }
                )
            {
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
                        // Nested items are indented by their depth; the marker
                        // and any wrapped continuation line then align under it.
                        let indent = "  ".repeat(item.depth);
                        let prefix_width = cell_len(&indent) + cell_len(&prefix);
                        let item_lines = item.text.render_lines(
                            console.theme(),
                            base,
                            Some(width.saturating_sub(prefix_width)),
                        );
                        for (line_index, line) in item_lines.into_iter().enumerate() {
                            let mut row = Vec::new();
                            if line_index == 0 {
                                row.push(Segment::new(indent.clone(), None));
                                row.push(Segment::new(prefix.clone(), Some(prefix_style.clone())));
                            } else {
                                row.push(Segment::new(" ".repeat(prefix_width), None));
                            }
                            row.extend(line);
                            lines.push(row);
                        }
                    }
                }
                Block::Quote(paragraphs) => {
                    let prefix_style = Style::parse("magenta").expect("valid style");
                    // Upstream renders quote content at `max_width - 4`.
                    let content_width = width.saturating_sub(4);
                    for paragraph in paragraphs {
                        let quote_lines =
                            paragraph.render_lines(console.theme(), base, Some(content_width));
                        for line in quote_lines {
                            let mut row = vec![Segment::new(
                                QUOTE_PREFIX.to_string(),
                                Some(prefix_style.clone()),
                            )];
                            row.extend(line);
                            lines.push(row);
                        }
                    }
                }
                Block::Code { language, code } => {
                    // Render the code block via the Syntax renderable (functional,
                    // not byte-parity — see DIVERGENCES). Split its segment stream
                    // back into per-line rows for the shared join below.
                    let syntax = Syntax::new(code.as_str(), language.as_str());
                    let segments = syntax.rich_render(console, options);
                    lines.extend(Segment::split_lines(&segments));
                }
                Block::Rule => {
                    let style = Style::parse("dim").expect("valid style");
                    lines.push(vec![Segment::new("-".repeat(width), Some(style))]);
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
                    let segments = table.rich_render(console, options);
                    lines.extend(Segment::split_lines(&segments));
                }
            }
        }

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
mod nested_list_tests {
    use super::*;

    fn plain(source: &str, width: usize) -> String {
        let console = Console::builder().width(width).no_color(true).build();
        console.render_to_string(&Markdown::new(source))
    }

    /// The parser tracked the open list in a single `Option`, so a nested list
    /// overwrote its parent: the parent's collected items were discarded and its
    /// `End(List)` found nothing to emit. Three items became one.
    #[test]
    fn a_nested_list_keeps_every_item() {
        let out = plain("- one\n- two\n  - nested\n", 40);
        for expected in ["one", "two", "nested"] {
            assert!(
                out.contains(expected),
                "{expected:?} was dropped from:\n{out}"
            );
        }
    }

    #[test]
    fn nesting_three_deep_keeps_every_item() {
        let out = plain("- top\n  - mid\n    - deep\n", 40);
        for expected in ["top", "mid", "deep"] {
            assert!(
                out.contains(expected),
                "{expected:?} was dropped from:\n{out}"
            );
        }
    }

    /// Items after a sublist arrive at `End(Item)` only once the sublist has
    /// closed, so a naive flush puts them out of order.
    #[test]
    fn an_item_following_a_sublist_keeps_its_place() {
        let out = plain("- one\n  - nested\n- two\n", 40);
        let (one, nested, two) = (
            out.find("one").expect("one"),
            out.find("nested").expect("nested"),
            out.find("two").expect("two"),
        );
        assert!(one < nested && nested < two, "order was wrong:\n{out}");
    }

    /// Each nesting level numbers independently; a merged sublist must not
    /// advance its parent's sequence.
    #[test]
    fn each_level_of_an_ordered_list_numbers_independently() {
        let out = plain("1. first\n2. second\n   1. sub\n", 40);
        for expected in ["1 first", "2 second", "1 sub"] {
            assert!(out.contains(expected), "expected {expected:?} in:\n{out}");
        }
    }

    #[test]
    fn nested_items_are_indented_under_their_parent() {
        let out = plain("- top\n  - child\n", 40);
        let top = out.lines().find(|l| l.contains("top")).expect("top line");
        let child = out
            .lines()
            .find(|l| l.contains("child"))
            .expect("child line");
        let indent = |line: &str| line.len() - line.trim_start().len();
        assert!(
            indent(child) > indent(top),
            "child should be indented under its parent:\n{out}"
        );
    }
}
