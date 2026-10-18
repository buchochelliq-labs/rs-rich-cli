//! Org mode, parsed into the [document model](super::model).
//!
//! Covered: the headline tree with TODO keywords (`TODO`/`DONE`, or those a
//! `#+TODO:` line declares), priorities and tags; `#+TITLE`, `#+SUBTITLE`,
//! `#+AUTHOR`, `#+DATE` and `#+EMAIL` as the title and byline, `#+CAPTION`
//! on the next table, image or block, and the remaining keywords shown dim;
//! paragraphs; `*bold*`, `/italic/`, `_underline_`, `=verbatim=`, `~code~`
//! and `+strike+`; plain, numbered and description lists with checkboxes and
//! `[@N]` counters, nested by indentation; `#+BEGIN_SRC lang`, `EXAMPLE`,
//! `QUOTE`, `VERSE`, `CENTER` and `COMMENT` blocks, `NOTE`/`TIP`/`WARNING`
//! style special blocks as admonitions, and `: ` fixed-width lines; tables
//! with `|---|` rules and `<l>`/`<c>`/`<r>` alignment cookies; `[[link][desc]]`,
//! `[[link]]`, angle and bare links, and image links as placeholders;
//! `PROPERTIES` and other drawers and planning lines in a compact dim form;
//! timestamps, footnote references and definitions, statistics cookies,
//! `\\` line breaks, horizontal rules and a few common entities (`\alpha`).
//!
//! Shown as text: macros (`{{{name}}}`), radio targets and inline Babel calls.
//! Not shown: comments, `COMMENT` and `EXPORT` blocks, export snippets
//! (`@@html:…@@`), `#+NAME`/`#+ATTR_*`-style element keywords, and `#+TBLFM`
//! lines. `#+INCLUDE` and `#+SETUPFILE` are not followed (a warning says so).

use std::collections::HashMap;

use super::model::{
    AdmonitionKind, Align, Block, Checkbox, CodeBlock, Definition, Document, Heading, Inline,
    Keyword, List, ListItem, ListKind, TableBlock,
};
use super::scan::{
    char_at, char_before, dedent, indent_of, join_lines, normalize, sample, trim_inlines, url_end,
    InlineBuf, Positions, BLOCK_WEIGHT, MAX_NESTING,
};
use super::{Diagnostic, Parsed, Severity};

/// Parse an Org document.
pub fn parse(source: &str) -> Parsed {
    let source = normalize(source);
    let lines: Vec<Line> = source
        .split('\n')
        .enumerate()
        .map(|(index, text)| Line {
            no: index + 1,
            text: text.to_string(),
        })
        .collect();
    let mut org = Org::new(&lines);
    let mut blocks = org.front_matter();
    blocks.extend(org.elements(&lines, true, 0));
    Parsed {
        document: Document { blocks },
        diagnostics: org.diagnostics,
    }
}

/// How confident it is that `text` is Org (0 to 1).
pub fn sniff(text: &str) -> f32 {
    let text = sample(text);
    if text.starts_with("#!") {
        return 0.0;
    }
    let first = text.lines().next().unwrap_or("").to_ascii_lowercase();
    if first.contains("-*-") && first.contains("mode: org") {
        return 1.0;
    }
    let (mut keywords, mut blocks, mut drawers, mut todo, mut headlines, mut links) =
        (0, 0, 0, 0, 0, 0);
    for line in text.lines().take(4000) {
        let trimmed = line.trim_start();
        let upper = trimmed
            .get(..trimmed.len().min(16))
            .unwrap_or("")
            .to_ascii_uppercase();
        if let Some(name) = upper.strip_prefix("#+") {
            if STRONG_KEYWORDS
                .iter()
                .any(|keyword| name.starts_with(keyword))
            {
                keywords += 1;
            } else if name.starts_with("BEGIN_") {
                blocks += 1;
            }
        }
        if trimmed.eq_ignore_ascii_case(":PROPERTIES:") || trimmed.eq_ignore_ascii_case(":END:") {
            drawers += 1;
        }
        let stars = line.bytes().take_while(|&b| b == b'*').count();
        if stars > 0 && line[stars..].starts_with(' ') {
            headlines += 1;
            let title = line[stars..].trim_start();
            if title.starts_with("TODO ") || title.starts_with("DONE ") {
                todo += 1;
            }
        }
        if trimmed.contains("[[") && trimmed.contains("][") && trimmed.contains("]]") {
            links += 1;
        }
    }
    let mut score: f32 = 0.0;
    let mut signals = 0;
    for (present, weight) in [
        (keywords > 0, 0.9),
        (blocks > 0, 0.8),
        (drawers >= 2, 0.8),
        (todo > 0, 0.75),
        (links > 0 && headlines > 0, 0.7),
        (headlines >= 2, 0.4),
    ] {
        if present {
            score = score.max(weight);
            signals += 1;
        }
    }
    if signals > 1 {
        score += 0.05 * (signals - 1) as f32;
    }
    score.min(1.0)
}

/// Keywords that say "this is Org" (sniffing).
const STRONG_KEYWORDS: &[&str] = &[
    "TITLE:",
    "AUTHOR:",
    "DATE:",
    "OPTIONS:",
    "STARTUP:",
    "FILETAGS:",
    "SETUPFILE:",
    "PROPERTY:",
    "TODO:",
    "SEQ_TODO:",
    "LANGUAGE:",
    "EMAIL:",
    "SUBTITLE:",
    "DESCRIPTION:",
];

/// One source line and its 1-based number.
#[derive(Clone, Debug)]
struct Line {
    no: usize,
    text: String,
}

struct Org {
    todo: Vec<String>,
    done: Vec<String>,
    title: Option<String>,
    subtitle: Option<String>,
    byline: Vec<String>,
    /// Headline levels move down one when the document has a title, so the
    /// title is the only level-1 heading, as in an exported Org file.
    level_offset: u8,
    diagnostics: Vec<Diagnostic>,
}

impl Org {
    /// Read the keywords that apply to the whole file: TODO sequences, the
    /// title and the byline.
    fn new(lines: &[Line]) -> Self {
        let mut org = Org {
            todo: Vec::new(),
            done: Vec::new(),
            title: None,
            subtitle: None,
            byline: Vec::new(),
            level_offset: 0,
            diagnostics: Vec::new(),
        };
        let mut author = None;
        let mut date = None;
        let mut email = None;
        for line in lines {
            let Some((name, value)) = keyword(line.text.trim_start()) else {
                continue;
            };
            let value = value.trim();
            match name.to_ascii_uppercase().as_str() {
                "TODO" | "SEQ_TODO" | "TYP_TODO" => org.todo_sequence(value),
                "TITLE" if !value.is_empty() => {
                    // Several TITLE lines join, as Org joins them.
                    org.title = Some(match org.title.take() {
                        Some(title) => format!("{title} {value}"),
                        None => value.to_string(),
                    });
                }
                "SUBTITLE" if !value.is_empty() => org.subtitle = Some(value.to_string()),
                "AUTHOR" if !value.is_empty() => author = Some(value.to_string()),
                "DATE" if !value.is_empty() => date = Some(value.to_string()),
                "EMAIL" if !value.is_empty() => email = Some(value.to_string()),
                _ => {}
            }
        }
        if org.todo.is_empty() && org.done.is_empty() {
            org.todo = vec!["TODO".into()];
            org.done = vec!["DONE".into()];
        }
        org.byline = [author, email, date].into_iter().flatten().collect();
        if org.title.is_some() {
            org.level_offset = 1;
        }
        org
    }

