//! The block and inline model that the Org, AsciiDoc and man parsers build,
//! and [`DocumentView`](super::DocumentView) renders.
//!
//! The model is deliberately small: what the three formats share, in the
//! shapes the Markdown renderer already draws (headings, paragraphs, lists,
//! code, tables, quotes, rules, image placeholders), plus the few things
//! they add (admonitions, definition lists, drawers, a man page's running
//! header). A parser maps anything else onto these, or onto plain text, and
//! says so in a [`Diagnostic`](super::Diagnostic).

/// A parsed document: its blocks, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Document {
    /// The document's plain text, block by block: what a reader would see,
    /// without styles or markers. Handy for searching and tests.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            block.push_plain(&mut out);
        }
        out.trim_end().to_string()
    }
}

/// One block of a document.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A heading. Level 1 is the document title's level and is centred, as
    /// Markdown's `#` heading is; deeper levels are left-aligned.
    Heading(Heading),
    /// Author, date or revision lines under a title, shown dim.
    Byline(Vec<Inline>),
    /// A paragraph of running text, wrapped to the width.
    Paragraph(Vec<Inline>),
    /// Lines whose breaks are kept (verse, a man page's no-fill text), each
    /// still wrapped when it is wider than the width.
    Lines(Vec<Vec<Inline>>),
    /// A bullet, numbered, checkbox or callout list.
    List(List),
    /// Terms and their definitions.
    Definitions(Vec<Definition>),
    /// Source code or preformatted text, drawn with `Syntax`.
    Code(CodeBlock),
    /// A table, drawn with core's `Table` as Markdown tables are.
    Table(TableBlock),
    /// A block quote, with an optional attribution.
    Quote {
        blocks: Vec<Block>,
        attribution: Option<Vec<Inline>>,
    },
    /// A note, tip, warning and the like, drawn in a titled panel.
    Admonition {
        kind: AdmonitionKind,
        title: Option<Vec<Inline>>,
        blocks: Vec<Block>,
    },
    /// A sidebar or other aside, drawn in a plain panel.
    Aside {
        title: Option<Vec<Inline>>,
        blocks: Vec<Block>,
    },
    /// Blocks indented by `amount` cells (a man page's `.RS`/`.IP`).
    Indent { amount: usize, blocks: Vec<Block> },
    /// A horizontal rule.
    Rule,
    /// An image, shown as a placeholder: the image marker and its alt text.
    Image { target: String, alt: String },
    /// Metadata shown compactly and dim: an Org drawer or property list,
    /// planning lines, unrecognised keywords. `name` is the drawer's name.
    Properties {
        name: Option<String>,
        entries: Vec<(String, String)>,
    },
    /// A running header or footer: left, centre and right parts on one row
    /// (a man page's `LS(1)  User Commands  LS(1)`).
    Banner {
        left: String,
        center: String,
        right: String,
    },
    /// A caption or block title, shown dim and italic above what it names.
    Caption(Vec<Inline>),
}

impl Block {
    fn push_plain(&self, out: &mut String) {
        match self {
            Block::Heading(heading) => {
                if let Some(keyword) = &heading.keyword {
                    out.push_str(&keyword.text);
                    out.push(' ');
                }
                push_inlines(&heading.content, out);
                out.push('\n');
            }
            Block::Byline(inlines) | Block::Paragraph(inlines) | Block::Caption(inlines) => {
                push_inlines(inlines, out);
                out.push('\n');
            }
            Block::Lines(lines) => {
                for line in lines {
                    push_inlines(line, out);
                    out.push('\n');
                }
            }
            Block::List(list) => {
                for item in &list.items {
                    for block in &item.blocks {
                        block.push_plain(out);
                    }
                }
            }
            Block::Definitions(definitions) => {
                for definition in definitions {
                    push_inlines(&definition.term, out);
                    out.push('\n');
                    for block in &definition.blocks {
                        block.push_plain(out);
                    }
                }
            }
            Block::Code(code) => {
                out.push_str(&code.code);
                out.push('\n');
            }
            Block::Table(table) => {
                for row in table.header.iter().chain(&table.rows) {
                    let cells: Vec<String> = row.iter().map(|cell| inlines_text(cell)).collect();
                    out.push_str(&cells.join(" | "));
                    out.push('\n');
                }
            }
            Block::Quote {
                blocks,
                attribution,
            } => {
                for block in blocks {
                    block.push_plain(out);
                }
                if let Some(attribution) = attribution {
                    push_inlines(attribution, out);
                    out.push('\n');
                }
            }
            Block::Admonition { title, blocks, .. } | Block::Aside { title, blocks } => {
                if let Some(title) = title {
                    push_inlines(title, out);
                    out.push('\n');
                }
                for block in blocks {
                    block.push_plain(out);
                }
            }
            Block::Indent { blocks, .. } => {
                for block in blocks {
                    block.push_plain(out);
                }
            }
            Block::Rule => out.push_str("---\n"),
            Block::Image { alt, .. } => {
                out.push_str(alt);
                out.push('\n');
            }
            Block::Properties { name, entries } => {
                if let Some(name) = name {
                    out.push_str(name);
                    out.push('\n');
                }
                for (key, value) in entries {
                    out.push_str(key);
                    out.push_str(": ");
                    out.push_str(value);
                    out.push('\n');
                }
            }
            Block::Banner {
                left,
                center,
                right,
            } => {
                out.push_str(&format!("{left} {center} {right}\n"));
            }
        }
    }
}

