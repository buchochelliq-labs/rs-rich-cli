//! The block structure of reStructuredText: docutils' `Body` state machine,
//! for the constructs `rich-rst` renders, into a document tree
//! ([`Element`]) as docutils builds it before its transforms.

use std::sync::LazyLock;

use fancy_regex::Regex;

use super::doctree::{Element, Node};
use super::inline::{inline_nodes, parse_inline};

/// Parse a reStructuredText document into its tree, with the transforms
/// `docutils.core.publish_doctree` applies.
pub fn parse(source: &str) -> Element {
    let mut document = parse_untransformed(source);
    super::transforms::apply(&mut document);
    document
}

/// The document as the parser leaves it, before the transforms.
pub(super) fn parse_untransformed(source: &str) -> Element {
    let lines: Vec<String> = source
        .lines()
        .map(|line| expand_tabs(line.trim_end()))
        .collect();
    Nesting::reset(source.len());
    let body = parse_body(&lines, true);
    let mut document = Element::new("document");
    document.children = sections(body);
    document
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
static OPTION_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(--?[\w][\w-]*|/\w)(?:([ =])(<[^>]+>|[\w][\w-]*))?$").unwrap()
});
static DIRECTIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.\. +([\w][\w:+.-]*[\w]|[\w])::( +|$)(.*)$").unwrap());
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

/// One level of [`parse_body`] nesting, released on drop.
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

fn inlines(tag: &'static str, text: &str) -> Element {
    Element::new(tag).children(inline_nodes(parse_inline(text)))
}

fn paragraph(text: &str) -> Element {
    inlines("paragraph", text)
}

/// A system message. The visitor shows its literal block (the offending
/// source) unless the message is about a title.
fn message(kind: &str, text: &str, source: Option<String>) -> Element {
    let mut message = Element::new("system_message")
        .with("type", kind)
        .child(Element::text("paragraph", text));
    if let Some(source) = source {
        message = message.child(Element::text("literal_block", source));
    }
    message
}

/// A section title found in the body, nested into sections at the top.
fn title_marker(style: String, text: &str, source: String) -> Element {
    Element::new("__title")
        .with("style", style)
        .with("source", source)
        .child(inlines("title", text))
}

/// The body elements of the top level into sections, by title style:
/// docutils' `check_subsection`.
fn sections(body: Vec<Node>) -> Vec<Node> {
    fn add(stack: &mut [Element], root: &mut Vec<Node>, node: Node) {
        match stack.last_mut() {
            Some(section) => section.children.push(node),
            None => root.push(node),
        }
    }
    fn close(stack: &mut Vec<Element>, root: &mut Vec<Node>) {
        let section = stack.pop().expect("an open section");
        add(stack, root, Node::Element(section));
    }
    let mut styles: Vec<String> = Vec::new();
    // The open sections, outermost first, and the document's children.
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Vec<Node> = Vec::new();
    for node in body {
        let Node::Element(mut element) = node else {
            add(&mut stack, &mut root, node);
            continue;
        };
        if element.tag != "__title" {
            add(&mut stack, &mut root, Node::Element(element));
            continue;
        }
        let style = element.get("style").unwrap_or("").to_string();
        let source = element.get("source").unwrap_or("").to_string();
        let old = stack.len();
        let new = styles
            .iter()
            .position(|s| *s == style)
            .map_or(styles.len() + 1, |i| i + 1);
        if new > old + 1 {
            let error = message(
                "ERROR",
                &format!("Inconsistent title style: skip from level {old} to {new}."),
                Some(source),
            );
            add(&mut stack, &mut root, Node::Element(error));
            continue;
        }
        while stack.len() >= new {
            close(&mut stack, &mut root);
        }
        if new > styles.len() {
            styles.push(style);
        }
        let title = element.children.remove(0);
        let mut section = Element::new("section");
        section.names = vec![normalize(&title.astext())];
        stack.push(section.child(title));
    }
    while !stack.is_empty() {
        close(&mut stack, &mut root);
    }
    root
}

/// The body elements of `lines`, which start at column 0. `titles`: section
/// titles may stand here (the document's top level); elsewhere one is an
/// error, which shows nothing.
fn parse_body(lines: &[String], titles: bool) -> Vec<Node> {
    let Some(_nesting) = Nesting::enter(lines) else {
        let text = lines
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return vec![Element::text("paragraph", text).into()];
    };
    let mut out: Vec<Node> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if blank(line) {
            i += 1;
            continue;
        }
        if indent(line) > 0 {
            let (block, next) = indented_block(lines, i);
            out.extend(block_quotes(block).into_iter().map(Node::from));
            i = next;
            continue;
        }
        if let Some((list, next)) = parse_list(lines, i) {
            out.push(list.into());
            i = next;
            continue;
        }
        if captures(&FIELD, line).is_some() {
            let (block, next) = field_list(lines, i);
            out.push(block.into());
            i = next;
            continue;
        }
        if captures(&OPTION, line).is_some() {
            let (block, next) = option_list(lines, i);
            out.push(block.into());
            i = next;
            continue;
        }
        if line.starts_with(">>>") && (line.len() == 3 || line[3..].starts_with(' ')) {
            let mut end = i;
            while end < lines.len() && !blank(&lines[end]) {
                end += 1;
            }
            out.push(Element::text("doctest_block", lines[i..end].join("\n")).into());
            i = end;
            continue;
        }
        if line == "|" || line.starts_with("| ") {
            let (block, next, finished) = line_block(lines, i);
            out.push(block.into());
            if !finished {
                out.push(message("WARNING", "Line block ends without a blank line.", None).into());
            }
            i = next;
            continue;
        }
        if is_grid_border(line) {
            let (nodes, next) = grid_table(lines, i);
            out.extend(nodes);
            i = next;
            continue;
        }
        if is_simple_table_border(line) {
            if let Some((nodes, next)) = simple_table(lines, i) {
                out.extend(nodes);
                i = next;
                continue;
            }
        }
        if line == ".." || line.starts_with(".. ") {
            let (nodes, next) = explicit_markup(lines, i);
            out.extend(nodes);
            i = next;
            continue;
        }
        if line == "__" || line.starts_with("__ ") {
            let (block, next) = item_block(lines, i, line.get(3..).unwrap_or(""));
            let uri: String = block.iter().map(|line| line.trim()).collect();
            let mut target = Element::new("target").with("anonymous", "1");
            if uri.ends_with('_') && !uri.ends_with("\\_") {
                target = target.with("refname", normalize(&uri[..uri.len() - 1]));
            } else {
                target = target.with("refuri", uri);
            }
            out.push(target.into());
            i = next;
            continue;
        }
        // Overline, title, underline; or a transition.
        if let Some(c) = adornment(line) {
            if i + 2 < lines.len() && !blank(&lines[i + 1]) && adornment(&lines[i + 2]) == Some(c)
            {
                let text = lines[i + 1].trim();
                let source = lines[i..i + 3].join("\n");
                if titles {
                    if lines[i].len() < width(text) {
                        out.push(
                            message("WARNING", "Title overline too short.", Some(source.clone()))
                                .into(),
                        );
                    }
                    out.push(title_marker(format!("{c}{c}"), text, source).into());
                } else {
                    out.push(message("SEVERE", "Unexpected section title.", Some(source)).into());
                }
                i += 3;
                continue;
            }
            if line.len() >= 4 && (i + 1 == lines.len() || blank(&lines[i + 1])) {
                out.push(Element::new("transition").into());
                i += 1;
                continue;
            }
        }
        // Text: a title, a definition list or a paragraph.
        if i + 1 < lines.len() && underlines(line, &lines[i + 1]) {
            let source = lines[i..i + 2].join("\n");
            let c = adornment(&lines[i + 1]).unwrap_or('=');
            if titles {
                out.push(title_marker(c.to_string(), line.trim(), source).into());
            } else {
                out.push(message("SEVERE", "Unexpected section title.", Some(source)).into());
            }
            i += 2;
            continue;
        }
        if i + 1 < lines.len() && !blank(&lines[i + 1]) && indent(&lines[i + 1]) > 0 {
            let (block, next) = definition_list(lines, i);
            out.push(block.into());
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
            out.push(paragraph(&text).into());
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
                out.push(
                    Element::text("literal_block", dedent(block, min_indent(block)).join("\n"))
                        .into(),
                );
                i = end;
            }
        }
    }
    out
}