    /// `#+TODO: TODO NEXT | DONE CANCELLED`: before the bar are open states,
    /// after it done states; with no bar the last word is the done state.
    /// Fast-access keys (`TODO(t)`) are dropped.
    fn todo_sequence(&mut self, value: &str) {
        let words: Vec<String> = value
            .split_whitespace()
            .map(|word| word.split('(').next().unwrap_or(word).to_string())
            .collect();
        match words.iter().position(|word| word == "|") {
            Some(bar) => {
                self.todo.extend(words[..bar].iter().cloned());
                self.done.extend(words[bar + 1..].iter().cloned());
            }
            None => {
                if let Some((last, open)) = words.split_last() {
                    self.todo.extend(open.iter().cloned());
                    self.done.push(last.clone());
                }
            }
        }
        self.todo.retain(|word| !word.is_empty());
        self.done.retain(|word| !word.is_empty());
    }

    fn front_matter(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();
        if let Some(title) = self.title.clone() {
            blocks.push(Block::Heading(Heading {
                level: 1,
                content: inline(&title, 0),
                ..Heading::default()
            }));
        }
        if let Some(subtitle) = self.subtitle.clone() {
            blocks.push(Block::Byline(inline(&subtitle, 0)));
        }
        if !self.byline.is_empty() {
            blocks.push(Block::Byline(inline(&self.byline.join(" \u{b7} "), 0)));
        }
        blocks
    }

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

