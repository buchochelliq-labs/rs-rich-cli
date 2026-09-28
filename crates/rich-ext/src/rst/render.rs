//! `rich-rst`'s `RSTVisitor` and `RestructuredText`: the renderables each
//! node becomes, and how they print.
//!
//! The visitor keeps a flat list of renderables. Inline markup appends to the
//! last one while it is a `Text`, so text runs on across paragraphs when
//! nothing else stands between them; each `Text` also carries upstream's
//! `end`, which the core `Text` does not (docs/DIVERGENCES.md §29), so the
//! list tracks it beside the text.

use std::collections::HashMap;

use rich::r#box::{DOUBLE, SQUARE};
use rich::{
    Align, Console, ConsoleOptions, Panel, Renderable, Rule, Segment, Style, StyleType, Syntax,
    Table, Text,
};

use super::entities::ENTITIES;
use super::parse::{blocks_text, parse, Block, DefinitionItem, Field, Inline};

/// A reStructuredText document, rendered as `rich-rst` 1.3.2 renders it.
/// Port of `rich_rst.RestructuredText`.
///
/// ```
/// use rich::Console;
/// use rich_ext::rst::RestructuredText;
///
/// let console = Console::builder().width(30).color_system(None).build();
/// let doc = RestructuredText::new("Title\n=====\n\nSome *text*.");
/// let out = console.render_to_string(&doc);
/// assert!(out.contains("Title"));
/// assert!(out.contains("Some text."));
/// ```
#[derive(Clone, Debug)]
pub struct RestructuredText {
    blocks: Vec<Block>,
    code_theme: Option<String>,
    default_lexer: String,
}

impl RestructuredText {
    /// Parse `markup`. Code blocks use `Syntax`'s default theme (`monokai`,
    /// or the console's code highlighting) and the `python` lexer by
    /// default, as upstream's do.
    pub fn new(markup: &str) -> Self {
        RestructuredText {
            blocks: parse(markup),
            code_theme: None,
            default_lexer: "python".into(),
        }
    }

    /// The theme code blocks are highlighted with (upstream's `code_theme`).
    pub fn code_theme(mut self, theme: impl Into<String>) -> Self {
        self.code_theme = Some(theme.into());
        self
    }

    /// The lexer for code blocks that name no language (upstream's
    /// `default_lexer`).
    pub fn default_lexer(mut self, lexer: impl Into<String>) -> Self {
        self.default_lexer = lexer.into();
        self
    }

    /// The parsed document.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
}

/// One renderable in the visitor's list.
enum Item {
    /// A `Text` and its `end`.
    Text(Text, String),
    /// `rich.console.NewLine`.
    NewLine,
    /// The field list's `Table("Field Name", "Field Value", show_lines=True)`,
    /// kept as rows so a following field can add one.
    Fields(Vec<(Text, Text)>),
    Other(Box<dyn Renderable>),
}

struct Visitor<'a> {
    console: &'a Console,
    code_theme: Option<&'a str>,
    default_lexer: &'a str,
    items: Vec<Item>,
    footer: Option<String>,
    /// `refname_to_renderable`: a reference's item and range, for its target
    /// to link.
    references: HashMap<String, (usize, usize, usize)>,
}

const SUPERSCRIPT: (&str, &str) = (
    "1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ=+-*/×÷",
    "¹²³⁴⁵⁶⁷⁸⁹⁰ᵃᵇᶜᵈᵉᶠᵍʰⁱʲᵏˡᵐⁿᵒᵖᑫʳˢᵗᵘᵛʷˣʸᶻᴬᴮᶜᴰᴱᶠᴳᴴᴵᴶᴷᴸᴹᴺᴼᴾQᴿˢᵀᵁⱽᵂˣʸᶻ⁼⁺⁻*/×÷",
);
const SUBSCRIPT: (&str, &str) = (
    "1234567890abcdefghijklmnopqrstuvwxyz=+-*/×÷",
    "₁₂₃₄₅₆₇₈₉₀abcdₑfgₕᵢⱼₖₗₘₙₒₚqᵣₛₜᵤᵥwₓyz₌₊₋*/×÷",
);