/// A line block from `start`: its lines, nested by indent, the next line's
/// index, and whether a blank line (or the end) closed it.
fn line_block(lines: &[String], start: usize) -> (Element, usize, bool) {
    // Each line: its indent (None for an empty one) and its text, with the
    // indented lines that continue it.
    let mut items: Vec<(Option<usize>, Element)> = Vec::new();
    let mut i = start;
    while i < lines.len() && (lines[i] == "|" || lines[i].starts_with("| ")) {
        let after = &lines[i][1..];
        let spaces = after.len() - after.trim_start_matches(' ').len();
        let mut text = vec![after.trim_start().to_string()];
        let mut j = i + 1;
        while j < lines.len() && !blank(&lines[j]) && indent(&lines[j]) > 0 {
            text.push(lines[j].trim_start().to_string());
            j += 1;
        }
        let empty = lines[i].trim_end() == "|";
        let line = inlines("line", &text.join("\n"));
        items.push(((!empty).then(|| spaces.saturating_sub(1)), line));
        i = j;
    }
    let finished = i >= lines.len() || blank(&lines[i]);
    // `nest_line_block_lines`: an empty line takes the indent before it.
    let mut last = 0;
    let items: Vec<(usize, Element)> = items
        .into_iter()
        .map(|(indent, line)| {
            let indent = indent.unwrap_or(last);
            last = indent;
            (indent, line)
        })
        .collect();
    (nest_line_block(items), i, finished)
}