    /// Parse `lines` into blocks. `top` is the document level, where a star
    /// in the first column starts a headline rather than a list item.
    fn elements(&mut self, lines: &[Line], top: bool, depth: usize) -> Vec<Block> {
        let ends = BlockEnds::new(lines);
        let mut blocks = Vec::new();
        let mut caption: Option<Vec<Inline>> = None;
        let mut keywords: Vec<(String, String)> = Vec::new();
        // Planning lines and a property drawer may follow a headline.
        let mut after_headline = false;
        let mut i = 0;
        while i < lines.len() {
            let line = &lines[i].text;
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                after_headline = false;
                i += 1;
                continue;
            }
            let is_keyword = keyword(trimmed).is_some() && !is_block_start(trimmed);
            if !is_keyword && !keywords.is_empty() {
                blocks.push(Block::Properties {
                    name: None,
                    entries: std::mem::take(&mut keywords),
                });
            }
            // A caption names the element after it, past any other keywords.
            let element_caption = if is_keyword { None } else { caption.take() };
            if top && is_headline(line) {
                blocks.push(Block::Heading(self.headline(line)));
                after_headline = true;
                i += 1;
                continue;
            }
            if after_headline {
                if let Some(entries) = planning(trimmed) {
                    blocks.push(Block::Properties {
                        name: None,
                        entries,
                    });
                    i += 1;
                    continue;
                }
            }
            after_headline = false;
            if let Some(name) = drawer_name(trimmed) {
                if let Some(end) = ends.drawer_end(i) {
                    blocks.push(self.drawer(name, &lines[i + 1..end]));
                    i = end + 1;
                    continue;
                }
            }
            if let Some((name, parameters)) = block_start(trimmed) {
                let end = ends.block_end(&name, i);
                if end.is_none() {
                    self.warn(
                        lines[i].no,
                        format!("#+BEGIN_{name} has no #+END_{name}; the block runs to the end"),
                    );
                }
                let stop = end.unwrap_or(lines.len());
                let body = &lines[i + 1..stop];
                if let Some(block) =
                    self.block(&name, parameters, body, lines[i].no, depth, element_caption)
                {
                    blocks.extend(block);
                }
                i = end.map_or(lines.len(), |end| end + 1);
                continue;
            }
            if let Some((name, value)) = keyword(trimmed) {
                let value = value.trim().to_string();
                match name.to_ascii_uppercase().as_str() {
                    // Used for the whole file (title, byline, TODO states).
                    "TITLE" | "SUBTITLE" | "AUTHOR" | "DATE" | "EMAIL" | "TODO" | "SEQ_TODO"
                    | "TYP_TODO" => {}
                    "CAPTION" => caption = Some(inline(&value, 0)),
                    // Properties of the element that follows; not shown.
                    "NAME" | "RESULTS" | "LABEL" | "HEADER" | "PLOT" | "TBLFM" => {}
                    upper if upper.starts_with("ATTR_") => {}
                    "INCLUDE" | "SETUPFILE" => {
                        self.warn(
                            lines[i].no,
                            format!("#+{name}: {value} is not followed; it is not shown"),
                        );
                    }
                    "BEGIN" => {
                        // A dynamic block: its content is ordinary Org, one
                        // level deeper (as text past the nesting limit).
                        // Without an #+END: it is a keyword line like any
                        // other, and the lines after it are read as usual.
                        if let Some(end) = ends.dynamic_end(i) {
                            let inner = self.nested(&lines[i + 1..end], depth);
                            blocks.extend(inner);
                            i = end + 1;
                            continue;
                        }
                        self.warn(
                            lines[i].no,
                            format!("#+BEGIN: {value} has no #+END:; it is not shown"),
                        );
                    }
                    "END" => {}
                    _ => keywords.push((name.to_string(), value)),
                }
                i += 1;
                continue;
            }
            if trimmed == "#" || trimmed.starts_with("# ") || trimmed.starts_with("#\t") {
                i += 1;
                continue;
            }
            if is_rule(trimmed) {
                blocks.push(Block::Rule);
                i += 1;
                continue;
            }
            if trimmed.starts_with('|') {
                let start = i;
                while i < lines.len() && lines[i].text.trim_start().starts_with('|') {
                    i += 1;
                }
                let mut table = table(&lines[start..i]);
                table.title = element_caption;
                blocks.push(Block::Table(table));
                continue;
            }
            if trimmed.starts_with("+-") && trimmed.ends_with('+') {
                // A table.el table: kept as drawn.
                let start = i;
                while i < lines.len() && {
                    let t = lines[i].text.trim_start();
                    t.starts_with('+') || t.starts_with('|')
                } {
                    i += 1;
                }
                let code = lines[start..i]
                    .iter()
                    .map(|line| line.text.trim())
                    .collect::<Vec<_>>()
                    .join("\n");
                blocks.push(Block::Code(CodeBlock {
                    code,
                    ..CodeBlock::default()
                }));
                continue;
            }
            if is_fixed_width(trimmed) {
                let start = i;
                while i < lines.len() && is_fixed_width(lines[i].text.trim_start()) {
                    i += 1;
                }
                let code = lines[start..i]
                    .iter()
                    .map(|line| {
                        let t = line.text.trim_start();
                        t.strip_prefix(": ").unwrap_or(&t[1..]).to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                blocks.push(Block::Code(CodeBlock {
                    code,
                    ..CodeBlock::default()
                }));
                continue;
            }
            if bullet(line, top).is_some() {
                let (block, next) = self.list(lines, i, top, depth);
                blocks.push(block);
                i = next;
                continue;
            }
            if indent_of(line) == 0 && footnote_label(line).is_some() {
                let (block, next) = self.footnote(lines, i, top, &ends);
                blocks.push(block);
                i = next;
                continue;
            }
            if let Some(after) = trimmed.strip_prefix("\\begin{") {
                let start = i;
                let environment = after.split('}').next().unwrap_or("").to_string();
                let close = format!("\\end{{{environment}}}");
                let end = (i..lines.len()).find(|&j| lines[j].text.contains(&close));
                if let Some(end) = end.filter(|end| end - start < 10_000) {
                    let code = lines[start..=end]
                        .iter()
                        .map(|line| line.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    blocks.push(Block::Code(CodeBlock {
                        language: Some("latex".into()),
                        code,
                        ..CodeBlock::default()
                    }));
                    i = end + 1;
                    continue;
                }
            }
            // A paragraph: lines up to a blank one or the next element.
            let start = i;
            i += 1;
            while i < lines.len() && !self.interrupts(&lines[i].text, top, &ends, i) {
                i += 1;
            }
            let text = join_lines(lines[start..i].iter().map(|line| line.text.as_str()));
            let inlines = inline(&text, 0);
            match image_only(&inlines) {
                Some((target, alt)) => {
                    if let Some(caption) = element_caption {
                        blocks.push(Block::Caption(caption));
                    }
                    blocks.push(Block::Image { target, alt });
                }
                None => blocks.push(Block::Paragraph(inlines)),
            }
        }
        if !keywords.is_empty() {
            blocks.push(Block::Properties {
                name: None,
                entries: keywords,
            });
        }
        blocks
    }

    /// Whether `line` ends a paragraph: a blank line or the start of another
    /// element.
    fn interrupts(&self, line: &str, top: bool, ends: &BlockEnds, index: usize) -> bool {
        let trimmed = line.trim_start();
        trimmed.is_empty()
            || (top && is_headline(line))
            || trimmed.starts_with("#+")
            || trimmed == "#"
            || trimmed.starts_with("# ")
            || trimmed.starts_with('|')
            || is_fixed_width(trimmed)
            || is_rule(trimmed)
            || bullet(line, top).is_some()
            || (indent_of(line) == 0 && footnote_label(line).is_some())
            || (drawer_name(trimmed).is_some() && ends.drawer_end(index).is_some())
    }

    fn headline(&self, line: &str) -> Heading {
        let stars = line.bytes().take_while(|&b| b == b'*').count();
        let mut title = line[stars..].trim();
        let mut keyword = None;
        let (word, rest) = split_word(title);
        if self.todo.iter().any(|todo| todo == word) {
            keyword = Some(Keyword {
                text: word.to_string(),
                done: false,
            });
            title = rest;
        } else if self.done.iter().any(|done| done == word) {
            keyword = Some(Keyword {
                text: word.to_string(),
                done: true,
            });
            title = rest;
        }
        let mut priority = None;
        if let Some(after) = title.strip_prefix("[#") {
            if let Some(end) = after.find(']') {
                let cookie = &after[..end];
                if (1..=3).contains(&cookie.len())
                    && cookie.chars().all(|c| c.is_ascii_alphanumeric())
                {
                    priority = Some(cookie.to_string());
                    title = after[end + 1..].trim_start();
                }
            }
        }
        let (title, tags) = split_tags(title);
        let level = (stars.min(u8::MAX as usize) as u8).saturating_add(self.level_offset);
        Heading {
            level,
            content: inline(title, 0),
            keyword,
            priority,
            tags,
        }
    }

    fn drawer(&mut self, name: &str, body: &[Line]) -> Block {
        if name.eq_ignore_ascii_case("PROPERTIES") {
            let entries = body
                .iter()
                .filter_map(|line| {
                    let trimmed = line.text.trim();
                    let rest = trimmed.strip_prefix(':')?;
                    let (key, value) = rest.split_once(':')?;
                    (!key.is_empty() && !key.contains(char::is_whitespace))
                        .then(|| (key.to_string(), value.trim().to_string()))
                })
                .collect();
            return Block::Properties {
                name: None,
                entries,
            };
        }
        // Other drawers (LOGBOOK, …): their first lines, then how many more.
        let lines: Vec<&str> = body
            .iter()
            .map(|line| line.text.trim())
            .filter(|line| !line.is_empty())
            .collect();
        let mut entries: Vec<(String, String)> = lines
            .iter()
            .take(3)
            .map(|line| (String::new(), line.trim_start_matches("- ").to_string()))
            .collect();
        if lines.len() > 3 {
            entries.push((String::new(), format!("\u{2026} {} more", lines.len() - 3)));
        }
        Block::Properties {
            name: Some(name.to_string()),
            entries,
        }
    }

    /// A `#+BEGIN_NAME` block. `None` for blocks that show nothing.
    fn block(
        &mut self,
        name: &str,
        parameters: &str,
        body: &[Line],
        line: usize,
        depth: usize,
        caption: Option<Vec<Inline>>,
    ) -> Option<Vec<Block>> {
        let upper = name.to_ascii_uppercase();
        let mut out = Vec::new();
        if let Some(caption) = caption {
            out.push(Block::Caption(caption));
        }
        let verbatim = || {
            let common = body
                .iter()
                .filter(|line| !line.text.trim().is_empty())
                .map(|line| indent_of(&line.text))
                .min()
                .unwrap_or(0);
            body.iter()
                .map(|line| unescape_comma(&dedent(&line.text, common)))
                .collect::<Vec<_>>()
                .join("\n")
        };
        match upper.as_str() {
            "SRC" => {
                let language = parameters
                    .split_whitespace()
                    .next()
                    .filter(|word| !word.starts_with('-') && !word.starts_with(':'))
                    .map(str::to_string);
                out.push(Block::Code(CodeBlock {
                    language,
                    code: verbatim(),
                    ..CodeBlock::default()
                }));
            }
            "EXAMPLE" => out.push(Block::Code(CodeBlock {
                code: verbatim(),
                ..CodeBlock::default()
            })),
            "QUOTE" => {
                let blocks = self.nested(body, depth);
                out.push(Block::Quote {
                    blocks,
                    attribution: None,
                });
            }
            "VERSE" => {
                let lines = body
                    .iter()
                    .map(|line| inline(line.text.trim_end(), 0))
                    .collect();
                out.push(Block::Lines(lines));
            }
            "CENTER" => out.extend(self.nested(body, depth)),
            "COMMENT" => return None,
            "EXPORT" => {
                let backend = parameters.split_whitespace().next().unwrap_or("");
                self.info(
                    line,
                    format!("an export block for {backend:?} is not shown"),
                );
                return None;
            }
            _ => {
                let blocks = self.nested(body, depth);
                let title = (!parameters.trim().is_empty()).then(|| inline(parameters.trim(), 0));
                match AdmonitionKind::from_label(name) {
                    Some(kind) => out.push(Block::Admonition {
                        kind,
                        title,
                        blocks,
                    }),
                    None => out.push(Block::Aside {
                        title: Some(title.unwrap_or_else(|| vec![Inline::text(name)])),
                        blocks,
                    }),
                }
            }
        }
        Some(out)
    }

    /// The Org inside a block, dedented, one level deeper.
    fn nested(&mut self, body: &[Line], depth: usize) -> Vec<Block> {
        if depth + BLOCK_WEIGHT > MAX_NESTING {
            let text = join_lines(body.iter().map(|line| line.text.as_str()));
            return vec![Block::Paragraph(vec![Inline::Text(
                text.replace('\n', " "),
            )])];
        }
        let common = body
            .iter()
            .filter(|line| !line.text.trim().is_empty())
            .map(|line| indent_of(&line.text))
            .min()
            .unwrap_or(0);
        let lines: Vec<Line> = body
            .iter()
            .map(|line| Line {
                no: line.no,
                text: dedent(&line.text, common),
            })
            .collect();
        self.elements(&lines, false, depth + BLOCK_WEIGHT)
    }

    /// A list starting at `lines[start]`: its items at the same indentation,
    /// each holding the lines indented under it.
    fn list(&mut self, lines: &[Line], start: usize, top: bool, depth: usize) -> (Block, usize) {
        let first = bullet(&lines[start].text, top).expect("a bullet starts the list");
        let base = first.indent;
        let ordered = first.number.is_some();
        let mut items: Vec<(Bullet, Vec<Line>)> = Vec::new();
        let mut i = start;
        loop {
            let item = bullet(&lines[i].text, top).expect("checked before the loop");
            let mut content = vec![Line {
                no: lines[i].no,
                text: item.rest.clone(),
            }];
            i += 1;
            let mut blanks = 0;
            while i < lines.len() {
                let text = &lines[i].text;
                if text.trim().is_empty() {
                    blanks += 1;
                    if blanks >= 2 {
                        break;
                    }
                    content.push(Line {
                        no: lines[i].no,
                        text: String::new(),
                    });
                    i += 1;
                    continue;
                }
                let indent = indent_of(text);
                if indent <= base || (top && is_headline(text)) {
                    break;
                }
                blanks = 0;
                content.push(Line {
                    no: lines[i].no,
                    text: dedent(text, item.content_column.min(indent)),
                });
                i += 1;
            }
            while content
                .last()
                .is_some_and(|line| line.text.trim().is_empty())
            {
                content.pop();
            }
            items.push((item, content));
            // A sibling: the same indentation and the same kind of bullet
            // (a numbered item after bullets starts a new list).
            let sibling = i < lines.len()
                && blanks < 2
                && bullet(&lines[i].text, top)
                    .is_some_and(|next| next.indent == base && next.number.is_some() == ordered);
            if !sibling {
                break;
            }
        }
        let description = !ordered && items[0].0.term.is_some();
        if description {
            let definitions = items
                .into_iter()
                .map(|(item, content)| Definition {
                    term: item
                        .term
                        .as_deref()
                        .map(|term| inline(term, 0))
                        .unwrap_or_default(),
                    blocks: self.item_blocks(&content, depth),
                })
                .collect();
            return (Block::Definitions(definitions), i);
        }
        let start_number = items[0].0.counter.or(items[0].0.number).unwrap_or(1);
        let list_items = items
            .into_iter()
            .map(|(item, content)| {
                let mut blocks = self.item_blocks(&content, depth);
                if let Some(term) = &item.term {
                    // A term on a numbered list: kept, in bold, before the text.
                    let mut inlines = vec![Inline::Strong(inline(term, 0)), Inline::text(" ")];
                    match blocks.first_mut() {
                        Some(Block::Paragraph(first)) => {
                            inlines.append(first);
                            *first = inlines;
                        }
                        _ => blocks.insert(0, Block::Paragraph(inlines)),
                    }
                }
                ListItem {
                    checkbox: item.checkbox,
                    number: item.counter,
                    blocks,
                }
            })
            .collect();
        let kind = if ordered {
            ListKind::Ordered {
                start: start_number,
            }
        } else {
            ListKind::Bullet
        };
        (
            Block::List(List {
                kind,
                items: list_items,
            }),
            i,
        )
    }

    fn item_blocks(&mut self, content: &[Line], depth: usize) -> Vec<Block> {
        if depth >= MAX_NESTING {
            let text = join_lines(content.iter().map(|line| line.text.as_str()));
            return vec![Block::Paragraph(vec![Inline::Text(
                text.replace('\n', " "),
            )])];
        }
        self.elements(content, false, depth + 1)
    }

    /// `[fn:label] text`: a paragraph led by its label, up to a blank line
    /// or the next definition.
    fn footnote(
        &mut self,
        lines: &[Line],
        start: usize,
        top: bool,
        ends: &BlockEnds,
    ) -> (Block, usize) {
        let (label, rest) = footnote_label(&lines[start].text).expect("checked by the caller");
        let mut text = rest.to_string();
        let mut i = start + 1;
        while i < lines.len()
            && !self.interrupts(&lines[i].text, top, ends, i)
            && footnote_label(&lines[i].text).is_none()
        {
            text.push('\n');
            text.push_str(lines[i].text.trim());
            i += 1;
        }
        let mut inlines = vec![
            Inline::styled(
                "document.footnote",
                vec![Inline::text(format!("[{label}]"))],
            ),
            Inline::text(" "),
        ];
        inlines.extend(inline(text.trim(), 0));
        (Block::Paragraph(inlines), i)
    }
}

/// Where each drawer and block that starts in a run of lines ends, found
/// once, so no line is searched for its end more than a few times.
struct BlockEnds {
    /// Line indexes of `:END:` lines.
    drawer: Vec<usize>,
    /// Line indexes of `#+END_NAME` lines, by lowercase name.
    blocks: HashMap<String, Vec<usize>>,
    /// Line indexes of `#+END:` lines (dynamic blocks).
    dynamic: Vec<usize>,
    /// Line indexes of headlines, which no drawer or block runs past.
    headlines: Vec<usize>,
}

impl BlockEnds {
    fn new(lines: &[Line]) -> Self {
        let mut ends = BlockEnds {
            drawer: Vec::new(),
            blocks: HashMap::new(),
            dynamic: Vec::new(),
            headlines: Vec::new(),
        };
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.text.trim();
            if trimmed.eq_ignore_ascii_case(":END:") {
                ends.drawer.push(index);
            } else if let Some(name) = block_end(trimmed) {
                ends.blocks
                    .entry(name.to_ascii_lowercase())
                    .or_default()
                    .push(index);
            } else if trimmed
                .get(..6)
                .is_some_and(|start| start.eq_ignore_ascii_case("#+END:"))
            {
                ends.dynamic.push(index);
            } else if is_headline(&line.text) {
                ends.headlines.push(index);
            }
        }
        ends
    }

    fn next(positions: &[usize], after: usize) -> Option<usize> {
        let index = positions.partition_point(|&at| at <= after);
        positions.get(index).copied()
    }

    /// The `:END:` closing a drawer opened at line `start`, before the next
    /// headline.
    fn drawer_end(&self, start: usize) -> Option<usize> {
        let end = Self::next(&self.drawer, start)?;
        let headline = Self::next(&self.headlines, start);
        headline
            .is_none_or(|headline| end < headline)
            .then_some(end)
    }

    fn block_end(&self, name: &str, start: usize) -> Option<usize> {
        Self::next(self.blocks.get(&name.to_ascii_lowercase())?, start)
    }

    fn dynamic_end(&self, start: usize) -> Option<usize> {
        Self::next(&self.dynamic, start)
    }
}

/// `#+NAME: value`, for a name without spaces.
fn keyword(trimmed: &str) -> Option<(&str, &str)> {
    let rest = trimmed.strip_prefix("#+")?;
    let colon = rest.find(':')?;
    let name = &rest[..colon];
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-'))
    .then(|| (name, &rest[colon + 1..]))
}

fn is_block_start(trimmed: &str) -> bool {
    block_start(trimmed).is_some()
}

/// `#+BEGIN_NAME parameters`.
fn block_start(trimmed: &str) -> Option<(String, &str)> {
    if trimmed.len() < 8
        || !trimmed.is_char_boundary(8)
        || !trimmed[..8].eq_ignore_ascii_case("#+BEGIN_")
    {
        return None;
    }
    let rest = &trimmed[8..];
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..name_end];
    (!name.is_empty()).then(|| (name.to_string(), rest[name_end..].trim()))
}