/// `str.translate` with a `maketrans(from, to)` table.
fn translate(text: &str, (from, to): (&str, &str)) -> String {
    let to: Vec<char> = to.chars().collect();
    text.chars()
        .map(|c| {
            from.chars()
                .position(|f| f == c)
                .map_or(c, |index| to[index])
        })
        .collect()
}

/// `rich_rst.strip_tags`: the data an `html.parser.HTMLParser` with
/// `convert_charrefs` reports, fed `html` and never closed. Tags, comments,
/// declarations and processing instructions go; character references are
/// decoded as `html.unescape` decodes them; `script` and `style` bodies are
/// kept as written. Like the parser, it holds back (and so drops) text it
/// cannot finish: an unclosed tag, or a trailing `&` that may start a
/// reference.
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let n = html.len();
    let mut i = 0;
    while i < n {
        let rest = &html[i..];
        let j = match rest.find('<') {
            Some(offset) => i + offset,
            None => {
                // A reference may be cut off at the end of the input.
                let from = i.max(n.saturating_sub(34));
                let from = (from..=n).find(|&k| html.is_char_boundary(k)).unwrap_or(n);
                if let Some(amp) = html[from..].rfind('&') {
                    let after = &html[from + amp..];
                    if !after.contains(|c: char| c.is_whitespace() || c == ';') {
                        break;
                    }
                }
                n
            }
        };
        out.push_str(&unescape_html(&html[i..j]));
        i = j;
        if i == n {
            break;
        }
        let rest = &html[i..];
        let next = rest[1..].chars().next();
        let end = if next.is_some_and(|c| c.is_ascii_alphabetic()) {
            tag_end(rest).and_then(|end| {
                // `script` and `style` hold raw text up to their end tag.
                let name: String = rest[1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
                    .to_ascii_lowercase();
                if name == "script" || name == "style" {
                    let body = &rest[end..];
                    let close = body.to_ascii_lowercase().find(&format!("</{name}"));
                    return close.map(|close| {
                        out.push_str(&body[..close]);
                        end + close
                    });
                }
                Some(end)
            })
        } else if rest.starts_with("<!--") {
            rest.find("-->").map(|end| end + 3)
        } else if rest.starts_with("</") || rest.starts_with("<?") || rest.starts_with("<!") {
            rest.find('>').map(|end| end + 1)
        } else if next.is_some() {
            out.push('<');
            Some(1)
        } else {
            None
        };
        match end {
            Some(end) => i += end,
            None => break,
        }
    }
    out
}

/// Where a start tag ends: after its `>`, skipping quoted attribute values.
fn tag_end(tag: &str) -> Option<usize> {
    let mut quote = None;
    for (index, c) in tag.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, '>') => return Some(index + 1),
            _ => {}
        }
    }
    None
}

/// Python's `html.unescape`.
fn unescape_html(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let reference = &rest[amp + 1..];
        match char_reference(reference) {
            Some((decoded, used)) => {
                out.push_str(&decoded);
                rest = &reference[used..];
            }
            None => {
                out.push('&');
                rest = reference;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A character reference after `&`: what it decodes to and how much of the
/// text it used, or `None` when nothing matches `html.unescape`'s pattern.
fn char_reference(text: &str) -> Option<(String, usize)> {
    if let Some(number) = text.strip_prefix('#') {
        let (digits, radix, prefix) = match number.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16, 2),
            None => (number, 10, 1),
        };
        let len = digits.chars().take_while(|c| c.is_digit(radix)).count();
        if len == 0 {
            return None;
        }
        let semicolon = usize::from(digits[len..].starts_with(';'));
        let value = u32::from_str_radix(&digits[..len], radix).unwrap_or(u32::MAX);
        return Some((numeric_reference(value), prefix + len + semicolon));
    }
    // `[^\t\n\f <&#;]{1,32};?`
    let name_len: usize = text
        .char_indices()
        .take_while(|&(_, c)| !matches!(c, '\t' | '\n' | '\x0c' | ' ' | '<' | '&' | '#' | ';'))
        .take(32)
        .last()
        .map_or(0, |(index, c)| index + c.len_utf8());
    if name_len == 0 {
        return None;
    }
    let semicolon = usize::from(text[name_len..].starts_with(';'));
    let name = &text[..name_len + semicolon];
    if let Some(value) = entity(name) {
        return Some((value.to_string(), name.len()));
    }
    // The longest known prefix, as the standard reads a reference.
    let mut cut = name.len() - 1;
    while cut > 1 {
        if name.is_char_boundary(cut) {
            if let Some(value) = entity(&name[..cut]) {
                return Some((format!("{value}{}", &name[cut..]), name.len()));
            }
        }
        cut -= 1;
    }
    Some((format!("&{name}"), name.len()))
}

fn entity(name: &str) -> Option<&'static str> {
    ENTITIES
        .binary_search_by(|(key, _)| (*key).cmp(name))
        .ok()
        .map(|index| ENTITIES[index].1)
}

