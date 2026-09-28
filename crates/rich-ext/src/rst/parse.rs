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

/// The body elements of `lines`, which start at column 0.
fn parse_blocks(lines: &[String]) -> Vec<Block> {
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
            blocks.push(block_quote(parse_blocks(&block)));
            i = next;
            continue;
        }
        // Overline and underline.
        if let Some(c) = adornment(line) {
            if i + 2 < lines.len() && !blank(&lines[i + 1]) && adornment(&lines[i + 2]) == Some(c) {
                blocks.push(Block::Title(lines[i + 1].trim().to_string()));
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
        if line.starts_with("+-") && line.ends_with('+') {
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
            blocks.push(Block::Title(line.trim().to_string()));
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

/// A block quote, with its attribution (`-- Author`) split off the end.
fn block_quote(mut children: Vec<Block>) -> Block {
    if let Some(Block::Paragraph(inlines)) = children.last() {
        let text = inline_text(inlines);
        for dash in ["-- ", "--- ", "\u{2014} "] {
            if let Some(rest) = text.strip_prefix(dash) {
                let attribution = parse_inline(rest.trim());
                children.pop();
                children.push(Block::Attribution(attribution));
                break;
            }
        }
    }
    Block::BlockQuote(children)
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
    while i < lines.len() {
        let line = &lines[i];
        if is_simple_table_border(line) {
            borders += 1;
            i += 1;
            let closes = borders >= 2 && (i >= lines.len() || blank(&lines[i]));
            if closes {
                return Some((Block::Container(cells), i));
            }
            continue;
        }
        if blank(line) {
            i += 1;
            continue;
        }
        let chars = display_columns(line);
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

/// A grid table: its cells, each parsed as body elements, in row order.
fn grid_table(lines: &[String], start: usize) -> (Block, usize) {
    let mut end = start;
    while end < lines.len() && (lines[end].starts_with('+') || lines[end].starts_with('|')) {
        end += 1;
    }
    let table = &lines[start..end];
    // Columns are display cells, as docutils' `pad_double_width` makes them.
    let columns: Vec<usize> = table[0]
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == '+')
        .map(|(index, _)| index)
        .collect();
    let mut cells = Vec::new();
    let mut row: Vec<Vec<String>> = vec![Vec::new(); columns.len().saturating_sub(1)];
    for line in &table[1..] {
        if line.starts_with('+') {
            for cell in row.iter_mut() {
                let block = dedent(cell, min_indent(cell));
                if block.iter().any(|line| !blank(line)) {
                    cells.extend(parse_blocks(&block));
                }
                cell.clear();
            }
            continue;
        }
        let chars = display_columns(line);
        for (column, pair) in columns.windows(2).enumerate() {
            let end = pair[1].min(chars.len());
            let text: String = chars[(pair[0] + 1).min(end)..end]
                .iter()
                .flatten()
                .collect();
            row[column].push(text.trim_end().to_string());
        }
    }
    (Block::Container(cells), end)
}