/// The name of a `#+END_NAME` line.
fn block_end(trimmed: &str) -> Option<&str> {
    if trimmed.len() < 6
        || !trimmed.is_char_boundary(6)
        || !trimmed[..6].eq_ignore_ascii_case("#+END_")
    {
        return None;
    }
    let name = trimmed[6..].trim();
    (!name.is_empty() && !name.contains(char::is_whitespace)).then_some(name)
}

/// A headline: stars in the first column, then a space or the line's end.
fn is_headline(line: &str) -> bool {
    let stars = line.bytes().take_while(|&b| b == b'*').count();
    stars > 0 && {
        let rest = &line[stars..];
        rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')
    }
}

/// `:NAME:` alone on its line (not `:END:`).
fn drawer_name(trimmed: &str) -> Option<&str> {
    let name = trimmed.trim_end().strip_prefix(':')?.strip_suffix(':')?;
    (!name.is_empty()
        && !name.eq_ignore_ascii_case("END")
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-'))
    .then_some(name)
}

/// `SCHEDULED: <…> DEADLINE: <…> CLOSED: […]` after a headline.
fn planning(trimmed: &str) -> Option<Vec<(String, String)>> {
    const KEYS: [&str; 3] = ["SCHEDULED:", "DEADLINE:", "CLOSED:"];
    if !KEYS.iter().any(|key| trimmed.starts_with(key)) {
        return None;
    }
    let mut found: Vec<(usize, &str)> = KEYS
        .iter()
        .flat_map(|key| trimmed.match_indices(key))
        .collect();
    found.sort();
    let mut entries = Vec::new();
    for (index, (at, key)) in found.iter().enumerate() {
        let end = found
            .get(index + 1)
            .map_or(trimmed.len(), |(next, _)| *next);
        let value = trimmed[at + key.len()..end].trim();
        entries.push((key.trim_end_matches(':').to_string(), value.to_string()));
    }
    Some(entries)
}

/// Five or more dashes alone on a line.
fn is_rule(trimmed: &str) -> bool {
    let trimmed = trimmed.trim_end();
    trimmed.len() >= 5 && trimmed.bytes().all(|b| b == b'-')
}

/// A fixed-width line: `: text`, or a colon alone.
fn is_fixed_width(trimmed: &str) -> bool {
    trimmed == ":" || trimmed.starts_with(": ")
}

/// `[fn:label] definition` at the start of a line.
fn footnote_label(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("[fn:")?;
    let end = rest.find(']')?;
    let label = &rest[..end];
    (!label.is_empty()
        && label
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-'))
    .then(|| (label, &rest[end + 1..]))
}

/// Org's comma escape in source blocks: `,*` and `,#+` at a line's start.
fn unescape_comma(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    match trimmed.strip_prefix(',') {
        Some(rest) if rest.starts_with('*') || rest.starts_with("#+") || rest.starts_with(",*") => {
            format!("{indent}{rest}")
        }
        _ => line.to_string(),
    }
}

/// The first word of `text` and the rest after it.
fn split_word(text: &str) -> (&str, &str) {
    match text.find(char::is_whitespace) {
        Some(end) => (&text[..end], text[end..].trim_start()),
        None => (text, ""),
    }
}

/// A headline's title and its trailing `:tag:tag:`.
fn split_tags(title: &str) -> (&str, Vec<String>) {
    let trimmed = title.trim_end();
    if !trimmed.ends_with(':') {
        return (trimmed, Vec::new());
    }
    let start = trimmed.rfind(char::is_whitespace).map_or(0, |at| at + 1);
    let candidate = &trimmed[start..];
    let valid = candidate.len() >= 3
        && candidate.starts_with(':')
        && candidate
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%' | ':'));
    if !valid {
        return (trimmed, Vec::new());
    }
    let tags = candidate
        .split(':')
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect();
    (trimmed[..start].trim_end(), tags)
}

/// A list bullet: its indentation, number, content and what precedes the
/// content (`[@N]`, a checkbox, a `TERM ::` tag).
#[derive(Clone, Debug)]
struct Bullet {
    indent: usize,
    number: Option<u64>,
    /// The column the item's text starts at.
    content_column: usize,
    counter: Option<u64>,
    checkbox: Option<Checkbox>,
    term: Option<String>,
    /// The item's first line, after the bullet and its prefixes.
    rest: String,
}

/// The bullet `line` starts with, if it is a list item. At the document
/// level a star in the first column is a headline, not a bullet.
fn bullet(line: &str, top: bool) -> Option<Bullet> {
    let indent = indent_of(line);
    let trimmed = line.trim_start();
    let (marker, number) = match trimmed.as_bytes().first()? {
        b'-' | b'+' => (1, None),
        b'*' if !(top && indent == 0) => (1, None),
        b'0'..=b'9' => {
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 9 || !matches!(trimmed.as_bytes().get(digits), Some(b'.' | b')')) {
                return None;
            }
            (digits + 1, trimmed[..digits].parse().ok())
        }
        _ => return None,
    };
    let after = &trimmed[marker..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let mut rest = after.trim_start();
    let spaces = after.len() - rest.len();
    let mut counter = None;
    if let Some(after_at) = rest.strip_prefix("[@") {
        if let Some(end) = after_at.find(']') {
            if let Ok(value) = after_at[..end].parse::<u64>() {
                counter = Some(value);
                rest = after_at[end + 1..].trim_start();
            }
        }
    }
    let mut checkbox = None;
    for (prefix, state) in [
        ("[ ]", Checkbox::Unchecked),
        ("[X]", Checkbox::Checked),
        ("[x]", Checkbox::Checked),
        ("[-]", Checkbox::Partial),
    ] {
        if let Some(after_box) = rest.strip_prefix(prefix) {
            if after_box.is_empty() || after_box.starts_with(char::is_whitespace) {
                checkbox = Some(state);
                rest = after_box.trim_start();
                break;
            }
        }
    }
    let mut term = None;
    if number.is_none() {
        if let Some(at) = rest.find(" ::") {
            let after_tag = &rest[at + 3..];
            if after_tag.is_empty() || after_tag.starts_with(char::is_whitespace) {
                term = Some(rest[..at].trim().to_string());
                rest = after_tag.trim_start();
            }
        }
    }
    Some(Bullet {
        indent,
        number,
        content_column: indent + marker + spaces.max(1),
        counter,
        checkbox,
        term,
        rest: rest.to_string(),
    })
}

/// An Org table's rows: rules divide it into groups; with more than one
/// group, the first group's first row is the header.
fn table(lines: &[Line]) -> TableBlock {
    let mut groups: Vec<Vec<Vec<Vec<Inline>>>> = vec![Vec::new()];
    let mut align = Vec::new();
    for line in lines {
        let trimmed = line.text.trim();
        if trimmed.starts_with("|-") {
            if !groups.last().is_some_and(Vec::is_empty) {
                groups.push(Vec::new());
            }
            continue;
        }
        let cells = split_cells(trimmed);
        if !cells.is_empty()
            && cells
                .iter()
                .all(|cell| cell.is_empty() || alignment_cookie(cell).is_some())
            && cells.iter().any(|cell| !cell.is_empty())
        {
            align = cells
                .iter()
                .map(|cell| alignment_cookie(cell).unwrap_or_default())
                .collect();
            continue;
        }
        let row = cells.iter().map(|cell| inline(cell, 0)).collect();
        groups
            .last_mut()
            .expect("there is always a group")
            .push(row);
    }
    groups.retain(|group| !group.is_empty());
    let mut table = TableBlock {
        align,
        ..TableBlock::default()
    };
    let has_header = groups.len() > 1;
    for (index, group) in groups.into_iter().enumerate() {
        let mut rows = group.into_iter();
        if index == 0 && has_header {
            table.header = rows.next();
        } else if !table.rows.is_empty() {
            table.sections.push(table.rows.len());
        }
        table.rows.extend(rows);
    }
    table
}

/// The cells of `| a | b |`, trimmed.
fn split_cells(row: &str) -> Vec<&str> {
    let inner = row.strip_prefix('|').unwrap_or(row);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(str::trim).collect()
}

/// `<l>`, `<c10>`, `<r>`, `<20>`: a column's alignment and width cookie.
fn alignment_cookie(cell: &str) -> Option<Align> {
    let inner = cell.strip_prefix('<')?.strip_suffix('>')?;
    let (letter, digits) = match inner.chars().next() {
        Some(c @ ('l' | 'c' | 'r')) => (Some(c), &inner[1..]),
        _ => (None, inner),
    };
    if !digits.chars().all(|c| c.is_ascii_digit()) || (letter.is_none() && digits.is_empty()) {
        return None;
    }
    Some(match letter {
        Some('c') => Align::Center,
        Some('r') => Align::Right,
        _ => Align::Left,
    })
}

/// A paragraph that is nothing but one image link.
fn image_only(inlines: &[Inline]) -> Option<(String, String)> {
    match inlines {
        [Inline::Image { target, alt }] => Some((target.clone(), alt.clone())),
        _ => None,
    }
}

/// File extensions an Org link shows as an image.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "tif", "tiff", "ico",
];

fn is_image(target: &str) -> bool {
    let path = target.strip_prefix("file:").unwrap_or(target);
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        IMAGE_EXTENSIONS
            .iter()
            .any(|image| ext.eq_ignore_ascii_case(image))
    })
}