/// `html.unescape` for `&#N;`: the Windows-1252 fixes, then replacement or
/// removal of what is not a character.
fn numeric_reference(value: u32) -> String {
    const WINDOWS_1252: [u32; 32] = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
        0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
        0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
    ];
    let value = match value {
        0x00 => 0xfffd,
        0x0d => 0x0d,
        0x80..=0x9f => WINDOWS_1252[(value - 0x80) as usize],
        0xd800..=0xdfff => 0xfffd,
        value if value > 0x10ffff => 0xfffd,
        0x01..=0x08 | 0x0b | 0x0e..=0x1f | 0x7f..=0x9f | 0xfdd0..=0xfdef => return String::new(),
        value if value & 0xfffe == 0xfffe => return String::new(),
        value => value,
    };
    char::from_u32(value).map(String::from).unwrap_or_default()
}

impl<'a> Visitor<'a> {
    /// `console.get_style("restructuredtext.<name>", default=default)`.
    fn style(&self, name: &str, default: &str) -> Style {
        self.console
            .get_style(&StyleType::Name(format!("restructuredtext.{name}")))
            .or_else(|_| self.console.get_style(&StyleType::Name(default.into())))
            .unwrap_or_default()
    }

    fn syntax(&self, code: String, lexer: String) -> Syntax {
        let syntax = Syntax::new(code, lexer);
        match self.code_theme {
            Some(theme) => syntax.theme(theme),
            None => syntax,
        }
    }

    fn last_text(&mut self) -> Option<&mut Text> {
        match self.items.last_mut() {
            Some(Item::Text(text, _)) => Some(text),
            _ => None,
        }
    }

    fn push_text(&mut self, text: Text, end: &str) {
        self.items.push(Item::Text(text, end.into()));
    }

    fn push(&mut self, renderable: impl Renderable + 'static) {
        self.items.push(Item::Other(Box::new(renderable)));
    }

    /// Append to the last `Text`, or start a new one with `end=""`: what
    /// every inline visitor does. Returns the range the text took.
    fn inline(&mut self, text: &str, style: Style) -> (usize, usize, usize) {
        let piece = Text::styled(text, style);
        if let Some(last) = self.last_text() {
            let start = last.plain().len();
            *last = std::mem::take(last).append_text(&piece);
            let end = last.plain().len();
            return (self.items.len() - 1, start, end);
        }
        let end = piece.plain().len();
        self.push_text(piece, "");
        (self.items.len() - 1, 0, end)
    }

