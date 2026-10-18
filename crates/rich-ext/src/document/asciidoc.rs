//! AsciiDoc, parsed into the [document model](super::model).
//!
//! Covered: the document header (`= Title`, the author and revision lines,
//! attribute entries) and section titles `==` to `======` (and Markdown-style
//! `#`); paragraphs with `*strong*`, `_emphasis_`, `` `monospace` `` and
//! `#highlight#` in their constrained and `**unconstrained**` forms,
//! `[.line-through]#…#` and `[.underline]#…#`, passthroughs, hard line breaks
//! (` +`) and the typographic replacements (`(C)`, `--`, `...`, `->`);
//! attribute entries (`:name: value`, `:name!:`) and `{name}` references, with
//! the built-in character attributes; unordered, ordered, checklist,
//! description (`term::`) and callout lists, nested by marker, with `+` list
//! continuations; listing and source blocks (`[source,lang]` + `----`, and
//! ` ``` ` fences) with callout markers (`<1>`), literal blocks and indented
//! literal paragraphs; example, sidebar, quote, verse and open blocks; block
//! titles (`.Title`); admonitions (`NOTE:` paragraphs and `[NOTE]` blocks);
//! tables (`|===` with an implicit or `%header` header row, `cols` alignment,
//! cell styles; `,===` and `:===` data tables); bare URLs, `link:`, `mailto:`,
//! URL macros and cross references; `image::` and `image:` as placeholders;
//! `kbd:`, `btn:` and `menu:`; footnotes (collected at the end); character
//! references (`&copy;`, `&#169;`); horizontal rules; comments.
//!
//! Shown as text: other block and inline macros (written as they are), and
//! `include::` directives, which are not followed (a warning says so).
//! Not shown: comment blocks and lines, passthrough blocks (`++++`), `toc::[]`
//! and page breaks. Conditional preprocessor directives (`ifdef::`,
//! `ifndef::`, `ifeval::`, `endif::`) are dropped; their content is kept.

use std::collections::HashMap;

use super::model::{
    AdmonitionKind, Align, Block, Callout, Checkbox, CodeBlock, Definition, Document, Heading,
    Inline, List, ListItem, ListKind, TableBlock,
};
use super::scan::{
    char_at, char_before, normalize, prefix, sample, trim_inlines, url_end, InlineBuf, Positions,
    BLOCK_WEIGHT, MAX_NESTING,
};
use super::{Diagnostic, Parsed, Severity};

/// Parse an AsciiDoc document.
pub fn parse(source: &str) -> Parsed {
    let source = normalize(source);
    let lines: Vec<Line> = source
        .split('\n')
        .enumerate()
        .filter(|(_, text)| !is_conditional(text))
        .map(|(index, text)| Line {
            no: index + 1,
            text: text.to_string(),
        })
        .collect();
    let mut doc = AsciiDoc {
        attributes: HashMap::new(),
        diagnostics: Vec::new(),
        footnotes: Vec::new(),
        footnote_ids: HashMap::new(),
    };
    let (mut blocks, start) = doc.header(&lines);
    blocks.extend(doc.blocks(&lines[start..], 0));
    if !doc.footnotes.is_empty() {
        blocks.push(Block::Rule);
        let items = std::mem::take(&mut doc.footnotes)
            .into_iter()
            .map(|text| ListItem {
                blocks: vec![Block::Paragraph(text)],
                ..ListItem::default()
            })
            .collect();
        blocks.push(Block::List(List {
            kind: ListKind::Ordered { start: 1 },
            items,
        }));
    }
    Parsed {
        document: Document { blocks },
        diagnostics: doc.diagnostics,
    }
}

/// How confident it is that `text` is AsciiDoc (0 to 1).
pub fn sniff(text: &str) -> f32 {
    let text = sample(text);
    if text.starts_with("#!") || text.starts_with("-----BEGIN PGP") {
        return 0.0;
    }
    let mut lines = text.lines().filter(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with("//")
    });
    let mut title = false;
    let mut header = false;
    if let Some(first) = lines.next() {
        // `# Title` is Markdown's too: only `= Title` counts.
        if first.starts_with("= ") && is_document_title(first) {
            title = true;
            header = lines
                .next()
                .is_some_and(|second| attribute_entry(second).is_some() || second.contains('<'));
        }
    }
    let (mut attributes, mut sections, mut tables, mut sources, mut admonitions, mut macros) =
        (0, 0, 0, 0, 0, 0);
    for line in text.lines().take(4000) {
        if attribute_entry(line).is_some() {
            attributes += 1;
        }
        if section_level(line).is_some_and(|(level, _)| level >= 2) && line.starts_with("==") {
            sections += 1;
        }
        if line.starts_with("|===") {
            tables += 1;
        }
        if line.starts_with("[source") || line.starts_with("[,") && line.ends_with(']') {
            sources += 1;
        }
        if admonition_label(line).is_some() {
            admonitions += 1;
        }
        if (line.starts_with("image::") || line.starts_with("include::")) && line.ends_with(']') {
            macros += 1;
        }
    }
    let mut score: f32 = 0.0;
    let mut signals = 0;
    for (present, weight) in [
        (title && header, 0.95),
        (title, 0.85),
        (tables > 0, 0.9),
        (sources > 0, 0.9),
        (macros > 0, 0.85),
        (attributes >= 3, 0.85),
        (attributes > 0, 0.7),
        (sections >= 2, 0.8),
        (sections == 1, 0.7),
        (admonitions > 0, 0.6),
    ] {
        if present {
            score = score.max(weight);
            signals += 1;
        }
    }
    if signals > 1 {
        score += 0.04 * (signals - 1) as f32;
    }
    score.min(1.0)
}

#[derive(Clone, Debug)]
struct Line {
    no: usize,
    text: String,
}

struct AsciiDoc {
    attributes: HashMap<String, String>,
    diagnostics: Vec<Diagnostic>,
    footnotes: Vec<Vec<Inline>>,
    footnote_ids: HashMap<String, usize>,
}

/// What a `[…]` line and a `.Title` line said about the block after them.
#[derive(Clone, Debug, Default)]
struct Pending {
    title: Option<String>,
    /// Positional attributes: the style first (`source`, `NOTE`, `quote`).
    positional: Vec<String>,
    named: HashMap<String, String>,
    /// `%header`-style options.
    options: Vec<String>,
}

impl Pending {
    fn style(&self) -> Option<&str> {
        self.positional
            .first()
            .map(|style| style.trim())
            .filter(|style| !style.is_empty())
    }

    fn option(&self, option: &str) -> bool {
        self.options.iter().any(|o| o == option)
            || self
                .named
                .get("options")
                .or_else(|| self.named.get("opts"))
                .is_some_and(|value| value.split(',').any(|o| o.trim() == option))
    }

    fn is_empty(&self) -> bool {
        self.title.is_none() && self.positional.is_empty() && self.named.is_empty()
    }
}

/// The most bytes an attribute value, or a line, may grow to once its
/// `{name}` references are replaced.
const ATTRIBUTE_LIMIT: usize = 64 * 1024;

