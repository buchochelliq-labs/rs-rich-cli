//! The block structure of reStructuredText: docutils' `Body` state, for the
//! constructs `rich-rst` renders, into the nodes its visitor sees.

use std::sync::LazyLock;

use fancy_regex::Regex;

use super::inline::parse_inline;

/// Inline markup: the leaves of a paragraph, a title or a list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    /// Plain text, and markup whose text the visitor shows plainly: title
    /// references, footnote references, substitution references, math and
    /// markup docutils cannot read.
    Text(String),
    Emphasis(String),
    Strong(String),
    Literal(String),
    Subscript(String),
    Superscript(String),
    /// A hyperlink: `` `text <uri>`_ ``, a standalone URI, or `name_` (which
    /// has no URI until its target, later in the document, styles it).
    Reference {
        text: String,
        uri: Option<String>,
        name: Option<String>,
    },
    CitationReference(String),
    /// `` _`text` ``: an inline target, whose text the visitor skips.
    Target(String),
}

impl Inline {
    fn text(&self) -> &str {
        match self {
            Inline::Text(text)
            | Inline::Emphasis(text)
            | Inline::Strong(text)
            | Inline::Literal(text)
            | Inline::Subscript(text)
            | Inline::Superscript(text)
            | Inline::CitationReference(text)
            | Inline::Target(text) => text,
            Inline::Reference { text, .. } => text,
        }
    }
}

/// A term of a definition list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinitionItem {
    pub term: String,
    pub classifiers: Vec<String>,
    pub definition: Vec<Block>,
}

/// A field of a field list: `:name: body`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub body: Vec<Block>,
}

/// An item of an option list: its options, each with an optional argument,
/// and the description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionItem {
    pub options: Vec<(String, Option<String>)>,
    pub description: Vec<Block>,
}

/// A body element, as docutils builds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// A section title, a `contents` topic's title or a `rubric`.
    Title(String),
    Paragraph(Vec<Inline>),
    BulletList(Vec<Vec<Block>>),
    EnumeratedList(Vec<Vec<Block>>),
    DefinitionList(Vec<DefinitionItem>),
    FieldList(Vec<Field>),
    OptionList(Vec<OptionItem>),
    /// A literal block (`::`) or a `code` directive with its language.
    LiteralBlock {
        text: String,
        language: Option<String>,
    },
    DoctestBlock(String),
    BlockQuote(Vec<Block>),
    Attribution(Vec<Inline>),
    LineBlock(Vec<String>),
    /// `note`, `warning`, … or the generic `admonition` (with a title).
    Admonition {
        kind: String,
        body: Vec<Block>,
    },
    Transition,
    Image {
        uri: String,
        alt: Option<String>,
        /// The directive's raw text, which `rich-rst` reads options from.
        raw: String,
        /// A `:target:` makes docutils wrap the image in a reference.
        wrapped: bool,
    },
    Footnote {
        label: String,
        body: Vec<Block>,
    },
    Citation {
        label: String,
        body: Vec<Block>,
    },
    MathBlock(String),
    Sidebar {
        title: String,
        subtitle: Option<String>,
        body: Vec<Block>,
    },
    Raw {
        format: String,
        text: String,
    },
    /// An explicit hyperlink target, `.. _name: uri`.
    Target {
        names: Vec<String>,
        uri: String,
    },
    /// Elements the visitor has no method for, so it visits their children:
    /// tables (cell by cell), topics, containers.
    Container(Vec<Block>),
    /// Inline markup outside a paragraph: a substitution definition's text,
    /// which the visitor shows where the definition stands.
    Inlines(Vec<Inline>),
}