    fn visit_inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Text(text) => {
                let style = self.style("text", "default on default not underline");
                self.inline(&text.replace('\n', " "), style);
            }
            Inline::Emphasis(text) => {
                let style = self.style("emphasis", "italic");
                self.inline(&text.replace('\n', " "), style);
            }
            Inline::Strong(text) => {
                let style = self.style("strong", "bold");
                self.inline(&text.replace('\n', " "), style);
            }
            Inline::Literal(text) => {
                let style = self.style("inline_codeblock", "grey78 on grey7");
                self.inline(&text.replace('\n', " "), style);
            }
            Inline::Subscript(text) => {
                let style = self.style("subscript", "none");
                self.inline(&translate(text, SUBSCRIPT), style);
            }
            Inline::Superscript(text) => {
                let style = self.style("superscript", "none");
                self.inline(&translate(text, SUPERSCRIPT), style);
            }
            Inline::Reference { text, uri, name } => {
                self.visit_reference(text, uri.as_deref(), name.as_deref());
            }
            Inline::CitationReference(text) => {
                let style = self.style("citation_reference", "grey74");
                self.inline(&text.replace('\n', " "), style);
            }
            // `visit_target` skips an inline target's text.
            Inline::Target(_) => {}
        }
    }

    fn visit_reference(&mut self, text: &str, uri: Option<&str>, name: Option<&str>) {
        let mut style = self.style("reference", "blue underline on default");
        if let Some(uri) = uri {
            style = style.update_link(Some(uri.to_string()));
        }
        let range = self.inline(&text.replace('\n', " "), style);
        match (uri, name) {
            // Linked when its target turns up.
            (None, Some(name)) => {
                self.references.insert(name.to_string(), range);
            }
            // `` `text <uri>`_ `` is also a target for its name.
            (Some(uri), Some(name)) => self.visit_target(&[name.to_string()], uri),
            _ => {}
        }
    }

    fn visit_target(&mut self, names: &[String], uri: &str) {
        for name in names {
            let Some(&(index, start, end)) = self.references.get(name) else {
                continue;
            };
            if let Some(Item::Text(text, _)) = self.items.get_mut(index) {
                text.stylize(Style::new().update_link(Some(uri.to_string())), start, end);
            }
        }
    }

    fn visit_blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.visit(block);
        }
    }

    // Every string the visitor hands a `Panel` renders unhighlighted: the
    // panel gives its children `highlight=False`.
    fn title_panel(&self, text: &str) -> Panel {
        let style = self.style("title", "bold");
        let title = self.console.render_str(text, Some(false));
        Panel::new(Box::new(Align::center(Box::new(title))))
            .box_set(DOUBLE)
            .style(style)
    }

    fn visit(&mut self, block: &Block) {
        match block {
            Block::Title(text) => {
                let panel = self.title_panel(text);
                self.push(panel);
            }
            Block::Paragraph(inlines) => {
                for inline in inlines {
                    self.visit_inline(inline);
                }
                // `depart_paragraph`.
                if let Some(Item::Text(text, end)) = self.items.last_mut() {
                    if end.is_empty() {
                        text.append("\n\n", None);
                    }
                }
            }
            Block::Inlines(inlines) | Block::Attribution(inlines) => {
                for inline in inlines {
                    self.visit_inline(inline);
                }
            }
            Block::BulletList(items) => self.visit_bullet_list(items),
            Block::EnumeratedList(items) => {
                let marker = self.style("enumerated_list_marker", "bold yellow");
                let text_style = self.style("enumerated_text", "none");
                for (index, item) in items.iter().enumerate() {
                    self.push_text(Text::styled(format!(" {}", index + 1), marker.clone()), " ");
                    let text = blocks_text(item).replace('\n', " ");
                    self.push_text(Text::styled(text, text_style.clone()), "\n");
                }
                self.items.push(Item::NewLine);
            }
            Block::DefinitionList(items) => self.visit_definition_list(items),
            Block::FieldList(fields) => self.visit_field_list(fields),
            Block::OptionList(items) => {
                let string_style = self.style("option_string", "none");
                let argument_style = self.style("option_argument", "none");
                let separator_style = self.style("option_child_text_separator", "none");
                let description_style = self.style("option_description", "none");
                for item in items {
                    let mut text = Text::new("");
                    for (option, argument) in &item.options {
                        text =
                            text.append_text(&Text::styled(option.as_str(), string_style.clone()));
                        if let Some(argument) = argument {
                            text = text.append_text(&Text::styled(
                                argument.as_str(),
                                argument_style.clone(),
                            ));
                        }
                        if item.options.len() > 1 {
                            text = text.append_text(&Text::styled(", ", separator_style.clone()));
                        }
                    }
                    if !item.description.is_empty() {
                        text = text.append_text(&Text::new("\n    "));
                        text = text.append_text(&Text::styled(
                            blocks_text(&item.description),
                            description_style.clone(),
                        ));
                    }
                    text = text.append_text(&Text::new("\n"));
                    self.push_text(text, "");
                }
            }
            Block::LiteralBlock { text, language } => {
                if let Some(last) = self.last_text() {
                    last.rstrip();
                    last.append("\n", None);
                }
                let lexer = language
                    .as_deref()
                    .unwrap_or(self.default_lexer)
                    .to_string();
                let border = self.style("literal_block_border", "grey58");
                let syntax = self.syntax(text.clone(), lexer.clone());
                self.push(
                    Panel::new(Box::new(syntax))
                        .border_style(border)
                        .box_set(SQUARE)
                        .title(lexer),
                );
            }
            Block::DoctestBlock(text) => {
                let border = self.style("literal_block_border", "grey58");
                let syntax = self.syntax(text.clone(), "pycon".into());
                self.push(
                    Panel::new(Box::new(syntax))
                        .border_style(border)
                        .box_set(SQUARE),
                );
            }
            Block::BlockQuote(children) => self.visit_block_quote(children),
            Block::LineBlock(lines) => {
                for line in lines {
                    self.push_text(Text::new(line.as_str()), "\n");
                }
            }
            Block::Admonition { kind, body } => {
                let (default, title) = match kind.as_str() {
                    "attention" => ("bold black on yellow", "Attention: "),
                    "caution" => ("red", "Caution: "),
                    "danger" => ("bold white on red", "DANGER: "),
                    "error" => ("bold red", "ERROR: "),
                    "hint" => ("yellow", "Hint: "),
                    "important" => ("bold blue", "IMPORTANT: "),
                    "note" => ("bold white", "Note: "),
                    "tip" => ("bold green", "Tip: "),
                    "warning" => ("bold yellow", "Warning: "),
                    _ => ("bold white", "Admonition: "),
                };
                let style = self.style(kind, default);
                let text = self
                    .console
                    .render_str(&blocks_text(body).replace('\n', " "), Some(false));
                self.push(
                    Panel::new(Box::new(text))
                        .title(title)
                        .style(style.clone())
                        .border_style(style),
                );
            }
            Block::Transition => {
                let style = self.style("hr", "yellow");
                self.push(Rule::line().style(style));
            }
            Block::Image {
                alt, raw, wrapped, ..
            } => {
                let target = raw.contains(":target:").then(|| {
                    raw.rsplit(":target:")
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                });
                if *wrapped {
                    // docutils wraps the image in a reference, whose text
                    // (none) is all the visitor shows.
                    self.visit_reference("", target.as_deref(), None);
                    return;
                }
                let link = target.unwrap_or_else(|| "Image".into());
                let style = Style::parse("#6088ff")
                    .unwrap_or_default()
                    .update_link(Some(link));
                let alt = alt.clone().unwrap_or_else(|| "Image".into());
                let text = Text::new("🌆 ").append_text(&Text::styled(alt, style));
                self.push_text(text, "\n");
            }
            Block::Footnote { .. } => self.footer = Some(block.astext()),
            Block::Citation { .. } => {
                let border = self.style("citation_border", "grey74");
                let text = self.console.render_str(&block.astext(), Some(false));
                self.push(
                    Panel::new(Box::new(text))
                        .title("citation")
                        .border_style(border),
                );
            }
            Block::MathBlock(text) => {
                if let Some(last) = self.last_text() {
                    last.append(text, None);
                } else {
                    self.push_text(Text::new(text.as_str()), "\n");
                }
            }
            Block::Sidebar {
                title,
                subtitle,
                body,
            } => {
                let mut children: Vec<String> = subtitle.iter().cloned().collect();
                children.extend(body.iter().map(Block::astext));
                let (subtitle, paragraph) = match children.as_slice() {
                    [subtitle, paragraph, ..] => (subtitle.clone(), paragraph.clone()),
                    [paragraph] => (String::new(), paragraph.clone()),
                    [] => return,
                };
                let text = self.console.render_str(&paragraph, Some(false));
                let mut panel = Panel::new(Box::new(text))
                    .title(title.as_str())
                    .expand(false);
                if !subtitle.is_empty() {
                    panel = panel.subtitle(subtitle);
                }
                self.push(panel);
            }
            Block::Raw { format, text } => {
                let format = format
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                let border = self.style("literal_block_border", "grey58");
                let (title, text, lexer) = if format == "html" {
                    (
                        "raw stripped raw html".to_string(),
                        strip_tags(text),
                        self.default_lexer.to_string(),
                    )
                } else {
                    (format!("raw {format}"), text.clone(), format)
                };
                let syntax = self.syntax(text, lexer);
                self.push(
                    Panel::new(Box::new(syntax))
                        .border_style(border)
                        .box_set(SQUARE)
                        .title(title),
                );
            }
            Block::Target { names, uri } => self.visit_target(names, uri),
            Block::Container(children) => self.visit_blocks(children),
        }
    }

    fn visit_bullet_list(&mut self, items: &[Vec<Block>]) {
        let marker = self.style("bullet_list_marker", "bold yellow");
        let text_style = self.style("bullet_list_text", "none");
        let line = |visitor: &mut Self, indent: &str, bullet: &str, text: String| {
            let prefix = Text::new(indent).append_text(&Text::styled(bullet, marker.clone()));
            visitor.push_text(prefix, "");
            visitor.push_text(
                Text::styled(text.replace('\n', " "), text_style.clone()),
                "\n",
            );
        };
        for item in items {
            // Upstream rebinds its loop variable in the nested loops, so the
            // bullet line shows whatever was bound last.
            let mut shown = blocks_text(item);
            if item
                .iter()
                .any(|child| matches!(child, Block::BulletList(_)))
            {
                for child in item {
                    line(self, "  ", " ∘ ", child.astext());
                    shown = child.astext();
                    if let Block::BulletList(nested) = child {
                        for nested_item in nested {
                            line(self, "    ", " ▪ ", blocks_text(nested_item));
                            shown = blocks_text(nested_item);
                        }
                    }
                }
            }
            let bullet = Text::styled(" • ", marker.clone());
            self.push_text(bullet, "");
            self.push_text(
                Text::styled(shown.replace('\n', " "), text_style.clone()),
                "\n",
            );
        }
        self.items.push(Item::NewLine);
    }

    fn visit_definition_list(&mut self, items: &[DefinitionItem]) {
        let term_style = self.style("term_style", "none");
        let classifier_style = self.style("classifier_style", "cyan");
        let definitions_style = self.style("definitions_style", "none");
        for item in items {
            let definition = blocks_text(&item.definition).replace('\n', " ");
            match item.classifiers.as_slice() {
                // `term, classifier = child.children`: the "classifier" is
                // the definition.
                [] => {
                    let markup = format!(
                        "[{style}]{term}[/{style}]",
                        style = classifier_style.definition(),
                        term = item.term
                    );
                    let term = Text::from_markup(&markup).unwrap_or_else(|_| {
                        Text::styled(item.term.as_str(), classifier_style.clone())
                    });
                    let text = term
                        .append_text(&Text::new("\n    "))
                        .append_text(&Text::styled(definition, definitions_style.clone()))
                        .append_text(&Text::new("\n      "));
                    self.push_text(text, "\n");
                }
                [classifier] => {
                    let text = Text::new("    ")
                        .append_text(&Text::styled(item.term.as_str(), term_style.clone()))
                        .append_text(&Text::new(" : "))
                        .append_text(&Text::styled(classifier.as_str(), classifier_style.clone()))
                        .append_text(&Text::new("\n      "))
                        .append_text(&Text::styled(definition, definitions_style.clone()))
                        .append_text(&Text::new("\n"));
                    self.push_text(text, "\n");
                }
                // More classifiers: upstream looks for lists, code and quotes
                // among the remaining children, which are classifiers and a
                // definition, and so renders nothing.
                _ => {}
            }
        }
    }

    fn visit_field_list(&mut self, fields: &[Field]) {
        let name_style = self.style("field_name", "bold");
        let value_style = self.style("field_value", "none");
        for field in fields {
            let row = (
                Text::styled(field.name.as_str(), name_style.clone()),
                Text::styled(blocks_text(&field.body), value_style.clone()),
            );
            if let Some(Item::Fields(rows)) = self.items.last_mut() {
                rows.push(row);
            } else {
                self.items.push(Item::Fields(vec![row]));
            }
        }
    }

    fn visit_block_quote(&mut self, children: &[Block]) {
        let text_style = self.style("blockquote_text", "white");
        let marker_style = self.style("blockquote_attribution_marker", "bright_magenta");
        let author_style = self.style("blockquote_attribution_text", "grey89");
        let text = match children {
            [paragraph, attribution] => Text::styled("▌ ", marker_style)
                .append_text(&Text::styled(paragraph.astext(), text_style))
                .append_text(&Text::new("\n"))
                .append_text(&Text::styled(
                    format!("  - {}", attribution.astext()),
                    author_style,
                )),
            [paragraph, ..] => Text::new("    ")
                .append_text(&Text::styled(
                    paragraph.astext().replace('\n', " "),
                    text_style,
                ))
                .append_text(&Text::new("\n\n")),
            [] => return,
        };
        self.push_text(text, "\n");
    }
}