impl AsciiDoc {
    fn warn(&mut self, line: usize, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line: Some(line),
            severity: Severity::Warning,
            message: message.into(),
        });
    }

    fn info(&mut self, line: usize, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line: Some(line),
            severity: Severity::Info,
            message: message.into(),
        });
    }

    /// The document header: `= Title`, the author and revision lines and
    /// attribute entries, up to the first blank line. Returns its blocks and
    /// the index of the first body line.
    fn header(&mut self, lines: &[Line]) -> (Vec<Block>, usize) {
        let mut i = 0;
        // Attribute entries and comments may come before the title.
        while i < lines.len() {
            let text = lines[i].text.as_str();
            if text.trim().is_empty() || is_comment_line(text) {
                i += 1;
            } else if let Some((name, value)) = attribute_entry(text) {
                self.set_attribute(lines[i].no, name, value);
                i += 1;
            } else {
                break;
            }
        }
        if i >= lines.len() || !is_document_title(&lines[i].text) {
            return (Vec::new(), i.min(lines.len()));
        }
        let title_text = lines[i].text[1..].trim().to_string();
        let title_text = title_text.trim_end_matches(" =").trim_end().to_string();
        self.attributes
            .insert("doctitle".into(), title_text.clone());
        let mut blocks = vec![Block::Heading(Heading {
            level: 1,
            content: self.inline(&title_text, 0),
            ..Heading::default()
        })];
        i += 1;
        let mut byline = Vec::new();
        let mut header_lines = 0;
        while i < lines.len() && !lines[i].text.trim().is_empty() {
            let text = lines[i].text.as_str();
            if let Some((name, value)) = attribute_entry(text) {
                self.set_attribute(lines[i].no, name, value);
            } else if !is_comment_line(text) {
                header_lines += 1;
                if header_lines == 1 {
                    // The author line: `Name <email>; Name <email>`.
                    let authors: Vec<&str> = text.split(';').map(str::trim).collect();
                    if let Some(first) = authors.first() {
                        let name = first.split('<').next().unwrap_or(first).trim();
                        self.attributes.insert("author".into(), name.to_string());
                        if let Some(email) = first
                            .split_once('<')
                            .and_then(|(_, rest)| rest.split('>').next())
                        {
                            self.attributes.insert("email".into(), email.to_string());
                        }
                    }
                    byline.push(authors.join(", "));
                } else if header_lines == 2 {
                    // The revision line: `v1.0, 2024-05-01: Remark`.
                    let (version, rest) = match text.split_once(',') {
                        Some((version, rest)) => (Some(version.trim()), rest.trim()),
                        None => (None, text.trim()),
                    };
                    let date = rest.split(':').next().unwrap_or(rest).trim();
                    if let Some(version) = version {
                        let number = version.trim_start_matches(['v', 'V']);
                        self.attributes
                            .insert("revnumber".into(), number.to_string());
                    }
                    if !date.is_empty() {
                        self.attributes.insert("revdate".into(), date.to_string());
                    }
                    byline.push(text.trim().to_string());
                }
            }
            i += 1;
        }
        if !byline.is_empty() {
            let byline = byline.join(" \u{b7} ");
            blocks.push(Block::Byline(self.inline(&byline, 0)));
        }
        (blocks, i)
    }

    fn set_attribute(&mut self, line: usize, name: &str, value: Option<&str>) {
        match value {
            Some(value) => {
                let (value, cut) = self.substitute_within(value.trim());
                if cut {
                    self.warn(
                        line,
                        format!(
                            "the value of :{name}: is longer than {ATTRIBUTE_LIMIT} bytes once \
                             its references are replaced; it is cut there"
                        ),
                    );
                }
                self.attributes.insert(name.to_ascii_lowercase(), value);
            }
            None => {
                self.attributes.remove(&name.to_ascii_lowercase());
            }
        }
    }

    /// `{name}` references replaced; unknown ones stay as written.
    fn substitute(&self, text: &str) -> String {
        self.substitute_within(text).0
    }

    /// [`substitute`](Self::substitute), cut at [`ATTRIBUTE_LIMIT`] bytes
    /// so references to references cannot grow without bound; the flag says
    /// whether it was cut.
    fn substitute_within(&self, text: &str) -> (String, bool) {
        if !text.contains('{') {
            return (text.to_string(), false);
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let escaped = rest[..open].ends_with('\\');
            out.push_str(&rest[..open - usize::from(escaped)]);
            let after = &rest[open + 1..];
            let name_len = after
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
                .count();
            if name_len > 0 && after[name_len..].starts_with('}') {
                let name = &after[..name_len];
                let value = (!escaped)
                    .then(|| {
                        self.attributes
                            .get(&name.to_ascii_lowercase())
                            .cloned()
                            .or_else(|| builtin_attribute(name).map(str::to_string))
                    })
                    .flatten();
                match value {
                    Some(value) => {
                        if out.len() + value.len() > ATTRIBUTE_LIMIT {
                            let mut end = ATTRIBUTE_LIMIT.saturating_sub(out.len());
                            while !value.is_char_boundary(end) {
                                end -= 1;
                            }
                            out.push_str(&value[..end]);
                            return (out, true);
                        }
                        out.push_str(&value);
                    }
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[name_len + 1..];
            } else {
                out.push('{');
                rest = after;
            }
        }
        out.push_str(rest);
        (out, false)
    }

    /// Parse inline markup, after attribute references.
    fn inline(&mut self, text: &str, depth: usize) -> Vec<Inline> {
        let text = self.substitute(text);
        let inliner = Inliner::new(&text);
        let mut footnotes = Vec::new();
        let inlines = inliner.run(0, text.len(), depth, &mut footnotes);
        // Footnotes are numbered as they are met.
        let mut inlines = trim_inlines(inlines);
        if !footnotes.is_empty() {
            self.number_footnotes(&mut inlines, footnotes);
        }
        inlines
    }

    /// Number the footnotes met in `inlines`, in order, and put the numbers
    /// in their markers.
    fn number_footnotes(&mut self, inlines: &mut [Inline], found: Vec<Footnote>) {
        let mut numbers = Vec::with_capacity(found.len());
        for footnote in found {
            let number = match (&footnote.id, footnote.text) {
                (Some(id), None) => self.footnote_ids.get(id).copied(),
                (id, Some(text)) => {
                    self.footnotes.push(text);
                    let number = self.footnotes.len();
                    if let Some(id) = id {
                        self.footnote_ids.insert(id.clone(), number);
                    }
                    Some(number)
                }
                (None, None) => None,
            };
            numbers.push(number);
        }
        renumber(inlines, &numbers);
    }

    /// Parse `lines` into blocks.
    fn blocks(&mut self, lines: &[Line], depth: usize) -> Vec<Block> {
        let ends = Delimiters::new(lines);
        let mut blocks = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let (found, next) = self.block_at(lines, i, depth, &ends, Context::Body);
            blocks.extend(found);
            i = next.max(i + 1);
        }
        blocks
    }

    /// The block starting at `lines[start]`, with any attribute and title
    /// lines before it. Returns its blocks and the next line's index.
    fn block_at(
        &mut self,
        lines: &[Line],
        start: usize,
        depth: usize,
        ends: &Delimiters,
        context: Context,
    ) -> (Vec<Block>, usize) {
        let mut pending = Pending::default();
        let mut i = start;
        // Attribute, title, anchor and comment lines, and blank lines between.
        while i < lines.len() {
            let text = lines[i].text.as_str();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                if pending.is_empty() {
                    return (Vec::new(), i + 1);
                }
                i += 1;
                continue;
            }
            if is_comment_line(text) {
                i += 1;
                continue;
            }
            if let Some((name, value)) = attribute_entry(text) {
                if context == Context::Body {
                    self.set_attribute(lines[i].no, name, value);
                    i += 1;
                    continue;
                }
            }
            if let Some(title) = block_title(text) {
                pending.title = Some(title.to_string());
                i += 1;
                continue;
            }
            if trimmed.starts_with("[[") && trimmed.ends_with("]]") && !trimmed.contains(' ') {
                // An anchor.
                i += 1;
                continue;
            }
            if let Some(attributes) = attribute_line(trimmed) {
                merge_attributes(&mut pending, attributes);
                i += 1;
                continue;
            }
            break;
        }
        if i >= lines.len() {
            return (Vec::new(), i);
        }
        let mut out = Vec::new();
        let text = lines[i].text.as_str();
        let trimmed = text.trim_end();

        if let Some((level, title)) = section_level(trimmed) {
            let content = self.inline(title, 0);
            out.push(Block::Heading(Heading {
                level,
                content,
                ..Heading::default()
            }));
            return (out, i + 1);
        }

        if let Some(delimiter) = delimiter(trimmed) {
            return self.delimited(lines, i, delimiter, pending, depth, ends);
        }

        if is_break(trimmed) {
            return (vec![Block::Rule], i + 1);
        }
        if trimmed == "<<<" {
            return (Vec::new(), i + 1);
        }

        if let Some((name, target, attributes)) = block_macro(trimmed) {
            return (
                self.macro_block(name, target, attributes, pending, lines[i].no),
                i + 1,
            );
        }

        if context == Context::Body && list_entry(trimmed).is_some() {
            let (blocks, next) = self.list(lines, i, depth, ends);
            if let Some(title) = pending.title {
                let content = self.inline(&title, 0);
                out.push(Block::Caption(content));
            }
            out.extend(blocks);
            return (out, next);
        }

        // A paragraph, possibly styled by its attribute line, its label, or
        // its indentation (a literal paragraph).
        let literal = text.starts_with(' ') || text.starts_with('\t');
        let mut end = i + 1;
        while end < lines.len() && !self.ends_paragraph(&lines[end].text, literal) {
            end += 1;
        }
        let paragraph = &lines[i..end];
        if let Some(title) = &pending.title {
            let content = self.inline(title, 0);
            out.push(Block::Caption(content));
        }
        let style = pending.style().map(str::to_ascii_lowercase);
        let raw = || {
            paragraph
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        match style.as_deref() {
            Some("source") | Some("listing") => {
                let language = pending.positional.get(1).cloned().or_else(|| {
                    (style.as_deref() == Some("source"))
                        .then(|| self.attributes.get("source-language").cloned())
                        .flatten()
                });
                out.push(Block::Code(code_block(language, &dedent_block(&raw()))));
            }
            Some("literal") => out.push(Block::Code(code_block(None, &dedent_block(&raw())))),
            Some("quote") => {
                let blocks = vec![Block::Paragraph(self.paragraph_inlines(paragraph))];
                let attribution = self.attribution(&pending);
                out.push(Block::Quote {
                    blocks,
                    attribution,
                });
            }
            Some("verse") => {
                let lines = paragraph
                    .iter()
                    .map(|line| self.inline(&line.text, 0))
                    .collect();
                let attribution = self.attribution(&pending);
                out.push(Block::Quote {
                    blocks: vec![Block::Lines(lines)],
                    attribution,
                });
            }
            Some("comment") => {}
            Some("pass") => self.info(paragraph[0].no, "passthrough content is not shown"),
            Some("sidebar") | Some("example") => {
                let blocks = vec![Block::Paragraph(self.paragraph_inlines(paragraph))];
                out.push(Block::Aside {
                    title: None,
                    blocks,
                });
            }
            Some(label) if AdmonitionKind::from_label(label).is_some() => {
                let kind = AdmonitionKind::from_label(label).expect("checked by the guard");
                let blocks = vec![Block::Paragraph(self.paragraph_inlines(paragraph))];
                out.push(Block::Admonition {
                    kind,
                    title: None,
                    blocks,
                });
            }
            _ if literal => {
                out.push(Block::Code(code_block(None, &dedent_block(&raw()))));
            }
            _ => {
                let first = paragraph[0].text.as_str();
                if let Some((label, rest)) = admonition_label(first) {
                    let kind = AdmonitionKind::from_label(label).expect("a known label");
                    let mut text = rest.to_string();
                    for line in &paragraph[1..] {
                        text.push('\n');
                        text.push_str(line.text.trim());
                    }
                    let inlines = self.inline(&text, 0);
                    out.push(Block::Admonition {
                        kind,
                        title: None,
                        blocks: vec![Block::Paragraph(inlines)],
                    });
                } else {
                    let inlines = self.paragraph_inlines(paragraph);
                    match inlines.as_slice() {
                        [Inline::Image { target, alt }] => out.push(Block::Image {
                            target: target.clone(),
                            alt: alt.clone(),
                        }),
                        _ => out.push(Block::Paragraph(inlines)),
                    }
                }
            }
        }
        (out, end)
    }

    /// Whether `line` ends the paragraph before it.
    fn ends_paragraph(&self, line: &str, literal: bool) -> bool {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            return true;
        }
        if literal {
            return false;
        }
        delimiter(trimmed).is_some()
            || trimmed == "+"
            || attribute_line(trimmed.trim()).is_some()
            || block_title(line).is_some()
            || section_level(trimmed).is_some()
            || list_entry(trimmed).is_some()
            || is_comment_line(line)
            || (is_break(trimmed) && trimmed.len() < 6)
    }

    fn paragraph_inlines(&mut self, lines: &[Line]) -> Vec<Inline> {
        let mut text = String::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            text.push_str(line.text.trim());
        }
        self.inline(&text, 0)
    }

    /// A quote's attribution from `[quote, Author, Source]`.
    fn attribution(&mut self, pending: &Pending) -> Option<Vec<Inline>> {
        let author = pending
            .positional
            .get(1)
            .or_else(|| pending.named.get("attribution"))
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty());
        let source = pending
            .positional
            .get(2)
            .or_else(|| pending.named.get("citetitle"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let text = match (author, source) {
            (Some(author), Some(source)) => format!("{author}, {source}"),
            (Some(author), None) => author,
            (None, Some(source)) => source,
            (None, None) => return None,
        };
        Some(self.inline(&text, 0))
    }

    /// A delimited block: from its opening delimiter at `lines[start]` to
    /// the same delimiter closing it.
    fn delimited(
        &mut self,
        lines: &[Line],
        start: usize,
        delimiter: &str,
        pending: Pending,
        depth: usize,
        ends: &Delimiters,
    ) -> (Vec<Block>, usize) {
        let fence = delimiter.starts_with("```");
        let closing = if fence { "```" } else { delimiter };
        let end = ends.next(closing, start);
        if end.is_none() {
            self.warn(
                lines[start].no,
                format!("the block opened by {delimiter} is not closed; it runs to the end"),
            );
        }
        let stop = end.unwrap_or(lines.len());
        let body = &lines[start + 1..stop];
        let next = end.map_or(lines.len(), |end| end + 1);
        let raw = || {
            body.iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut out = Vec::new();
        let caption = pending.title.as_ref().map(|title| self.inline(title, 0));
        let style = pending.style().map(str::to_ascii_lowercase);
        let kind = delimiter.as_bytes()[0];
        let block = match kind {
            b'`' => {
                let language = delimiter[3..].trim();
                let language = (!language.is_empty())
                    .then(|| language.to_string())
                    .or_else(|| pending.positional.get(1).cloned());
                Some(Block::Code(code_block(language, &raw())))
            }
            b'-' if delimiter.len() >= 4 => {
                let language = match style.as_deref() {
                    Some("source") | None if pending.positional.len() > 1 => {
                        pending.positional.get(1).cloned()
                    }
                    Some("source") => self.attributes.get("source-language").cloned(),
                    _ => None,
                };
                Some(Block::Code(code_block(language, &raw())))
            }
            b'.' => Some(Block::Code(code_block(None, &raw()))),
            b'/' => None,
            b'+' => {
                self.info(lines[start].no, "passthrough content is not shown");
                None
            }
            b'|' | b',' | b':' => Some(Block::Table(self.table(
                body,
                &pending,
                kind,
                caption.clone(),
            ))),
            b'_' => {
                let attribution = self.attribution(&pending);
                if style.as_deref() == Some("verse") {
                    let lines = body.iter().map(|line| self.inline(&line.text, 0)).collect();
                    Some(Block::Quote {
                        blocks: vec![Block::Lines(lines)],
                        attribution,
                    })
                } else {
                    let blocks = self.nested(body, depth);
                    Some(Block::Quote {
                        blocks,
                        attribution,
                    })
                }
            }
            // `====` example, `****` sidebar and `--` open blocks.
            _ => {
                let blocks = self.nested(body, depth);
                match style.as_deref() {
                    Some(label) if AdmonitionKind::from_label(label).is_some() => {
                        Some(Block::Admonition {
                            kind: AdmonitionKind::from_label(label).expect("checked"),
                            title: caption.clone(),
                            blocks,
                        })
                    }
                    Some("source") | Some("listing") | Some("literal") => Some(Block::Code(
                        code_block(pending.positional.get(1).cloned(), &raw()),
                    )),
                    Some("comment") => None,
                    Some("quote") | Some("verse") => {
                        let attribution = self.attribution(&pending);
                        Some(Block::Quote {
                            blocks,
                            attribution,
                        })
                    }
                    _ if kind == b'-' => {
                        // An open block: its content as it is.
                        out.extend(caption.clone().map(Block::Caption));
                        out.extend(blocks);
                        return (out, next);
                    }
                    _ => Some(Block::Aside {
                        title: caption.clone(),
                        blocks,
                    }),
                }
            }
        };
        let titled_inside = matches!(
            block,
            Some(Block::Admonition { .. } | Block::Aside { .. } | Block::Table(_))
        );
        if let (Some(caption), false) = (caption, titled_inside) {
            if block.is_some() {
                out.push(Block::Caption(caption));
            }
        }
        out.extend(block);
        (out, next)
    }

    /// The AsciiDoc inside a block, one level deeper.
    fn nested(&mut self, body: &[Line], depth: usize) -> Vec<Block> {
        if depth + BLOCK_WEIGHT > MAX_NESTING {
            let text = body
                .iter()
                .map(|line| line.text.trim())
                .collect::<Vec<_>>()
                .join(" ");
            return vec![Block::Paragraph(vec![Inline::Text(text)])];
        }
        self.blocks(body, depth + BLOCK_WEIGHT)
    }

    fn macro_block(
        &mut self,
        name: &str,
        target: &str,
        attributes: &str,
        pending: Pending,
        line: usize,
    ) -> Vec<Block> {
        let target = self.substitute(target);
        let mut out = Vec::new();
        match name {
            "image" => {
                if let Some(title) = &pending.title {
                    let content = self.inline(title, 0);
                    out.push(Block::Caption(content));
                }
                let alt = attributes.split(',').next().unwrap_or("").trim();
                out.push(Block::Image {
                    target,
                    alt: alt.trim_matches('"').to_string(),
                });
            }
            "include" => {
                self.warn(
                    line,
                    format!("include::{target}[] is not followed; it is shown as written"),
                );
                out.push(Block::Properties {
                    name: None,
                    entries: vec![(String::new(), format!("include::{target}[{attributes}]"))],
                });
            }
            "toc" => {}
            "video" | "audio" => out.push(Block::Paragraph(vec![
                Inline::text(format!("{name}: ")),
                Inline::Link {
                    target,
                    content: Vec::new(),
                },
            ])),
            _ => {
                self.info(
                    line,
                    format!("the {name}:: block macro is shown as written"),
                );
                out.push(Block::Paragraph(vec![Inline::Text(format!(
                    "{name}::{target}[{attributes}]"
                ))]));
            }
        }
        out
    }

    /// A list starting at `lines[start]`, with the items after it and those
    /// nested in them (by marker), up to a line that is not part of a list.
    fn list(
        &mut self,
        lines: &[Line],
        start: usize,
        depth: usize,
        ends: &Delimiters,
    ) -> (Vec<Block>, usize) {
        let mut entries: Vec<(Entry, Vec<Block>)> = Vec::new();
        let mut i = start;
        loop {
            let entry = list_entry(lines[i].text.trim_end()).expect("checked by the caller");
            i += 1;
            // The item's text runs on until a blank line or another item.
            let mut text = entry.text.clone();
            while i < lines.len() {
                let line = lines[i].text.as_str();
                let trimmed = line.trim();
                if trimmed.is_empty()
                    || trimmed == "+"
                    || list_entry(line.trim_end()).is_some()
                    || delimiter(line.trim_end()).is_some()
                    || attribute_line(trimmed).is_some()
                    || block_title(line).is_some()
                    || is_comment_line(line)
                    || section_level(line.trim_end()).is_some()
                {
                    break;
                }
                text.push('\n');
                text.push_str(trimmed);
                i += 1;
            }
            let mut blocks = Vec::new();
            let mut checkbox = None;
            let mut body = text.as_str();
            if entry.marker.starts_with('*') || entry.marker == "-" {
                for (prefix, state) in [
                    ("[ ]", Checkbox::Unchecked),
                    ("[x]", Checkbox::Checked),
                    ("[X]", Checkbox::Checked),
                    ("[*]", Checkbox::Checked),
                ] {
                    if let Some(rest) = body.strip_prefix(prefix) {
                        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                            checkbox = Some(state);
                            body = rest.trim_start();
                            break;
                        }
                    }
                }
            }
            if !body.trim().is_empty() {
                let inlines = self.inline(body, 0);
                blocks.push(Block::Paragraph(inlines));
            }
            // `+` attaches the next block to the item.
            while i < lines.len() && lines[i].text.trim() == "+" {
                let (attached, next) = if depth >= MAX_NESTING {
                    (Vec::new(), i + 1)
                } else {
                    self.block_at(lines, i + 1, depth + 1, ends, Context::Item)
                };
                blocks.extend(attached);
                i = next.max(i + 1);
            }
            let term_inlines = match &entry.term {
                Some(term) => self.inline(term, 0),
                None => Vec::new(),
            };
            entries.push((
                Entry {
                    checkbox,
                    term_inlines,
                    ..entry
                },
                blocks,
            ));
            // The list goes on past blank lines to another item.
            let mut next = i;
            while next < lines.len() && lines[next].text.trim().is_empty() {
                next += 1;
            }
            if next < lines.len() && list_entry(lines[next].text.trim_end()).is_some() {
                i = next;
                continue;
            }
            break;
        }
        (build_lists(entries), i)
    }

    /// A table's rows from its body lines.
    fn table(
        &mut self,
        body: &[Line],
        pending: &Pending,
        kind: u8,
        title: Option<Vec<Inline>>,
    ) -> TableBlock {
        let columns = pending.named.get("cols").map(|cols| parse_cols(cols));
        let separator = match kind {
            b',' => ',',
            b':' => ':',
            _ => pending
                .named
                .get("separator")
                .and_then(|sep| sep.chars().next())
                .unwrap_or('|'),
        };
        let mut cells: Vec<Cell> = Vec::new();
        // The number of cells on the first line that has any.
        let mut first_line_cells = None;
        let mut first_row_end_line = None;
        let mut blank_after_first = false;
        for (index, line) in body.iter().enumerate() {
            let text = line.text.as_str();
            if text.trim().is_empty() {
                if first_line_cells.is_some() && first_row_end_line == Some(index - 1) {
                    blank_after_first = true;
                }
                continue;
            }
            let before = cells.len();
            if kind == b'|' && separator == '|' {
                split_psv(text, &mut cells);
            } else {
                for value in text.split(separator) {
                    cells.push(Cell {
                        text: value.trim().trim_matches('"').to_string(),
                        style: None,
                        span: 1,
                    });
                }
            }
            if first_line_cells.is_none() && cells.len() > before {
                first_line_cells = Some(cells.len() - before);
                first_row_end_line = Some(index);
            }
        }
        let count = columns
            .as_ref()
            .map(|(count, _)| *count)
            .or(first_line_cells)
            .unwrap_or(1)
            .max(1);
        let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
        let mut row: Vec<Vec<Inline>> = Vec::new();
        let mut width = 0;
        for cell in cells {
            let span = cell.span.clamp(1, count);
            let content = self.cell(&cell);
            row.push(content);
            for _ in 1..span {
                row.push(Vec::new());
            }
            width += span;
            if width >= count {
                rows.push(std::mem::take(&mut row));
                width = 0;
            }
        }
        if !row.is_empty() {
            rows.push(row);
        }
        let header_option = pending.option("header");
        let no_header = pending.option("noheader");
        let implicit = blank_after_first && first_line_cells == Some(count);
        let header =
            (!no_header && (header_option || implicit) && !rows.is_empty()).then(|| rows.remove(0));
        TableBlock {
            header,
            rows,
            align: columns.map(|(_, align)| align).unwrap_or_default(),
            sections: Vec::new(),
            title,
        }
    }

    fn cell(&mut self, cell: &Cell) -> Vec<Inline> {
        let text = cell.text.trim();
        match cell.style {
            Some('m') | Some('l') => vec![Inline::Code(text.to_string())],
            Some('s') | Some('h') => vec![Inline::Strong(self.inline(text, 1))],
            Some('e') => vec![Inline::Emphasis(self.inline(text, 1))],
            _ => self.inline(&text.replace("\n\n", "\n"), 0),
        }
    }
}