impl Block {
    /// docutils' `Node.astext()`: text elements join their inline text; other
    /// elements join their children with a blank line.
    pub fn astext(&self) -> String {
        match self {
            Block::Title(text)
            | Block::DoctestBlock(text)
            | Block::MathBlock(text)
            | Block::LiteralBlock { text, .. }
            | Block::Raw { text, .. } => text.clone(),
            Block::Paragraph(inlines) | Block::Attribution(inlines) | Block::Inlines(inlines) => {
                inline_text(inlines)
            }
            Block::BulletList(items) | Block::EnumeratedList(items) => {
                join(items.iter().map(|item| blocks_text(item)))
            }
            Block::DefinitionList(items) => join(items.iter().map(|item| {
                join(
                    std::iter::once(item.term.clone())
                        .chain(item.classifiers.iter().cloned())
                        .chain(std::iter::once(blocks_text(&item.definition))),
                )
            })),
            Block::FieldList(fields) => join(
                fields
                    .iter()
                    .map(|field| join([field.name.clone(), blocks_text(&field.body)])),
            ),
            Block::OptionList(items) => join(items.iter().map(|item| {
                let options = item
                    .options
                    .iter()
                    .map(|(option, argument)| {
                        format!("{option}{}", argument.as_deref().unwrap_or(""))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                join([options, blocks_text(&item.description)])
            })),
            Block::BlockQuote(children)
            | Block::Container(children)
            | Block::Admonition { body: children, .. } => blocks_text(children),
            Block::LineBlock(lines) => lines.join("\n\n"),
            Block::Transition | Block::Target { .. } | Block::Image { .. } => String::new(),
            // An auto-numbered footnote has no label until a transform
            // numbers it.
            Block::Footnote { label, body } if label.is_empty() => blocks_text(body),
            Block::Footnote { label, body } | Block::Citation { label, body } => {
                join([label.clone(), blocks_text(body)])
            }
            Block::Sidebar {
                title,
                subtitle,
                body,
            } => join(
                std::iter::once(title.clone())
                    .chain(subtitle.clone())
                    .chain(std::iter::once(blocks_text(body))),
            ),
        }
    }
}

/// The text of inline markup, as `TextElement.astext()`.
pub(super) fn inline_text(inlines: &[Inline]) -> String {
    inlines.iter().map(Inline::text).collect()
}

/// `Element.astext()` over body elements: joined with a blank line.
pub(super) fn blocks_text(blocks: &[Block]) -> String {
    join(blocks.iter().map(Block::astext))
}

fn join(parts: impl IntoIterator<Item = String>) -> String {
    parts.into_iter().collect::<Vec<_>>().join("\n\n")
}

/// Parse a reStructuredText document into its body elements.
pub fn parse(source: &str) -> Vec<Block> {
    let lines: Vec<String> = source
        .lines()
        .map(|line| expand_tabs(line.trim_end()))
        .collect();
    Nesting::reset(source.len());
    parse_blocks(&lines)
}

fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::new();
    for c in line.chars() {
        if c == '\t' {
            let spaces = 8 - out.chars().count() % 8;
            out.extend(std::iter::repeat_n(' ', spaces));
        } else {
            out.push(c);
        }
    }
    out
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// Remove `by` columns of indentation from each line (fewer where a line has
/// less), and trailing blank lines.
fn dedent(lines: &[String], by: usize) -> Vec<String> {
    let mut out: Vec<String> = lines
        .iter()
        .map(|line| line.get(by.min(indent(line))..).unwrap_or("").to_string())
        .collect();
    while out.last().is_some_and(|line| blank(line)) {
        out.pop();
    }
    out
}

/// The smallest indentation of the non-blank lines.
fn min_indent(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|line| !blank(line))
        .map(|line| indent(line))
        .min()
        .unwrap_or(0)
}

/// The lines from `start` that are blank or indented: an indented block.
/// Returns the block, dedented by its smallest indentation, and the next
/// line's index.
fn indented_block(lines: &[String], start: usize) -> (Vec<String>, usize) {
    let mut end = start;
    while end < lines.len() && (blank(&lines[end]) || indent(&lines[end]) > 0) {
        end += 1;
    }
    let block = &lines[start..end];
    (dedent(block, min_indent(block)), end)
}

/// An item's lines: the text after its marker, then the indented lines that
/// follow, dedented by their own smallest indentation (docutils'
/// `get_first_known_indented`).
fn item_block(lines: &[String], start: usize, first: &str) -> (Vec<String>, usize) {
    let (rest, end) = indented_block(lines, start + 1);
    let mut block = vec![first.to_string()];
    block.extend(rest);
    while block.last().is_some_and(|line| blank(line)) {
        block.pop();
    }
    (block, end)
}

const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

/// The character a line of section adornment repeats.
fn adornment(line: &str) -> Option<char> {
    let line = line.trim_end();
    let first = line.chars().next()?;
    if !PUNCTUATION.contains(first) || !line.chars().all(|c| c == first) {
        return None;
    }
    Some(first)
}

fn width(text: &str) -> usize {
    rich::cells::cell_len(text)
}

/// Whether `underline` underlines `title`: docutils takes a shorter one only
/// when it is at least four characters.
fn underlines(title: &str, underline: &str) -> bool {
    adornment(underline).is_some()
        && (underline.len() >= width(title.trim()) || underline.len() >= 4)
        && indent(title) == 0
}

static BULLET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([-*+\x{2022}\x{2023}\x{2043}])( +|$)").unwrap());
static ENUMERATOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\(?)([0-9]+|#|[a-zA-Z]|[ivxlcdm]+|[IVXLCDM]+)([.)])( +|$)").unwrap()
});
static FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:((?:\\.|[^:\\])+):( +|$)").unwrap());
static OPTION: LazyLock<Regex> = LazyLock::new(|| {
    let option = r"(?:--?[\w][\w-]*|/\w)(?:[ =](?:<[^>]+>|[\w][\w-]*))?";
    Regex::new(&format!(r"^({option}(?:, {option})*)(  +|$)")).unwrap()
});
static OPTION_PART: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(--?[\w][\w-]*|/\w)([ =](?:<[^>]+>|[\w][\w-]*))?$").unwrap());
static DIRECTIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.\. +([\w][\w:+.-]*[\w])::( +|$)(.*)$").unwrap());
static SUBSTITUTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.\. +\|([^|]+)\| +([\w][\w:+.-]*)::( +|$)(.*)$").unwrap());
static TARGET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.\. +_((?:`[^`]+`)|(?:[^:]|\\:)+):( +|$)(.*)$").unwrap());
static FOOTNOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.\. +\[([^\]]+)\]( +|$)(.*)$").unwrap());

fn captures(regex: &Regex, line: &str) -> Option<Vec<String>> {
    let found = regex.captures(line).ok()??;
    Some(
        (0..found.len())
            .map(|i| {
                found
                    .get(i)
                    .map_or(String::new(), |m| m.as_str().to_string())
            })
            .collect(),
    )
}

/// How deep body elements may nest: lists in lists, quotes in quotes. docutils
/// itself fails with a `RecursionError` well before this (about 170 nested
/// lists), so the cap changes nothing upstream renders; past it the rest of
/// the text is one plain paragraph, so no document can exhaust the stack.
const MAX_DEPTH: usize = 200;

/// Each level of nesting re-reads the indented lines it holds, so a deeply
/// nested document costs its depth times its size. Past [`CHARGED_DEPTH`]
/// the bytes re-read are counted, and once they pass a budget of
/// [`DEEP_BUDGET`] plus [`DEEP_PER_BYTE`] per byte of the document, deeper
/// levels are plain text too. That keeps the parse linear in the document's
/// size, and no document nests this deep and wide in practice.
const CHARGED_DEPTH: usize = 8;
const DEEP_BUDGET: usize = 4 << 20;
const DEEP_PER_BYTE: usize = 8;

thread_local! {
    /// The current nesting depth, the bytes re-read past [`CHARGED_DEPTH`]
    /// in this parse, and the parse's budget for them.
    static NESTING: std::cell::Cell<(usize, usize, usize)> =
        const { std::cell::Cell::new((0, 0, 0)) };
}

/// One level of [`parse_blocks`] nesting, released on drop.
struct Nesting;

impl Nesting {
    fn enter(lines: &[String]) -> Option<Nesting> {
        NESTING.with(|nesting| {
            let (depth, mut work, budget) = nesting.get();
            if depth >= CHARGED_DEPTH {
                work += lines.iter().map(String::len).sum::<usize>();
            }
            (depth < MAX_DEPTH && work <= budget).then(|| {
                nesting.set((depth + 1, work, budget));
                Nesting
            })
        })
    }

    /// Start the parse of a document of `size` bytes, with nothing charged.
    fn reset(size: usize) {
        let budget = DEEP_BUDGET.saturating_add(size.saturating_mul(DEEP_PER_BYTE));
        NESTING.with(|nesting| nesting.set((0, 0, budget)));
    }
}

impl Drop for Nesting {
    fn drop(&mut self) {
        NESTING.with(|nesting| {
            let (depth, work, budget) = nesting.get();
            nesting.set((depth - 1, work, budget));
        });
    }
}

/// The body elements of `lines`, which start at column 0.
fn parse_blocks(lines: &[String]) -> Vec<Block> {
    let Some(_nesting) = Nesting::enter(lines) else {
        let text = lines
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return vec![Block::Paragraph(vec![Inline::Text(text)])];
    };
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if blank(line) {
            i += 1;
            continue;
        }
        if indent(line) > 0 {
            let (block, next) = indented_block(lines, i);
            blocks.extend(block_quotes(block));
            i = next;
            continue;
        }
        // Overline and underline.
        if let Some(c) = adornment(line) {
            if i + 2 < lines.len() && !blank(&lines[i + 1]) && adornment(&lines[i + 2]) == Some(c) {
                blocks.push(Block::Title(inline_text(&parse_inline(
                    lines[i + 1].trim(),
                ))));
                i += 3;
                continue;
            }
            if line.len() >= 4 && (i + 1 == lines.len() || blank(&lines[i + 1])) {
                blocks.push(Block::Transition);
                i += 1;
                continue;
            }
        }
        if let Some(parsed) = parse_list(lines, i) {
            let (block, next) = parsed;
            blocks.push(block);
            i = next;
            continue;
        }
        if captures(&FIELD, line).is_some() {
            let (block, next) = field_list(lines, i);
            blocks.push(block);
            i = next;
            continue;
        }
        if captures(&OPTION, line).is_some() {
            let (block, next) = option_list(lines, i);
            blocks.push(block);
            i = next;
            continue;
        }
        if line.starts_with(">>>") {
            let mut end = i;
            while end < lines.len() && !blank(&lines[end]) {
                end += 1;
            }
            blocks.push(Block::DoctestBlock(lines[i..end].join("\n")));
            i = end;
            continue;
        }
        if line == "|" || line.starts_with("| ") {
            let mut end = i;
            let mut out = Vec::new();
            while end < lines.len() && (lines[end] == "|" || lines[end].starts_with("| ")) {
                let line = lines[end].get(2..).unwrap_or("");
                // An empty line takes the indent of the line before it.
                let depth = if line.trim().is_empty() {
                    out.last().map_or(0, |(depth, _)| *depth)
                } else {
                    indent(line)
                };
                out.push((depth, inline_text(&parse_inline(line.trim_start()))));
                end += 1;
            }
            blocks.push(Block::LineBlock(line_block(&out)));
            i = end;
            continue;
        }
        if line == ".." || line.starts_with(".. ") {
            let (block, next) = explicit_markup(lines, i);
            blocks.extend(block);
            i = next;
            continue;
        }
        if line.starts_with("__ ") {
            // An anonymous target: it names nothing, so it styles nothing.
            let (_, next) = item_block(lines, i, "");
            i = next;
            continue;
        }
        if is_grid_border(line) {
            let (block, next) = grid_table(lines, i);
            blocks.push(block);
            i = next;
            continue;
        }
        if is_simple_table_border(line) {
            if let Some((block, next)) = simple_table(lines, i) {
                blocks.push(block);
                i = next;
                continue;
            }
        }
        // Text: a title, a definition list or a paragraph.
        if i + 1 < lines.len() && underlines(line, &lines[i + 1]) {
            blocks.push(Block::Title(inline_text(&parse_inline(line.trim()))));
            i += 2;
            continue;
        }
        if i + 1 < lines.len() && !blank(&lines[i + 1]) && indent(&lines[i + 1]) > 0 {
            let (block, next) = definition_list(lines, i);
            blocks.push(block);
            i = next;
            continue;
        }
        let mut end = i;
        while end < lines.len() && !blank(&lines[end]) && indent(&lines[end]) == 0 {
            end += 1;
        }
        let text = lines[i..end].join("\n");
        i = end;
        let (text, literal) = literal_marker(&text);
        if !text.is_empty() {
            blocks.push(Block::Paragraph(parse_inline(&text)));
        }
        if literal {
            let mut start = i;
            while start < lines.len() && blank(&lines[start]) {
                start += 1;
            }
            if start < lines.len() && indent(&lines[start]) > 0 {
                let mut end = start;
                while end < lines.len() && (blank(&lines[end]) || indent(&lines[end]) > 0) {
                    end += 1;
                }
                let block = &lines[start..end];
                blocks.push(Block::LiteralBlock {
                    text: dedent(block, min_indent(block)).join("\n"),
                    language: None,
                });
                i = end;
            }
        }
    }
    blocks
}

/// A line block's children: each line at the shallowest indent, and each
/// run of deeper lines as one nested block, whose text joins its lines with
/// a blank line (`Element.astext()`).
fn line_block(lines: &[(usize, String)]) -> Vec<String> {
    let base = lines.iter().map(|(depth, _)| *depth).min().unwrap_or(0);
    let mut children: Vec<String> = Vec::new();
    let mut nested: Option<Vec<&str>> = None;
    for (depth, line) in lines {
        if *depth == base {
            if let Some(run) = nested.take() {
                children.push(run.join("\n\n"));
            }
            children.push(line.clone());
        } else {
            nested.get_or_insert_with(Vec::new).push(line);
        }
    }
    if let Some(run) = nested {
        children.push(run.join("\n\n"));
    }
    children
}

/// A paragraph that ends in `::` introduces a literal block: `::` alone
/// disappears, `text ::` loses it, and `text::` keeps one colon.
fn literal_marker(text: &str) -> (String, bool) {
    if !text.ends_with("::") {
        return (text.to_string(), false);
    }
    let body = &text[..text.len() - 2];
    if body.trim().is_empty() {
        (String::new(), true)
    } else if body.ends_with(char::is_whitespace) {
        (body.trim_end().to_string(), true)
    } else {
        (format!("{body}:"), true)
    }
}

/// An indented block as docutils' `block_quote` splits it: a quote, its
/// attribution (`-- Author` after a blank line), and a new quote for
/// whatever follows the attribution, parsed as it stands.
fn block_quotes(mut lines: Vec<String>) -> Vec<Block> {
    let mut quotes = Vec::new();
    while !lines.is_empty() {
        let (quote, attribution, rest) = split_attribution(&lines);
        let mut children = parse_blocks(quote);
        if let Some(attribution) = attribution {
            children.push(Block::Attribution(parse_inline(&attribution)));
        }
        quotes.push(Block::BlockQuote(children));
        let mut rest = rest.to_vec();
        let leading = rest.iter().take_while(|line| blank(line)).count();
        rest.drain(..leading);
        lines = rest;
    }
    quotes
}

/// docutils' `split_attribution`: the quote's lines, the attribution's text
/// and the lines after it.
fn split_attribution(lines: &[String]) -> (&[String], Option<String>, &[String]) {
    let mut blank_at = None;
    let mut nonblank_seen = false;
    for (i, line) in lines.iter().enumerate() {
        if blank(line) {
            blank_at = Some(i);
            continue;
        }
        if nonblank_seen && blank_at.is_some_and(|at| at + 1 == i) {
            if let Some(marker) = attribution_marker(line) {
                if let Some((end, indent)) = attribution_shape(lines, i) {
                    let text =
                        std::iter::once(&line[marker..])
                            .chain(lines[i + 1..end].iter().map(|line| {
                                line.get(indent.min(self::indent(line))..).unwrap_or("")
                            }))
                            .collect::<Vec<_>>()
                            .join("\n");
                    return (
                        &lines[..i],
                        Some(text.trim_end().to_string()),
                        &lines[end..],
                    );
                }
            }
        }
        nonblank_seen = true;
    }
    (lines, None, &[])
}

/// docutils' `attribution_pattern`, `(---?(?!-)|\u2014) *(?=[^ \n])`: where
/// the attribution's text starts.
fn attribution_marker(line: &str) -> Option<usize> {
    let dashes = if let Some(rest) = line.strip_prefix("---") {
        (!rest.starts_with('-')).then_some(3)?
    } else if let Some(rest) = line.strip_prefix("--") {
        (!rest.starts_with('-')).then_some(2)?
    } else if line.starts_with('\u{2014}') {
        '\u{2014}'.len_utf8()
    } else {
        return None;
    };
    let text = dashes + (line.len() - dashes - line[dashes..].trim_start_matches(' ').len());
    (text < line.len()).then_some(text)
}

/// docutils' `check_attribution`: the attribution's continuation lines must
/// share one indent. Returns the index past its end, and that indent.
fn attribution_shape(lines: &[String], start: usize) -> Option<(usize, usize)> {
    let mut indent = None;
    let mut end = start + 1;
    while end < lines.len() && !blank(&lines[end]) {
        let this = self::indent(&lines[end]);
        if *indent.get_or_insert(this) != this {
            return None;
        }
        end += 1;
    }
    Some((end, indent.unwrap_or(0)))
}

/// The kind of a list marker: the same kind continues the list.
fn list_marker(line: &str) -> Option<(String, String)> {
    if let Some(found) = captures(&BULLET, line) {
        return Some((
            format!("bullet{}", found[1]),
            line[found[0].len()..].to_string(),
        ));
    }
    let found = captures(&ENUMERATOR, line)?;
    let (open, value, close) = (&found[1], &found[2], &found[3]);
    // `(1.` is not a marker; `(1)`, `1)` and `1.` are.
    if !open.is_empty() && close != ")" {
        return None;
    }
    let kind = if value.chars().all(|c| c.is_ascii_digit()) || value == "#" {
        "number"
    } else if value.chars().all(|c| c.is_ascii_lowercase()) {
        "lower"
    } else {
        "upper"
    };
    Some((
        format!("enum{open}{kind}{close}"),
        line[found[0].len()..].to_string(),
    ))
}

fn parse_list(lines: &[String], start: usize) -> Option<(Block, usize)> {
    let (kind, _) = list_marker(&lines[start])?;
    // A lone letter or word followed by text on the next line is a
    // paragraph, not a list: docutils needs a blank line or an indent there.
    if kind.starts_with("enum")
        && start + 1 < lines.len()
        && !blank(&lines[start + 1])
        && indent(&lines[start + 1]) == 0
        && list_marker(&lines[start + 1]).is_none_or(|(next, _)| next != kind)
    {
        return None;
    }
    let mut items = Vec::new();
    let mut i = start;
    while let Some((_, first)) = list_marker(&lines[i]).filter(|(this, _)| *this == kind) {
        let (block, next) = item_block(lines, i, &first);
        items.push(parse_blocks(&block));
        i = next;
        let mut peek = i;
        while peek < lines.len() && blank(&lines[peek]) {
            peek += 1;
        }
        if peek < lines.len() && list_marker(&lines[peek]).is_some_and(|(next, _)| next == kind) {
            i = peek;
        } else {
            break;
        }
    }
    let block = if kind.starts_with("bullet") {
        Block::BulletList(items)
    } else {
        Block::EnumeratedList(items)
    };
    Some((block, i))
}

fn field_list(lines: &[String], start: usize) -> (Block, usize) {
    let mut fields = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let Some(found) = captures(&FIELD, &lines[i]) else {
            break;
        };
        let name = found[1].replace("\\:", ":");
        let first = lines[i][found[0].len()..].to_string();
        let (block, next) = item_block(lines, i, &first);
        fields.push(Field {
            name: inline_text(&parse_inline(&name)),
            body: parse_blocks(&block),
        });
        i = next;
        let mut peek = i;
        while peek < lines.len() && blank(&lines[peek]) {
            peek += 1;
        }
        if peek < lines.len() && captures(&FIELD, &lines[peek]).is_some() {
            i = peek;
        } else {
            break;
        }
    }
    (Block::FieldList(fields), i)
}

fn option_list(lines: &[String], start: usize) -> (Block, usize) {
    let mut items = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let Some(found) = captures(&OPTION, &lines[i]) else {
            break;
        };
        let options = found[1]
            .split(", ")
            .filter_map(|part| {
                let parts = captures(&OPTION_PART, part)?;
                // `option_argument.astext()` keeps its delimiter.
                let argument = Some(parts[2].clone()).filter(|argument| !argument.is_empty());
                Some((parts[1].clone(), argument))
            })
            .collect();
        let first = lines[i][found[0].len()..].to_string();
        let (block, next) = item_block(lines, i, &first);
        items.push(OptionItem {
            options,
            description: parse_blocks(&block),
        });
        i = next;
        let mut peek = i;
        while peek < lines.len() && blank(&lines[peek]) {
            peek += 1;
        }
        if peek < lines.len() && captures(&OPTION, &lines[peek]).is_some() {
            i = peek;
        } else {
            break;
        }
    }
    (Block::OptionList(items), i)
}

fn definition_list(lines: &[String], start: usize) -> (Block, usize) {
    let mut items = Vec::new();
    let mut i = start;
    while i + 1 < lines.len()
        && !blank(&lines[i])
        && indent(&lines[i]) == 0
        && !blank(&lines[i + 1])
        && indent(&lines[i + 1]) > 0
    {
        let mut parts = lines[i].split(" : ");
        let term = inline_text(&parse_inline(parts.next().unwrap_or("")));
        let classifiers = parts
            .map(|part| inline_text(&parse_inline(part.trim())))
            .collect();
        let (block, next) = indented_block(lines, i + 1);
        items.push(DefinitionItem {
            term,
            classifiers,
            definition: parse_blocks(&block),
        });
        i = next;
        let mut peek = i;
        while peek < lines.len() && blank(&lines[peek]) {
            peek += 1;
        }
        if peek + 1 < lines.len()
            && indent(&lines[peek]) == 0
            && !blank(&lines[peek + 1])
            && indent(&lines[peek + 1]) > 0
            && list_marker(&lines[peek]).is_none()
            && !lines[peek].starts_with("..")
        {
            i = peek;
        } else {
            break;
        }
    }
    (Block::DefinitionList(items), i)
}

/// `.. ` explicit markup: a directive, a hyperlink target, a footnote or
/// citation, a substitution definition, or a comment.
fn explicit_markup(lines: &[String], start: usize) -> (Vec<Block>, usize) {
    let line = &lines[start];
    if let Some(found) = captures(&SUBSTITUTION, line) {
        let (block, next) = item_block(lines, start, &found[4]);
        // Without transforms the definition stays in the tree, and the
        // visitor, which has no method for it, shows its text there.
        let blocks = match found[2].to_lowercase().as_str() {
            "replace" => vec![Block::Inlines(parse_inline(&block.join("\n")))],
            "image" => {
                let (body, _) = directive_parts(&block[1..]);
                let option = |key: &str| {
                    body.iter()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.clone())
                };
                vec![Block::Image {
                    uri: block[0].trim().to_string(),
                    alt: option("alt").or_else(|| Some(found[1].clone())),
                    raw: lines[start..next].join("\n"),
                    wrapped: option("target").is_some(),
                }]
            }
            _ => Vec::new(),
        };
        return (blocks, next);
    }
    if let Some(found) = captures(&DIRECTIVE, line) {
        return directive(lines, start, &found[1].to_lowercase(), &found[3]);
    }
    if let Some(found) = captures(&TARGET, line) {
        let (block, next) = item_block(lines, start, &found[3]);
        let name = found[1].trim_matches('`').replace("\\:", ":");
        let uri: String = block.iter().map(|line| line.trim()).collect();
        // An indirect target (`name_`) points at another name, not a URI.
        let blocks = if uri.is_empty() || uri.ends_with('_') {
            Vec::new()
        } else {
            vec![Block::Target {
                names: vec![normalize_name(&name)],
                uri,
            }]
        };
        return (blocks, next);
    }
    if let Some(found) = captures(&FOOTNOTE, line) {
        let (block, next) = item_block(lines, start, &found[3]);
        let label = found[1].clone();
        let body = parse_blocks(&block);
        let footnote =
            label.chars().all(|c| c.is_ascii_digit()) || label.starts_with('#') || label == "*";
        let block = if footnote {
            // Auto-numbered footnotes are numbered by a transform.
            let label = if label.starts_with('#') || label == "*" {
                String::new()
            } else {
                label
            };
            Block::Footnote { label, body }
        } else {
            Block::Citation { label, body }
        };
        return (vec![block], next);
    }
    // A comment: the rest of the line and the indented block after it.
    let (_, next) = item_block(lines, start, "");
    (Vec::new(), next)
}