impl RestructuredText {
    fn items<'a>(&'a self, console: &'a Console) -> (Vec<Item>, Option<String>) {
        let mut visitor = Visitor {
            console,
            code_theme: self.code_theme.as_deref(),
            default_lexer: &self.default_lexer,
            items: Vec::new(),
            footer: None,
            references: HashMap::new(),
        };
        visitor.visit_blocks(&self.blocks);
        let mut items = visitor.items;
        // Strip trailing newlines and empty text; the last text ends a line.
        loop {
            match items.last_mut() {
                Some(Item::Text(text, end)) => {
                    text.rstrip();
                    *end = "\n".into();
                    if !text.is_empty() {
                        break;
                    }
                    items.pop();
                }
                Some(Item::NewLine) => {
                    items.pop();
                }
                _ => break,
            }
        }
        (items, visitor.footer)
    }
}

impl Renderable for RestructuredText {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (items, footer) = self.items(console);
        let mut segments = Vec::new();
        let mut render = |renderable: &dyn Renderable, end: &str| {
            segments.extend(console.render(renderable, Some(options)));
            match end {
                "" => {}
                "\n" => segments.push(Segment::line()),
                end => segments.push(Segment::new(end, None)),
            }
        };
        for item in &items {
            match item {
                Item::Text(text, end) => render(text, end),
                Item::NewLine => segments_newline(&mut render),
                Item::Fields(rows) => {
                    let mut table = Table::new().show_lines(true);
                    table.add_column("Field Name");
                    table.add_column("Field Value");
                    for (name, value) in rows {
                        table.add_row_text(vec![name.clone(), value.clone()]);
                    }
                    render(&table, "\n");
                }
                Item::Other(renderable) => render(renderable.as_ref(), "\n"),
            }
        }
        if let Some(footer) = footer {
            let style = console
                .get_style(&StyleType::Name("restructuredtext.footer".into()))
                .unwrap_or_default();
            let border = console
                .get_style(&StyleType::Name("restructuredtext.footer_border".into()))
                .or_else(|_| console.get_style(&StyleType::Name("grey74".into())))
                .unwrap_or_default();
            let text = console.render_str(&footer, Some(false));
            let panel = Panel::new(Box::new(Align::center(Box::new(text))))
                .title("Footer")
                .box_set(SQUARE)
                .border_style(border)
                .style(style);
            render(&panel, "\n");
        }
        // Lines are separated, not terminated, in this port.
        if segments.last().is_some_and(|segment| segment.text == "\n") {
            segments.pop();
        }
        segments
    }
}

/// `NewLine()` renders one line break.
fn segments_newline(render: &mut impl FnMut(&dyn Renderable, &str)) {
    render(&Text::new(""), "\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(source: &str, width: usize) -> String {
        let console = Console::builder().width(width).color_system(None).build();
        console.render_to_string(&RestructuredText::new(source))
    }

    #[test]
    fn a_paragraph_runs_on_into_the_next() {
        assert_eq!(render("One *two*.\n\nThree.", 40), "One two.\n\nThree.");
    }

    #[test]
    fn a_title_is_a_double_panel() {
        let out = render("Title\n=====", 11);
        assert_eq!(out, "╔═════════╗\n║  Title  ║\n╚═════════╝");
    }

    #[test]
    fn lists() {
        assert_eq!(
            render("- a\n- b\n\n1. c\n2. d", 20),
            " • a\n • b\n\n 1 c\n 2 d"
        );
    }
}
