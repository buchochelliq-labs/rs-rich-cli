//! Markdown rendering.
//!
//! Port of upstream `rich/markdown.py` (core block/inline elements). Parses
//! CommonMark with `pulldown-cmark` and renders each block as justified,
//! full-width lines separated by blank lines.
//!
//! Scope: paragraphs, ATX headings (h1–h6), bullet + ordered lists, block quotes,
//! thematic breaks, fenced/indented **code blocks** (syntax-highlighted via
//! [`Syntax`]), **links** (OSC 8 hyperlinks), inline strong/emphasis/code, and
//! **GFM tables** (rendered via [`Table`], each cell a styled [`Text`] carrying
//! its inline strong/emphasis/code/link/strike runs, as upstream's
//! `TableDataElement` builds it).

use pulldown_cmark::{
    Alignment, CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};

use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions, Justify};
use crate::markdown_url::{normalize_link, normalize_link_text, validate_link};
use crate::protocol::Renderable;
use crate::r#box::SIMPLE;
use crate::segment::Segment;
use crate::style::Style;
use crate::syntax::Syntax;
use crate::table::Table;
use crate::text::Text;

const CODE_STYLE: &str = "bold cyan on black"; // markdown.code
const QUOTE_STYLE: &str = "magenta"; // markdown.block_quote
/// The placeholder upstream's `ImageItem` puts in front of an image
/// (`Text.assemble("🌆 ", title, " ")`). U+1F306 measures two cells.
const IMAGE_MARKER: &str = "\u{1f306} ";
const BULLET: &str = " \u{2022} "; // " • ", markdown.item.bullet = bold
const QUOTE_PREFIX: &str = "\u{258c} "; // "▌ ", markdown.block_quote = magenta
const LINK_STYLE: &str = "bright_blue"; // markdown.link
const LINK_URL_STYLE: &str = "underline blue"; // markdown.link_url
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
    Quote {
        blocks: Vec<Block>,
        leading_break: bool,
    },
    /// An ignored HTML block still participates in upstream block spacing.
    Html,
    /// A fenced/indented code block, syntax-highlighted via [`Syntax`].
    Code {
        language: String,
        code: String,
        /// `Markdown(code_theme=…)`; `None` keeps the `Syntax` default.
        theme: Option<String>,
    },
    /// A thematic break (horizontal rule).
    Rule,
    /// An image placeholder. Upstream's `ImageItem` renders `🌆 <title> ` and
    /// says nothing about the picture itself; `text` is that whole assembly.
    ///
    /// `joins_next` reproduces `ImageItem.new_line = False` together with the
    /// `end=""` on its text: nothing separates the marker from whatever renders
    /// next, so the following block continues on the marker's own row. Only an
    /// image lifted out of a *top-level* paragraph or heading behaves that way —
    /// see [`parse`] for why one inside a list or quote does not.
    ///
    /// `leading_break` is upstream's `new_line` flag frozen at the moment the
    /// image was reached: a break precedes it only if some element had already
    /// closed. It replaces the usual inter-block gap rather than adding to it.
    Image {
        text: Text,
        joins_next: bool,
        leading_break: bool,
    },
    /// A GFM table: per-column justify (from the alignment row), header cells,
    /// and body rows. Rendered via [`Table`], matching upstream's construction.
    Table {
        alignments: Vec<Justify>,
        headers: Vec<Text>,
        rows: Vec<Vec<Text>>,
    },
}

/// Accumulates a GFM table across `pulldown-cmark`'s table events.
#[derive(Default)]
struct TableAccum {
    alignments: Vec<Justify>,
    headers: Vec<Text>,
    rows: Vec<Vec<Text>>,
    in_head: bool,
    in_cell: bool,
    cur_row: Vec<Text>,
    /// The open cell's content: upstream's `TableDataElement.content`, which
    /// appends each text run under the context's current style.
    cur_cell: Text,
}

/// Where an inline run lands: the open table cell if there is one (upstream's
/// `TableDataElement.on_text`), else the open paragraph-level buffer.
fn inline_target<'a>(
    current: &'a mut Option<Text>,
    table: &'a mut Option<TableAccum>,
) -> &'a mut Text {
    match table.as_mut().filter(|acc| acc.in_cell) {
        Some(acc) => &mut acc.cur_cell,
        None => current.get_or_insert_with(|| Text::new("")),
    }
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
    source: String,
    options: MarkdownOptions,
    blocks: Vec<Block>,
}

/// The constructor options of `rich.markdown.Markdown` that change what the
/// parsed blocks contain, so changing one re-parses the document.
#[derive(Clone, Default)]
struct MarkdownOptions {
    /// `hyperlinks` (see [`Markdown::hyperlinks`]); stored inverted so the
    /// derived default matches upstream's `True`.
    no_hyperlinks: bool,
    /// `justify` for paragraphs; `None` is upstream's `markdown.justify or "left"`.
    justify: Option<Justify>,
    /// `style`, the root of upstream's style stack; `None` is `"none"`.
    style: Option<Style>,
    /// `code_theme` for fenced and indented code blocks.
    code_theme: Option<String>,
    /// `inline_code_lexer`: when set, inline code is highlighted as this language.
    inline_code_lexer: Option<String>,
    /// `inline_code_theme`, defaulting to `code_theme`.
    inline_code_theme: Option<String>,
}

impl Markdown {
    /// Parse CommonMark `source` into renderable blocks.
    ///
    /// Hyperlinks are on, matching `rich.markdown.Markdown(hyperlinks=True)`.
    /// **The CLI wants them off** — see [`hyperlinks`](Self::hyperlinks).
    pub fn new(source: &str) -> Self {
        let options = MarkdownOptions::default();
        Markdown {
            source: source.to_string(),
            blocks: parse(source, &options),
            options,
        }
    }

    /// Choose how a `[text](url)` is rendered. Port of
    /// `rich.markdown.Markdown(hyperlinks=…)`, default `true`.
    ///
    /// * `true` — the text becomes an OSC 8 hyperlink pointing at the URL.
    /// * `false` — the URL is written out after the text, as
    ///   `text (https://example.com)`.
    ///
    /// The distinction is not cosmetic. An OSC 8 escape is only emitted when
    /// the console has a colour system, so with hyperlinks on a piped or
    /// `NO_COLOR` render drops every destination with nothing left to recover
    /// it from. That is why upstream's **`rich-cli` passes `hyperlinks=False`
    /// by default** and puts the OSC 8 form behind its opt-in `-y/--hyperlinks`
    /// flag; a CLI built on this crate should do the same:
    ///
    /// ```
    /// # use rich::markdown::Markdown;
    /// let opt_in = false; // set by `-y/--hyperlinks`
    /// let md = Markdown::new("A [link](https://example.com).").hyperlinks(opt_in);
    /// ```
    pub fn hyperlinks(mut self, hyperlinks: bool) -> Self {
        // The flag changes what the *text* of a paragraph or table cell is, not
        // just how it is painted, so the document has to be re-parsed.
        self.options.no_hyperlinks = !hyperlinks;
        self.reparse()
    }