/// Parse Org inline markup.
pub(crate) fn inline(text: &str, depth: usize) -> Vec<Inline> {
    trim_inlines(Inliner::new(text).run(0, text.len(), depth))
}

/// The emphasis markers and what each makes.
const MARKERS: [u8; 6] = *b"*/_=~+";

struct Inliner<'a> {
    text: &'a str,
    /// For each marker in [`MARKERS`], where it can close a span.
    closers: [Positions; 6],
    link_ends: Positions,
}

impl<'a> Inliner<'a> {
    fn new(text: &'a str) -> Self {
        let mut closers: [Vec<usize>; 6] = Default::default();
        for (at, byte) in text.bytes().enumerate() {
            if let Some(index) = MARKERS.iter().position(|&marker| marker == byte) {
                if can_close(text, at) {
                    closers[index].push(at);
                }
            }
        }
        Inliner {
            text,
            closers: closers.map(Positions::from_sorted),
            link_ends: Positions::of(text, "]]"),
        }
    }

    fn run(&self, start: usize, end: usize, depth: usize) -> Vec<Inline> {
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
                b'[' => {
                    if let Some((inline, next)) = self.bracket(i, end, depth) {
                        out.inline(inline);
                        i = next;
                        continue;
                    }
                }
                b'<' => {
                    if let Some((inline, next)) = self.angle(i, end) {
                        out.inline(inline);
                        i = next;
                        continue;
                    }
                }
                b'\\' => {
                    if let Some((piece, next)) = self.backslash(i, end) {
                        if let Some(inline) = piece {
                            out.inline(inline);
                        }
                        i = next;
                        continue;
                    }
                }
                b'@' if text[i..end].starts_with("@@") => {
                    // An export snippet, `@@backend:value@@`: not shown.
                    if let Some(close) = text[i + 2..end].find("@@") {
                        let inner = &text[i + 2..i + 2 + close];
                        if inner.contains(':') && !inner.contains('\n') {
                            i += close + 4;
                            continue;
                        }
                    }
                }
                _ => {}
            }
            if let Some(index) = MARKERS.iter().position(|&marker| marker == byte) {
                if let Some((inline, next)) = self.emphasis(index, i, end, depth) {
                    out.inline(inline);
                    i = next;
                    continue;
                }
            }
            if let Some(url_end) = url_end(text, i).filter(|&url_end| url_end <= end) {
                let url = &text[i..url_end];
                let inline = if is_image(url) {
                    Inline::Image {
                        target: url.to_string(),
                        alt: String::new(),
                    }
                } else {
                    Inline::Link {
                        target: url.to_string(),
                        content: Vec::new(),
                    }
                };
                out.inline(inline);
                i = url_end;
                continue;
            }
            let ch = char_at(text, i).expect("i is a char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
        out.finish()
    }