/// `nest_line_block_segment`: lines deeper than the least indented go into
/// a nested line block.
fn nest_line_block(items: Vec<(usize, Element)>) -> Element {
    let least = items.iter().map(|(indent, _)| *indent).min().unwrap_or(0);
    let mut block = Element::new("line_block");
    let mut deeper: Vec<(usize, Element)> = Vec::new();
    for (indent, line) in items {
        if indent > least {
            deeper.push((indent, line));
        } else {
            if !deeper.is_empty() {
                block = block.child(nest_line_block(std::mem::take(&mut deeper)));
            }
            block = block.child(line);
        }
    }
    if !deeper.is_empty() {
        block = block.child(nest_line_block(deeper));
    }
    block
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
fn block_quotes(mut lines: Vec<String>) -> Vec<Element> {
    let mut quotes = Vec::new();
    while !lines.is_empty() {
        let (quote, attribution, rest) = split_attribution(&lines);
        let mut quote_node = Element::new("block_quote").children(parse_body(quote, false));
        if let Some(attribution) = attribution {
            quote_node = quote_node.child(inlines("attribution", &attribution));
        }
        quotes.push(quote_node);
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

/// docutils' `attribution_pattern`, `(---?(?!-)|—) *(?=[^ \n])`: where
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

/// A list marker: its kind (the same kind continues the list), its text,
/// the text after it, and for an enumerator its sequence and ordinal.
#[derive(Clone, Debug, PartialEq)]
struct Marker {
    kind: String,
    text: String,
    rest: String,
    sequence: Option<String>,
    ordinal: Option<i64>,
    prefix: String,
    suffix: String,
}

/// docutils' `parse_enumerator`: the sequence of an enumerator's text,
/// trying `expected` first.
fn sequence_of(text: &str, expected: Option<&str>) -> Option<String> {
    if text == "#" {
        return Some("#".into());
    }
    let matches = |sequence: &str| match sequence {
        "arabic" => text.chars().all(|c| c.is_ascii_digit()),
        "loweralpha" => text.len() == 1 && text.chars().all(|c| c.is_ascii_lowercase()),
        "upperalpha" => text.len() == 1 && text.chars().all(|c| c.is_ascii_uppercase()),
        "lowerroman" => text.chars().all(|c| "ivxlcdm".contains(c)),
        "upperroman" => text.chars().all(|c| "IVXLCDM".contains(c)),
        _ => false,
    };
    if let Some(expected) = expected {
        if matches(expected) {
            return Some(expected.to_string());
        }
    } else if text == "i" {
        return Some("lowerroman".into());
    } else if text == "I" {
        return Some("upperroman".into());
    }
    [
        "arabic",
        "loweralpha",
        "upperalpha",
        "lowerroman",
        "upperroman",
    ]
    .into_iter()
    .find(|sequence| matches(sequence))
    .map(String::from)
}

fn roman_value(text: &str) -> Option<i64> {
    let upper = text.to_uppercase();
    let value = |c: char| match c {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        'M' => 1000,
        _ => 0,
    };
    let chars: Vec<i64> = upper.chars().map(value).collect();
    let mut total = 0;
    for (i, &v) in chars.iter().enumerate() {
        if chars.get(i + 1).is_some_and(|&next| next > v) {
            total -= v;
        } else {
            total += v;
        }
    }
    // Only canonical numerals are valid.
    (total > 0 && total < 4000 && roman(total) == upper).then_some(total)
}

/// An upper-case Roman numeral.
pub(super) fn roman(mut n: i64) -> String {
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, numeral) in TABLE {
        while n >= value {
            out.push_str(numeral);
            n -= value;
        }
    }
    out
}

fn ordinal_of(sequence: &str, text: &str) -> Option<i64> {
    match sequence {
        "#" => Some(1),
        "arabic" => text.parse().ok(),
        "loweralpha" | "upperalpha" => {
            Some(i64::from(text.to_ascii_lowercase().as_bytes()[0] - b'a') + 1)
        }
        _ => roman_value(text),
    }
}

/// `make_enumerator`: the marker of `ordinal` in `sequence`.
fn enumerator_text(sequence: &str, ordinal: i64) -> Option<String> {
    Some(match sequence {
        "#" => "#".into(),
        "arabic" => ordinal.to_string(),
        "loweralpha" | "upperalpha" => {
            if !(1..=26).contains(&ordinal) {
                return None;
            }
            let c = char::from(b'a' + u8::try_from(ordinal - 1).ok()?);
            if sequence == "upperalpha" {
                c.to_ascii_uppercase().to_string()
            } else {
                c.to_string()
            }
        }
        _ => {
            if !(1..4000).contains(&ordinal) {
                return None;
            }
            let numeral = roman(ordinal);
            if sequence == "lowerroman" {
                numeral.to_lowercase()
            } else {
                numeral
            }
        }
    })
}

fn list_marker(line: &str, expected: Option<&str>) -> Option<Marker> {
    if let Some(found) = captures(&BULLET, line) {
        return Some(Marker {
            kind: "bullet".into(),
            text: found[1].clone(),
            rest: line[found[0].len()..].to_string(),
            sequence: None,
            ordinal: None,
            prefix: String::new(),
            suffix: String::new(),
        });
    }
    let found = captures(&ENUMERATOR, line)?;
    let (open, value, close) = (&found[1], &found[2], &found[3]);
    // `(1.` is not a marker; `(1)`, `1)` and `1.` are.
    if !open.is_empty() && close != ")" {
        return None;
    }
    let sequence = sequence_of(value, expected)?;
    let ordinal = ordinal_of(&sequence, value);
    Some(Marker {
        kind: format!("enum{open}{close}"),
        text: value.clone(),
        rest: line[found[0].len()..].to_string(),
        sequence: Some(sequence),
        ordinal,
        prefix: open.clone(),
        suffix: close.clone(),
    })
}

/// `is_enumerated_list_item`: the ordinal is valid and the next line is
/// blank, indented, or the next enumerator.
fn is_enumerated_item(lines: &[String], i: usize, marker: &Marker) -> bool {
    let (Some(sequence), Some(ordinal)) = (&marker.sequence, marker.ordinal) else {
        return false;
    };
    let Some(next) = lines.get(i + 1) else {
        return true;
    };
    if next.chars().next().is_none_or(char::is_whitespace) {
        return true;
    }
    let Some(following) = enumerator_text(sequence, ordinal + 1) else {
        return false;
    };
    let (open, close) = (&marker.prefix, &marker.suffix);
    next.starts_with(&format!("{open}{following}{close}"))
        || next.starts_with(&format!("{open}#{close}"))
}

fn parse_list(lines: &[String], start: usize) -> Option<(Element, usize)> {
    let first = list_marker(&lines[start], None)?;
    if first.sequence.is_some() && !is_enumerated_item(lines, start, &first) {
        return None;
    }
    let mut list = match &first.sequence {
        None => Element::new("bullet_list").with("bullet", first.text.clone()),
        Some(sequence) => {
            let enumtype = if sequence == "#" {
                "arabic"
            } else {
                sequence.as_str()
            };
            let mut list = Element::new("enumerated_list")
                .with("enumtype", enumtype)
                .with("prefix", first.prefix.clone())
                .with("suffix", first.suffix.clone());
            if let Some(ordinal) = first.ordinal.filter(|&o| o != 1) {
                list = list.with("start", ordinal.to_string());
            }
            list
        }
    };
    let enumtype = list.get("enumtype").map(String::from);
    let mut auto = first.sequence.as_deref() == Some("#");
    let mut last = first.ordinal.unwrap_or(0);
    let mut i = start;
    let mut marker = first;
    loop {
        let (block, next) = item_block(lines, i, &marker.rest);
        list = list.child(Element::new("list_item").children(parse_body(&block, false)));
        i = next;
        let mut peek = i;
        while peek < lines.len() && blank(&lines[peek]) {
            peek += 1;
        }
        if peek >= lines.len() || indent(&lines[peek]) > 0 {
            break;
        }
        let Some(next_marker) = list_marker(&lines[peek], enumtype.as_deref()) else {
            break;
        };
        if next_marker.kind != marker.kind {
            break;
        }
        match &next_marker.sequence {
            Some(sequence) => {
                let continues = sequence == "#"
                    || (Some(sequence.as_str()) == enumtype.as_deref()
                        && !auto
                        && next_marker.ordinal == Some(last + 1));
                if !continues || !is_enumerated_item(lines, peek, &next_marker) {
                    break;
                }
                if sequence == "#" {
                    auto = true;
                }
                last = next_marker.ordinal.unwrap_or(last + 1);
            }
            None if next_marker.text != marker.text => break,
            None => {}
        }
        marker = next_marker;
        i = peek;
    }
    Some((list, i))
}

fn field_list(lines: &[String], start: usize) -> (Element, usize) {
    let mut list = Element::new("field_list");
    let mut i = start;
    while i < lines.len() {
        let Some(found) = captures(&FIELD, &lines[i]) else {
            break;
        };
        let name = found[1].clone();
        let first = lines[i][found[0].len()..].to_string();
        let (block, next) = item_block(lines, i, &first);
        let block: Vec<String> = block.into_iter().skip_while(|line| blank(line)).collect();
        list = list.child(
            Element::new("field")
                .child(inlines("field_name", &name))
                .child(Element::new("field_body").children(parse_body(&block, false))),
        );
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
    (list, i)
}

fn option_list(lines: &[String], start: usize) -> (Element, usize) {
    let mut list = Element::new("option_list");
    let mut i = start;
    while i < lines.len() {
        let Some(found) = captures(&OPTION, &lines[i]) else {
            break;
        };
        let mut group = Element::new("option_group");
        for part in found[1].split(", ") {
            let Some(parts) = captures(&OPTION_PART, part) else {
                continue;
            };
            let mut option =
                Element::new("option").child(Element::text("option_string", parts[1].clone()));
            if !parts[3].is_empty() {
                option = option.child(
                    Element::text("option_argument", parts[3].clone())
                        .with("delimiter", parts[2].clone()),
                );
            }
            group = group.child(option);
        }
        let first = lines[i][found[0].len()..].to_string();
        let (block, next) = item_block(lines, i, &first);
        let block: Vec<String> = block.into_iter().skip_while(|line| blank(line)).collect();
        list = list.child(
            Element::new("option_list_item")
                .child(group)
                .child(Element::new("description").children(parse_body(&block, false))),
        );
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
    (list, i)
}

fn definition_list(lines: &[String], start: usize) -> (Element, usize) {
    let mut list = Element::new("definition_list");
    let mut i = start;
    while i + 1 < lines.len()
        && !blank(&lines[i])
        && indent(&lines[i]) == 0
        && !blank(&lines[i + 1])
        && indent(&lines[i + 1]) > 0
    {
        let (block, next) = indented_block(lines, i + 1);
        let mut item = Element::new("definition_list_item");
        let mut parts = lines[i].split(" : ");
        item = item.child(inlines("term", parts.next().unwrap_or("")));
        for part in parts {
            item = item.child(inlines("classifier", part.trim()));
        }
        item = item.child(Element::new("definition").children(parse_body(&block, false)));
        list = list.child(item);
        i = next;
        let mut peek = i;
        while peek < lines.len() && blank(&lines[peek]) {
            peek += 1;
        }
        if peek + 1 < lines.len()
            && indent(&lines[peek]) == 0
            && !blank(&lines[peek + 1])
            && indent(&lines[peek + 1]) > 0
            && list_marker(&lines[peek], None).is_none()
            && !lines[peek].starts_with("..")
        {
            i = peek;
        } else {
            break;
        }
    }
    (list, i)
}

/// Link names compare lowercased with whitespace collapsed.
pub(super) fn normalize(name: &str) -> String {
    super::inline::normalize_name(name)
}

/// `.. ` explicit markup: a directive, a hyperlink target, a footnote or
/// citation, a substitution definition, or a comment.
fn explicit_markup(lines: &[String], start: usize) -> (Vec<Node>, usize) {
    let line = &lines[start];
    if let Some(found) = captures(&SUBSTITUTION, line) {
        let name = found[1].clone();
        let (nodes, next) = directive(lines, start, &found[2].to_lowercase(), &found[4], true);
        let mut definition = Element::new("substitution_definition");
        definition.names = vec![normalize(&name)];
        definition.attrs.insert("name", name.clone());
        for node in nodes {
            match node {
                // An image's alt text defaults to the substitution's name.
                Node::Element(mut image) if image.tag == "image" => {
                    if image.get("alt").is_none() {
                        image.attrs.insert("alt", name.clone());
                    }
                    definition.children.push(Node::Element(image));
                }
                Node::Element(paragraph) if paragraph.tag == "paragraph" => {
                    definition.children.extend(paragraph.children);
                }
                Node::Element(reference) if reference.tag == "reference" => {
                    definition.children.push(Node::Element(reference));
                }
                // An error in the definition stands after it.
                Node::Element(error) if error.tag == "system_message" => {
                    return (vec![definition.into(), error.into()], next);
                }
                _ => {}
            }
        }
        return (vec![definition.into()], next);
    }
    if let Some(found) = captures(&DIRECTIVE, line) {
        return directive(lines, start, &found[1].to_lowercase(), &found[3], false);
    }
    if let Some(found) = captures(&TARGET, line) {
        let (block, next) = item_block(lines, start, &found[3]);
        let name = found[1].trim_matches('`').replace("\\:", ":");
        let uri: String = block.iter().map(|line| line.trim()).collect();
        let mut target = Element::new("target");
        target.names = vec![normalize(&name)];
        if uri.ends_with('_') && !uri.ends_with("\\_") {
            // An indirect target: another name.
            let other = uri[..uri.len() - 1].trim_matches('`');
            target = target.with("refname", normalize(other));
        } else if !uri.is_empty() {
            target = target.with("refuri", uri);
        }
        return (vec![target.into()], next);
    }
    if let Some(found) = captures(&FOOTNOTE, line) {
        let (block, next) = item_block(lines, start, &found[3]);
        let block: Vec<String> = block.into_iter().skip_while(|line| blank(line)).collect();
        let label = found[1].clone();
        let body = parse_body(&block, false);
        let footnote =
            label.chars().all(|c| c.is_ascii_digit()) || label.starts_with('#') || label == "*";
        let node = if footnote {
            let mut note = Element::new("footnote");
            if label == "*" {
                note = note.with("auto", "*");
            } else if let Some(name) = label.strip_prefix('#') {
                note = note.with("auto", "1");
                if !name.is_empty() {
                    note.names = vec![normalize(name)];
                }
            } else {
                note.names = vec![label.clone()];
                note = note.child(Element::text("label", label));
            }
            note.children(body)
        } else {
            let mut citation =
                Element::new("citation").child(Element::text("label", label.clone()));
            citation.names = vec![normalize(&label)];
            citation.children(body)
        };
        return (vec![node.into()], next);
    }
    // A comment: the rest of the line and the indented block after it.
    let (_, next) = item_block(lines, start, "");
    (vec![Element::new("comment").into()], next)
}

/// A directive's options (`:name: value` lines first) and its content.
fn directive_parts(block: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let mut options = Vec::new();
    let mut i = 0;
    while i < block.len() {
        let Some(found) = captures(&FIELD, &block[i]) else {
            break;
        };
        let mut value = block[i][found[0].len()..].trim().to_string();
        // An option's value may continue on more indented lines.
        while i + 1 < block.len() && !blank(&block[i + 1]) && indent(&block[i + 1]) > 0 {
            i += 1;
            value.push(' ');
            value.push_str(block[i].trim());
        }
        options.push((found[1].to_lowercase(), value.trim().to_string()));
        i += 1;
    }
    while i < block.len() && blank(&block[i]) {
        i += 1;
    }
    (options, block[i..].to_vec())
}

/// Directives docutils (or rich-rst's Sphinx registry) knows but this port
/// does not build; they render nothing rather than an error.
const UNPORTED: &[&str] = &[
    "sectnum",
    "section-numbering",
    "header",
    "footer",
    "target-notes",
    "meta",
    "include",
    "unicode",
    "class",
    "role",
    "default-role",
    "title",
    "date",
    "csv-table",
    "highlight",
    "index",
    "tabularcolumns",
    "currentmodule",
    "py:currentmodule",
    "hlist",
    "toctree",
    "literalinclude",
    "productionlist",
    "glossary",
    "flat-table",
    "impl-detail",
    "availability",
    "soft-deprecated",
    "deprecated-removed",
    "centered",
    "only",
];

/// The parts of a directive block, for building its nodes.
struct Directive<'a> {
    name: &'a str,
    /// The argument as written, its lines joined with newlines.
    argument_text: String,
    /// The argument on one line.
    argument: String,
    options: Vec<(String, String)>,
    content: Vec<String>,
    /// The directive's source, for errors and images.
    raw: String,
}

impl Directive<'_> {
    fn option(&self, key: &str) -> Option<String> {
        self.options
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    fn has_option(&self, key: &str) -> bool {
        self.options.iter().any(|(name, _)| name == key)
    }

    fn error(&self, text: &str) -> Vec<Node> {
        let mut source = self.raw.clone();
        source.push('\n');
        vec![message("ERROR", text, Some(source)).into()]
    }

    fn missing_argument(&self) -> Vec<Node> {
        self.error(&format!(
            "Error in \"{}\" directive:\n1 argument(s) required, 0 supplied.",
            self.name
        ))
    }

    /// The argument's lines then the content: an admonition's body.
    fn body_with_argument(&self) -> Vec<Node> {
        let mut lines = Vec::new();
        if !self.argument_text.is_empty() {
            lines.extend(self.argument_text.lines().map(String::from));
            if !self.content.is_empty() {
                lines.push(String::new());
            }
        }
        lines.extend(self.content.iter().cloned());
        parse_body(&lines, false)
    }

    fn content_body(&self) -> Vec<Node> {
        parse_body(&self.content, false)
    }
}

fn directive(
    lines: &[String],
    start: usize,
    name: &str,
    argument: &str,
    substitution: bool,
) -> (Vec<Node>, usize) {
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
    let argument_text = argument_lines
        .iter()
        .filter(|line| !line.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let (options, content) = directive_parts(&rest[body_start..]);
    let raw = std::iter::once(lines[start].as_str())
        .chain(lines[start + 1..next].iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let raw = raw.trim_end().to_string();
    let directive = Directive {
        name,
        argument: argument_text.replace('\n', " "),
        argument_text,
        options,
        content,
        raw,
    };
    let nodes = if substitution {
        substitution_directive(&directive)
    } else {
        body_directive(&directive)
    };
    (nodes, next)
}

fn substitution_directive(d: &Directive) -> Vec<Node> {
    match d.name {
        "replace" => {
            let mut lines: Vec<String> = Vec::new();
            if !d.argument_text.is_empty() {
                lines.extend(d.argument_text.lines().map(String::from));
            }
            lines.extend(d.content.iter().cloned());
            parse_body(&lines, false)
        }
        "image" => vec![image(d).into()],
        "unicode" | "date" => Vec::new(),
        name => d.error(&format!("Unknown directive type \"{name}\".")),
    }
}

/// docutils' `make_id`: lowercase, runs of other characters to one hyphen.
pub(super) fn make_id(text: &str) -> String {
    let mut out = String::new();
    let mut hyphen = false;
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            if hyphen && !out.is_empty() {
                out.push('-');
            }
            hyphen = false;
            out.push(c);
        } else {
            hyphen = true;
        }
    }
    out
}

fn image(d: &Directive) -> Element {
    let uri: String = d.argument.split_whitespace().collect();
    let mut image = Element::new("image").with("uri", uri);
    image.rawsource.clone_from(&d.raw);
    if let Some(alt) = d.option("alt") {
        image = image.with("alt", alt);
    }
    match d.option("target") {
        Some(target) => {
            let target: String = target.split_whitespace().collect();
            let reference = if target.ends_with('_') && !target.ends_with("\\_") {
                Element::new("reference").with("refname", normalize(&target[..target.len() - 1]))
            } else {
                Element::new("reference").with("refuri", target)
            };
            reference.child(image)
        }
        None => image,
    }
}

fn code_block(d: &Directive) -> Element {
    let mut block = Element::text("literal_block", d.content.join("\n")).class("code");
    if !d.argument.is_empty() {
        block = block.class(d.argument.clone());
    }
    if let Some(name) = d.option("name") {
        block = block.with("name", name);
    }
    if d.has_option("linenos") {
        block = block.with("linenos", "1");
    }
    if let Some(start) = d.option("lineno-start").or_else(|| d.option("number-lines")) {
        if start.parse::<i64>().is_ok() {
            block = block.with("start_line", start);
        }
    }
    if let Some(lines) = d.option("emphasize-lines") {
        let mut highlight: Vec<i64> = Vec::new();
        for part in lines.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            if let Some((a, b)) = part.split_once('-') {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
                    highlight.extend(a..=b);
                }
            } else if let Ok(n) = part.parse::<i64>() {
                highlight.push(n);
            }
        }
        highlight.sort_unstable();
        highlight.dedup();
        if !highlight.is_empty() {
            let list: Vec<String> = highlight.iter().map(i64::to_string).collect();
            block = block.with("highlight_lines", list.join(","));
        }
    }
    block
}

fn admonition_tag(name: &str) -> Option<&'static str> {
    Some(match name {
        "note" => "note",
        "warning" => "warning",
        "tip" => "tip",
        "hint" => "hint",
        "important" => "important",
        "caution" => "caution",
        "danger" => "danger",
        "error" => "error",
        "attention" => "attention",
        _ => return None,
    })
}