/// Where a run of blocks is: the body, or the block a list continuation
/// attaches to an item.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Body,
    Item,
}

/// The line indexes of each delimiter line, found once.
struct Delimiters(HashMap<String, Vec<usize>>);

impl Delimiters {
    fn new(lines: &[Line]) -> Self {
        let mut map: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.text.trim_end();
            if let Some(delimiter) = delimiter(trimmed) {
                let key = if delimiter.starts_with("```") {
                    "```"
                } else {
                    delimiter
                };
                map.entry(key.to_string()).or_default().push(index);
            }
        }
        Delimiters(map)
    }

    /// The first line after `start` that is exactly `delimiter`.
    fn next(&self, delimiter: &str, start: usize) -> Option<usize> {
        let positions = self.0.get(delimiter)?;
        let index = positions.partition_point(|&at| at <= start);
        positions.get(index).copied()
    }
}

/// A table cell before parsing: its text, style letter and column span.
struct Cell {
    text: String,
    style: Option<char>,
    span: usize,
}

/// Split one line of a `|===` table into cells. A cell spec (`2+`, `a`,
/// `^.^`, `3*`) may precede each `|`; text before the first `|` continues
/// the previous cell.
fn split_psv(line: &str, cells: &mut Vec<Cell>) {
    let bytes = line.as_bytes();
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if bytes.get(i + 1) == Some(&b'|') => i += 2,
            b'|' => {
                segments.push((start, i));
                start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    let unescape = |text: &str| text.replace("\\|", "|");
    if segments.is_empty() {
        // No separator: the line continues the last cell.
        if let Some(last) = cells.last_mut() {
            last.text.push('\n');
            last.text.push_str(&unescape(line));
        }
        return;
    }
    // Text before the first separator is the previous cell's, unless it is
    // a cell spec.
    let (lead_start, lead_end) = segments[0];
    let lead = &line[lead_start..lead_end];
    let mut spec = parse_cell_spec(lead.trim());
    if spec.is_none() && !lead.trim().is_empty() {
        if let Some(last) = cells.last_mut() {
            last.text.push('\n');
            last.text.push_str(&unescape(lead));
        }
    }
    // Each cell runs from its separator to the next one.
    let mut pieces: Vec<&str> = Vec::new();
    for (index, (_, separator)) in segments.iter().enumerate() {
        let end = segments.get(index + 1).map_or(line.len(), |next| next.1);
        pieces.push(&line[separator + 1..end]);
    }
    for piece in pieces {
        // A trailing cell spec belongs to the next cell: `a| x 2+| y`.
        let (text, next_spec) = split_trailing_spec(piece);
        let (span, style, repeat) = spec.take().unwrap_or((1, None, 1));
        for _ in 0..repeat.clamp(1, 64) {
            cells.push(Cell {
                text: unescape(text),
                style,
                span,
            });
        }
        spec = next_spec;
    }
}

/// A table cell spec: `(span, style, repeat)`.
type CellSpec = (usize, Option<char>, usize);

/// `2+`, `3*`, `a`, `^.>m`, `2+^`: a cell's span or repeat, alignment and
/// style.
fn parse_cell_spec(spec: &str) -> Option<CellSpec> {
    if spec.is_empty() {
        return Some((1, None, 1));
    }
    if spec.len() > 12 {
        return None;
    }
    let digits: String = spec.chars().take_while(char::is_ascii_digit).collect();
    let rest = &spec[digits.len()..];
    let (span, repeat, rest) = match rest.chars().next() {
        Some('+') => (digits.parse().unwrap_or(1), 1, &rest[1..]),
        Some('*') => (1, digits.parse().unwrap_or(1), &rest[1..]),
        _ if digits.is_empty() => (1, 1, rest),
        // `2.3+` row spans and the like: not supported, but still a spec.
        Some('.') => (1, 1, rest),
        _ => return None,
    };
    let rest = rest.trim_start_matches(['<', '^', '>', '.']);
    let rest = rest.trim_start_matches(char::is_numeric);
    let rest = rest.trim_start_matches(['<', '^', '>', '.']);
    let style = match rest {
        "" => None,
        letter if letter.len() == 1 && "adehlmsv".contains(letter) => letter.chars().next(),
        _ => return None,
    };
    Some((span, style, repeat))
}

/// The cell text and, when it ends with one, the spec of the next cell.
fn split_trailing_spec(piece: &str) -> (&str, Option<CellSpec>) {
    // A spec sits right against the `|`: `a|`, never `a |`.
    if piece.ends_with(char::is_whitespace) {
        return (piece, None);
    }
    let trimmed = piece;
    if let Some(space) = trimmed.rfind(char::is_whitespace) {
        let candidate = &trimmed[space + 1..];
        if candidate.chars().any(|c| matches!(c, '+' | '*'))
            || (candidate.len() == 1 && "adehlmsv".contains(candidate))
        {
            if let Some(spec) = parse_cell_spec(candidate) {
                return (&trimmed[..space], Some(spec));
            }
        }
    }
    (piece, None)
}

/// `cols="1,2,3"`, `cols="3*"`, `cols="<,^,>"`, `cols="2*<,1"`: the column
/// count and alignments.
fn parse_cols(cols: &str) -> (usize, Vec<Align>) {
    let cols = cols.trim().trim_matches('"');
    if let Ok(count) = cols.parse::<usize>() {
        return (count.clamp(1, 64), vec![Align::Left; count.clamp(1, 64)]);
    }
    let mut align = Vec::new();
    for spec in cols.split([',', ';']) {
        let spec = spec.trim();
        let (repeat, spec) = match spec.split_once('*') {
            Some((count, rest)) => (count.trim().parse::<usize>().unwrap_or(1), rest),
            None => (1, spec),
        };
        let column = if spec.contains('^') && !spec.starts_with('.') {
            Align::Center
        } else if spec.starts_with('>') {
            Align::Right
        } else {
            Align::Left
        };
        for _ in 0..repeat.clamp(1, 64) {
            if align.len() < 64 {
                align.push(column);
            }
        }
    }
    (align.len().max(1), align)
}

/// One list item as written: its marker and text.
#[derive(Clone, Debug)]
struct Entry {
    /// `*`, `**`, `-`, `.`, `..`, `1.`, `a.`, `::`, `;;`, `<>`.
    marker: String,
    kind: EntryKind,
    number: Option<u64>,
    term: Option<String>,
    text: String,
    checkbox: Option<Checkbox>,
    /// The term, parsed (with attribute references) by the list parser.
    term_inlines: Vec<Inline>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryKind {
    Bullet,
    Ordered,
    Description,
    Callout,
}

/// A list item line: `* text`, `.. text`, `3. text`, `term:: text`, `<1> text`.
fn list_entry(line: &str) -> Option<Entry> {
    let trimmed = line.trim_start();
    let first = *trimmed.as_bytes().first()?;
    let entry = |marker: &str, kind, number, text: &str| Entry {
        marker: marker.to_string(),
        kind,
        number,
        term: None,
        text: text.trim().to_string(),
        checkbox: None,
        term_inlines: Vec::new(),
    };
    match first {
        b'*' | b'.' | b'-' => {
            let run = trimmed.bytes().take_while(|&b| b == first).count();
            let rest = &trimmed[run..];
            let valid = (first != b'-' || run == 1)
                && run <= 5
                && rest.starts_with([' ', '\t'])
                && !rest.trim().is_empty();
            if valid {
                let kind = if first == b'.' {
                    EntryKind::Ordered
                } else {
                    EntryKind::Bullet
                };
                return Some(entry(&trimmed[..run], kind, None, rest));
            }
        }
        b'0'..=b'9' => {
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            let rest = &trimmed[digits..];
            if digits <= 9 && rest.starts_with(". ") {
                let number = trimmed[..digits].parse().ok();
                return Some(entry(".", EntryKind::Ordered, number, &rest[2..]));
            }
        }
        b'<' => {
            let close = trimmed.find('>')?;
            let label = &trimmed[1..close];
            let rest = &trimmed[close + 1..];
            if (label == "." || (!label.is_empty() && label.bytes().all(|b| b.is_ascii_digit())))
                && rest.starts_with(' ')
            {
                let number = label.parse().ok();
                return Some(entry("<>", EntryKind::Callout, number, rest));
            }
            return None;
        }
        b'a'..=b'z' | b'A'..=b'Z' if trimmed[1..].starts_with(". ") => {
            let number = (first.to_ascii_lowercase() - b'a') as u64 + 1;
            return Some(entry("a.", EntryKind::Ordered, Some(number), &trimmed[3..]));
        }
        _ => {}
    }
    // A description list item: `term:: definition`, `term:::`, `term;;`.
    if line.starts_with(' ') || line.starts_with("//") {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let pair = &bytes[i..i + 2];
        if i > 0 && (pair == b"::" || pair == b";;") {
            let marker_len = if pair == b"::" {
                bytes[i..].iter().take_while(|&&b| b == b':').count()
            } else {
                2
            };
            if marker_len > 4 {
                return None;
            }
            let after = &trimmed[i + marker_len..];
            let term = trimmed[..i].trim();
            if (after.is_empty() || after.starts_with([' ', '\t']))
                && !term.is_empty()
                && !term.ends_with(' ')
                && !trimmed[..i].ends_with(char::is_whitespace)
            {
                return Some(Entry {
                    marker: trimmed[i..i + marker_len].to_string(),
                    kind: EntryKind::Description,
                    number: None,
                    term: Some(term.to_string()),
                    text: after.trim().to_string(),
                    checkbox: None,
                    term_inlines: Vec::new(),
                });
            }
            return None;
        }
        i += 1;
    }
    None
}

/// An open list while building the tree: a list or a description list.
enum Node {
    List(List),
    Definitions(Vec<Definition>),
}

impl Node {
    fn last_blocks(&mut self) -> Option<&mut Vec<Block>> {
        match self {
            Node::List(list) => list.items.last_mut().map(|item| &mut item.blocks),
            Node::Definitions(definitions) => definitions.last_mut().map(|d| &mut d.blocks),
        }
    }

    fn into_block(self) -> Block {
        match self {
            Node::List(list) => Block::List(list),
            Node::Definitions(definitions) => Block::Definitions(definitions),
        }
    }
}

/// Nest list items by marker: an item whose marker an open list uses is a
/// sibling there; any other marker starts a list inside the last item.
fn build_lists(entries: Vec<(Entry, Vec<Block>)>) -> Vec<Block> {
    let mut out = Vec::new();
    let mut stack: Vec<(String, Node)> = Vec::new();
    for (entry, blocks) in entries {
        let marker = entry.marker.clone();
        let position = stack.iter().position(|(open, _)| *open == marker);
        let position = position.or_else(|| (stack.len() >= MAX_NESTING).then(|| stack.len() - 1));
        match position {
            Some(position) => {
                while stack.len() > position + 1 {
                    close(&mut stack, &mut out);
                }
                push_entry(
                    &mut stack.last_mut().expect("position is in the stack").1,
                    entry,
                    blocks,
                );
            }
            None => {
                let mut node = match entry.kind {
                    EntryKind::Description => Node::Definitions(Vec::new()),
                    EntryKind::Bullet => Node::List(List {
                        kind: ListKind::Bullet,
                        items: Vec::new(),
                    }),
                    EntryKind::Ordered => Node::List(List {
                        kind: ListKind::Ordered {
                            start: entry.number.unwrap_or(1),
                        },
                        items: Vec::new(),
                    }),
                    EntryKind::Callout => Node::List(List {
                        kind: ListKind::Callout,
                        items: Vec::new(),
                    }),
                };
                push_entry(&mut node, entry, blocks);
                stack.push((marker, node));
            }
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut out);
    }
    out
}

/// Close the innermost open list into its parent's last item, or into
/// `out` at the top.
fn close(stack: &mut Vec<(String, Node)>, out: &mut Vec<Block>) {
    let (_, node) = stack.pop().expect("the caller checked");
    let block = node.into_block();
    match stack
        .last_mut()
        .and_then(|(_, parent)| parent.last_blocks())
    {
        Some(blocks) => blocks.push(block),
        None => out.push(block),
    }
}

fn push_entry(node: &mut Node, entry: Entry, blocks: Vec<Block>) {
    match node {
        Node::List(list) => {
            let number = match (list.kind, entry.number) {
                (ListKind::Ordered { .. } | ListKind::Callout, Some(number)) => Some(number),
                _ => None,
            };
            list.items.push(ListItem {
                checkbox: entry.checkbox,
                number,
                blocks,
            });
        }
        Node::Definitions(definitions) => {
            definitions.push(Definition {
                term: entry.term_inlines,
                blocks,
            });
        }
    }
}

/// A code block, with callout markers (`<1>`, `// <2>`, `<.>`) at line ends
/// located for the renderer.
fn code_block(language: Option<String>, code: &str) -> CodeBlock {
    let language = language.filter(|language| !language.trim().is_empty());
    let mut callouts = Vec::new();
    let has_markers = code.contains('>') && code.contains('<');
    let code = if has_markers {
        super::scan::expand_tabs(code)
    } else {
        code.to_string()
    };
    if has_markers {
        for (line_index, line) in code.split('\n').enumerate() {
            let mut end = line.trim_end().len();
            let mut found = Vec::new();
            // Markers are read from the end of the line backwards.
            while end > 0 {
                let head = &line[..end];
                let Some(open) = head.rfind('<') else {
                    break;
                };
                if !head.ends_with('>') {
                    break;
                }
                let label = &head[open + 1..end - 1];
                let label = label.trim_start_matches('!').trim_start_matches("--");
                let label = label.trim_end_matches("--");
                let valid = label == "."
                    || (!label.is_empty() && label.bytes().all(|b| b.is_ascii_digit()));
                if !valid {
                    break;
                }
                let start = line[..open].chars().count();
                let marker_end = start + head[open..end].chars().count();
                found.push((start, marker_end));
                end = head[..open].trim_end().len();
            }
            for (start, end) in found.into_iter().rev() {
                callouts.push(Callout {
                    line: line_index,
                    start,
                    end,
                });
            }
        }
    }
    CodeBlock {
        language,
        code,
        callouts,
    }
}

/// Strip the common indentation of a literal paragraph.
fn dedent_block(text: &str) -> String {
    let common = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(super::scan::indent_of)
        .min()
        .unwrap_or(0);
    text.lines()
        .map(|line| super::scan::dedent(line, common))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `= Title` (a level-0 section) or `# Title`.
fn is_document_title(line: &str) -> bool {
    matches!(section_level(line.trim_end()), Some((1, _)))
}

/// A section title: `== Title` (level 2) to `====== Title`, `= Title`, or
/// the Markdown forms with `#`. Returns the heading level and the title.
fn section_level(line: &str) -> Option<(u8, &str)> {
    let marker = *line.as_bytes().first()?;
    if marker != b'=' && marker != b'#' {
        return None;
    }
    let count = line.bytes().take_while(|&b| b == marker).count();
    if count > 6 {
        return None;
    }
    let rest = &line[count..];
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let mut title = rest.trim();
    // `== Title ==` closing marks.
    let closing: String = std::iter::repeat_n(marker as char, count).collect();
    if let Some(stripped) = title.strip_suffix(closing.as_str()) {
        if stripped.ends_with(' ') {
            title = stripped.trim_end();
        }
    }
    if title.is_empty() {
        return None;
    }
    Some((count as u8, title))
}

/// `:name: value`, `:name!:` and `:!name:`. `None` value: unset.
fn attribute_entry(line: &str) -> Option<(&str, Option<&str>)> {
    let rest = line.strip_prefix(':')?;
    let close = rest.find(':')?;
    let name = &rest[..close];
    let value = &rest[close + 1..];
    if !(value.is_empty() || value.starts_with([' ', '\t'])) {
        return None;
    }
    let (name, unset) = if let Some(name) = name.strip_prefix('!') {
        (name, true)
    } else if let Some(name) = name.strip_suffix('!') {
        (name, true)
    } else {
        (name, false)
    };
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && name.as_bytes()[0].is_ascii_alphanumeric();
    valid.then_some((name, (!unset).then(|| value.trim())))
}

/// `.Title`: a block title (not `..` or `. item`).
fn block_title(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('.')?;
    let first = rest.chars().next()?;
    (!first.is_whitespace() && first != '.').then_some(rest.trim_end())
}

/// A `[...]` block attribute line's contents.
fn attribute_line(trimmed: &str) -> Option<&str> {
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    if inner.starts_with('[') || inner.contains('\n') {
        return None;
    }
    // A checkbox or link text alone is not an attribute line.
    if inner.is_empty() || inner == " " || inner == "x" || inner == "*" {
        return (inner.is_empty()).then_some(inner);
    }
    Some(inner)
}

/// Parse `source,python,linenums`, `quote, Author, Title`, `%header`,
/// `cols="1,2",options="header"`, `#id.role%option` into `pending`.
fn merge_attributes(pending: &mut Pending, attributes: &str) {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in attributes.chars() {
        match (quote, ch) {
            (None, '"' | '\'') => {
                quote = Some(ch);
                current.push(ch);
            }
            (Some(open), _) if ch == open => {
                quote = None;
                current.push(ch);
            }
            (None, ',') => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    parts.push(current);
    pending.positional.clear();
    for (index, part) in parts.into_iter().enumerate() {
        let part = part.trim();
        if let Some((name, value)) = part.split_once('=') {
            if !name.contains(' ') && !name.is_empty() {
                pending.named.insert(
                    name.trim().to_string(),
                    value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string(),
                );
                continue;
            }
        }
        if index == 0 {
            // The first positional attribute holds the style, an id (`#id`),
            // roles (`.role`) and options (`%option`).
            let mut style = String::new();
            let mut chars = part.char_indices().peekable();
            let mut current_kind = 's';
            let mut current_start = 0;
            let mut tokens: Vec<(char, String)> = Vec::new();
            while let Some((at, ch)) = chars.next() {
                if matches!(ch, '#' | '.' | '%') {
                    tokens.push((current_kind, part[current_start..at].to_string()));
                    current_kind = ch;
                    current_start = at + 1;
                }
                if chars.peek().is_none() {
                    tokens.push((current_kind, part[current_start..].to_string()));
                }
            }
            for (kind, value) in tokens {
                match kind {
                    's' => style = value,
                    '%' => pending.options.push(value),
                    _ => {}
                }
            }
            pending.positional.push(style);
        } else {
            pending
                .positional
                .push(part.trim_matches('"').trim_matches('\'').to_string());
        }
    }
}

/// A delimiter line: the delimiter itself.
fn delimiter(line: &str) -> Option<&str> {
    if line == "--" {
        return Some(line);
    }
    if let Some(info) = line.strip_prefix("```") {
        return (!info.contains('`')).then_some(line);
    }
    if line.len() >= 4 {
        let first = line.as_bytes()[0];
        if matches!(first, b'-' | b'.' | b'=' | b'*' | b'_' | b'+' | b'/')
            && line.bytes().all(|b| b == first)
        {
            return Some(line);
        }
        if matches!(first, b'|' | b',' | b':')
            && line[1..].len() >= 3
            && line[1..].bytes().all(|b| b == b'=')
        {
            return Some(line);
        }
    }
    None
}

/// A thematic break: `'''`, or Markdown's `---`, `***`, `- - -`, `* * *`.
fn is_break(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| *c != ' ').collect();
    let all = |c: char| compact.len() >= 3 && compact.chars().all(|x| x == c);
    (all('\'') && !line.contains(' ')) || (compact.len() == 3 && (all('-') || all('*')))
}

/// A comment line: `//` but not a `////` block delimiter.
fn is_comment_line(line: &str) -> bool {
    line.starts_with("//") && !line.starts_with("////")
}

/// `ifdef::`, `ifndef::`, `ifeval::` and `endif::` lines.
fn is_conditional(line: &str) -> bool {
    (line.starts_with("ifdef::")
        || line.starts_with("ifndef::")
        || line.starts_with("ifeval::")
        || line.starts_with("endif::"))
        && line.trim_end().ends_with(']')
}

/// `NOTE: text`: an admonition paragraph's label and text.
fn admonition_label(line: &str) -> Option<(&str, &str)> {
    for label in ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"] {
        if let Some(rest) = line.strip_prefix(label) {
            if let Some(rest) = rest.strip_prefix(": ") {
                return Some((label, rest));
            }
        }
    }
    None
}

/// `name::target[attributes]`: a block macro.
fn block_macro(line: &str) -> Option<(&str, &str, &str)> {
    let colons = line.find("::")?;
    let name = &line[..colons];
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let rest = &line[colons + 2..];
    let open = rest.find('[')?;
    if !rest.ends_with(']') {
        return None;
    }
    let target = &rest[..open];
    if target.contains(char::is_whitespace) {
        return None;
    }
    Some((name, target, &rest[open + 1..rest.len() - 1]))
}

/// The built-in attributes that stand for characters.
fn builtin_attribute(name: &str) -> Option<&'static str> {
    Some(match name {
        "nbsp" => "\u{a0}",
        "zwsp" => "\u{200b}",
        "wj" => "\u{2060}",
        "empty" => "",
        "sp" | "space" => " ",
        "blank" => "",
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "plus" => "+",
        "brvbar" => "¦",
        "vbar" => "|",
        "caret" => "^",
        "tilde" => "~",
        "apos" => "'",
        "backslash" => "\\",
        "backtick" => "`",
        "startsb" => "[",
        "endsb" => "]",
        "deg" => "°",
        "two-colons" => "::",
        "two-semicolons" => ";;",
        "asterisk" => "*",
        "quot" => "\"",
        "ldquo" => "“",
        "rdquo" => "”",
        "lsquo" => "‘",
        "rsquo" => "’",
        "cpp" => "C++",
        "pp" => "++",
        "cxx" => "C++",
        _ => return None,
    })
}

/// A footnote met while parsing a run of inlines: `footnote:id[text]`.
struct Footnote {
    id: Option<String>,
    text: Option<Vec<Inline>>,
}

/// The placeholder style footnote markers carry until they are numbered.
const FOOTNOTE_PLACEHOLDER: &str = "document.footnote";

/// Replace each footnote placeholder (`[?]` in order) with its number.
fn renumber(inlines: &mut [Inline], numbers: &[Option<usize>]) {
    fn walk(inlines: &mut [Inline], numbers: &[Option<usize>], next: &mut usize) {
        for inline in inlines {
            match inline {
                Inline::Styled { style, content } if style == FOOTNOTE_PLACEHOLDER => {
                    if let [Inline::Text(text)] = content.as_mut_slice() {
                        if text == "[?]" {
                            if let Some(Some(number)) = numbers.get(*next) {
                                *text = format!("[{number}]");
                            }
                            *next += 1;
                        }
                    }
                }
                Inline::Emphasis(content)
                | Inline::Strong(content)
                | Inline::Underline(content)
                | Inline::Strike(content)
                | Inline::Styled { content, .. }
                | Inline::Link { content, .. } => walk(content, numbers, next),
                _ => {}
            }
        }
    }
    let mut next = 0;
    walk(inlines, numbers, &mut next);
}

/// The quote marks AsciiDoc pairs, and what each makes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Strong,
    Emphasis,
    Mono,
    Highlight,
}

const MARKS: [(u8, Mark); 4] = [
    (b'*', Mark::Strong),
    (b'_', Mark::Emphasis),
    (b'`', Mark::Mono),
    (b'#', Mark::Highlight),
];

/// AsciiDoc inline markup, scanned left to right with closers indexed up
/// front.
struct Inliner<'a> {
    text: &'a str,
    /// Constrained closers per mark: the mark after a non-space, before a
    /// non-word character.
    constrained: [Positions; 4],
    /// Unconstrained closers per mark: every doubled mark.
    unconstrained: [Positions; 4],
    /// `]` not escaped with a backslash.
    brackets: Positions,
    xref_ends: Positions,
}