    /// `*bold*` and the other markers: an opener here, the first closer of
    /// the same marker after it, at most one line break between.
    fn emphasis(
        &self,
        index: usize,
        at: usize,
        end: usize,
        depth: usize,
    ) -> Option<(Inline, usize)> {
        let text = self.text;
        if !can_open(text, at) {
            return None;
        }
        let close = self.closers[index].at_or_after(at + 2)?;
        if close >= end || text[at + 1..close].matches('\n').count() > 1 {
            return None;
        }
        let inner = &text[at + 1..close];
        let next = close + 1;
        let literal = || inner.replace('\n', " ");
        let inline = match MARKERS[index] {
            b'=' | b'~' => Inline::Code(literal()),
            marker => {
                let content = if depth >= MAX_NESTING {
                    vec![Inline::Text(literal())]
                } else {
                    self.run(at + 1, close, depth + 1)
                };
                match marker {
                    b'*' => Inline::Strong(content),
                    b'/' => Inline::Emphasis(content),
                    b'_' => Inline::Underline(content),
                    _ => Inline::Strike(content),
                }
            }
        };
        Some((inline, next))
    }

    /// `[[link][description]]`, `[[link]]`, `[fn:label]`, timestamps and
    /// statistics cookies.
    fn bracket(&self, at: usize, end: usize, depth: usize) -> Option<(Inline, usize)> {
        let text = self.text;
        let rest = &text[at..end];
        if rest.starts_with("[[") {
            let close = self.link_ends.at_or_after(at + 2)?;
            if close + 2 > end {
                return None;
            }
            let inner = &text[at + 2..close];
            let (target, description) = match inner.find("][") {
                Some(split) => (&inner[..split], Some(&inner[split + 2..])),
                None => (inner, None),
            };
            if target.is_empty() || target.contains("[[") {
                return None;
            }
            let target = target.replace('\n', " ");
            let inline = match description {
                None if is_image(&target) => Inline::Image {
                    target,
                    alt: String::new(),
                },
                None => link(&target, Vec::new()),
                Some(description) => {
                    let content = match description.trim() {
                        image if is_image(image) && !image.contains(' ') => vec![Inline::Image {
                            target: image.to_string(),
                            alt: String::new(),
                        }],
                        _ if depth >= MAX_NESTING => vec![Inline::text(description)],
                        _ => {
                            let start = at + 2 + inner.len() - description.len();
                            trim_inlines(self.run(start, close, depth + 1))
                        }
                    };
                    link(&target, content)
                }
            };
            return Some((inline, close + 2));
        }
        if let Some(after) = rest.strip_prefix("[fn:") {
            let close = after.find(']')?;
            let label = &after[..close];
            if label.contains('\n') || label.contains('[') {
                return None;
            }
            let shown = match label.split_once(':') {
                // `[fn::inline definition]` or `[fn:name:definition]`.
                Some(("", definition)) => format!("[{}]", definition.trim()),
                Some((name, _)) => format!("[{name}]"),
                None => format!("[{label}]"),
            };
            return Some((
                Inline::styled("document.footnote", vec![Inline::Text(shown)]),
                at + 4 + close + 1,
            ));
        }
        if let Some(stamp) = timestamp(rest, '[', ']', true) {
            return Some((
                Inline::styled("document.timestamp", vec![Inline::text(stamp)]),
                at + stamp.len(),
            ));
        }
        // Statistics cookies: `[2/5]`, `[40%]`, `[/]`, `[%]`.
        let close = rest.get(..rest.len().min(16))?.find(']')?;
        let inner = &rest[1..close];
        let cookie = !inner.is_empty()
            && (inner.ends_with('%') || inner.contains('/'))
            && inner
                .chars()
                .all(|c| c.is_ascii_digit() || c == '/' || c == '%');
        cookie.then(|| {
            (
                Inline::styled("document.properties", vec![Inline::text(&rest[..=close])]),
                at + close + 1,
            )
        })
    }