fn body_directive(d: &Directive) -> Vec<Node> {
    if let Some(tag) = admonition_tag(d.name) {
        return vec![Element::new(tag).children(d.body_with_argument()).into()];
    }
    match d.name {
        "code" | "code-block" | "sourcecode" => vec![code_block(d).into()],
        "seealso" => vec![Element::new("seealso").children(d.body_with_argument()).into()],
        "versionadded" | "versionchanged" | "deprecated" => version_directive(d),
        "admonition" => {
            if d.argument.is_empty() {
                return d.missing_argument();
            }
            let class = format!("admonition-{}", make_id(&d.argument));
            vec![Element::new("admonition")
                .class(class)
                .child(inlines("title", &d.argument))
                .children(d.content_body())
                .into()]
        }
        "image" => vec![image(d).into()],
        "figure" => vec![figure(d).into()],
        "contents" => {
            let title = if d.argument.is_empty() {
                "Contents".to_string()
            } else {
                d.argument.clone()
            };
            let mut topic = Element::new("topic")
                .class("contents")
                .child(inlines("title", &title))
                .with("__contents", "1");
            if let Some(depth) = d.option("depth") {
                topic = topic.with("__depth", depth);
            }
            if d.has_option("local") {
                topic = topic.with("__local", "1");
            }
            vec![topic.into()]
        }
        "topic" => {
            if d.argument.is_empty() {
                return d.missing_argument();
            }
            vec![Element::new("topic")
                .child(inlines("title", &d.argument))
                .children(d.content_body())
                .into()]
        }
        "rubric" => vec![inlines("rubric", &d.argument).into()],
        "math" => {
            let mut text = vec![d.argument_text.clone()];
            text.extend(d.content.iter().cloned());
            let text: Vec<String> = text.into_iter().filter(|line| !line.is_empty()).collect();
            let mut block = Element::text("math_block", text.join("\n"));
            if let Some(label) = d.option("label").or_else(|| d.option("name")) {
                block = block.with("label", label);
            }
            vec![block.into()]
        }
        "sidebar" => {
            let mut sidebar = Element::new("sidebar");
            if !d.argument.is_empty() {
                sidebar = sidebar.child(inlines("title", &d.argument));
            }
            if let Some(subtitle) = d.option("subtitle") {
                sidebar = sidebar.child(inlines("subtitle", &subtitle));
            }
            vec![sidebar.children(d.content_body()).into()]
        }
        "raw" => {
            if d.argument.is_empty() || d.content.is_empty() {
                return d.missing_argument();
            }
            let format = d
                .argument
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            vec![Element::text("raw", d.content.join("\n"))
                .with("format", format)
                .into()]
        }
        "parsed-literal" => {
            let mut block = Element::new("literal_block")
                .class("parsed-literal")
                .children(inline_nodes(parse_inline(&d.content.join("\n"))));
            if let Some(name) = d.option("name") {
                block = block.with("name", name);
            }
            vec![block.into()]
        }
        "line-block" => {
            let items = d
                .content
                .iter()
                .map(|line| (indent(line), inlines("line", line.trim())))
                .collect();
            vec![nest_line_block(items).into()]
        }
        "epigraph" | "highlights" | "pull-quote" => block_quotes(d.content.clone())
            .into_iter()
            .map(|quote| quote.class(d.name).into())
            .collect(),
        "container" => {
            let mut container = Element::new("container");
            for class in d.argument.split_whitespace() {
                container = container.class(class);
            }
            vec![container.children(d.content_body()).into()]
        }
        "compound" => vec![Element::new("compound").children(d.content_body()).into()],
        "table" => d
            .content_body()
            .into_iter()
            .map(|node| match node {
                Node::Element(mut table) if table.tag == "table" && !d.argument.is_empty() => {
                    table.children.insert(0, inlines("title", &d.argument).into());
                    table.into()
                }
                other => other,
            })
            .collect(),
        "list-table" => list_table(d).unwrap_or_else(|| {
            d.error("Error parsing content block for the \"list-table\" directive.")
        }),
        name if UNPORTED.contains(&name) => Vec::new(),
        name => {
            let mut nodes: Vec<Node> = vec![message(
                "INFO",
                &format!("No directive entry for \"{name}\"."),
                None,
            )
            .into()];
            nodes.extend(d.error(&format!("Unknown directive type \"{name}\".")));
            nodes
        }
    }
}