/// A heading and what an Org headline carries besides its title.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heading {
    /// 1 to 6 as in Markdown; deeper levels draw as level 6.
    pub level: u8,
    pub content: Vec<Inline>,
    /// A TODO keyword (`TODO`, `DONE`, or one an Org file declares).
    pub keyword: Option<Keyword>,
    /// An Org priority cookie's letter or number (`A` for `[#A]`).
    pub priority: Option<String>,
    /// Org tags (`:work:urgent:`), without the colons.
    pub tags: Vec<String>,
}

/// A TODO keyword and whether it is a done state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyword {
    pub text: String,
    pub done: bool,
}

/// A list and its items.
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub kind: ListKind,
    pub items: Vec<ListItem>,
}

/// What marks a list's items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    /// ` • ` bullets.
    Bullet,
    /// Numbers counting up from `start`.
    Ordered { start: u64 },
    /// AsciiDoc callout numbers, `<1>`, matching the markers in the code
    /// block above.
    Callout,
}

/// One list item: an optional checkbox, an optional explicit number, and
/// the blocks the item holds (its text, then any nested lists or code).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListItem {
    pub checkbox: Option<Checkbox>,
    /// An explicit number (Org's `[@5]` counter, AsciiDoc's `3.`); the
    /// following items count on from it.
    pub number: Option<u64>,
    pub blocks: Vec<Block>,
}

/// A checklist item's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checkbox {
    Unchecked,
    Checked,
    /// Org's `[-]`: some of the item's children are done.
    Partial,
}

/// A term and the blocks that define it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definition {
    pub term: Vec<Inline>,
    pub blocks: Vec<Block>,
}

/// A code or preformatted block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeBlock {
    /// The language to highlight as; `None` is plain text (or the
    /// renderer's default language).
    pub language: Option<String>,
    pub code: String,
    /// AsciiDoc callout markers in the code, to draw like the callout list.
    pub callouts: Vec<Callout>,
}

/// Where a callout marker (`<1>`) sits in a code block: a 0-based line and
/// the character range of the marker on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callout {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// A table's cells, each a run of inlines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableBlock {
    pub header: Option<Vec<Vec<Inline>>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
    /// Per-column alignment; missing columns are left-aligned.
    pub align: Vec<Align>,
    /// Row indexes a rule is drawn above (an Org table's inner `|---|`).
    pub sections: Vec<usize>,
    /// A caption, drawn as the table's title.
    pub title: Option<Vec<Inline>>,
}

/// A table column's alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// The kinds of admonition. `Other` keeps a label the format named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmonitionKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
    Other(String),
}

impl AdmonitionKind {
    /// The kind for a label such as `NOTE` or `warning`, if it is one of
    /// the five AsciiDoc admonitions.
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label.to_ascii_uppercase().as_str() {
            "NOTE" => AdmonitionKind::Note,
            "TIP" | "HINT" => AdmonitionKind::Tip,
            "IMPORTANT" => AdmonitionKind::Important,
            "WARNING" => AdmonitionKind::Warning,
            "CAUTION" | "DANGER" => AdmonitionKind::Caution,
            _ => return None,
        })
    }

    /// The title the panel shows.
    pub fn label(&self) -> String {
        match self {
            AdmonitionKind::Note => "Note".into(),
            AdmonitionKind::Tip => "Tip".into(),
            AdmonitionKind::Important => "Important".into(),
            AdmonitionKind::Warning => "Warning".into(),
            AdmonitionKind::Caution => "Caution".into(),
            AdmonitionKind::Other(label) => label.clone(),
        }
    }

    /// The theme key of its style (`document.admonition.note`, …).
    pub fn style_key(&self) -> &'static str {
        match self {
            AdmonitionKind::Note => "document.admonition.note",
            AdmonitionKind::Tip => "document.admonition.tip",
            AdmonitionKind::Important => "document.admonition.important",
            AdmonitionKind::Warning => "document.admonition.warning",
            AdmonitionKind::Caution => "document.admonition.caution",
            AdmonitionKind::Other(_) => "document.admonition",
        }
    }
}

/// A run of inline content.
#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    /// Italic (`markdown.em`).
    Emphasis(Vec<Inline>),
    /// Bold (`markdown.strong`).
    Strong(Vec<Inline>),
    Underline(Vec<Inline>),
    /// Struck through (`markdown.s`).
    Strike(Vec<Inline>),
    /// Inline code or verbatim text (`markdown.code`).
    Code(String),
    /// A link. With no content the target is shown.
    Link {
        target: String,
        content: Vec<Inline>,
    },
    /// An inline image, shown as the image marker and its alt text.
    Image {
        target: String,
        alt: String,
    },
    /// A hard line break.
    LineBreak,
    /// Content in a named theme style (`document.timestamp`, …) or a style
    /// definition.
    Styled {
        style: String,
        content: Vec<Inline>,
    },
}

impl Inline {
    /// Plain text.
    pub fn text(text: impl Into<String>) -> Self {
        Inline::Text(text.into())
    }

    /// `content` in the theme style `style`.
    pub fn styled(style: impl Into<String>, content: Vec<Inline>) -> Self {
        Inline::Styled {
            style: style.into(),
            content,
        }
    }
}

/// The plain text of a run of inlines.
pub fn inlines_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    push_inlines(inlines, &mut out);
    out
}

fn push_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(text) | Inline::Code(text) => out.push_str(text),
            Inline::Emphasis(content)
            | Inline::Strong(content)
            | Inline::Underline(content)
            | Inline::Strike(content)
            | Inline::Styled { content, .. } => push_inlines(content, out),
            Inline::Link { target, content } => {
                if content.is_empty() {
                    out.push_str(target);
                } else {
                    push_inlines(content, out);
                }
            }
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::LineBreak => out.push('\n'),
        }
    }
}