    /// `<2024-05-01 Wed>`, `<<target>>`, `<https://…>`.
    fn angle(&self, at: usize, end: usize) -> Option<(Inline, usize)> {
        let text = self.text;
        let rest = &text[at..end];
        if let Some(stamp) = timestamp(rest, '<', '>', true) {
            return Some((
                Inline::styled("document.timestamp", vec![Inline::text(stamp)]),
                at + stamp.len(),
            ));
        }
        if let Some(after) = rest.strip_prefix("<<") {
            // `<<target>>` and `<<<radio target>>>` show their text.
            let after = after.strip_prefix('<').unwrap_or(after);
            let close = after.get(..after.len().min(256))?.find(">>")?;
            let target = &after[..close];
            if target.is_empty() || target.contains('\n') || target.contains('<') {
                return None;
            }
            let consumed = rest.len() - after.len() + close + 2;
            let consumed = consumed + usize::from(rest[consumed..].starts_with('>'));
            return Some((Inline::text(target), at + consumed));
        }
        let inner_end = rest.get(..rest.len().min(4096))?.find('>')?;
        let inner = &rest[1..inner_end];
        let url = url_end(inner, 0).filter(|&url| url == inner.len())?;
        Some((link(&inner[..url], Vec::new()), at + inner_end + 1))
    }

    /// `\\` at a line's end is a line break; `\alpha` an entity. `Some(None)`
    /// consumes text and shows nothing.
    fn backslash(&self, at: usize, end: usize) -> Option<(Option<Inline>, usize)> {
        let text = self.text;
        let rest = &text[at..end];
        if let Some(after) = rest.strip_prefix("\\\\") {
            let line_end = after.find('\n').unwrap_or(after.len());
            if after[..line_end].trim().is_empty() {
                let newline = usize::from(line_end < after.len());
                return Some((Some(Inline::LineBreak), at + 2 + line_end + newline));
            }
            return None;
        }
        let name_len = rest[1..]
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .count();
        if name_len == 0 {
            return None;
        }
        let name = &rest[1..1 + name_len];
        let value = ENTITIES
            .iter()
            .find(|(entity, _)| *entity == name)
            .map(|(_, value)| *value)?;
        let mut next = at + 1 + name_len;
        if text[next..end].starts_with("{}") {
            next += 2;
        }
        Some((Some(Inline::text(value)), next))
    }
}

/// A link to `target`, with Org's internal targets (`#id`, `*heading`) shown
/// as their text, without a link.
fn link(target: &str, content: Vec<Inline>) -> Inline {
    let internal = target.starts_with('#')
        || target.starts_with('*')
        || target.starts_with("id:")
        || target.starts_with("elisp:")
        || target.starts_with("shell:");
    if internal {
        if content.is_empty() {
            let shown = target.trim_start_matches(['#', '*']);
            return Inline::text(shown);
        }
        return Inline::Underline(content);
    }
    let target = target.strip_prefix("file:").unwrap_or(target);
    Inline::Link {
        target: target.to_string(),
        content,
    }
}

/// An Org timestamp at the start of `text`: `<2024-05-01 Wed 10:00>` or
/// `[2024-05-01]`, with an optional range end when `range`.
fn timestamp(text: &str, open: char, close: char, range: bool) -> Option<&str> {
    let rest = text.strip_prefix(open)?;
    let digits = rest.as_bytes();
    let date = digits.len() >= 10
        && digits[..4].iter().all(u8::is_ascii_digit)
        && digits[4] == b'-'
        && digits[5..7].iter().all(u8::is_ascii_digit)
        && digits[7] == b'-'
        && digits[8..10].iter().all(u8::is_ascii_digit);
    if !date {
        return None;
    }
    let close_at = rest.get(..rest.len().min(64))?.find(close)?;
    if rest[..close_at].contains('\n') || rest[..close_at].contains(open) {
        return None;
    }
    let mut end = 1 + close_at + 1;
    // A range: `<…>--<…>`.
    if range && text[end..].starts_with("--") {
        if let Some(second) = timestamp(&text[end + 2..], open, close, false) {
            end += 2 + second.len();
        }
    }
    Some(&text[..end])
}

/// Whether the marker at `at` can open a span: after the start, a space or
/// `-('"{`, and before something that is not a space.
fn can_open(text: &str, at: usize) -> bool {
    let before = char_before(text, at);
    let after = char_at(text, at + 1);
    before.is_none_or(|c| c.is_whitespace() || matches!(c, '-' | '(' | '\'' | '"' | '{'))
        && after.is_some_and(|c| !c.is_whitespace())
}

/// Whether the marker at `at` can close a span: after something that is not
/// a space, and before the end, a space or `-.,;:!?'")}[\`.
fn can_close(text: &str, at: usize) -> bool {
    let before = char_before(text, at);
    let after = char_at(text, at + 1);
    before.is_some_and(|c| !c.is_whitespace())
        && after.is_none_or(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '-' | '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | ')' | '}' | '[' | '\\'
                )
        })
}