    /// Justify every paragraph. Port of `Markdown(justify=…)`; by default
    /// paragraphs are left-justified. Headings keep their own alignment.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.options.justify = Some(justify);
        self.reparse()
    }

    /// The root style every run of text is drawn in. Port of
    /// `Markdown(style=…)`, default `"none"`.
    pub fn style(mut self, style: Style) -> Self {
        self.options.style = Some(style).filter(|style| !style.is_null());
        self.reparse()
    }

    /// The theme for code blocks. Port of `Markdown(code_theme=…)`. Names are
    /// `syntect` theme names, not Pygments styles (see DIVERGENCES #18).
    pub fn code_theme(mut self, theme: impl Into<String>) -> Self {
        self.options.code_theme = Some(theme.into());
        self.reparse()
    }

    /// Highlight inline code as `lexer`. Port of `Markdown(inline_code_lexer=…)`;
    /// by default inline code is not highlighted.
    pub fn inline_code_lexer(mut self, lexer: impl Into<String>) -> Self {
        self.options.inline_code_lexer = Some(lexer.into());
        self.reparse()
    }

    /// The theme for highlighted inline code. Port of
    /// `Markdown(inline_code_theme=…)`, defaulting to the code theme.
    pub fn inline_code_theme(mut self, theme: impl Into<String>) -> Self {
        self.options.inline_code_theme = Some(theme.into());
        self.reparse()
    }

    fn reparse(mut self) -> Self {
        self.blocks = parse(&self.source, &self.options);
        self
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

/// `markdown.link_url` plus the OSC 8 target, which is what upstream pushes for
/// a link when `hyperlinks=True`.
fn link_style(url: &str) -> Style {
    Style::parse(LINK_URL_STYLE)
        .expect("valid style")
        .with_link(url.to_string())
}

/// Upstream's `MarkdownContext.style_stack.current`: the product of every style
/// open at this point, outermost first, each layer overriding the last.
///
/// The order is what makes an inline style compose rather than replace. A link
/// inside `**bold**` is `bold underline blue`, not plain `underline blue`; a
/// `` `code` `` inside a link keeps the link *and* takes cyan over the link's
/// blue. Applying only the innermost layer dropped the outer attributes, and —
/// worse — a link whose whole text was inline code lost its URL entirely.
///
/// `extra` is the run's own style (`markdown.code` for a code span), pushed last
/// because upstream enters it after the link.
/// The bottom of upstream's style stack at this point in the parse: the
/// document `style`, with `markdown.block_quote` pushed for each enclosing quote
/// (`markdown.item` is `none`).
fn quote_root(md: &MarkdownOptions, stack: &[Frame]) -> Option<Style> {
    let mut root = md.style.clone();
    if stack
        .iter()
        .any(|frame| matches!(frame, Frame::Quote { .. }))
    {
        let quote = Style::parse(QUOTE_STYLE).expect("valid style");
        root = Some(match root {
            Some(root) => root.combine(&quote),
            None => quote,
        });
    }
    root
}

fn stack_style(
    root: Option<&Style>,
    heading: Option<&Style>,
    inline: Option<Style>,
    link: Option<&str>,
    extra: Option<Style>,
) -> Option<Style> {
    let mut current: Option<Style> = None;
    for layer in [
        root.cloned(),
        heading.cloned(),
        inline,
        link.map(link_style),
        extra,
    ] {
        let Some(next) = layer else { continue };
        current = Some(match current {
            Some(previous) => previous.combine(&next),
            None => next,
        });
    }
    current
}

/// The title upstream shows when an image has no alt text: the last path
/// component of its destination, `destination.strip("/").rsplit("/", 1)[-1]`.
///
/// Without it `![](logo.png)` rendered as a blank line — a badge row in a README
/// simply disappeared.
fn image_fallback_title(destination: &str) -> &str {
    let trimmed = destination.trim_matches('/');
    match trimmed.rsplit_once('/') {
        Some((_, last)) => last,
        None => trimmed,
    }
}

/// Assemble upstream's `Text.assemble("🌆 ", title, " ")` for one image.
///
/// `link` is the URL of an enclosing `[…](…)`, which upstream prefers over the
/// image's own destination (`self.link or self.destination`) so that a linked
/// badge points at the link, not at the picture.
///
/// With `hyperlinks` off the target is dropped entirely:
/// `ImageItem.__rich_console__` guards its `title.stylize(link_style)` behind
/// `if self.hyperlinks`, so the marker carries no OSC 8 escape at all.
fn image_text(
    destination: &str,
    alt: Text,
    link: Option<&str>,
    outer: Option<Style>,
    hyperlinks: bool,
) -> Text {
    let mut title = if alt.plain().is_empty() {
        Text::new(image_fallback_title(destination))
    } else {
        alt
    };
    let end = title.plain().len();
    // `ImageItem.on_text` appends with `context.current_style`, so the title
    // carries whatever was open around the image — a heading's style, and the
    // enclosing link's `markdown.link_url` for a badge wrapped in a link.
    if let Some(style) = outer {
        title.stylize(style, 0, end);
    }
    // `Style(link=self.link or self.destination or None)`: the enclosing link
    // wins, the image's own destination is the fallback, and neither being set
    // leaves the title unlinked.
    if hyperlinks {
        let target = link.unwrap_or(destination);
        if !target.is_empty() {
            title.stylize(Style::new().with_link(target.to_string()), 0, end);
        }
    }
    let mut text = Text::new(IMAGE_MARKER).append_text(&title);
    text.append(" ", None);
    text
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
fn flush_pending(
    current: &mut Option<Text>,
    blocks: &mut Vec<Block>,
    stack: &mut [Frame],
    justify: Justify,
) {
    let Some(mut text) = current.take() else {
        return;
    };
    // A freshly opened item holds an empty buffer; committing it would emit a
    // blank block.
    if text.plain().is_empty() {
        return;
    }
    text.set_justify(justify);
    sink(blocks, stack).push(Block::Text(text));
}

/// Emit a literal `~` for a single-tilde span, into whichever buffer the
/// surrounding characters are going to.
///
/// Inside a link label the label text is buffered separately, so appending
/// straight to `current` put BOTH tildes in front of the label: `[~a~ label]`
/// rendered as `~~a label`, characters reordered rather than restyled. Outside
/// one the buffer may not be open yet, so it still has to be created — routing
/// through a plain `as_mut()` silently DROPPED the tilde instead.
fn push_tilde(
    current: &mut Option<Text>,
    table: &mut Option<TableAccum>,
    link_label: &mut Option<String>,
) {
    if let Some(label) = link_label.as_mut() {
        label.push('~');
    } else {
        inline_target(current, table).append("~", None);
    }
}

/// Append a soft/hard break to the open link label if one is being buffered,
/// else to the open text buffer if there is one.
fn append_break(
    current: Option<&mut Text>,
    link_label: Option<&mut String>,
    text: &str,
    style: Option<Style>,
) {
    if let Some(label) = link_label {
        label.push_str(text);
    } else if let Some(block) = current {
        block.append(text, style.map(Into::into));
    }
}

/// One inline token while pairing tildes: an untouched event, literal source
/// text, a `~~` delimiter, or a delimiter that has been paired.
enum Piece<'a> {
    Event(Event<'a>, std::ops::Range<usize>),
    Literal(std::ops::Range<usize>),
    Tilde(std::ops::Range<usize>),
    Open(std::ops::Range<usize>),
    Close(std::ops::Range<usize>),
}

/// A `~~` delimiter in markdown-it's `Delimiter` sense. `length` is always 0
/// for strikethrough (upstream disables the "rule of 3"), so it is omitted.
struct Delimiter {
    piece: usize,
    open: bool,
    close: bool,
    end: Option<usize>,
    /// Innermost emphasis/strong span containing the delimiter. markdown-it
    /// pairs `*`/`_` and `~` in one pass, and a matched pair's jump hides every
    /// delimiter inside it from later closers; tildes are never paired across
    /// an emphasis span here for the same reason (see DIVERGENCES §21).
    emphasis: usize,
}

fn is_md_ascii_punct(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// markdown-it's `isPunctChar`: ASCII punctuation or a Unicode punctuation
/// category. `char::is_ascii_punctuation` plus general punctuation/symbols is
/// the closest std-only equivalent.
fn is_punct_char(c: char) -> bool {
    c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace() && !c.is_control())
}

/// Port of markdown-it's `StateInline.scanDelims` for a tilde run
/// (`canSplitWord = True`), returning `(can_open, can_close)`.
fn scan_delims(last: char, next: char) -> (bool, bool) {
    let last_punct = is_md_ascii_punct(last) || is_punct_char(last);
    let next_punct = is_md_ascii_punct(next) || is_punct_char(next);
    let last_space = last.is_whitespace();
    let next_space = next.is_whitespace();
    let left_flanking = !(next_space || (next_punct && !(last_space || last_punct)));
    let right_flanking = !(last_space || (last_punct && !(next_space || next_punct)));
    (left_flanking, right_flanking)
}

/// Port of markdown-it's `balance_pairs.processDelimiters` for one delimiter
/// list (a single marker, lengths 0).
fn process_delimiters(delimiters: &mut [Delimiter]) {
    if delimiters.is_empty() {
        return;
    }
    // `openersBottom[marker]`, indexed by `closer.open ? 3 : 0` (length % 3 = 0).
    let mut openers_bottom = [-1isize; 6];
    let mut header = 0usize;
    let mut last_piece: isize = -2;
    let mut jumps: Vec<usize> = Vec::with_capacity(delimiters.len());
    for closer_index in 0..delimiters.len() {
        jumps.push(0);
        if last_piece != delimiters[closer_index].piece as isize - 1 {
            header = closer_index;
        }
        last_piece = delimiters[closer_index].piece as isize;
        if !delimiters[closer_index].close {
            continue;
        }
        let slot = if delimiters[closer_index].open { 3 } else { 0 };
        let min_opener = openers_bottom[slot];
        let mut opener_index = header as isize - jumps[header] as isize - 1;
        let mut new_min = opener_index;
        while opener_index > min_opener {
            let i = opener_index as usize;
            let usable = delimiters[i].open
                && delimiters[i].end.is_none()
                && delimiters[i].emphasis == delimiters[closer_index].emphasis;
            if usable {
                let last_jump = if i > 0 && !delimiters[i - 1].open {
                    jumps[i - 1] + 1
                } else {
                    0
                };
                jumps[closer_index] = closer_index - i + last_jump;
                jumps[i] = last_jump;
                delimiters[closer_index].open = false;
                delimiters[i].end = Some(closer_index);
                delimiters[i].close = false;
                new_min = -1;
                last_piece = -2;
                break;
            }
            opener_index -= jumps[i] as isize + 1;
        }
        if new_min != -1 {
            openers_bottom[slot] = new_min;
        }
    }
}

/// A link or image whose destination markdown-it's `validateLink` refuses.
enum Rejected {
    /// `<javascript:…>`: the whole autolink is literal text.
    Autolink,
    /// `[label](…)` / `![alt](…)`: the brackets and destination are literal,
    /// the label still parses. `range` is the whole link's source; `last_end`
    /// where its last inner event ended (the closing `]` follows it).
    Bracket {
        range: std::ops::Range<usize>,
        last_end: usize,
    },
}

/// Un-link destinations markdown-it would refuse (`validateLink`): upstream's
/// link, image and autolink rules fail on them, so the source stays text —
/// `[j](javascript:x)` prints as written, with only its label's own inline
/// markup (emphasis, code…) still parsed. pulldown-cmark makes a link of any
/// destination, so the refused ones are turned back into their source here.
///
/// A refused *reference definition* (`[1]: javascript:x`) is not recovered
/// (DIVERGENCES #24): pulldown-cmark consumes the definition line, which
/// upstream prints as a paragraph. The link using it does print as literal text.
fn reject_invalid_links<'a>(
    source: &'a str,
    events: impl Iterator<Item = (Event<'a>, std::ops::Range<usize>)>,
) -> Vec<(Event<'a>, std::ops::Range<usize>)> {
    let literal = |range: std::ops::Range<usize>| {
        (Event::Text(CowStr::Borrowed(&source[range.clone()])), range)
    };
    let mut out = Vec::new();
    // One entry per open link or image: `None` when it is kept.
    let mut open: Vec<Option<Rejected>> = Vec::new();
    for (event, range) in events {
        let href = match &event {
            Event::Start(Tag::Link {
                link_type: LinkType::Email,
                dest_url,
                ..
            }) => Some(normalize_link(&format!("mailto:{dest_url}"))),
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                Some(normalize_link(dest_url))
            }
            _ => None,
        };
        let pushed = match (&event, href) {
            (_, Some(href)) if validate_link(&href) => {
                open.push(None);
                vec![(event, range.clone())]
            }
            (
                Event::Start(Tag::Link {
                    link_type: LinkType::Autolink | LinkType::Email,
                    ..
                }),
                Some(_),
            ) => {
                open.push(Some(Rejected::Autolink));
                vec![literal(range.clone())]
            }
            (Event::Start(tag), Some(_)) => {
                let opener = if matches!(tag, Tag::Image { .. }) {
                    2
                } else {
                    1
                };
                let opener = range.start..(range.start + opener).min(range.end);
                open.push(Some(Rejected::Bracket {
                    range: range.clone(),
                    last_end: opener.end,
                }));
                vec![literal(opener)]
            }
            (Event::End(TagEnd::Link | TagEnd::Image), _) => match open.pop() {
                Some(Some(Rejected::Autolink)) => Vec::new(),
                Some(Some(Rejected::Bracket { range, last_end })) => {
                    vec![literal(last_end.min(range.end)..range.end)]
                }
                Some(None) | None => vec![(event, range.clone())],
            },
            // The text inside a refused autolink is already in its literal.
            _ if matches!(open.last(), Some(Some(Rejected::Autolink))) => Vec::new(),
            _ => vec![(event, range.clone())],
        };
        for (_, pushed_range) in &pushed {
            for entry in open.iter_mut() {
                if let Some(Rejected::Bracket { last_end, .. }) = entry {
                    *last_end = (*last_end).max(pushed_range.end);
                }
            }
        }
        out.extend(pushed);
    }
    out
}

/// Pair tilde runs the way upstream's markdown-it does (its `strikethrough`
/// tokenize + `balance_pairs` + postProcess), over pulldown-cmark events parsed
/// *without* strikethrough.
///
/// Per inline run (a paragraph, heading, table cell or tight list item, with a
/// link label as its own nested scope, as markdown-it scopes delimiters per
/// opening token): each run of two or more tildes in literal text becomes an
/// optional leading `~` (odd runs) plus `~~` delimiters; paired delimiters turn
/// into `Strikethrough` events, and a lone `~` left before a closer moves after
/// it. `a ~~~x~~~ b` renders `a ~` + struck `x` + `~ b`, as upstream does.
fn pair_strikethrough<'a>(
    source: &'a str,
    events: impl Iterator<Item = (Event<'a>, std::ops::Range<usize>)>,
) -> Vec<(Event<'a>, std::ops::Range<usize>)> {
    let mut pieces: Vec<Piece<'a>> = Vec::new();
    // Delimiter lists: one per open scope; a link pushes a nested one.
    let mut scopes: Vec<Vec<Delimiter>> = vec![Vec::new()];
    let mut finished: Vec<Vec<Delimiter>> = Vec::new();
    let mut emphasis_stack: Vec<usize> = Vec::new();
    let mut next_emphasis = 1usize;
    let mut in_code = false;
    let mut in_cell = false;
    let mut image_depth = 0usize;
    // markdown-it's autolink rule consumes `<…>` whole, so tildes inside one
    // are never delimiters.
    let mut in_autolink = false;

    let neighbour = |c: Option<char>, in_cell: bool| match c {
        None => ' ',
        // markdown-it parses a trimmed cell, so a pipe reads as the edge.
        Some('|') if in_cell => ' ',
        Some(c) => c,
    };

    for (event, range) in events {
        if image_depth > 0 {
            match &event {
                Event::Start(Tag::Image { .. }) => image_depth += 1,
                Event::End(TagEnd::Image) => image_depth -= 1,
                _ => {}
            }
            pieces.push(Piece::Event(event, range));
            continue;
        }
        match &event {
            Event::Text(text) if !in_code && !in_autolink && **text == source[range.clone()] => {
                // Merge with a directly preceding literal so a run split across
                // two text events is scanned as one.
                let mut start = range.start;
                if let Some(Piece::Literal(previous)) = pieces.last() {
                    if previous.end == range.start {
                        start = previous.start;
                        pieces.pop();
                    }
                }
                let end = range.end;
                let bytes = source.as_bytes();
                let mut at = start;
                let mut literal_from = start;
                while at < end {
                    if bytes[at] != b'~' {
                        at += 1;
                        continue;
                    }
                    let run_start = at;
                    while at < end && bytes[at] == b'~' {
                        at += 1;
                    }
                    let length = at - run_start;
                    if length < 2 {
                        continue;
                    }
                    if literal_from < run_start {
                        pieces.push(Piece::Literal(literal_from..run_start));
                    }
                    let last = neighbour(source[..run_start].chars().next_back(), in_cell);
                    let next = neighbour(source[at..].chars().next(), in_cell);
                    let (open, close) = scan_delims(last, next);
                    let mut from = run_start;
                    if length % 2 == 1 {
                        pieces.push(Piece::Literal(from..from + 1));
                        from += 1;
                    }
                    let emphasis = emphasis_stack.last().copied().unwrap_or(0);
                    while from < at {
                        pieces.push(Piece::Tilde(from..from + 2));
                        scopes.last_mut().expect("scope").push(Delimiter {
                            piece: pieces.len() - 1,
                            open,
                            close,
                            end: None,
                            emphasis,
                        });
                        from += 2;
                    }
                    literal_from = at;
                }
                if literal_from < end {
                    pieces.push(Piece::Literal(literal_from..end));
                }
                continue;
            }
            Event::Start(Tag::Emphasis | Tag::Strong) => {
                emphasis_stack.push(next_emphasis);
                next_emphasis += 1;
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong) => {
                emphasis_stack.pop();
            }
            Event::Start(Tag::Link { link_type, .. }) => {
                in_autolink = matches!(link_type, LinkType::Autolink | LinkType::Email);
                scopes.push(Vec::new());
            }
            Event::End(TagEnd::Link) => {
                in_autolink = false;
                if scopes.len() > 1 {
                    finished.push(scopes.pop().expect("link scope"));
                }
            }
            Event::Start(Tag::Image { .. }) => image_depth = 1,
            Event::Text(_)
            | Event::Code(_)
            | Event::InlineHtml(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::FootnoteReference(_)
            | Event::InlineMath(_) => {}
            // Anything else is block structure: the inline run ends here.
            _ => {
                match &event {
                    Event::Start(Tag::CodeBlock(_)) => in_code = true,
                    Event::End(TagEnd::CodeBlock) => in_code = false,
                    Event::Start(Tag::TableCell) => in_cell = true,
                    Event::End(TagEnd::TableCell) => in_cell = false,
                    _ => {}
                }
                finished.append(&mut scopes);
                scopes.push(Vec::new());
                emphasis_stack.clear();
            }
        }
        pieces.push(Piece::Event(event, range));
    }
    finished.append(&mut scopes);

    // Pair, then mark: markdown-it's strikethrough `_postProcess`.
    let mut lone_markers: Vec<usize> = Vec::new();
    for mut delimiters in finished {
        process_delimiters(&mut delimiters);
        for delimiter in &delimiters {
            let Some(end) = delimiter.end else { continue };
            let closer = delimiters[end].piece;
            if let Piece::Tilde(range) = &pieces[delimiter.piece] {
                pieces[delimiter.piece] = Piece::Open(range.clone());
            }
            if let Piece::Tilde(range) = &pieces[closer] {
                pieces[closer] = Piece::Close(range.clone());
            }
            if let Some(Piece::Literal(range)) = closer.checked_sub(1).map(|i| &pieces[i]) {
                if &source[range.clone()] == "~" {
                    lone_markers.push(closer - 1);
                }
            }
        }
    }
    // An odd run is split as `~` + `~~`…, so a closer can leave its lone `~`
    // in front of it: move it after the closing tags.
    while let Some(i) = lone_markers.pop() {
        let mut j = i + 1;
        while j < pieces.len() && matches!(pieces[j], Piece::Close(_)) {
            j += 1;
        }
        j -= 1;
        if i != j {
            pieces.swap(i, j);
        }
    }

    // markdown-it's `fragments_join`: adjacent text tokens become one, so a
    // run like `a ~` renders as a single span rather than one per piece.
    let mut out: Vec<(Event<'a>, std::ops::Range<usize>)> = Vec::with_capacity(pieces.len());
    for piece in pieces {
        let (event, range) = match piece {
            Piece::Event(event, range) => (event, range),
            Piece::Literal(range) | Piece::Tilde(range) => {
                (Event::Text(CowStr::Borrowed(&source[range.clone()])), range)
            }
            Piece::Open(range) => (Event::Start(Tag::Strikethrough), range),
            Piece::Close(range) => (Event::End(TagEnd::Strikethrough), range),
        };
        if let (Event::Text(text), Some((Event::Text(previous), previous_range))) =
            (&event, out.last_mut())
        {
            let mut joined = previous.to_string();
            joined.push_str(text);
            *previous = CowStr::Boxed(joined.into_boxed_str());
            *previous_range =
                previous_range.start.min(range.start)..previous_range.end.max(range.end);
            continue;
        }
        out.push((event, range));
    }
    out
}

fn parse(source: &str, md: &MarkdownOptions) -> Vec<Block> {
    let hyperlinks = !md.no_hyperlinks;
    let paragraph_justify = md.justify.unwrap_or(Justify::Left);
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
    // Inside an autolink (`<http://…>`, `<user@host>`), whose text is shown
    // normalised.
    let mut autolink = false;
    // The label of the open link, when hyperlinks are off. Upstream pushes a
    // `Link` **element** at `link_close`-time rather than a style, so every
    // token in between is captured by it instead of by the paragraph, and only
    // `element.text.plain` is re-emitted at the close. That is why the label's
    // own emphasis is lost: `[**bold** label](u)` prints an unbolded
    // `bold label`. `None` whenever hyperlinks are on, where the label is
    // styled in place and this buffer must stay out of the way.
    let mut link_label: Option<String> = None;
    // Destination of the image being parsed, and the source span of its alt.
    let mut image: Option<String> = None;
    let mut image_span: Option<(usize, usize)> = None;
    // Upstream's `new_line` flag: set by every element that closes, cleared by
    // an image (`ImageItem.new_line = False`) and by a rule. Only images read
    // it, and it is why one lifted out of the *second* list item gets a blank
    // row above it while one lifted out of the first does not.
    let mut new_line = false;
    // The table being assembled while inside a GFM table.
    let mut table: Option<TableAccum> = None;

    // Strikethrough is *not* enabled in pulldown-cmark: it pairs tilde runs by
    // GFM rules (equal-length runs, single tildes allowed), while upstream's
    // markdown-it splits runs into `~~` delimiters and pairs those. The
    // tildes arrive as literal text and `pair_strikethrough` reproduces
    // markdown-it's pairing, emitting ordinary `Strikethrough` events whose
    // range is the `~~` delimiter.
    let options = Options::ENABLE_TABLES;
    let events = Parser::new_ext(source, options).into_offset_iter();
    let events = reject_invalid_links(source, events);
    for (event, range) in pair_strikethrough(source, events.into_iter()) {
        // Everything between an image's brackets is its alt text, and upstream
        // takes that from the *raw* markdown (`token.content`) rather than from
        // parsed inline events: `![alt *em*](u)` shows `alt *em*`, asterisks and
        // all. Widening the source span is the only way back to the literal
        // text once pulldown-cmark has turned the markers into events.
        if image.is_some() && !matches!(event, Event::End(TagEnd::Image)) {
            image_span = Some(match image_span {
                Some((start, end)) => (start.min(range.start), end.max(range.end)),
                None => (range.start, range.end),
            });
            continue;
        }
        // Upstream's `new_line = element.new_line` bookkeeping, which runs for
        // every element that closes. Everything declares `new_line = True`
        // except an image and a rule. Images and closing quotes read the
        // preceding value before their own closing event changes it.
        let preceding_new_line = new_line;
        match &event {
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::List(_)
                | TagEnd::Item
                | TagEnd::BlockQuote(_)
                | TagEnd::CodeBlock
                | TagEnd::Table
                | TagEnd::TableHead
                | TagEnd::TableRow
                | TagEnd::TableCell
                | TagEnd::HtmlBlock,
            ) => new_line = true,
            Event::Rule => new_line = false,
            _ => {}
        }
        match event {
            Event::End(TagEnd::HtmlBlock) => {
                sink(&mut blocks, &mut stack).push(Block::Html);
            }
            Event::Rule => {
                flush_pending(&mut current, &mut blocks, &mut stack, paragraph_justify);
                sink(&mut blocks, &mut stack).push(Block::Rule);
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                // An email autolink (`<user@example.org>`) carries a `mailto:`
                // destination in CommonMark, but pulldown-cmark leaves the
                // scheme to the renderer and hands us the bare address. Adding
                // it is what makes the destination a usable URL — upstream's
                // markdown-it puts it in the `href` itself.
                //
                // Every destination then goes through markdown-it's
                // `normalizeLink` (percent-encoding, punycoded host), as
                // upstream's does before rich ever sees it; pulldown-cmark
                // passes it through raw, control characters included.
                link = Some(normalize_link(&match link_type {
                    LinkType::Email => format!("mailto:{dest_url}"),
                    _ => dest_url.to_string(),
                }));
                // An autolink's text is its destination, which markdown-it
                // shows through `normalizeLinkText` instead.
                autolink = matches!(link_type, LinkType::Autolink | LinkType::Email);
                if !hyperlinks {
                    link_label = Some(String::new());
                }
            }
            Event::End(TagEnd::Link) => {
                autolink = false;
                let url = link.take();
                let label = link_label.take();
                // `hyperlinks=False`: upstream flushes the buffered label under
                // `markdown.link` and then writes the destination out after it —
                // `A link (https://example.com) here.`
                //
                // Emitting nothing here (our only behaviour before) loses the
                // URL outright the moment the console has no colour system, and
                // a pipe has no OSC 8 escape to recover it from. `rich -m`
                // passes `hyperlinks=False`, so that was every URL in every
                // redirected render.
                if let Some(url) = url.filter(|_| !hyperlinks) {
                    let label = label.unwrap_or_default();
                    let inline = inline_style(strong, emphasis, strike);
                    // In a table cell the URL is part of the cell's text, so it
                    // counts towards the column width, as upstream measures it.
                    let block = inline_target(&mut current, &mut table);
                    let layer = |style: Option<Style>| {
                        stack_style(
                            quote_root(md, &stack).as_ref(),
                            heading_style.as_ref(),
                            inline.clone(),
                            None,
                            style,
                        )
                    };
                    // An empty label appends a zero-length span upstream,
                    // which renders as nothing at all.
                    if !label.is_empty() {
                        block.append(&label, layer(Style::parse(LINK_STYLE).ok()).map(Into::into));
                    }
                    block.append(" (", layer(None).map(Into::into));
                    block.append(
                        &url,
                        layer(Style::parse(LINK_URL_STYLE).ok()).map(Into::into),
                    );
                    block.append(")", layer(None).map(Into::into));
                }
            }
            // Images are emitted immediately rather than appended to their
            // parent element. `TableDataElement` uses that same base
            // `on_child_close`, so an image in a cell is hoisted above the
            // eventual table and contributes no text to the cell.
            Event::Start(Tag::Image { dest_url, .. }) => {
                image = Some(normalize_link(&dest_url));
                image_span = None;
            }
            Event::End(TagEnd::Image) => {
                if let Some(destination) = image.take() {
                    let alt = image_span
                        .take()
                        .map(|(start, end)| Text::new(&source[start..end]))
                        .unwrap_or_default();
                    // Pushed to the *document*, not to `sink`: upstream renders
                    // the image element the moment its token is reached, while
                    // the list or quote containing it is still open and will not
                    // render until it closes. An image inside a list therefore
                    // appears above the whole list, not inside the item.
                    //
                    // `joins_next` is only true at the top level: upstream emits
                    // no line break after an image, but a container closing
                    // after it (its paragraph having been captured) emits one of
                    // its own, so only a top-level paragraph or heading really
                    // continues on the marker's row.
                    blocks.push(Block::Image {
                        text: image_text(
                            &destination,
                            alt,
                            link.as_deref(),
                            stack_style(
                                quote_root(md, &stack).as_ref(),
                                heading_style.as_ref(),
                                inline_style(strong, emphasis, strike),
                                link.as_deref().filter(|_| hyperlinks),
                                None,
                            ),
                            hyperlinks,
                        ),
                        // A table is a container too, even though it uses a
                        // dedicated accumulator rather than a `Frame`. Its own
                        // render begins after the hoisted image's open row.
                        joins_next: stack.is_empty() && table.is_none(),
                        leading_break: new_line,
                    });
                    new_line = false;
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_pending(&mut current, &mut blocks, &mut stack, paragraph_justify);
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
                        theme: md.code_theme.clone(),
                    });
                }
            }
            Event::Start(Tag::Table(aligns)) => {
                flush_pending(&mut current, &mut blocks, &mut stack, paragraph_justify);
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
                    acc.cur_cell = Text::new("");
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
                flush_pending(&mut current, &mut blocks, &mut stack, paragraph_justify);
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
                    sink(&mut blocks, &mut stack).push(Block::Quote {
                        blocks: quoted,
                        leading_break: preceding_new_line,
                    });
                }
            }
            Event::Start(Tag::List(first)) => {
                flush_pending(&mut current, &mut blocks, &mut stack, paragraph_justify);
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
                justify = paragraph_justify;
            }
            Event::End(TagEnd::Item) => {
                // A *tight* list emits its item text without a Paragraph, so
                // anything still pending belongs to this item. markdown-it still
                // emits a (hidden) paragraph for it, so upstream justifies it as
                // a paragraph.
                if let Some(mut text) = current.take() {
                    text.set_justify(paragraph_justify);
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
                flush_pending(&mut current, &mut blocks, &mut stack, paragraph_justify);
                current = Some(Text::new(""));
                heading_style = None;
                // `Paragraph.create`: `markdown.justify or "left"`.
                justify = paragraph_justify;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush_pending(&mut current, &mut blocks, &mut stack, paragraph_justify);
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
                        // Quote paragraph: the quote style (over the document
                        // style) as its base, so its padding carries it too.
                        if let Some(root) = quote_root(md, &stack) {
                            text.set_base_style(root);
                        }
                    }
                    // A heading's style rides on each run (upstream pushes
                    // `markdown.h<n>` onto the style stack at `heading_open`, so
                    // every inline style composes *over* it), never as a base
                    // style — a base style would paint the centring padding too,
                    // which upstream leaves unstyled. Only the alignment is left
                    // to apply here; treating a quoted heading as body text
                    // flattened h1 to plain magenta and left-aligned it.
                    text.set_justify(justify);
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
                    //
                    // Route it the same way as any other text: inside a link
                    // label the surrounding characters are buffered separately,
                    // so appending straight to `current` put BOTH tildes in
                    // front of the label — `[~a~ label]` came out as
                    // `~~a label`, characters reordered rather than restyled.
                    single_tilde += 1;
                    push_tilde(&mut current, &mut table, &mut link_label);
                }
            }
            Event::End(TagEnd::Strikethrough) => {
                if single_tilde > 0 {
                    single_tilde -= 1;
                    push_tilde(&mut current, &mut table, &mut link_label);
                } else {
                    strike = strike.saturating_sub(1);
                }
            }
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis = emphasis.saturating_sub(1),
            Event::Text(text) => {
                let text = if autolink {
                    CowStr::from(normalize_link_text(&text))
                } else {
                    text
                };
                if let Some(label) = link_label.as_mut() {
                    label.push_str(&text);
                } else if let Some((_, source)) = code.as_mut() {
                    source.push_str(&text);
                } else {
                    // A table cell appends under the current style, exactly as
                    // a paragraph does (`TableDataElement.on_text`). Otherwise
                    // open a buffer if none is active: in a tight list item the
                    // text after a nested block arrives bare, with the previous
                    // buffer already flushed by that block's start.
                    let block = inline_target(&mut current, &mut table);
                    let style = stack_style(
                        quote_root(md, &stack).as_ref(),
                        heading_style.as_ref(),
                        inline_style(strong, emphasis, strike),
                        link.as_deref().filter(|_| hyperlinks),
                        None,
                    );
                    block.append(&text, style.map(Into::into));
                }
            }
            Event::Code(text) => {
                if let Some(label) = link_label.as_mut() {
                    label.push_str(&text);
                } else {
                    // A table cell or the open buffer, as for plain text.
                    let block = inline_target(&mut current, &mut table);
                    // `markdown.code` is pushed on TOP of the link, so a link
                    // whose whole label is inline code — ``[`rich`](url)`` —
                    // keeps its destination. Applying the code style alone
                    // discarded it.
                    if let Some(lexer) = &md.inline_code_lexer {
                        // `MarkdownContext.on_text` for `code_inline` with a
                        // lexer: the highlighted text, right-stripped, assembled
                        // under the current style (no `markdown.code` layer).
                        let theme = md.inline_code_theme.as_ref().or(md.code_theme.as_ref());
                        let mut syntax = Syntax::new(text.to_string(), lexer.as_str());
                        if let Some(theme) = theme {
                            syntax = syntax.theme(theme.as_str());
                        }
                        let mut highlighted = syntax.highlight();
                        highlighted.rstrip();
                        let style = stack_style(
                            quote_root(md, &stack).as_ref(),
                            heading_style.as_ref(),
                            inline_style(strong, emphasis, strike),
                            link.as_deref().filter(|_| hyperlinks),
                            None,
                        );
                        let mut fragment = Text::new("");
                        if let Some(style) = style {
                            fragment.set_base_style(style);
                        }
                        let fragment = fragment.append_text(&highlighted);
                        *block = std::mem::take(block).append_text(&fragment);
                        continue;
                    }
                    let style = stack_style(
                        quote_root(md, &stack).as_ref(),
                        heading_style.as_ref(),
                        inline_style(strong, emphasis, strike),
                        link.as_deref().filter(|_| hyperlinks),
                        Style::parse(CODE_STYLE).ok(),
                    );
                    block.append(&text, style.map(Into::into));
                }
            }
            // `softbreak`/`hardbreak` go through `context.on_text`, so they land
            // in the open link label if there is one, and otherwise carry
            // whatever styles are open just like any other run.
            Event::SoftBreak => append_break(
                current.as_mut(),
                link_label.as_mut(),
                " ",
                stack_style(
                    quote_root(md, &stack).as_ref(),
                    heading_style.as_ref(),
                    inline_style(strong, emphasis, strike),
                    link.as_deref().filter(|_| hyperlinks),
                    None,
                ),
            ),
            Event::HardBreak => append_break(
                current.as_mut(),
                link_label.as_mut(),
                "\n",
                stack_style(
                    quote_root(md, &stack).as_ref(),
                    heading_style.as_ref(),
                    inline_style(strong, emphasis, strike),
                    link.as_deref().filter(|_| hyperlinks),
                    None,
                ),
            ),
            _ => {}
        }
    }
    blocks
}