/// Link names compare lowercased with whitespace collapsed.
pub(super) fn normalize_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A directive's options (`:name: value` lines first) and its content.
fn directive_parts(block: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let mut options = Vec::new();
    let mut i = 0;
    while i < block.len() {
        let Some(found) = captures(&FIELD, &block[i]) else {
            break;
        };
        options.push((
            found[1].to_lowercase(),
            block[i][found[0].len()..].trim().to_string(),
        ));
        i += 1;
    }
    while i < block.len() && blank(&block[i]) {
        i += 1;
    }
    (options, block[i..].to_vec())
}

fn directive(lines: &[String], start: usize, name: &str, argument: &str) -> (Vec<Block>, usize) {
    // The argument and options may continue on indented lines; the content
    // follows a blank line.
    let (rest, next) = indented_block(lines, start + 1);
    let mut argument_lines = vec![argument.trim().to_string()];
    let mut body_start = 0;
    while body_start < rest.len()
        && !blank(&rest[body_start])
        && captures(&FIELD, &rest[body_start]).is_none()
    {
        argument_lines.push(rest[body_start].trim().to_string());
        body_start += 1;
    }
    let argument = argument_lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let (options, content) = directive_parts(&rest[body_start..]);
    let raw = std::iter::once(lines[start].as_str())
        .chain(lines[start + 1..next].iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let option = |key: &str| {
        options
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    };
    let blocks = match name {
        "code" | "code-block" | "sourcecode" => vec![Block::LiteralBlock {
            text: content.join("\n"),
            language: Some(argument.clone()).filter(|language| !language.is_empty()),
        }],
        "note" | "warning" | "tip" | "hint" | "important" | "caution" | "danger" | "error"
        | "attention" => {
            // The argument is the first line of the body.
            let mut body_lines = Vec::new();
            if !argument.is_empty() {
                body_lines.push(argument.clone());
                if !content.is_empty() {
                    body_lines.push(String::new());
                }
            }
            body_lines.extend(content);
            vec![Block::Admonition {
                kind: name.to_string(),
                body: parse_blocks(&body_lines),
            }]
        }
        "admonition" => {
            let mut body = vec![Block::Title(inline_text(&parse_inline(&argument)))];
            body.extend(parse_blocks(&content));
            vec![Block::Admonition {
                kind: name.to_string(),
                body,
            }]
        }
        "image" | "figure" => vec![Block::Image {
            uri: argument.clone(),
            alt: option("alt"),
            raw,
            wrapped: option("target").is_some(),
        }],
        "contents" | "topic" => {
            let title = if argument.is_empty() && name == "contents" {
                "Contents".to_string()
            } else {
                argument.clone()
            };
            let mut blocks = vec![Block::Title(title)];
            if name == "topic" {
                blocks.extend(parse_blocks(&content));
            }
            vec![Block::Container(blocks)]
        }
        "rubric" => vec![Block::Title(inline_text(&parse_inline(&argument)))],
        "math" => {
            let mut text = vec![argument.clone()];
            text.extend(content);
            vec![Block::MathBlock(
                text.into_iter()
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )]
        }
        "sidebar" => vec![Block::Sidebar {
            title: argument.clone(),
            subtitle: option("subtitle"),
            body: parse_blocks(&content),
        }],
        "raw" => vec![Block::Raw {
            format: argument.clone(),
            text: content.join("\n"),
        }],
        "container" | "compound" => vec![Block::Container(parse_blocks(&content))],
        _ => Vec::new(),
    };
    (blocks, next)
}

/// A table row by display cell: a wide character takes its cell and a
/// `None` after it, so column offsets from the ASCII border line up.
fn display_columns(line: &str) -> Vec<Option<char>> {
    let mut cells = Vec::new();
    for c in line.chars() {
        cells.push(Some(c));
        for _ in 1..width(c.encode_utf8(&mut [0; 4])) {
            cells.push(None);
        }
    }
    cells
}

fn is_simple_table_border(line: &str) -> bool {
    line.starts_with('=') && line.contains(' ') && line.chars().all(|c| c == '=' || c == ' ')
}

/// A simple table: its cells, each a paragraph, in row order.
fn simple_table(lines: &[String], start: usize) -> Option<(Block, usize)> {
    // Columns are display cells, as docutils' `pad_double_width` makes them.
    let border = &lines[start];
    let mut columns = Vec::new();
    let mut in_column = None;
    for (index, c) in border.chars().enumerate() {
        match (c, in_column) {
            ('=', None) => in_column = Some(index),
            (' ', Some(from)) => {
                columns.push((from, index));
                in_column = None;
            }
            _ => {}
        }
    }
    if let Some(from) = in_column {
        columns.push((from, usize::MAX));
    }
    let mut borders = 0;
    let mut i = start;
    let mut cells = Vec::new();
    let mut malformed = false;
    while i < lines.len() {
        let line = &lines[i];
        if is_simple_table_border(line) {
            borders += 1;
            i += 1;
            let closes = borders >= 2 && (i >= lines.len() || blank(&lines[i]));
            if closes {
                // Text in a column margin is docutils' "Malformed table"
                // error, which renders nothing.
                let cells = if malformed { Vec::new() } else { cells };
                return Some((Block::Container(cells), i));
            }
            continue;
        }
        if blank(line) {
            i += 1;
            continue;
        }
        let chars = display_columns(line);
        malformed |= columns.windows(2).any(|pair| {
            let margin = &chars[pair[0].1.min(chars.len())..pair[1].0.min(chars.len())];
            margin.iter().flatten().any(|c| !c.is_whitespace())
        });
        for (column, &(from, to)) in columns.iter().enumerate() {
            let end = if column + 1 == columns.len() {
                chars.len()
            } else {
                to.min(chars.len())
            };
            let cell: String = chars[from.min(end)..end].iter().flatten().collect();
            let cell = cell.trim();
            if !cell.is_empty() {
                cells.push(Block::Paragraph(parse_inline(cell)));
            }
        }
        i += 1;
    }
    None
}

/// docutils' `grid_table_top_pat`, `\+-[-+]+-\+ *$`.
fn is_grid_border(line: &str) -> bool {
    line.len() >= 5
        && line.starts_with("+-")
        && line.ends_with("-+")
        && line.chars().all(|c| c == '-' || c == '+')
}

/// A grid table row by display cell, as docutils' `pad_double_width` and
/// `strip_combining_chars` leave it for tracing borders: a wide character
/// takes its cell and an empty one after it, and a zero-width character
/// joins the cell before it.
fn grid_cells(line: &str) -> Vec<String> {
    let mut cells: Vec<String> = Vec::new();
    for c in line.chars() {
        let cell_width = width(c.encode_utf8(&mut [0; 4]));
        match cells.last_mut() {
            Some(last) if cell_width == 0 => last.push(c),
            _ => cells.push(c.to_string()),
        }
        for _ in 1..cell_width {
            cells.push(String::new());
        }
    }
    cells
}

/// A grid table: its cells, each parsed as body elements, in row order. A
/// table docutils cannot trace is an error, which renders nothing.
fn grid_table(lines: &[String], start: usize) -> (Block, usize) {
    // `isolate_grid_table`: the lines that start with `+` or `|`, up to the
    // last border.
    let mut end = start;
    while end < lines.len() && (lines[end].starts_with('+') || lines[end].starts_with('|')) {
        end += 1;
    }
    let malformed = |next: usize| (Block::Container(Vec::new()), next);
    let mut rows = end - start;
    let mut next = end;
    if !is_grid_border(&lines[end - 1]) {
        let Some(bottom) = (2..rows.saturating_sub(1))
            .rev()
            .find(|&i| is_grid_border(&lines[start + i]))
        else {
            return malformed(end);
        };
        rows = bottom + 1;
        // docutils steps back one line too many here, so the row before the
        // bottom border is read again after the table.
        next = start + bottom - 1;
    }
    let grid: Vec<Vec<String>> = lines[start..start + rows]
        .iter()
        .map(|line| grid_cells(line))
        .collect();
    let right_edge = grid[0].len();
    if grid
        .iter()
        .any(|row| row.len() != right_edge || !row.last().is_some_and(|c| c == "+" || c == "|"))
    {
        return malformed(next);
    }
    match GridTable::parse(grid) {
        Some(cells) => (Block::Container(cells), next),
        None => malformed(next),
    }
}

/// docutils' `GridTableParser`: trace each cell from its top-left corner
/// along its borders, so cells may span rows and columns.
struct GridTable {
    grid: Vec<Vec<String>>,
    bottom: usize,
    right: usize,
    /// For each text column, the last row seen.
    done: Vec<Option<usize>>,
}

impl GridTable {
    fn parse(mut grid: Vec<Vec<String>>) -> Option<Vec<Block>> {
        // The head/body separator reads as a row border; there may be one,
        // neither first nor last.
        let separators: Vec<usize> = grid
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                let line: String = row.concat();
                line.len() >= 5
                    && line.starts_with("+=")
                    && line.ends_with("=+")
                    && line.chars().all(|c| c == '=' || c == '+')
            })
            .map(|(i, _)| i)
            .collect();
        match separators.as_slice() {
            [] => {}
            [i] if *i != 0 && *i != grid.len() - 1 => {
                for cell in grid[*i].iter_mut() {
                    if cell == "=" {
                        *cell = "-".into();
                    }
                }
            }
            _ => return None,
        }
        let bottom = grid.len() - 1;
        let right = grid[0].len() - 1;
        if bottom == 0 {
            // A lone border: a table of no rows.
            return Some(Vec::new());
        }
        let mut table = GridTable {
            grid,
            bottom,
            right,
            done: vec![None; right + 1],
        };
        let mut found = Vec::new();
        let mut corners = vec![(0, 0)];
        while !corners.is_empty() {
            let (top, left) = corners.remove(0);
            if top == table.bottom
                || left == table.right
                || table.done[left].is_some_and(|done| top <= done)
            {
                continue;
            }
            let Some((bottom, right)) = table.scan_right(top, left) else {
                continue;
            };
            for col in left..right {
                table.done[col] = Some(bottom - 1);
            }
            found.push((top, left, bottom, right));
            corners.push((top, right));
            corners.push((bottom, left));
            corners.sort_unstable();
        }
        // Each text column must be seen to the bottom.
        if table.done[..table.right]
            .iter()
            .any(|&done| done != Some(table.bottom - 1))
        {
            return None;
        }
        found.sort_unstable();
        let mut cells = Vec::new();
        for (top, left, bottom, right) in found {
            // `get_2D_block`: the cell's text, each line stripped on the
            // right, then dedented by the smallest indent.
            let block: Vec<String> = table.grid[top + 1..bottom]
                .iter()
                .map(|row| row[left + 1..right].concat().trim_end().to_string())
                .collect();
            let block = dedent(&block, min_indent(&block));
            if block.iter().any(|line| !blank(line)) {
                cells.extend(parse_blocks(&block));
            }
        }
        Some(cells)
    }

    fn at(&self, row: usize, col: usize) -> &str {
        self.grid[row][col].as_str()
    }

    fn scan_right(&self, top: usize, left: usize) -> Option<(usize, usize)> {
        if self.at(top, left) != "+" {
            return None;
        }
        for i in left + 1..=self.right {
            match self.at(top, i) {
                "+" => {
                    if let Some(bottom) = self.scan_down(top, left, i) {
                        return Some((bottom, i));
                    }
                }
                "-" => {}
                _ => return None,
            }
        }
        None
    }

    fn scan_down(&self, top: usize, left: usize, right: usize) -> Option<usize> {
        for i in top + 1..=self.bottom {
            match self.at(i, right) {
                "+" => {
                    if self.scan_left(top, left, i, right) {
                        return Some(i);
                    }
                }
                "|" => {}
                _ => return None,
            }
        }
        None
    }

    fn scan_left(&self, top: usize, left: usize, bottom: usize, right: usize) -> bool {
        (left + 1..right).all(|i| matches!(self.at(bottom, i), "+" | "-"))
            && self.at(bottom, left) == "+"
            && (top + 1..bottom).all(|i| matches!(self.at(i, left), "+" | "|"))
    }
}