impl<'a> Inliner<'a> {
    fn new(text: &'a str) -> Self {
        let mut constrained: [Vec<usize>; 4] = Default::default();
        let mut unconstrained: [Vec<usize>; 4] = Default::default();
        let mut brackets = Vec::new();
        let bytes = text.as_bytes();
        for (at, &byte) in bytes.iter().enumerate() {
            if byte == b']' && (at == 0 || bytes[at - 1] != b'\\') {
                brackets.push(at);
            }
            let Some(index) = MARKS.iter().position(|(mark, _)| *mark == byte) else {
                continue;
            };
            if bytes.get(at + 1) == Some(&byte) {
                unconstrained[index].push(at);
            }
            let before = char_before(text, at);
            let after = char_at(text, at + 1);
            if before.is_some_and(|c| !c.is_whitespace()) && after.is_none_or(|c| !is_word(c)) {
                constrained[index].push(at);
            }
        }
        Inliner {
            text,
            constrained: constrained.map(Positions::from_sorted),
            unconstrained: unconstrained.map(Positions::from_sorted),
            brackets: Positions::from_sorted(brackets),
            xref_ends: Positions::of(text, ">>"),
        }
    }

    /// Text without markup, as inlines (a description list's term is
    /// parsed again by the caller when it needs markup).
    fn plain_inline(text: &str) -> Vec<Inline> {
        let inliner = Inliner::new(text);
        let mut footnotes = Vec::new();
        trim_inlines(inliner.run(0, text.len(), 0, &mut footnotes))
    }