impl Renderable for Markdown {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut lines = render_blocks(
            &self.blocks,
            console,
            options,
            options.max_width,
            true,
            self.options.style.as_ref(),
        );

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
    root: Option<&Style>,
) -> Vec<Vec<Segment>> {
    let base = console.base_style();
    let mut lines: Vec<Vec<Segment>> = Vec::new();
    // Set by an image whose marker must stay on the same row as the block that
    // follows it (see [`Block::Image`]).
    let mut join_previous = false;

    for (index, block) in blocks.iter().enumerate() {
        let mut merge = std::mem::take(&mut join_previous);
        // Consecutive images share their open row even when hoisted from a
        // container. A closed cell/item sets leading_break and ends that row.
        if matches!(
            block,
            Block::Image {
                leading_break: false,
                ..
            }
        ) && index > 0
            && matches!(blocks[index - 1], Block::Image { .. })
        {
            merge = true;
        }
        // `new_line` before an image is a single line break, not the blank-row
        // separator used between ordinary blocks. In particular, images
        // hoisted from consecutive table rows must occupy consecutive output
        // rows. It also cancels the preceding image's open-row join.
        if matches!(
            block,
            Block::Image {
                leading_break: true,
                ..
            }
        ) {
            merge = false;
        }
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
        let own_gap = matches!(block, Block::List { .. } | Block::Table { .. });
        // An image emits no line break after itself, so the block that follows
        // one gets no separator at all — not even the leading gap a list, quote
        // or table would otherwise bring.
        let after_image = index > 0 && matches!(blocks[index - 1], Block::Image { .. });
        let separator = match block {
            Block::Quote { leading_break, .. } => top_level && *leading_break && !after_image,
            // After an ordinary element this is the usual blank-row gap;
            // after an image (whose text has `end=""`) it is only a line break,
            // represented above by declining to merge the two image rows.
            Block::Image { leading_break, .. } => top_level && *leading_break && !after_image,
            _ if after_image => false,
            _ => top_level && (own_gap || (index > 0 && !after_rule)),
        };
        if separator {
            lines.push(Vec::new());
        }
        let start = lines.len();
        match block {
            Block::Text(text) => {
                lines.extend(text.render_lines(console.theme(), base, Some(width)))
            }
            Block::Image {
                text, joins_next, ..
            } => {
                // No justify of its own, so the marker is wrapped but never
                // padded — upstream assembles a bare `Text` for it.
                lines.extend(text.render_lines(console.theme(), base, Some(width)));
                join_previous = *joins_next;
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
                        root,
                    );
                    // A leading blank row would push the marker off its content.
                    let mut item_lines: Vec<Vec<Segment>> = item_lines
                        .into_iter()
                        .skip_while(|line| line.is_empty())
                        .collect();
                    pad_lines(&mut item_lines, width.saturating_sub(prefix_width));
                    for (line_index, line) in item_lines.into_iter().enumerate() {
                        let mut row = Vec::new();
                        // `render_bullet`/`render_number`: continuation rows are
                        // padded in the marker's own style.
                        if line_index == 0 {
                            row.push(Segment::new(prefix.clone(), Some(prefix_style.clone())));
                        } else {
                            row.push(Segment::new(
                                " ".repeat(prefix_width),
                                Some(prefix_style.clone()),
                            ));
                        }
                        // `render_lines(self.elements, …, style=self.style)`: the
                        // item style (the document style under `markdown.item`)
                        // sits under its content and padding.
                        match root {
                            Some(root) => row.extend(Segment::apply_style(&line, root)),
                            None => row.extend(line),
                        }
                        lines.push(row);
                    }
                }
            }
            Block::Html => {}
            Block::Quote { blocks: quoted, .. } => {
                // `context.enter_style("markdown.block_quote")`: the quote style
                // over the enclosing style.
                let quote = Style::parse(QUOTE_STYLE).expect("valid style");
                let prefix_style = match root {
                    Some(root) => root.combine(&quote),
                    None => quote,
                };
                // Upstream renders quote content at `max_width - 4`.
                let content_width = width.saturating_sub(4);
                let quoted_lines = render_blocks(
                    quoted,
                    console,
                    options,
                    content_width,
                    false,
                    Some(&prefix_style),
                );
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
            Block::Code {
                language,
                code,
                theme,
            } => {
                // Render the code block via the Syntax renderable (functional,
                // not byte-parity — see DIVERGENCES). Split its segment stream
                // back into per-line rows for the shared join below.
                // Upstream: `Syntax(code, lexer, theme=..., word_wrap=True, padding=1)`.
                // Upstream: `Syntax(code, lexer, theme=..., word_wrap=True, padding=1)`.
                // Without word_wrap a long line was cropped dead at the console
                // width and its tail discarded entirely — a README's install
                // command lost half its flags, with no marker that anything went.
                let mut syntax = Syntax::new(code.as_str(), language.as_str())
                    .word_wrap(true)
                    .padding(1);
                if let Some(theme) = theme {
                    syntax = syntax.theme(theme.as_str());
                }
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
                    // `heading.stylize("markdown.table.header")`: a span over the
                    // header's own inline spans, applied at render.
                    table.add_column_text(header.clone(), justify);
                    table.column_header_style(header_style.clone());
                }
                for row in rows {
                    table.add_row_text(row.clone());
                }
                let inner = options.update_width(width);
                lines.extend(Segment::split_lines(&table.rich_render(console, &inner)));
            }
        }
        // Fold this block's first row onto the row the image left open. `merge`
        // is only ever set by a preceding image, which always pushed at least
        // one row, so `start` is never zero here.
        if merge && lines.len() > start {
            let first = lines.remove(start);
            lines[start - 1].extend(first);
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

    fn render_with(markdown: &Markdown) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(30)
            .build();
        console.render_to_string(markdown)
    }

    #[test]
    fn code_theme_changes_the_code_block_colours() {
        let source = "```rust\nfn main() {}\n```";
        let default = render_with(&Markdown::new(source));
        let themed = render_with(&Markdown::new(source).code_theme("InspiredGitHub"));
        assert_ne!(default, themed);
        // An unknown theme falls back to the default, as `Syntax::theme` does.
        assert_eq!(
            default,
            render_with(&Markdown::new(source).code_theme("no-such-theme"))
        );
    }

    #[test]
    fn inline_code_lexer_highlights_instead_of_the_code_style() {
        let source = "Call `fn main() {}` now.";
        let plain = render_with(&Markdown::new(source));
        // `markdown.code` (bold cyan on black) without a lexer.
        assert!(plain.contains("\x1b[1;36;40m"), "{plain:?}");
        let highlighted = render_with(&Markdown::new(source).inline_code_lexer("rust"));
        assert!(!highlighted.contains("\x1b[1;36;40m"), "{highlighted:?}");
        assert_ne!(plain, highlighted);
        let text = Console::builder().width(30).color_system(None).build();
        assert_eq!(
            text.render_to_string(&Markdown::new(source).inline_code_lexer("rust")),
            text.render_to_string(&Markdown::new(source)),
            "highlighting changes colours only, never the text"
        );
        // `inline_code_theme` defaults to `code_theme`, and overrides it.
        let by_code_theme = render_with(
            &Markdown::new(source)
                .inline_code_lexer("rust")
                .code_theme("InspiredGitHub"),
        );
        let by_inline_theme = render_with(
            &Markdown::new(source)
                .inline_code_lexer("rust")
                .inline_code_theme("InspiredGitHub"),
        );
        assert_ne!(highlighted, by_code_theme);
        assert_eq!(by_code_theme, by_inline_theme);
    }

    #[test]
    fn a_code_only_list_item_keeps_the_bullet_on_its_padding_row() {
        let console = Console::builder().width(30).color_system(None).build();
        assert_eq!(console.render_export(&Markdown::new("- ```\n  code\n  ```")),
            "\n •                            \n    code                      \n                              \n");
    }

    #[test]
    fn table_cell_images_share_a_row_until_the_cell_closes() {
        let console = Console::builder().width(30).color_system(None).build();
        let output = console.render_to_string(&Markdown::new(
            "| h |\n|---|\n| ![a](x) ![b](y) |\n| ![c](z) |",
        ));
        assert!(output.starts_with("\n🌆 a 🌆 b \n🌆 c \n"), "{output:?}");
    }

    #[test]
    fn quoted_rule_spacing_uses_the_last_closed_child() {
        let console = Console::builder().width(30).color_system(None).build();
        assert_eq!(
            console.render_to_string(&Markdown::new("> ---")),
            "▌ --------------------------\n▌                           "
        );
        let output = console.render_to_string(&Markdown::new("> ---\n>\n> text"));
        assert!(
            output.starts_with("\n▌ --------------------------\n"),
            "{output:?}"
        );
    }

    #[test]
    fn ignored_html_blocks_keep_upstream_paragraph_spacing() {
        let console = Console::builder().width(30).color_system(None).build();
        for (source, expected) in [
            (
                "<div>hidden</div>\n\nParagraph",
                "\nParagraph                     ",
            ),
            ("<div>hidden</div>", ""),
            (
                "A\n\n<div>x</div>\n\nB",
                "A                             \n\n\nB                             ",
            ),
        ] {
            assert_eq!(console.render_to_string(&Markdown::new(source)), expected);
        }
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
        let console = Console::builder().width(width).color_system(None).build();
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

    /// Upstream's `ImageItem` renders `🌆 <title> ` and yields it *before* the
    /// element it was lifted out of, with no line break of its own. We rendered
    /// the alt text inline with no marker at all, and `![](url)` — a badge row,
    /// which is what most READMEs open with — came out as a blank line.
    ///
    /// Every expectation captured verbatim from real rich 15.0.0 at width 40:
    ///
    /// ```text
    /// ![alt text](https://example.com/pic.png)  -> '🌆 alt text'
    /// ![](https://example.com/pic.png)          -> '🌆 pic.png'   <- filename
    /// ![](img/)                                 -> '🌆 img'
    /// Before ![alt text](img/pic.png) after.    -> '🌆 alt text Before  after.'
    /// ![alt *em*](u/v.png)                      -> '🌆 alt *em*'  <- raw alt
    /// ```
    #[test]
    fn an_image_is_marked_and_hoisted() {
        let row = |source: &str| {
            plain(source, 40)
                .lines()
                .next()
                .expect("a row")
                .trim_end()
                .to_string()
        };
        assert_eq!(
            row("![alt text](https://example.com/pic.png)"),
            "🌆 alt text"
        );
        assert_eq!(row("![](https://example.com/pic.png)"), "🌆 pic.png");
        assert_eq!(row("![](img/)"), "🌆 img");
        // Hoisted to the front of the paragraph it sat inside, on the same row.
        assert_eq!(
            row("Before ![alt text](img/pic.png) after."),
            "🌆 alt text Before  after."
        );
        // The alt is the raw markdown source, markers included: upstream reads
        // markdown-it's `token.content`, which is never inline-parsed.
        assert_eq!(row("![alt *em*](u/v.png)"), "🌆 alt *em*");
    }

    /// An image inside a container is lifted clear of it: upstream renders the
    /// element the moment its token is reached, while the list or quote holding
    /// it is still open and will not render until it closes.
    ///
    /// Real rich 15.0.0 at width 40 (trailing padding trimmed):
    ///
    /// ```text
    /// '- item with ![pic](a/b.png) inside'
    ///     -> ['🌆 pic', ' • item with  inside']
    /// '> quoted ![pic](a/b.png) end'
    ///     -> ['🌆 pic', '▌ quoted  end']
    /// ```
    ///
    /// Note the absence of the blank row a list or quote normally brings with
    /// it: the image asks for no line break after itself.
    #[test]
    fn an_image_is_lifted_out_of_a_list_or_quote() {
        let rows = |source: &str| -> Vec<String> {
            plain(source, 40)
                .lines()
                .map(|line| line.trim_end().to_string())
                .collect()
        };
        assert_eq!(
            rows("- item with ![pic](a/b.png) inside"),
            vec!["🌆 pic", " • item with  inside"]
        );
        assert_eq!(
            rows("> quoted ![pic](a/b.png) end"),
            vec!["🌆 pic", "▌ quoted  end"]
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

    /// A tab in a fenced block reaches the terminal as U+0009, which jumps to
    /// the next 8-cell stop while we had counted it as one cell — so the block
    /// overran the width it was given. Upstream expands tabs before
    /// highlighting; the fenced block inherits that through `Syntax`.
    #[test]
    fn a_fenced_block_expands_its_tabs() {
        // Rows captured from rich 15.0.0 at width 30.
        let out = plain("```python\ndef f():\n\tif x:\n\t\treturn 1\n```", 30);
        assert_eq!(
            out.split('\n').collect::<Vec<_>>(),
            [
                "                              ",
                " def f():                     ",
                "     if x:                    ",
                "         return 1             ",
                "                              ",
            ]
        );
    }
}

/// `Markdown(hyperlinks=…)`. Every expectation here was captured verbatim from
/// real rich 15.0.0 (with its random OSC 8 `id=` field removed, which we
/// deliberately do not reproduce — see docs/DIVERGENCES.md).
#[cfg(test)]
mod hyperlink_tests {
    use super::*;
    use crate::color::ColorSystem;

    fn plain(source: &str, width: usize, hyperlinks: bool) -> String {
        Console::builder()
            .width(width)
            .color_system(None)
            .build()
            .render_to_string(&Markdown::new(source).hyperlinks(hyperlinks))
    }

    fn ansi(source: &str, width: usize, hyperlinks: bool) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .no_color(false)
            .build()
            .render_to_string(&Markdown::new(source).hyperlinks(hyperlinks))
    }

    /// THE defect: an OSC 8 escape is only written when the console has a colour
    /// system, so with hyperlinks on a piped or `NO_COLOR` render dropped every
    /// destination and left nothing to recover it from. `rich -m` passes
    /// `hyperlinks=False` precisely so the URL is written out as text.
    #[test]
    fn hyperlinks_off_writes_the_url_out_after_the_label() {
        assert_eq!(
            plain("A [link](https://example.com) here.", 40, false),
            "A link (https://example.com) here.      "
        );
    }

    #[test]
    fn hyperlinks_on_keeps_the_label_alone() {
        assert_eq!(
            plain("A [link](https://example.com) here.", 40, true),
            "A link here.                            "
        );
    }

    /// The knock-on: the URL is part of the cell's *text*, so it drives the
    /// column width. Laying the table out against the bare label made it far too
    /// narrow and the URL was then wrapped or cropped away.
    #[test]
    fn hyperlinks_off_widens_a_table_column_to_fit_the_url() {
        let source = "| T | W |\n| :-- | --: |\n| r | [repo](https://ex.org/a) |\n";
        assert_eq!(
            plain(source, 60, false).split('\n').collect::<Vec<_>>(),
            [
                "",
                "                            ",
                " T                        W ",
                " ────────────────────────── ",
                " r  repo (https://ex.org/a) ",
                "                            ",
            ]
        );
        // ...and with hyperlinks on the column stays at the label's width.
        assert_eq!(
            plain(source, 60, true).split('\n').collect::<Vec<_>>(),
            [
                "",
                "         ",
                " T     W ",
                " ─────── ",
                " r  repo ",
                "         "
            ]
        );
    }

    /// Upstream buffers the label in a `Link` element and re-emits only
    /// `element.text.plain`, so emphasis *inside* the label is lost.
    #[test]
    fn hyperlinks_off_flattens_the_labels_own_emphasis() {
        assert_eq!(
            plain("A [**b** and *i* l](https://e.org) t.", 60, false),
            "A b and i l (https://e.org) t.                              "
        );
    }

    /// `markdown.link` (bright_blue) paints the label, `markdown.link_url`
    /// (underline blue) the URL, and both compose over the heading's own style —
    /// h2's magenta loses to each in turn.
    #[test]
    fn hyperlinks_off_styles_the_label_and_the_url_under_a_heading() {
        assert_eq!(
            ansi("## H [x](https://e.org)", 40, false),
            "\x1b[4;35mH \x1b[0m\x1b[4;94mx\x1b[0m\x1b[4;35m (\x1b[0m\
             \x1b[4;34mhttps://e.org\x1b[0m\x1b[4;35m)\x1b[0m                     "
        );
        assert_eq!(
            ansi("## H [x](https://e.org)", 40, true),
            "\x1b[4;35mH \x1b[0m\x1b]8;;https://e.org\x1b\\\x1b[4;34mx\x1b[0m\
             \x1b]8;;\x1b\\                                     "
        );
    }

    /// Upstream pushes `markdown.link_url` *onto* the open style stack, so a
    /// link inside `**bold**` is bold as well. Replacing the stack with the link
    /// style alone dropped the bold.
    #[test]
    fn a_link_inside_bold_stays_bold() {
        assert_eq!(
            ansi("x **b [l](https://e.org) b** y", 60, true),
            "x \x1b[1mb \x1b[0m\x1b]8;;https://e.org\x1b\\\x1b[1;4;34ml\x1b[0m\
             \x1b]8;;\x1b\\\x1b[1m b\x1b[0m y                                                   "
        );
    }

    /// `markdown.code` is pushed on top of the link, so a label that is entirely
    /// inline code keeps its destination. Applying the code style alone threw the
    /// URL away even with hyperlinks *on*.
    #[test]
    fn a_link_labelled_with_inline_code_keeps_its_destination() {
        assert_eq!(
            ansi("A [`code`](https://e.org/x) tail.", 60, true),
            "A \x1b]8;;https://e.org/x\x1b\\\x1b[1;4;36;40mcode\x1b[0m\x1b]8;;\x1b\\ \
             tail.                                                "
        );
    }

    /// CommonMark gives an email autolink a `mailto:` destination, but
    /// pulldown-cmark leaves the scheme to the renderer and hands over the bare
    /// address — so the URL we printed was not a URL.
    #[test]
    fn an_email_autolink_keeps_its_mailto_scheme() {
        assert_eq!(
            plain("Mail <who@where.net> now.", 50, false),
            "Mail who@where.net (mailto:who@where.net) now.    "
        );
        assert_eq!(
            ansi("Mail <who@where.net> now.", 50, true),
            "Mail \x1b]8;;mailto:who@where.net\x1b\\\x1b[4;34mwho@where.net\x1b[0m\
             \x1b]8;;\x1b\\ now.                           "
        );
    }

    /// A badge wrapped in a link: `ImageItem` appends its title with the style
    /// open around it, so the alt text carries the link's `markdown.link_url`
    /// too, not just the OSC 8 target.
    #[test]
    fn an_image_inside_a_link_carries_the_links_style() {
        assert_eq!(
            ansi("[![badge](b.svg)](https://e.org)", 40, true),
            "\u{1f306} \x1b]8;;https://e.org\x1b\\\x1b[4;34mbadge\x1b[0m\
             \x1b]8;;\x1b\\                                "
        );
    }

    /// A single-tilde span inside a link label put BOTH tildes in front of the
    /// label, because the tilde went to the paragraph buffer while the label
    /// text accumulated in its own — characters reordered, not restyled.
    #[test]
    fn a_single_tilde_inside_a_link_label_keeps_its_place() {
        let out = plain("A [~a~ label](https://e.com) here.\n", 60, false);
        assert!(
            out.contains("~a~ label"),
            "tilde moved out of the label: {out:?}"
        );
        assert!(!out.contains("~~a"), "tildes were reordered: {out:?}");
    }

    /// Outside a link there may be no open buffer yet; routing the tilde
    /// through `as_mut()` dropped it and 11 of 102 sweep cases regressed.
    #[test]
    fn a_single_tilde_survives_with_no_buffer_open() {
        let out = plain("~5~10 and ~x~\n", 40, false);
        assert!(out.contains("~5~10"), "tilde dropped: {out:?}");
        assert!(out.contains("~x~"), "tilde dropped: {out:?}");
    }

    /// Table cells render unstyled (#9), but their tildes still pair by
    /// markdown-it's rules: upstream shows `~c~` with the `c` struck.
    #[test]
    fn table_cell_tildes_pair_like_markdown_it() {
        let out = plain("| h |\n|---|\n| ~~~c~~~ |\n", 20, false);
        assert!(out.contains("~c~"), "{out:?}");
        assert!(!out.contains("~~"), "{out:?}");
    }

    /// Tildes in a fenced block are code, not delimiters.
    #[test]
    fn code_block_tildes_are_untouched() {
        let out = plain("```\na ~~~x~~~ b\n```\n", 30, false);
        assert!(out.contains("a ~~~x~~~ b"), "{out:?}");
    }

    /// DIVERGENCES §21: when a tilde pair would cross an emphasis span whose
    /// opener comes after the tilde opener, upstream dissolves the emphasis
    /// (`~~a *b~~ c*` strikes `a *b`); this port keeps pulldown-cmark's emphasis
    /// and leaves the tildes literal. Pinned so a fix shows up here.
    #[test]
    fn tildes_crossing_a_later_emphasis_stay_literal() {
        let out = plain("~~a *b~~ c*", 30, false);
        assert!(out.contains("~~a b~~ c"), "{out:?}");
    }
}