fn version_directive(d: &Directive) -> Vec<Node> {
    let (version, explanation) = match d.argument_text.split_once(char::is_whitespace) {
        Some((version, explanation)) => (version.to_string(), explanation.trim().to_string()),
        None => (d.argument_text.clone(), String::new()),
    };
    if version.is_empty() {
        return d.missing_argument();
    }
    let mut node = Element::new("versionmodified")
        .with("type", d.name)
        .with("version", version);
    if !explanation.is_empty() {
        node = node.child(paragraph(&explanation));
    }
    vec![node.children(d.content_body()).into()]
}

fn figure(d: &Directive) -> Element {
    let mut figure = Element::new("figure").child(image(d));
    let caption: Vec<String> = d
        .content
        .iter()
        .take_while(|line| !blank(line))
        .cloned()
        .collect();
    if !caption.is_empty() {
        figure = figure.child(inlines("caption", &caption.join("\n")));
    }
    let legend: Vec<String> = d
        .content
        .iter()
        .skip_while(|line| !blank(line))
        .skip_while(|line| blank(line))
        .cloned()
        .collect();
    if !legend.is_empty() {
        figure = figure.child(Element::new("legend").children(parse_body(&legend, false)));
    }
    figure
}

/// `list-table`: a two-level bullet list as a table.
fn list_table(d: &Directive) -> Option<Vec<Node>> {
    let body = d.content_body();
    let [Node::Element(list)] = body.as_slice() else {
        return None;
    };
    if list.tag != "bullet_list" {
        return None;
    }
    let mut rows: Vec<Vec<Vec<Node>>> = Vec::new();
    for item in list.elements() {
        let [Node::Element(row)] = item.children.as_slice() else {
            return None;
        };
        if row.tag != "bullet_list" {
            return None;
        }
        rows.push(row.elements().map(|cell| cell.children.clone()).collect());
    }
    let columns = rows.first()?.len();
    if columns == 0 || rows.iter().any(|row| row.len() != columns) {
        return None;
    }
    let header_rows: usize = d
        .option("header-rows")
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let make_row = |cells: &Vec<Vec<Node>>| {
        Element::new("row").children(
            cells
                .iter()
                .map(|cell| Element::new("entry").children(cell.clone()).into()),
        )
    };
    let head: Vec<Element> = rows.iter().take(header_rows).map(make_row).collect();
    let body: Vec<Element> = rows.iter().skip(header_rows).map(make_row).collect();
    let mut table = table(&vec![0; columns], head, body);
    if !d.argument.is_empty() {
        table.children.insert(0, inlines("title", &d.argument).into());
    }
    Some(vec![table.into()])
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

/// One table cell: its lines, parsed as body elements.
fn entry(lines: &[String], morerows: usize, morecols: usize) -> Element {
    let block = dedent(lines, min_indent(lines));
    let mut entry = Element::new("entry");
    if morerows > 0 {
        entry = entry.with("morerows", morerows.to_string());
    }
    if morecols > 0 {
        entry = entry.with("morecols", morecols.to_string());
    }
    if block.iter().any(|line| !blank(line)) {
        entry = entry.children(parse_body(&block, false));
    }
    entry
}

/// docutils' `build_table`.
fn table(columns: &[usize], head: Vec<Element>, body: Vec<Element>) -> Element {
    let mut tgroup = Element::new("tgroup").with("cols", columns.len().to_string());
    for width in columns {
        tgroup = tgroup.child(Element::new("colspec").with("colwidth", width.to_string()));
    }
    if !head.is_empty() {
        tgroup = tgroup.child(Element::new("thead").children(head.into_iter().map(Node::from)));
    }
    tgroup = tgroup.child(Element::new("tbody").children(body.into_iter().map(Node::from)));
    Element::new("table").child(tgroup)
}

fn malformed(lines: &[String], detail: &str) -> Node {
    let mut source = lines.join("\n");
    source.push('\n');
    message("ERROR", &format!("Malformed table.\n{detail}"), Some(source)).into()
}

/// The columns (`[start, end)` display cells) of a simple table's border or
/// span line, whose runs are `mark`.
fn runs(line: &[Option<char>], mark: char) -> Vec<(usize, usize)> {
    let mut columns = Vec::new();
    let mut from = None;
    for (index, c) in line.iter().enumerate() {
        match (*c == Some(mark), from) {
            (true, None) => from = Some(index),
            (false, Some(start)) if *c == Some(' ') => {
                columns.push((start, index));
                from = None;
            }
            _ => {}
        }
    }
    if let Some(start) = from {
        columns.push((start, line.len()));
    }
    columns
}

/// A simple table: docutils' `SimpleTableParser`.
fn simple_table(lines: &[String], start: usize) -> Option<(Vec<Node>, usize)> {
    // `isolate_simple_table`: up to the border a blank line (or the end)
    // follows.
    let mut borders = Vec::new();
    let mut end = None;
    for (i, line) in lines.iter().enumerate().skip(start) {
        if is_simple_table_border(line) {
            borders.push(i - start);
            if borders.len() >= 2 && lines.get(i + 1).is_none_or(|next| blank(next)) {
                end = Some(i + 1);
                break;
            }
        }
    }
    let end = end?;
    let source = &lines[start..end];
    let block: Vec<Vec<Option<char>>> = source.iter().map(|l| display_columns(l)).collect();
    let text = |row: &[Option<char>], from: usize, to: usize| -> String {
        let to = to.min(row.len());
        row[from.min(to)..to].iter().flatten().collect()
    };
    let mut columns = runs(&block[0], '=');
    let head_sep = (borders.len() >= 3).then(|| borders[1]);
    // Rows: a line whose first column has text starts one; a span line or
    // a border ends one.
    let span_line = |row: &[Option<char>]| {
        let line: String = row.iter().flatten().collect();
        !line.is_empty()
            && (line.starts_with('=') || line.starts_with('-'))
            && line.chars().all(|c| c == '=' || c == '-' || c == ' ')
    };
    // (the row's lines, the span line ending it)
    let mut rows: Vec<(Vec<usize>, Option<usize>)> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut text_found = false;
    let (first_start, first_end) = columns[0];
    for (offset, row) in block.iter().enumerate().skip(1) {
        if span_line(row) {
            if !current.is_empty() || borders.contains(&offset) {
                rows.push((std::mem::take(&mut current), Some(offset)));
            }
            text_found = false;
        } else if !text(row, first_start, first_end).trim().is_empty() {
            if text_found && !current.is_empty() {
                rows.push((std::mem::take(&mut current), None));
            }
            current = vec![offset];
            text_found = true;
        } else if text_found {
            current.push(offset);
        }
    }
    let rows: Vec<(Vec<usize>, Option<usize>)> = rows
        .into_iter()
        .filter(|(lines, _)| !lines.is_empty())
        .collect();
    let border_end = columns.last()?.1;
    let mut parsed: Vec<(usize, Vec<(usize, usize, Vec<String>)>)> = Vec::new();
    for (row_lines, span) in rows {
        // The columns this row uses: its span line's, else the border's.
        let mut cols = match span {
            Some(span) if !borders.contains(&span) => {
                let mut cols = runs(&block[span], '-');
                if cols.last().map(|c| c.1) != Some(border_end) {
                    let detail = format!("Column span incomplete in table line {}.", span + 1);
                    return Some((vec![malformed(source, &detail)], end));
                }
                if let Some(last) = cols.last_mut() {
                    last.1 = columns.last().map_or(last.1, |c| c.1);
                }
                cols
            }
            _ => columns.clone(),
        };
        // `check_columns`: text in a margin is an error; text past the last
        // column widens it.
        let last = cols.len() - 1;
        for &line in &row_lines {
            let row = &block[line];
            for index in 0..cols.len() {
                let (from, to) = cols[index];
                if index == last {
                    if !text(row, to, usize::MAX).trim().is_empty() {
                        let rest = text(row, from, usize::MAX);
                        let new_end = from + width(rest.trim_end());
                        cols[index].1 = cols[index].1.max(new_end);
                        if let Some(main) = columns.last_mut() {
                            main.1 = main.1.max(new_end);
                        }
                    }
                } else if !text(row, to, cols[index + 1].0).trim().is_empty() {
                    let detail = format!("Text in column margin in table line {}.", line + 1);
                    return Some((vec![malformed(source, &detail)], end));
                }
            }
        }
        // `init_row`: how many border columns each cell spans.
        let mut cells = Vec::new();
        let mut at = 0;
        for &(from, to) in &cols {
            if columns.get(at).map(|c| c.0) != Some(from) {
                let detail = format!(
                    "Column span alignment problem in table line {}.",
                    row_lines[0] + 1
                );
                return Some((vec![malformed(source, &detail)], end));
            }
            let mut morecols = 0;
            while columns.get(at).is_some_and(|c| c.1 != to) {
                at += 1;
                morecols += 1;
            }
            if at >= columns.len() {
                let detail = format!(
                    "Column span alignment problem in table line {}.",
                    row_lines[0] + 1
                );
                return Some((vec![malformed(source, &detail)], end));
            }
            at += 1;
            let cell_lines: Vec<String> = row_lines
                .iter()
                .map(|&line| text(&block[line], from, to).trim_end().to_string())
                .collect();
            cells.push((morecols, 0, cell_lines));
        }
        parsed.push((row_lines[0], cells));
    }
    let mut head = Vec::new();
    let mut body = Vec::new();
    for (first, cells) in parsed {
        let row = Element::new("row").children(
            cells
                .into_iter()
                .map(|(morecols, morerows, lines)| entry(&lines, morerows, morecols).into()),
        );
        if head_sep.is_some_and(|sep| first < sep) {
            head.push(row);
        } else {
            body.push(row);
        }
    }
    let widths: Vec<usize> = columns.iter().map(|(from, to)| to - from).collect();
    Some((vec![table(&widths, head, body).into()], end))
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

/// A grid table. A table docutils cannot trace is an error.
fn grid_table(lines: &[String], start: usize) -> (Vec<Node>, usize) {
    // `isolate_grid_table`: the lines that start with `+` or `|`, up to the
    // last border.
    let mut end = start;
    while end < lines.len() && (lines[end].starts_with('+') || lines[end].starts_with('|')) {
        end += 1;
    }
    let mut rows = end - start;
    let mut next = end;
    if !is_grid_border(&lines[end - 1]) {
        let Some(bottom) = (2..rows.saturating_sub(1))
            .rev()
            .find(|&i| is_grid_border(&lines[start + i]))
        else {
            return (
                vec![malformed(
                    &lines[start..end],
                    "Bottom border missing or corrupt.",
                )],
                end,
            );
        };
        rows = bottom + 1;
        // docutils steps back one line too many here, so the row before the
        // bottom border is read again after the table.
        next = start + bottom - 1;
    }
    let source = &lines[start..start + rows];
    let grid: Vec<Vec<String>> = source.iter().map(|line| grid_cells(line)).collect();
    let right_edge = grid[0].len();
    if grid
        .iter()
        .any(|row| row.len() != right_edge || !row.last().is_some_and(|c| c == "+" || c == "|"))
    {
        return (
            vec![malformed(source, "Right border not aligned or missing.")],
            next,
        );
    }
    match GridTable::parse(grid) {
        Some(table) => (vec![table.into()], next),
        None => (vec![malformed(source, "Malformed table.")], next),
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
    fn parse(mut grid: Vec<Vec<String>>) -> Option<Element> {
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
        let head_sep = match separators.as_slice() {
            [] => None,
            [i] if *i != 0 && *i != grid.len() - 1 => {
                for cell in &mut grid[*i] {
                    if cell == "=" {
                        *cell = "-".into();
                    }
                }
                Some(*i)
            }
            _ => return None,
        };
        let bottom = grid.len() - 1;
        let right = grid[0].len() - 1;
        if bottom == 0 {
            return None;
        }
        let mut tracer = GridTable {
            grid,
            bottom,
            right,
            done: vec![None; right + 1],
        };
        let mut found = Vec::new();
        let mut rowseps = std::collections::BTreeSet::from([0usize]);
        let mut colseps = std::collections::BTreeSet::from([0usize]);
        let mut corners = vec![(0, 0)];
        while !corners.is_empty() {
            let (top, left) = corners.remove(0);
            if top == tracer.bottom
                || left == tracer.right
                || tracer.done[left].is_some_and(|done| top <= done)
            {
                continue;
            }
            let Some((bottom, right)) = tracer.scan_right(top, left) else {
                continue;
            };
            for col in left..right {
                tracer.done[col] = Some(bottom - 1);
            }
            rowseps.insert(bottom);
            colseps.insert(right);
            found.push((top, left, bottom, right));
            corners.push((top, right));
            corners.push((bottom, left));
            corners.sort_unstable();
        }
        // Each text column must be seen to the bottom.
        if tracer.done[..tracer.right]
            .iter()
            .any(|&done| done != Some(tracer.bottom - 1))
        {
            return None;
        }
        let rowseps: Vec<usize> = rowseps.into_iter().collect();
        let colseps: Vec<usize> = colseps.into_iter().collect();
        let widths: Vec<usize> = colseps.windows(2).map(|w| w[1] - w[0] - 1).collect();
        let mut rows: Vec<Vec<Option<Element>>> =
            vec![vec![None; colseps.len() - 1]; rowseps.len() - 1];
        found.sort_unstable();
        for (top, left, bottom, right) in found {
            let row = rowseps.iter().position(|&r| r == top)?;
            let col = colseps.iter().position(|&c| c == left)?;
            let morerows = rowseps.iter().position(|&r| r == bottom)? - row - 1;
            let morecols = colseps.iter().position(|&c| c == right)? - col - 1;
            // `get_2D_block`: the cell's text, each line stripped on the
            // right, then dedented by the smallest indent.
            let block: Vec<String> = tracer.grid[top + 1..bottom]
                .iter()
                .map(|line| line[left + 1..right].concat().trim_end().to_string())
                .collect();
            rows[row][col] = Some(entry(&block, morerows, morecols));
        }
        let head_rows = head_sep
            .and_then(|sep| rowseps.iter().position(|&r| r == sep))
            .unwrap_or(0);
        let make = |cells: Vec<Option<Element>>| {
            Element::new("row").children(cells.into_iter().flatten().map(Node::from))
        };
        let mut head = Vec::new();
        let mut body = Vec::new();
        for (index, cells) in rows.into_iter().enumerate() {
            if index < head_rows {
                head.push(make(cells));
            } else {
                body.push(make(cells));
            }
        }
        Some(table(&widths, head, body))
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