    fn run(
        &self,
        start: usize,
        end: usize,
        depth: usize,
        footnotes: &mut Vec<Footnote>,
    ) -> Vec<Inline> {
        let text = self.text;
        let bytes = text.as_bytes();
        let mut out = InlineBuf::default();
        let mut i = start;
        while i < end {
            let byte = bytes[i];
            match byte {
                b'\n' => {
                    out.push(' ');
                    i += 1;
                    continue;
                }
                b' ' if text[i..end].starts_with(" +\n")
                    || (i + 2 == end && text[i..end] == *" +") =>
                {
                    out.inline(Inline::LineBreak);
                    i += 3.min(end - i);
                    continue;
                }
                b'\\' if i + 1 < end => {
                    // An escaped URL, mark, macro or replacement stays text.
                    if let Some(url) = url_end(text, i + 1).filter(|&url| url <= end) {
                        out.push_str(&text[i + 1..url]);
                        i = url;
                        continue;
                    }
                    let next = char_at(text, i + 1).expect("a char boundary");
                    if matches!(
                        next,
                        '*' | '_'
                            | '`'
                            | '#'
                            | '{'
                            | '['
                            | '<'
                            | '+'
                            | '^'
                            | '~'
                            | '\\'
                            | '('
                            | '-'
                            | '.'
                            | '&'
                    ) {
                        out.push(next);
                        i += 1 + next.len_utf8();
                        continue;
                    }
                }
                b'+' => {
                    if let Some((inline, next)) = self.passthrough(i, end) {
                        out.inline(inline);
                        i = next;
                        continue;
                    }
                }
                b'[' => {
                    if let Some((inline, next)) = self.role(i, end, depth, footnotes) {
                        out.inline(inline);
                        i = next;
                        continue;
                    }
                }
                b'<' => {
                    if let Some((inline, next)) = self.xref(i, end) {
                        out.inline(inline);
                        i = next;
                        continue;
                    }
                }
                b'&' => {
                    if let Some((decoded, next)) = char_reference(text, i, end) {
                        out.push_str(&decoded);
                        i = next;
                        continue;
                    }
                }
                b'(' | b'-' | b'.' | b'=' => {
                    if let Some((replacement, next)) = replacement(text, i, end) {
                        out.push_str(replacement);
                        i = next;
                        continue;
                    }
                }
                _ => {}
            }
            if let Some(index) = MARKS.iter().position(|(mark, _)| *mark == byte) {
                if let Some((inline, next)) = self.quote(index, i, end, depth, footnotes) {
                    out.inline(inline);
                    i = next;
                    continue;
                }
            }
            if byte.is_ascii_alphabetic() {
                if let Some((inline, next)) = self.inline_macro(i, end, depth, footnotes) {
                    out.inline(inline);
                    i = next;
                    continue;
                }
            }
            let ch = char_at(text, i).expect("i is a char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
        out.finish()
    }

    /// A quoted span opening at `at`: unconstrained (doubled) first, then
    /// constrained.
    fn quote(
        &self,
        index: usize,
        at: usize,
        end: usize,
        depth: usize,
        footnotes: &mut Vec<Footnote>,
    ) -> Option<(Inline, usize)> {
        let text = self.text;
        let bytes = text.as_bytes();
        let mark = bytes[at];
        let (open_len, close) = if bytes.get(at + 1) == Some(&mark) {
            let close = self.unconstrained[index].at_or_after(at + 3);
            match close {
                Some(close) if close + 2 <= end => (2, close),
                _ => return self.constrained_quote(index, at, end, depth, footnotes),
            }
        } else {
            return self.constrained_quote(index, at, end, depth, footnotes);
        };
        Some((
            self.span(MARKS[index].1, at + open_len, close, depth, footnotes),
            close + open_len,
        ))
    }

    fn constrained_quote(
        &self,
        index: usize,
        at: usize,
        end: usize,
        depth: usize,
        footnotes: &mut Vec<Footnote>,
    ) -> Option<(Inline, usize)> {
        let text = self.text;
        let before = char_before(text, at);
        let after = char_at(text, at + 1)?;
        let opens = before.is_none_or(|c| !is_word(c) && !matches!(c, ';' | ':' | '}'))
            && !after.is_whitespace();
        if !opens {
            return None;
        }
        let close = self.constrained[index].at_or_after(at + 2)?;
        if close >= end || text[at + 1..close].contains("\n\n") {
            return None;
        }
        Some((
            self.span(MARKS[index].1, at + 1, close, depth, footnotes),
            close + 1,
        ))
    }

    fn span(
        &self,
        mark: Mark,
        start: usize,
        end: usize,
        depth: usize,
        footnotes: &mut Vec<Footnote>,
    ) -> Inline {
        let raw = &self.text[start..end];
        if mark == Mark::Mono {
            // `` `+literal+` `` keeps its text as written.
            let inner = raw
                .strip_prefix('+')
                .and_then(|r| r.strip_suffix('+'))
                .unwrap_or(raw);
            return Inline::Code(inner.replace('\n', " "));
        }
        let content = if depth >= MAX_NESTING {
            vec![Inline::Text(raw.replace('\n', " "))]
        } else {
            self.run(start, end, depth + 1, footnotes)
        };
        match mark {
            Mark::Strong => Inline::Strong(content),
            Mark::Emphasis => Inline::Emphasis(content),
            Mark::Highlight => Inline::styled("document.mark", content),
            Mark::Mono => unreachable!("returned above"),
        }
    }

    /// `+text+`, `++text++` and `+++text+++`: literal text. The single form
    /// is constrained: no space just inside it, no word character outside.
    fn passthrough(&self, at: usize, end: usize) -> Option<(Inline, usize)> {
        let text = self.text;
        let rest = &text[at..end];
        let marks = if rest.starts_with("+++") {
            "+++"
        } else if rest.starts_with("++") {
            "++"
        } else {
            "+"
        };
        let body = &rest[marks.len()..];
        let close = prefix(body, 4096).find(marks)?;
        if close == 0 {
            return None;
        }
        let inner = &body[..close];
        let next = at + marks.len() + close + marks.len();
        if marks == "+"
            && (inner.starts_with(char::is_whitespace)
                || inner.ends_with(char::is_whitespace)
                || char_before(text, at).is_some_and(is_word)
                || char_at(text, next).is_some_and(is_word))
        {
            return None;
        }
        Some((Inline::Text(inner.replace('\n', " ")), next))
    }

    /// `[.role]#text#`: a role on highlighted text. `line-through` strikes it
    /// and `underline` underlines it; other roles leave it plain.
    fn role(
        &self,
        at: usize,
        end: usize,
        depth: usize,
        footnotes: &mut Vec<Footnote>,
    ) -> Option<(Inline, usize)> {
        let text = self.text;
        let close = self.brackets.at_or_after(at + 1)?;
        if close + 1 >= end || text.as_bytes()[close + 1] != b'#' || close - at > 128 {
            // `[[[bibliography]]]` and `[[anchor]]` inline anchors.
            if text[at..end].starts_with("[[") {
                let close = text[at..end.min(at + 256)].find("]]")?;
                let inner = &text[at + 2..at + close];
                if inner.contains(char::is_whitespace) && !inner.contains(',') {
                    return None;
                }
                let shown = inner
                    .strip_prefix('[')
                    .map(|bib| format!("[{}]", bib.split(',').next().unwrap_or(bib)))
                    .unwrap_or_default();
                let next = at + close + 2 + usize::from(inner.starts_with('['));
                return Some((Inline::Text(shown), next.min(end)));
            }
            return None;
        }
        let roles = &text[at + 1..close];
        if roles.contains('\n') {
            return None;
        }
        let hash = close + 1;
        let doubled = text.as_bytes().get(hash + 1) == Some(&b'#');
        let open_len = if doubled { 2 } else { 1 };
        let content_start = hash + open_len;
        let index = 3;
        let close_at = if doubled {
            self.unconstrained[index].at_or_after(content_start + 1)?
        } else {
            self.constrained[index].at_or_after(content_start + 1)?
        };
        if close_at >= end {
            return None;
        }
        let content = if depth >= MAX_NESTING {
            vec![Inline::Text(text[content_start..close_at].to_string())]
        } else {
            self.run(content_start, close_at, depth + 1, footnotes)
        };
        let inline = if roles.contains("line-through") {
            Inline::Strike(content)
        } else if roles.contains("underline") {
            Inline::Underline(content)
        } else {
            Inline::styled("none", content)
        };
        Some((inline, close_at + open_len))
    }

    /// `<<id>>` and `<<id,text>>`: a cross reference, shown as its text.
    fn xref(&self, at: usize, end: usize) -> Option<(Inline, usize)> {
        let text = self.text;
        if !text[at..end].starts_with("<<") {
            return None;
        }
        let close = self.xref_ends.at_or_after(at + 2)?;
        if close + 2 > end || close - at > 512 {
            return None;
        }
        let inner = &text[at + 2..close];
        if inner.is_empty() || inner.contains('\n') || inner.contains('<') {
            return None;
        }
        let shown = match inner.split_once(',') {
            Some((_, label)) => label.trim().to_string(),
            None => inner
                .trim_start_matches('#')
                .replace(['_', '-'], " ")
                .trim()
                .to_string(),
        };
        Some((Inline::Underline(vec![Inline::Text(shown)]), close + 2))
    }

    /// `name:target[text]` inline macros, bare URLs and their link text.
    fn inline_macro(
        &self,
        at: usize,
        end: usize,
        depth: usize,
        footnotes: &mut Vec<Footnote>,
    ) -> Option<(Inline, usize)> {
        let text = self.text;
        if let Some(url) = url_end(text, at).filter(|&url| url <= end) {
            let target = &text[at..url];
            // `https://example.com[Example]`.
            if url < end && text.as_bytes()[url] == b'[' {
                if let Some(close) = self.brackets.at_or_after(url + 1).filter(|&c| c < end) {
                    let label = link_label(&text[url + 1..close]);
                    let content = if label.is_empty() || depth >= MAX_NESTING {
                        Vec::new()
                    } else {
                        Inliner::plain_inline(label)
                    };
                    return Some((
                        Inline::Link {
                            target: target.to_string(),
                            content,
                        },
                        close + 1,
                    ));
                }
            }
            return Some((
                Inline::Link {
                    target: target.to_string(),
                    content: Vec::new(),
                },
                url,
            ));
        }
        if char_before(text, at).is_some_and(|c| c.is_alphanumeric()) {
            return None;
        }
        let name_len = text[at..end]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
            .count();
        let name = &text[at..at + name_len];
        if !MACROS.contains(&name) || !text[at + name_len..end].starts_with(':') {
            return None;
        }
        let target_start = at + name_len + 1;
        let open = prefix(&text[target_start..end], 1024).find('[')? + target_start;
        let target = &text[target_start..open];
        if target.contains(char::is_whitespace) && name != "menu" {
            return None;
        }
        let close = self.brackets.at_or_after(open + 1).filter(|&c| c < end)?;
        let inner = &text[open + 1..close];
        let next = close + 1;
        let inline = match name {
            "link" => Inline::Link {
                target: target.to_string(),
                content: if link_label(inner).is_empty() {
                    Vec::new()
                } else {
                    Inliner::plain_inline(link_label(inner))
                },
            },
            "mailto" => Inline::Link {
                target: format!("mailto:{target}"),
                content: vec![Inline::text(if link_label(inner).is_empty() {
                    target
                } else {
                    link_label(inner)
                })],
            },
            "image" => Inline::Image {
                target: target.to_string(),
                alt: inner.split(',').next().unwrap_or("").trim().to_string(),
            },
            "footnote" | "footnoteref" => {
                let (id, body) = match name {
                    "footnoteref" => match inner.split_once(',') {
                        Some((id, body)) => (Some(id.trim().to_string()), Some(body)),
                        None => (Some(inner.trim().to_string()), None),
                    },
                    _ => (
                        (!target.is_empty()).then(|| target.to_string()),
                        (!inner.trim().is_empty()).then_some(inner),
                    ),
                };
                let body = body.map(|body| {
                    if depth >= MAX_NESTING {
                        vec![Inline::text(body.replace("\\]", "]"))]
                    } else {
                        Inliner::plain_inline(&body.replace("\\]", "]"))
                    }
                });
                footnotes.push(Footnote { id, text: body });
                Inline::styled(FOOTNOTE_PLACEHOLDER, vec![Inline::text("[?]")])
            }
            "kbd" => {
                let keys: Vec<&str> = inner
                    .split(['+', ','])
                    .map(str::trim)
                    .filter(|key| !key.is_empty())
                    .collect();
                Inline::styled("markdown.kbd", vec![Inline::Text(keys.join("+"))])
            }
            "btn" => Inline::Strong(vec![Inline::Text(format!("[{}]", inner.trim()))]),
            "menu" => {
                let mut parts = vec![target.trim().to_string()];
                parts.extend(
                    inner
                        .split('>')
                        .map(|part| part.trim().to_string())
                        .filter(|part| !part.is_empty()),
                );
                Inline::Strong(vec![Inline::Text(parts.join(" \u{203a} "))])
            }
            "xref" => {
                let shown = if inner.trim().is_empty() {
                    target.trim_start_matches('#').replace(['_', '-'], " ")
                } else {
                    inner.trim().to_string()
                };
                Inline::Underline(vec![Inline::Text(shown)])
            }
            "anchor" => Inline::Text(String::new()),
            "pass" => Inline::Text(inner.to_string()),
            "stem" | "latexmath" | "asciimath" => Inline::Code(inner.to_string()),
            "icon" => Inline::Text(format!("[{target}]")),
            _ => return None,
        };
        Some((inline, next))
    }
}

/// The inline macros the parser draws.
const MACROS: &[&str] = &[
    "link",
    "mailto",
    "image",
    "footnote",
    "footnoteref",
    "kbd",
    "btn",
    "menu",
    "xref",
    "anchor",
    "pass",
    "stem",
    "latexmath",
    "asciimath",
    "icon",
];

/// A link macro's text: up to the first `,` when attributes follow it,
/// without a trailing `^` (open in a new window), and unquoted.
fn link_label(inner: &str) -> &str {
    let label = match inner.split_once(',') {
        Some((label, rest)) if rest.contains('=') => label,
        _ => inner,
    };
    label.trim().trim_end_matches('^').trim_matches('"')
}

/// `\w` as AsciiDoc's patterns read it.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `&copy;`, `&#169;`, `&#xa9;`: the character and where the reference ends.
fn char_reference(text: &str, at: usize, end: usize) -> Option<(String, usize)> {
    let rest = &text[at + 1..end];
    let semicolon = rest.get(..rest.len().min(40))?.find(';')?;
    let name = &rest[..semicolon];
    let next = at + 1 + semicolon + 1;
    if let Some(number) = name.strip_prefix('#') {
        let value = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(value).map(|c| (c.to_string(), next));
    }
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let key = format!("{name};");
    let entities = crate::rst::entities::ENTITIES;
    let index = entities
        .binary_search_by(|(entity, _)| (*entity).cmp(key.as_str()))
        .ok()?;
    Some((entities[index].1.to_string(), next))
}

/// AsciiDoc's typographic replacements at `at`.
fn replacement(text: &str, at: usize, end: usize) -> Option<(&'static str, usize)> {
    let rest = &text[at..end];
    for (from, to) in [("(C)", "©"), ("(R)", "®"), ("(TM)", "™")] {
        if rest.starts_with(from) {
            return Some((to, at + from.len()));
        }
    }
    if rest.starts_with("...") {
        return Some(("…", at + 3));
    }
    if rest.starts_with("->") {
        return Some(("→", at + 2));
    }
    if rest.starts_with("=>") {
        return Some(("⇒", at + 2));
    }
    if rest.starts_with("--") && !rest.starts_with("---") {
        let before = char_before(text, at);
        let after = char_at(text, at + 2);
        // ` -- ` and `word--word` are em dashes.
        let spaced = before == Some(' ') && after.is_none_or(|c| c == ' ' || c == '\n');
        let joined = before.is_some_and(is_word) && after.is_some_and(is_word);
        if spaced || joined {
            return Some(("—", at + 2));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::inlines_text;

    fn blocks(source: &str) -> Vec<Block> {
        parse(source).document.blocks
    }

    #[test]
    fn the_header_is_a_title_and_byline() {
        let blocks = blocks("= The Guide\nAda Lovelace <ada@example.com>\nv1.2, 2024-05-01\n:toc:\n\n== Intro\n\nHello {author}.");
        assert!(
            matches!(&blocks[0], Block::Heading(h) if h.level == 1 && inlines_text(&h.content) == "The Guide")
        );
        let Block::Byline(byline) = &blocks[1] else {
            panic!("{blocks:?}");
        };
        assert!(inlines_text(byline).contains("Ada Lovelace"));
        assert!(matches!(&blocks[2], Block::Heading(h) if h.level == 2));
        assert_eq!(
            blocks[3],
            Block::Paragraph(vec![Inline::text("Hello Ada Lovelace.")])
        );
    }

    #[test]
    fn quotes_constrained_and_unconstrained() {
        let mut doc = AsciiDoc {
            attributes: HashMap::new(),
            diagnostics: Vec::new(),
            footnotes: Vec::new(),
            footnote_ids: HashMap::new(),
        };
        let inlines = doc.inline("*bold* _it_ `code` **un**constrained #mark# a*b*c", 0);
        assert!(matches!(&inlines[0], Inline::Strong(_)));
        assert!(matches!(&inlines[2], Inline::Emphasis(_)));
        assert_eq!(inlines[4], Inline::Code("code".into()));
        assert!(matches!(&inlines[6], Inline::Strong(c) if inlines_text(c) == "un"));
        assert!(matches!(&inlines[8], Inline::Styled { style, .. } if style == "document.mark"));
        assert!(matches!(inlines.last(), Some(Inline::Text(t)) if t.ends_with("a*b*c")));
    }

    #[test]
    fn lists_nest_by_marker() {
        // As in Asciidoctor, a line comment keeps lists apart.
        let blocks = blocks("* one\n** nested\n* [x] two\n\n//\n. first\n. second\n\n//\nCPU:: the brain\nRAM::\n  memory");
        let Block::List(list) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert_eq!(list.items.len(), 2);
        assert!(matches!(&list.items[0].blocks[1], Block::List(_)));
        assert_eq!(list.items[1].checkbox, Some(Checkbox::Checked));
        assert!(matches!(&blocks[1], Block::List(l) if l.kind == ListKind::Ordered { start: 1 }));
        let Block::Definitions(definitions) = &blocks[2] else {
            panic!("{blocks:?}");
        };
        assert_eq!(definitions.len(), 2);
    }

    #[test]
    fn source_blocks_keep_callouts() {
        let blocks = blocks("[source,ruby]\n----\nputs 1 # <1>\n----\n<1> Prints.");
        let Block::Code(code) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert_eq!(code.language.as_deref(), Some("ruby"));
        assert_eq!(code.callouts.len(), 1);
        assert_eq!((code.callouts[0].start, code.callouts[0].end), (9, 12));
        assert!(matches!(&blocks[1], Block::List(l) if l.kind == ListKind::Callout));
    }

    #[test]
    fn admonitions_both_forms() {
        let blocks = blocks("NOTE: Mind the gap.\n\n[WARNING]\n====\nHot.\n====");
        assert!(matches!(
            &blocks[0],
            Block::Admonition {
                kind: AdmonitionKind::Note,
                ..
            }
        ));
        assert!(matches!(
            &blocks[1],
            Block::Admonition {
                kind: AdmonitionKind::Warning,
                ..
            }
        ));
    }

    #[test]
    fn tables_with_implicit_headers() {
        let blocks =
            blocks(".Sizes\n[cols=\"1,>1\"]\n|===\n| Name | Size\n\n| a | 1\n| b | 2\n|===");
        let Block::Table(table) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert!(table.header.is_some());
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.align, [Align::Left, Align::Right]);
        assert!(table.title.is_some());
    }

    #[test]
    fn links_and_macros() {
        let blocks =
            blocks("See https://asciidoc.org[AsciiDoc], kbd:[Ctrl+C] and image:logo.png[Logo].");
        let Block::Paragraph(inlines) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert!(
            matches!(&inlines[1], Inline::Link { target, .. } if target == "https://asciidoc.org")
        );
        assert!(inlines.iter().any(|i| matches!(i, Inline::Image { .. })));
    }

    #[test]
    fn attribute_values_are_cut_at_the_limit() {
        // Each attribute is ten of the one before: 10^9 bytes unbounded.
        let mut source = String::from(":a0: xxxxxxxxxx\n");
        for level in 1..10 {
            let refs = format!("{{a{}}}", level - 1).repeat(10);
            source.push_str(&format!(":a{level}: {refs}\n"));
        }
        source.push_str("\n{a9}{a9}{a9}\n");
        let parsed = parse(&source);
        assert!(parsed
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning && d.message.contains(":a5:")));
        let Some(Block::Paragraph(inlines)) = parsed.document.blocks.last() else {
            panic!("{:?}", parsed.document.blocks);
        };
        assert!(inlines_text(inlines).len() <= ATTRIBUTE_LIMIT);
    }

    #[test]
    fn includes_warn_and_show_as_written() {
        let parsed = parse("include::chapter.adoc[]");
        assert_eq!(parsed.diagnostics.len(), 1);
        assert!(matches!(
            &parsed.document.blocks[0],
            Block::Properties { .. }
        ));
    }

    #[test]
    fn footnotes_collect_at_the_end() {
        let blocks = blocks("A claim.footnote:[Citation needed.] Done.");
        let Block::Paragraph(inlines) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert!(inlines_text(inlines).contains("[1]"));
        assert!(matches!(blocks.last(), Some(Block::List(_))));
    }

    #[test]
    fn sniffing() {
        assert!(sniff("= Title\n:toc:\n\n== Part\n") >= 0.9);
        assert!(sniff("[source,python]\n----\nx\n----\n") >= 0.9);
        assert!(sniff("# Title\n\nSome *markdown*.\n") < 0.6);
        assert_eq!(sniff("-----BEGIN PGP SIGNATURE-----\n"), 0.0);
    }
}