/// A few Org entities (`\alpha`, `\to`, `\nbsp`), by name.
const ENTITIES: &[(&str, &str)] = &[
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ε"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("pi", "π"),
    ("sigma", "σ"),
    ("tau", "τ"),
    ("phi", "φ"),
    ("omega", "ω"),
    ("Delta", "Δ"),
    ("Sigma", "Σ"),
    ("Omega", "Ω"),
    ("to", "→"),
    ("rarr", "→"),
    ("larr", "←"),
    ("rArr", "⇒"),
    ("lArr", "⇐"),
    ("uarr", "↑"),
    ("darr", "↓"),
    ("nbsp", "\u{a0}"),
    ("mdash", "—"),
    ("ndash", "–"),
    ("hellip", "…"),
    ("dots", "…"),
    ("deg", "°"),
    ("times", "×"),
    ("div", "÷"),
    ("pm", "±"),
    ("le", "≤"),
    ("ge", "≥"),
    ("ne", "≠"),
    ("infin", "∞"),
    ("copy", "©"),
    ("reg", "®"),
    ("trade", "™"),
    ("euro", "€"),
    ("pound", "£"),
    ("checkmark", "✓"),
    ("laquo", "«"),
    ("raquo", "»"),
    ("lsquo", "‘"),
    ("rsquo", "’"),
    ("ldquo", "“"),
    ("rdquo", "”"),
    ("bull", "•"),
    ("sect", "§"),
    ("para", "¶"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::inlines_text;

    fn blocks(source: &str) -> Vec<Block> {
        parse(source).document.blocks
    }

    #[test]
    fn dynamic_blocks_are_bounded() {
        // Unclosed: each is a keyword line, not a block holding the rest.
        let parsed = parse(&"#+BEGIN: clock\n".repeat(100_000));
        assert!(parsed.document.blocks.is_empty());
        let parsed = parse("#+BEGIN: clock\ntext");
        assert_eq!(parsed.diagnostics.len(), 1);
        assert!(matches!(&parsed.document.blocks[..], [Block::Paragraph(_)]));
        // Openers before one #+END: the first holds the rest, which are
        // keyword lines, and its text.
        let deep = format!(
            "{}text\n{}",
            "#+BEGIN: a\n".repeat(5_000),
            "#+END:\n".repeat(5_000)
        );
        assert!(
            matches!(&blocks(&deep)[..], [Block::Paragraph(text)] if inlines_text(text) == "text")
        );
    }

    #[test]
    fn headlines_carry_keywords_priorities_and_tags() {
        let blocks = blocks("* TODO [#A] Write the report :work:urgent:\n** DONE Plan");
        let Block::Heading(first) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert_eq!(first.level, 1);
        assert_eq!(first.keyword.as_ref().unwrap().text, "TODO");
        assert!(!first.keyword.as_ref().unwrap().done);
        assert_eq!(first.priority.as_deref(), Some("A"));
        assert_eq!(first.tags, ["work", "urgent"]);
        assert_eq!(inlines_text(&first.content), "Write the report");
        let Block::Heading(second) = &blocks[1] else {
            panic!("{blocks:?}");
        };
        assert_eq!(second.level, 2);
        assert!(second.keyword.as_ref().unwrap().done);
    }

    #[test]
    fn declared_todo_keywords_are_recognised() {
        let blocks = blocks("#+TODO: NEXT WAIT | DONE CANCELLED\n* WAIT Reply\n* CANCELLED Trip");
        let keywords: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Heading(heading) => heading.keyword.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(keywords.len(), 2);
        assert!(!keywords[0].done && keywords[1].done);
    }

    #[test]
    fn emphasis_markers() {
        let inlines = inline("*bold* /it/ _u_ =v= ~c~ +s+ a*b*c", 0);
        assert!(matches!(&inlines[0], Inline::Strong(_)));
        assert!(matches!(&inlines[2], Inline::Emphasis(_)));
        assert!(matches!(&inlines[4], Inline::Underline(_)));
        assert_eq!(inlines[6], Inline::Code("v".into()));
        assert_eq!(inlines[8], Inline::Code("c".into()));
        assert!(matches!(&inlines[10], Inline::Strike(_)));
        // Inside a word, a marker is text.
        assert_eq!(inlines.last(), Some(&Inline::text(" a*b*c")));
    }

    #[test]
    fn links_and_images() {
        let inlines = inline("[[https://orgmode.org][Org]] and [[./a.png]]", 0);
        assert_eq!(
            inlines[0],
            Inline::Link {
                target: "https://orgmode.org".into(),
                content: vec![Inline::text("Org")]
            }
        );
        assert!(matches!(&inlines[2], Inline::Image { target, .. } if target == "./a.png"));
    }

    #[test]
    fn lists_nest_and_carry_checkboxes() {
        let blocks = blocks("- [X] one\n  - [ ] nested\n- two\n\n1. first\n2. [@5] fifth");
        let Block::List(list) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert_eq!(list.kind, ListKind::Bullet);
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].checkbox, Some(Checkbox::Checked));
        assert!(matches!(&list.items[0].blocks[1], Block::List(_)));
        let Block::List(numbered) = &blocks[1] else {
            panic!("{blocks:?}");
        };
        assert_eq!(numbered.kind, ListKind::Ordered { start: 1 });
        assert_eq!(numbered.items[1].number, Some(5));
    }

    #[test]
    fn description_lists_become_definitions() {
        let blocks = blocks("- Emacs :: an editor\n- Org :: a mode");
        let Block::Definitions(definitions) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert_eq!(inlines_text(&definitions[1].term), "Org");
    }

    #[test]
    fn blocks_and_tables() {
        let source = "#+CAPTION: Sizes\n| a | b |\n|---+---|\n| 1 | 2 |\n\n#+BEGIN_SRC python\nprint(1)\n#+END_SRC\n#+begin_quote\nWise.\n#+end_quote";
        let blocks = blocks(source);
        let Block::Table(table) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert!(table.header.is_some() && table.title.is_some());
        assert_eq!(table.rows.len(), 1);
        assert!(
            matches!(&blocks[1], Block::Code(code) if code.language.as_deref() == Some("python"))
        );
        assert!(matches!(&blocks[2], Block::Quote { .. }));
    }

    #[test]
    fn drawers_and_planning_are_properties() {
        let blocks =
            blocks("* Task\nSCHEDULED: <2024-05-01 Wed>\n:PROPERTIES:\n:ID: x1\n:END:\nBody.");
        assert!(
            matches!(&blocks[1], Block::Properties { entries, .. } if entries[0].0 == "SCHEDULED")
        );
        assert!(
            matches!(&blocks[2], Block::Properties { entries, .. } if entries[0] == ("ID".into(), "x1".into()))
        );
        assert!(matches!(&blocks[3], Block::Paragraph(_)));
    }

    #[test]
    fn the_title_leads_and_headlines_move_down() {
        let blocks = blocks("#+TITLE: Notes\n#+AUTHOR: Ada\n* One");
        assert!(matches!(&blocks[0], Block::Heading(h) if h.level == 1));
        assert!(matches!(&blocks[1], Block::Byline(_)));
        assert!(matches!(&blocks[2], Block::Heading(h) if h.level == 2));
    }

    #[test]
    fn sniffing() {
        assert!(sniff("#+TITLE: x\n* a\n") >= 0.9);
        assert!(sniff("* TODO thing\n** DONE other\n") >= 0.6);
        assert!(sniff("* item\n* item\n") < 0.6);
        assert_eq!(sniff("#!/bin/sh\n#+TITLE: x\n"), 0.0);
        assert!(sniff("int main() { return 0; }\n") < 0.1);
    }

    #[test]
    fn unterminated_blocks_warn_and_run_to_the_end() {
        let parsed = parse("#+BEGIN_SRC sh\necho hi");
        assert!(matches!(&parsed.document.blocks[0], Block::Code(code) if code.code == "echo hi"));
        assert_eq!(parsed.diagnostics.len(), 1);
    }
}
