//! Manual pages written in roff, parsed into the [document model](super::model).
//!
//! Covered, from the man macros: `.TH` (drawn as the page's running header
//! and footer), `.SH` and `.SS` headings, `.PP`/`.P`/`.LP`/`.HP` paragraphs,
//! `.TP`/`.TQ` and `.IP` tagged paragraphs (as definition lists, or bullet
//! and numbered lists when the tag is a bullet or a number), `.RS`/`.RE`
//! indentation, the font macros `.B`, `.I`, `.SM`, `.SB`, `.BI`, `.BR`,
//! `.IB`, `.IR`, `.RB` and `.RI`, `.nf`/`.fi` no-fill text, `.EX`/`.EE`
//! examples (and pod2man's `.Vb`/`.Ve`), `.br`, `.sp`, `.UR`/`.UE` and
//! `.MT`/`.ME` links, `.SY`/`.OP`/`.YS` synopses and `.MR` page references;
//! from roff: font escapes (`\fB`, `\fI`, `\fR`, `\fP`, `\f(CW`, `\f[…]`),
//! special characters (`\-`, `\(em`, `\(bu`, `\[u2014]`, …), strings (`.ds`,
//! `\*(lq`), comments, `.ft`, line continuation, `\c`, `.if`/`.ie`/`.el`
//! conditionals (as nroff evaluates them), and `.de`/`.ig` blocks (skipped);
//! tbl tables (`.TS`/`.TE`); and from mdoc (BSD pages): `.Dd`/`.Dt`/`.Os`,
//! `.Sh`/`.Ss`, `.Pp`, `.Nm`/`.Nd`, the semantic font macros (`.Fl`, `.Ar`,
//! `.Cm`, `.Pa`, `.Ev`, `.Xr`, …), the enclosures (`.Op`, `.Dq`, `.Pq`, …),
//! `.Bl`/`.It`/`.El` lists, `.Bd`/`.Ed` displays, `.D1`/`.Dl`, `.Ex` and
//! `.Rv`.
//!
//! Not shown: size and colour changes, motions and drawing escapes, and
//! `eqn` (`.EQ`) and `pic` (`.PS`) blocks; `.so` is not followed. A warning
//! says so for the last three. Unknown macros show their arguments as text.

use std::collections::{HashMap, VecDeque};

use super::model::{
    Align, Block, CodeBlock, Definition, Document, Heading, Inline, List, ListItem, ListKind,
    TableBlock,
};
use super::scan::{normalize, sample, MAX_NESTING};
use super::{Diagnostic, Parsed, Severity};

/// How far `.RS` and an untagged `.IP` indent, in cells.
const INDENT: usize = 4;

/// Parse a man page (man or mdoc macros).
pub fn parse(source: &str) -> Parsed {
    let source = normalize(source);
    let mut man = Man::default();
    man.run(&source);
    man.finish()
}

/// How confident it is that `text` is a roff man page (0 to 1).
pub fn sniff(text: &str) -> f32 {
    let text = sample(text);
    if text.starts_with("#!") {
        return 0.0;
    }
    let mut first_macro: Option<&str> = None;
    let (mut macros, mut text_lines, mut comments, mut th, mut sh) = (0, 0, 0, false, false);
    let (mut dd, mut dt) = (false, false);
    for line in text.lines().take(4000) {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with(".\\\"") || trimmed.starts_with("'\\\"") || trimmed == "." {
            comments += 1;
            continue;
        }
        let Some(rest) = trimmed.strip_prefix(['.', '\'']) else {
            text_lines += 1;
            continue;
        };
        let name = rest.split_whitespace().next().unwrap_or("");
        if !KNOWN_MACROS.contains(&name) {
            text_lines += 1;
            continue;
        }
        macros += 1;
        first_macro.get_or_insert(name);
        match name {
            "TH" => th = true,
            "SH" | "Sh" => sh = true,
            "Dd" => dd = true,
            "Dt" => dt = true,
            _ => {}
        }
    }
    let ratio = macros as f32 / (macros + text_lines).max(1) as f32;
    let mut score: f32 = if (th && sh) || (dd && dt) {
        0.95
    } else if th {
        0.85
    } else if sh && macros >= 3 && ratio >= 0.1 {
        0.75
    } else if macros >= 5 && ratio >= 0.2 {
        0.6
    } else {
        0.0
    };
    if score > 0.0 && matches!(first_macro, Some("TH" | "Dd")) {
        score += 0.03;
    }
    if score > 0.0 && comments > 0 {
        score += 0.02;
    }
    score.min(1.0)
}

/// Macro and request names the sniffer counts.
const KNOWN_MACROS: &[&str] = &[
    "TH", "SH", "SS", "PP", "P", "LP", "TP", "IP", "HP", "RS", "RE", "B", "I", "BI", "BR", "IB",
    "IR", "RB", "RI", "SM", "SB", "nf", "fi", "br", "sp", "EX", "EE", "UR", "UE", "MT", "ME", "SY",
    "OP", "YS", "TQ", "PD", "ft", "in", "ad", "na", "Dd", "Dt", "Os", "Sh", "Ss", "Pp", "Nm", "Nd",
    "Fl", "Ar", "Op", "It", "Bl", "El", "Xr", "Bd", "Ed", "TS", "TE",
];

/// A roff font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Font {
    #[default]
    Roman,
    Bold,
    Italic,
    BoldItalic,
    Mono,
}

impl Font {
    fn from_name(name: &str) -> Option<Font> {
        Some(match name {
            "R" | "1" | "" | "CR" | "TR" | "HR" | "NR" => Font::Roman,
            "B" | "3" | "CB" | "TB" | "HB" | "NB" => Font::Bold,
            "I" | "2" | "CI" | "TI" | "HI" | "NI" | "U" => Font::Italic,
            "BI" | "4" | "CBI" | "TBI" | "HBI" => Font::BoldItalic,
            "CW" | "C" | "CO" | "L" => Font::Mono,
            _ => return None,
        })
    }
}

/// One piece of a paragraph as roff produced it.
#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Run(Font, String),
    Break,
    LinkStart(String),
    LinkEnd,
}

/// Pieces to inlines: runs by font, links around what they enclose.
fn to_inlines(pieces: &[Piece]) -> Vec<Inline> {
    fn run_inline(font: Font, text: &str) -> Inline {
        let text = Inline::Text(text.to_string());
        match font {
            Font::Roman => text,
            Font::Bold => Inline::Strong(vec![text]),
            Font::Italic => Inline::Emphasis(vec![text]),
            Font::BoldItalic => Inline::Strong(vec![Inline::Emphasis(vec![text])]),
            Font::Mono => match text {
                Inline::Text(code) => Inline::Code(code),
                other => other,
            },
        }
    }
    let mut out: Vec<Inline> = Vec::new();
    let mut link: Option<(String, Vec<Inline>)> = None;
    // Merge neighbouring runs of one font.
    let mut merged: Vec<Piece> = Vec::new();
    for piece in pieces {
        match (merged.last_mut(), piece) {
            (Some(Piece::Run(last_font, last)), Piece::Run(font, text))
                if last_font == font || text.trim().is_empty() && *last_font != Font::Mono =>
            {
                last.push_str(text);
            }
            _ => merged.push(piece.clone()),
        }
    }
    for piece in &merged {
        let target = match &mut link {
            Some((_, content)) => content,
            None => &mut out,
        };
        match piece {
            Piece::Run(font, text) if !text.is_empty() => target.push(run_inline(*font, text)),
            Piece::Run(..) => {}
            Piece::Break => target.push(Inline::LineBreak),
            Piece::LinkStart(url) => {
                if let Some((url, content)) = link.take() {
                    out.push(Inline::Link {
                        target: url,
                        content,
                    });
                }
                link = Some((url.clone(), Vec::new()));
            }
            Piece::LinkEnd => {
                if let Some((url, content)) = link.take() {
                    out.push(Inline::Link {
                        target: url,
                        content,
                    });
                }
            }
        }
    }
    if let Some((url, content)) = link.take() {
        out.push(Inline::Link {
            target: url,
            content,
        });
    }
    super::scan::trim_inlines(out)
}

/// An open container while building the page.
#[derive(Debug)]
enum FrameKind {
    Root,
    /// `.RS`: indented content.
    Indent,
    /// The body of the last item of the block before this frame (a `.TP`
    /// or `.IP` item, or an untagged `.IP`).
    Item,
}

/// The most bytes of `\*` string values one line may expand to.
const EXPANSION_BUDGET: usize = 64 * 1024;

#[derive(Debug)]
struct Frame {
    kind: FrameKind,
    blocks: Vec<Block>,
}

/// Where the next line's text goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The tag of the `.TP` just opened (or `.TQ`).
    Tag { more: bool },
    /// The heading `.SH`/`.SS` without arguments announced.
    Heading(u8),
}

/// The `.TH` (or mdoc `.Dt`/`.Dd`/`.Os`) fields.
#[derive(Clone, Debug, Default)]
struct Title {
    name: String,
    section: String,
    date: String,
    source: String,
    manual: String,
}

#[derive(Default)]
struct Man {
    frames: Vec<Frame>,
    /// The paragraph being filled.
    paragraph: Vec<Piece>,
    /// No-fill lines (`.nf`), each a run of pieces.
    lines: Vec<Vec<Piece>>,
    fill: bool,
    example: bool,
    font: Font,
    previous_font: Font,
    /// The font `.B` (and friends) without arguments gives the next line.
    next_line_font: Option<Font>,
    /// `\c` ended the last text: the next joins it without a space.
    join_next: bool,
    /// `\c` was met by the last [`Man::escapes`] call.
    saw_continuation: bool,
    /// How deep mdoc macros call each other on this line.
    mdoc_depth: usize,
    target: Option<Target>,
    tag: Vec<Piece>,
    strings: HashMap<String, String>,
    /// The last `.ie` condition, for `.el`.
    last_condition: bool,
    title: Option<Title>,
    diagnostics: Vec<Diagnostic>,
    /// mdoc: the name `.Nm` repeats, and whether this is the SYNOPSIS.
    mdoc_name: Option<String>,
    section: String,
    /// mdoc `.Bl` lists: the list type of each open list.
    mdoc_lists: Vec<String>,
    /// Lines of an mdoc `.Bd -literal` display.
    display: Option<(bool, Vec<Vec<Piece>>)>,
    line_no: usize,
    /// Each distinct unknown macro is reported once.
    reported: Vec<String>,
}

impl Man {
    fn run(&mut self, source: &str) {
        self.fill = true;
        self.frames = vec![Frame {
            kind: FrameKind::Root,
            blocks: Vec::new(),
        }];
        let mut input: VecDeque<(usize, String)> = join_continuations(source).into();
        let mut processed = 0usize;
        while let Some((no, line)) = input.pop_front() {
            processed += 1;
            // Conditionals push lines back; never let a page loop forever.
            if processed > 4_000_000 {
                break;
            }
            self.line_no = no;
            self.line(&line, &mut input);
        }
    }

    fn finish(mut self) -> Parsed {
        self.flush();
        if let Some((literal, lines)) = self.display.take() {
            self.push_display(literal, lines);
        }
        while self.frames.len() > 1 {
            self.close_frame();
        }
        let mut blocks = Vec::new();
        if let Some(title) = &self.title {
            let page = page_name(title);
            blocks.push(Block::Banner {
                left: page.clone(),
                center: title.manual.clone(),
                right: page.clone(),
            });
        }
        blocks.append(&mut self.frames[0].blocks);
        if let Some(title) = &self.title {
            if !title.source.is_empty() || !title.date.is_empty() {
                blocks.push(Block::Banner {
                    left: title.source.clone(),
                    center: title.date.clone(),
                    right: page_name(title),
                });
            }
        }
        Parsed {
            document: Document { blocks },
            diagnostics: self.diagnostics,
        }
    }

    fn warn(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line: Some(self.line_no),
            severity: Severity::Warning,
            message: message.into(),
        });
    }

    fn info(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line: Some(self.line_no),
            severity: Severity::Info,
            message: message.into(),
        });
    }

    fn top(&mut self) -> &mut Vec<Block> {
        &mut self.frames.last_mut().expect("the root frame").blocks
    }

    /// One input line: a request or macro, or text.
    fn line(&mut self, line: &str, input: &mut VecDeque<(usize, String)>) {
        if line.starts_with(".\\\"") || line.starts_with("'\\\"") || line.starts_with(".\\#") {
            return;
        }
        if let Some(rest) = line.strip_prefix(['.', '\'']) {
            let rest = rest.trim_start_matches([' ', '\t']);
            if rest.is_empty() || rest.starts_with("\\\"") {
                return;
            }
            let name_end = rest.find([' ', '\t']).unwrap_or(rest.len());
            let (name, arguments) = rest.split_at(name_end);
            let arguments = strip_comment(arguments).trim_start();
            self.request(name, arguments, input);
            return;
        }
        self.text_line(line);
    }

    /// A text line: into the paragraph, a no-fill line, a tag or a heading.
    fn text_line(&mut self, line: &str) {
        let font = self.next_line_font.take();
        if self.display.is_some() {
            let pieces = self.escapes(line, font);
            if let Some((_, lines)) = &mut self.display {
                lines.push(pieces);
            }
            return;
        }
        if line.trim().is_empty() && self.target.is_none() {
            if self.fill && !self.example {
                self.flush();
            } else {
                self.lines.push(Vec::new());
            }
            return;
        }
        let joined = std::mem::take(&mut self.join_next);
        let pieces = self.escapes(line, font);
        let continues = self.saw_continuation;
        // In fill mode a line that starts with a space starts a new line,
        // keeping its indentation.
        if self.fill
            && !self.example
            && line.starts_with([' ', '\t'])
            && !self.paragraph.is_empty()
            && self.target.is_none()
        {
            self.paragraph.push(Piece::Break);
        }
        self.route_text(pieces, joined);
        self.join_next = continues;
    }

    /// Text goes to the open tag or heading if there is one, else to the
    /// paragraph (with a space before it, unless `\c` joined it) or the
    /// no-fill lines.
    fn route_text(&mut self, pieces: Vec<Piece>, joined: bool) {
        match self.target {
            Some(Target::Tag { more }) => {
                self.tag.extend(pieces);
                self.close_tag(more);
            }
            Some(Target::Heading(level)) => {
                self.target = None;
                let content = to_inlines(&pieces);
                self.heading(level, content);
            }
            None if !self.fill || self.example => match self.lines.last_mut() {
                Some(last) if joined => last.extend(pieces),
                _ => self.lines.push(pieces),
            },
            None => {
                let starts_line = matches!(
                    self.paragraph.last(),
                    None | Some(Piece::Break | Piece::LinkStart(_))
                );
                if !starts_line && !joined {
                    self.paragraph.push(Piece::Run(self.font, " ".into()));
                }
                self.paragraph.extend(pieces);
            }
        }
    }

    /// The `.TP` tag is complete: it becomes the open item's term (or,
    /// after `.TQ`, another line of it).
    fn close_tag(&mut self, more: bool) {
        self.target = None;
        let term = to_inlines(&std::mem::take(&mut self.tag));
        let frames = self.frames.len();
        if frames < 2 {
            return;
        }
        if let Some(Block::Definitions(definitions)) = self.frames[frames - 2].blocks.last_mut() {
            if let Some(definition) = definitions.last_mut() {
                if more && !definition.term.is_empty() {
                    definition.term.push(Inline::LineBreak);
                    definition.term.extend(term);
                } else {
                    definition.term = term;
                }
            }
        }
    }

    /// Emit the paragraph or no-fill lines being collected.
    fn flush(&mut self) {
        if let Some(Target::Tag { more }) = self.target {
            if self.tag.is_empty() {
                self.target = None;
            } else {
                self.close_tag(more);
            }
        }
        if !self.paragraph.is_empty() {
            let pieces = std::mem::take(&mut self.paragraph);
            let inlines = to_inlines(&pieces);
            if !inlines.is_empty() {
                self.top().push(Block::Paragraph(inlines));
            }
        }
        if !self.lines.is_empty() {
            let mut lines = std::mem::take(&mut self.lines);
            while lines.last().is_some_and(Vec::is_empty) {
                lines.pop();
            }
            while lines.first().is_some_and(Vec::is_empty) {
                lines.remove(0);
            }
            if lines.is_empty() {
                return;
            }
            let monospace = self.example
                || lines.iter().flatten().all(|piece| match piece {
                    Piece::Run(font, text) => *font == Font::Mono || text.trim().is_empty(),
                    _ => true,
                });
            if monospace {
                let code = lines
                    .iter()
                    .map(|line| plain(line))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.top().push(Block::Code(CodeBlock {
                    code,
                    ..CodeBlock::default()
                }));
            } else {
                let lines = lines.iter().map(|line| to_inlines_keep(line)).collect();
                self.top().push(Block::Lines(lines));
            }
        }
    }

    fn heading(&mut self, level: u8, content: Vec<Inline>) {
        self.flush();
        while self.frames.len() > 1 {
            self.close_frame();
        }
        self.section = super::model::inlines_text(&content).to_ascii_uppercase();
        if !content.is_empty() {
            self.top().push(Block::Heading(Heading {
                level,
                content,
                ..Heading::default()
            }));
        }
    }

    /// Close the innermost frame into its parent.
    fn close_frame(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        let Some(parent) = self.frames.last_mut() else {
            self.frames.push(frame);
            return;
        };
        match frame.kind {
            FrameKind::Root => {}
            FrameKind::Indent => {
                if !frame.blocks.is_empty() {
                    parent.blocks.push(Block::Indent {
                        amount: INDENT,
                        blocks: frame.blocks,
                    });
                }
            }
            FrameKind::Item => match parent.blocks.last_mut() {
                Some(Block::Definitions(definitions)) => {
                    if let Some(definition) = definitions.last_mut() {
                        definition.blocks.extend(frame.blocks);
                    }
                }
                Some(Block::List(list)) => {
                    if let Some(item) = list.items.last_mut() {
                        item.blocks.extend(frame.blocks);
                    }
                }
                Some(Block::Indent { blocks, .. }) => blocks.extend(frame.blocks),
                _ => parent.blocks.extend(frame.blocks),
            },
        }
    }

    /// Close item frames back to the nearest `.RS` level or the root.
    fn close_items(&mut self) {
        while matches!(
            self.frames.last().map(|frame| &frame.kind),
            Some(FrameKind::Item)
        ) && self.frames.len() > 1
        {
            self.close_frame();
        }
    }

    fn open_item(&mut self) {
        if self.frames.len() > MAX_NESTING {
            return;
        }
        self.frames.push(Frame {
            kind: FrameKind::Item,
            blocks: Vec::new(),
        });
    }

    /// A paragraph break that ends any tagged paragraph (`.PP`).
    fn paragraph_break(&mut self) {
        self.flush();
        self.close_items();
        self.font = Font::Roman;
    }

    /// `.TP`: a new definition whose term is the next line.
    fn tagged(&mut self) {
        self.paragraph_break();
        let top = self.top();
        if !matches!(top.last(), Some(Block::Definitions(_))) {
            top.push(Block::Definitions(Vec::new()));
        }
        if let Some(Block::Definitions(definitions)) = top.last_mut() {
            definitions.push(Definition::default());
        }
        self.open_item();
        self.target = Some(Target::Tag { more: false });
    }

    /// `.IP tag`: a bullet or numbered item, a definition, or (with no tag)
    /// an indented paragraph.
    fn indented_paragraph(&mut self, tag: Option<&str>) {
        self.paragraph_break();
        let tag_pieces = tag.map(|tag| self.escapes(tag, None));
        let tag_text = tag_pieces.as_deref().map(plain).unwrap_or_default();
        let tag_text = tag_text.trim();
        let bullet = matches!(
            tag_text,
            "•" | "*" | "-" | "o" | "·" | "+" | "–" | "∙" | "◦"
        );
        let number = tag_text
            .trim_end_matches(['.', ')'])
            .trim_start_matches('(')
            .parse::<u64>()
            .ok()
            .filter(|_| tag_text.ends_with(['.', ')']));
        let top = self.top();
        if tag_text.is_empty() {
            top.push(Block::Indent {
                amount: INDENT,
                blocks: Vec::new(),
            });
        } else if bullet || number.is_some() {
            let kind = if bullet {
                ListKind::Bullet
            } else {
                ListKind::Ordered {
                    start: number.unwrap_or(1),
                }
            };
            let same = matches!(top.last(), Some(Block::List(list)) if
                matches!((list.kind, kind), (ListKind::Bullet, ListKind::Bullet) | (ListKind::Ordered { .. }, ListKind::Ordered { .. })));
            if !same {
                top.push(Block::List(List {
                    kind,
                    items: Vec::new(),
                }));
            }
            if let Some(Block::List(list)) = top.last_mut() {
                list.items.push(ListItem {
                    number,
                    ..ListItem::default()
                });
            }
        } else {
            if !matches!(top.last(), Some(Block::Definitions(_))) {
                top.push(Block::Definitions(Vec::new()));
            }
            if let Some(Block::Definitions(definitions)) = top.last_mut() {
                definitions.push(Definition {
                    term: to_inlines(&tag_pieces.unwrap_or_default()),
                    blocks: Vec::new(),
                });
            }
        }
        self.open_item();
    }

    fn request(&mut self, name: &str, arguments: &str, input: &mut VecDeque<(usize, String)>) {
        match name {
            "TH" => {
                let mut fields = split_args(arguments)
                    .iter()
                    .map(|arg| plain(&self.escapes(arg, None)))
                    .collect::<Vec<_>>()
                    .into_iter();
                let mut field = || fields.next().unwrap_or_default();
                self.title = Some(Title {
                    name: field(),
                    section: field(),
                    date: field(),
                    source: field(),
                    manual: field(),
                });
            }
            "SH" | "SS" | "Sh" | "Ss" => {
                let level = if name.eq_ignore_ascii_case("SH") {
                    2
                } else {
                    3
                };
                self.paragraph_break();
                self.close_lists();
                if arguments.trim().is_empty() {
                    self.target = Some(Target::Heading(level));
                } else if name == "Sh" || name == "Ss" {
                    let args = split_args(arguments);
                    let pieces = self.callable(&args);
                    let content = to_inlines(&pieces);
                    self.heading(level, content);
                } else {
                    let text = split_args(arguments).join(" ");
                    let pieces = self.escapes(&text, None);
                    self.heading(level, to_inlines(&pieces));
                }
            }
            "PP" | "P" | "LP" | "HP" | "Pp" | "Lp" => self.paragraph_break(),
            "TP" => self.tagged(),
            "TQ" => {
                self.flush();
                self.target = Some(Target::Tag { more: true });
            }
            "IP" => {
                let args = split_args(arguments);
                self.indented_paragraph(args.first().map(String::as_str));
            }
            "RS" => {
                self.flush();
                if self.frames.len() <= MAX_NESTING {
                    self.frames.push(Frame {
                        kind: FrameKind::Indent,
                        blocks: Vec::new(),
                    });
                }
            }
            "RE" => {
                self.flush();
                while self.frames.len() > 1 {
                    let indent =
                        matches!(self.frames.last().map(|f| &f.kind), Some(FrameKind::Indent));
                    self.close_frame();
                    if indent {
                        break;
                    }
                }
            }
            "B" | "I" | "SM" | "SB" => {
                let font = match name {
                    "B" | "SB" => Font::Bold,
                    "I" => Font::Italic,
                    _ => Font::Roman,
                };
                if arguments.trim().is_empty() {
                    self.next_line_font = Some(font);
                } else {
                    let text = split_args(arguments).join(" ");
                    let pieces = self.escapes(&text, Some(font));
                    self.emit(pieces);
                }
            }
            "BI" | "BR" | "IB" | "IR" | "RB" | "RI" => {
                let fonts: Vec<Font> = name
                    .chars()
                    .map(|c| match c {
                        'B' => Font::Bold,
                        'I' => Font::Italic,
                        _ => Font::Roman,
                    })
                    .collect();
                let mut pieces = Vec::new();
                for (index, argument) in split_args(arguments).iter().enumerate() {
                    pieces.extend(self.escapes(argument, Some(fonts[index % 2])));
                }
                self.emit(pieces);
            }
            "nf" => {
                self.flush();
                self.fill = false;
            }
            "fi" => {
                self.flush();
                self.fill = true;
            }
            "EX" | "Vb" => {
                self.flush();
                self.fill = false;
                self.example = true;
            }
            "EE" | "Ve" => {
                self.flush();
                self.fill = true;
                self.example = false;
            }
            "br" => {
                if self.fill && !self.paragraph.is_empty() {
                    self.paragraph.push(Piece::Break);
                }
            }
            "sp" | "Sp" => {
                if self.fill {
                    self.flush();
                } else {
                    self.lines.push(Vec::new());
                }
            }
            "ft" => {
                let font = arguments.split_whitespace().next().unwrap_or("P");
                self.set_font(font);
            }
            "UR" | "MT" => {
                let target = split_args(arguments).into_iter().next().unwrap_or_default();
                let target = if name == "MT" {
                    format!("mailto:{target}")
                } else {
                    target
                };
                if !self.fill {
                    self.flush();
                }
                if !matches!(self.paragraph.last(), None | Some(Piece::Break)) {
                    self.paragraph.push(Piece::Run(self.font, " ".into()));
                }
                self.paragraph.push(Piece::LinkStart(target));
            }
            "UE" | "ME" => {
                if !self
                    .paragraph
                    .iter()
                    .rev()
                    .take_while(|piece| !matches!(piece, Piece::LinkStart(_)))
                    .any(|piece| matches!(piece, Piece::Run(_, text) if !text.trim().is_empty()))
                {
                    // No link text: show the target.
                    if let Some(Piece::LinkStart(target)) = self
                        .paragraph
                        .iter()
                        .rev()
                        .find(|piece| matches!(piece, Piece::LinkStart(_)))
                        .cloned()
                    {
                        let shown = target.trim_start_matches("mailto:").to_string();
                        self.paragraph.push(Piece::Run(Font::Roman, shown));
                    }
                }
                self.paragraph.push(Piece::LinkEnd);
                let trailer = split_args(arguments).join(" ");
                if !trailer.is_empty() {
                    let pieces = self.escapes(&trailer, None);
                    self.paragraph.extend(pieces);
                }
            }
            "SY" => {
                self.paragraph_break();
                let command = split_args(arguments).join(" ");
                let pieces = self.escapes(&command, Some(Font::Bold));
                self.emit(pieces);
            }
            "OP" => {
                let args = split_args(arguments);
                let mut pieces = vec![Piece::Run(Font::Roman, "[".into())];
                if let Some(option) = args.first() {
                    pieces.extend(self.escapes(option, Some(Font::Bold)));
                }
                if let Some(argument) = args.get(1) {
                    pieces.push(Piece::Run(Font::Roman, " ".into()));
                    pieces.extend(self.escapes(argument, Some(Font::Italic)));
                }
                pieces.push(Piece::Run(Font::Roman, "]".into()));
                self.emit(pieces);
            }
            "YS" => self.paragraph_break(),
            "MR" => {
                let args = split_args(arguments);
                let mut pieces = Vec::new();
                if let Some(page) = args.first() {
                    pieces.extend(self.escapes(page, Some(Font::Italic)));
                }
                if let Some(section) = args.get(1) {
                    pieces.push(Piece::Run(Font::Roman, format!("({section})")));
                }
                if let Some(trailer) = args.get(2) {
                    pieces.extend(self.escapes(trailer, None));
                }
                self.emit(pieces);
            }
            "ds" | "as" => {
                let (key, value) = match arguments.split_once([' ', '\t']) {
                    Some((key, value)) => (key, value.trim_start()),
                    None => (arguments.trim(), ""),
                };
                let value = value.strip_prefix('"').unwrap_or(value).to_string();
                if !key.is_empty() {
                    if name == "as" {
                        self.strings
                            .entry(key.to_string())
                            .or_default()
                            .push_str(&value);
                    } else {
                        self.strings.insert(key.to_string(), value);
                    }
                }
            }
            "de" | "de1" | "am" | "ig" | "dei" | "ami" => {
                let end = if name == "ig" {
                    arguments.split_whitespace().next().unwrap_or(".")
                } else {
                    arguments.split_whitespace().nth(1).unwrap_or(".")
                };
                let end = if end == "." {
                    "..".to_string()
                } else {
                    format!(".{end}")
                };
                while let Some((_, line)) = input.pop_front() {
                    if line.trim_end() == end || line.trim_end().starts_with(&format!("{end} ")) {
                        break;
                    }
                }
            }
            "if" | "ie" => {
                let (condition, body) = condition(arguments, &self.strings);
                if name == "ie" {
                    self.last_condition = condition;
                }
                self.conditional(condition, body, input);
            }
            "el" => {
                let condition = !self.last_condition;
                self.conditional(condition, arguments, input);
            }
            "so" => {
                let file = arguments.trim();
                self.warn(format!(
                    ".so {file} is not followed; its content is not shown"
                ));
            }
            "TS" => self.table(input),
            "EQ" | "PS" | "[" => {
                let end = match name {
                    "EQ" => ".EN",
                    "PS" => ".PE",
                    _ => ".]",
                };
                let what = if name == "EQ" {
                    "an equation"
                } else {
                    "a picture"
                };
                if name != "[" {
                    self.warn(format!("{what} (.{name}) is not shown"));
                }
                while let Some((_, line)) = input.pop_front() {
                    if line.trim_end() == end {
                        break;
                    }
                }
            }
            // Formatting requests with nothing to show in a terminal.
            "PD" | "DT" | "UC" | "AT" | "ad" | "na" | "hy" | "nh" | "ne" | "ps" | "vs" | "ll"
            | "pl" | "in" | "ti" | "ta" | "lg" | "ss" | "cs" | "bd" | "fam" | "ev" | "ch"
            | "wh" | "mk" | "rt" | "ns" | "rs" | "pc" | "pn" | "bp" | "nr" | "rr" | "rm" | "rn"
            | "als" | "tr" | "cu" | "ul" | "ce" | "ec" | "eo" | "cc" | "c2" | "hc" | "hw"
            | "lt" | "tl" | "fp" | "ftr" | "mso" | "do" | "cp" | "nx" | "rd" | "ex" | "tm"
            | "ab" | "warn" | "hla" | "hlm" | "hym" | "hys" | "kern" | "fcolor" | "gcolor"
            | "defcolor" | "pso" | "psbb" | "open" | "close" | "write" | "char" | "fchar"
            | "schar" | "hcode" | "lc" | "fc" | "ls" | "nm" | "nn" | "po" | "sv" | "os" | "fl"
            | "it" | "itc" | "shc" | "sty" | "trf" | "cflags" | "chop" | "substring" | "length"
            | "index" | "blm" | "lsm" | "em" | "Bk" | "Ek" | "Sm" | "Xo" | "Xc" | "Dx" => {}
            "Os" => {
                let os = split_args(arguments).join(" ");
                if !os.is_empty() {
                    self.title.get_or_insert_with(Title::default).source = os;
                }
            }
            _ => {
                let pieces = self.mdoc(name, arguments);
                if !pieces.is_empty() {
                    self.emit(pieces);
                }
            }
        }
    }

    /// Text a macro produced, to wherever text goes now.
    fn emit(&mut self, pieces: Vec<Piece>) {
        let continues = self.saw_continuation;
        if self.display.is_some() {
            if let Some((_, lines)) = &mut self.display {
                lines.push(pieces);
            }
            return;
        }
        let joined = std::mem::take(&mut self.join_next);
        self.route_text(pieces, joined);
        self.join_next = continues;
    }

    fn set_font(&mut self, name: &str) {
        let name = name
            .trim_start_matches('(')
            .trim_start_matches('[')
            .trim_end_matches(']');
        if name == "P" || name.is_empty() {
            std::mem::swap(&mut self.font, &mut self.previous_font);
            return;
        }
        if let Some(font) = Font::from_name(name) {
            self.previous_font = self.font;
            self.font = font;
        }
    }

    /// `.if`/`.ie`/`.el`: run `body` (a request, text, or a `\{ … \}` block)
    /// when `condition` holds, else skip it.
    fn conditional(&mut self, condition: bool, body: &str, input: &mut VecDeque<(usize, String)>) {
        let body = body.trim_start();
        let Some(rest) = body.strip_prefix("\\{") else {
            if condition && !body.is_empty() {
                input.push_front((self.line_no, body.to_string()));
            }
            return;
        };
        // A block: this line's rest and the lines up to the matching `\}`.
        let mut depth = 1 + braces(rest);
        let mut block = vec![(self.line_no, rest.to_string())];
        while depth > 0 {
            let Some((no, line)) = input.pop_front() else {
                break;
            };
            depth += braces(&line);
            block.push((no, line));
        }
        if !condition {
            return;
        }
        for (no, line) in block.into_iter().rev() {
            let line = line.replace("\\}", "").replace("\\{", "");
            let line = line.trim_start();
            if !line.is_empty() {
                input.push_front((no, line.to_string()));
            }
        }
    }

    /// A tbl table, `.TS` to `.TE`.
    fn table(&mut self, input: &mut VecDeque<(usize, String)>) {
        self.flush();
        let mut body = Vec::new();
        while let Some((_, line)) = input.pop_front() {
            if line.trim_end() == ".TE" {
                break;
            }
            body.push(line);
        }
        let mut tab = '\t';
        let mut lines = body.iter().peekable();
        // Options: a line ending in `;`.
        if let Some(first) = lines.peek() {
            if first.trim_end().ends_with(';') {
                let options = first.to_ascii_lowercase();
                if let Some(at) = options.find("tab(") {
                    tab = first[at + 4..].chars().next().unwrap_or('\t');
                }
                lines.next();
            }
        }
        // Format lines, up to one ending in `.`.
        let mut bold_header = false;
        let mut align = Vec::new();
        let mut first_format = true;
        for line in lines.by_ref() {
            let format = line.trim();
            if first_format {
                let columns: Vec<&str> = format
                    .trim_end_matches('.')
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|c| !c.is_empty())
                    .collect();
                bold_header =
                    !columns.is_empty() && columns.iter().all(|column| column.contains(['b', 'B']));
                align = columns
                    .iter()
                    .map(|column| match column.chars().next() {
                        Some('c' | 'C') => Align::Center,
                        Some('r' | 'R' | 'n' | 'N') => Align::Right,
                        _ => Align::Left,
                    })
                    .collect();
                first_format = false;
            }
            if format.ends_with('.') {
                break;
            }
        }
        let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
        let mut sections = Vec::new();
        let mut rule_after_first = false;
        let mut pending: Vec<String> = Vec::new();
        let mut block: Option<String> = None;
        let mut format_section = false;
        for line in lines {
            if format_section {
                if line.trim_end().ends_with('.') {
                    format_section = false;
                }
                continue;
            }
            if line.trim_end() == ".T&" {
                format_section = true;
                continue;
            }
            if let Some(text) = block.as_mut() {
                // Inside `T{ … T}`.
                if let Some(rest) = line.strip_prefix("T}") {
                    let cell = std::mem::take(text);
                    block = None;
                    pending.push(cell);
                    let rest = rest.strip_prefix(tab).unwrap_or(rest);
                    if !rest.is_empty() {
                        pending.extend(rest.split(tab).map(str::to_string));
                    }
                    if !rest.ends_with("T{") {
                        rows.push(self.table_row(&std::mem::take(&mut pending)));
                    }
                } else {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(line.trim());
                }
                continue;
            }
            let trimmed = line.trim();
            if trimmed == "_" || trimmed == "=" {
                if rows.len() == 1 {
                    rule_after_first = true;
                } else if !rows.is_empty() {
                    sections.push(rows.len());
                }
                continue;
            }
            if trimmed.starts_with('.') {
                continue;
            }
            let mut cells: Vec<String> = line.split(tab).map(str::to_string).collect();
            if cells.last().is_some_and(|cell| cell.trim_end() == "T{") {
                cells.pop();
                pending = cells;
                block = Some(String::new());
                continue;
            }
            rows.push(self.table_row(&cells));
        }
        if rows.is_empty() {
            return;
        }
        let header = (bold_header || rule_after_first).then(|| rows.remove(0));
        let sections = sections
            .into_iter()
            .map(|at: usize| at.saturating_sub(usize::from(header.is_some())))
            .filter(|&at| at > 0)
            .collect();
        self.top().push(Block::Table(TableBlock {
            header,
            rows,
            align,
            sections,
            title: None,
        }));
    }

    fn table_row(&mut self, cells: &[String]) -> Vec<Vec<Inline>> {
        cells
            .iter()
            .map(|cell| {
                let pieces = self.escapes(cell.trim(), None);
                to_inlines(&pieces)
            })
            .collect()
    }

    /// Close every open mdoc list (a new section ends them).
    fn close_lists(&mut self) {
        self.mdoc_lists.clear();
    }

    /// An mdoc display's lines, as code (`-literal`) or kept lines.
    fn push_display(&mut self, literal: bool, lines: Vec<Vec<Piece>>) {
        if lines.is_empty() {
            return;
        }
        if literal {
            let code = lines
                .iter()
                .map(|line| plain(line))
                .collect::<Vec<_>>()
                .join("\n");
            self.top().push(Block::Code(CodeBlock {
                code,
                ..CodeBlock::default()
            }));
        } else {
            let lines = lines.iter().map(|line| to_inlines_keep(line)).collect();
            self.top().push(Block::Lines(lines));
        }
    }

    /// An mdoc macro (or an unknown one): what it shows.
    fn mdoc(&mut self, name: &str, arguments: &str) -> Vec<Piece> {
        let args = split_args(arguments);
        match name {
            "Dd" => {
                let date = plain(&self.escapes(&args.join(" "), None));
                self.title.get_or_insert_with(Title::default).date = date;
                Vec::new()
            }
            "Dt" => {
                let title = self.title.get_or_insert_with(Title::default);
                title.name = args.first().cloned().unwrap_or_default();
                title.section = args.get(1).cloned().unwrap_or_default();
                if title.manual.is_empty() {
                    title.manual = section_manual(&title.section).to_string();
                }
                Vec::new()
            }
            "Nd" => {
                let mut pieces = vec![Piece::Run(Font::Roman, "\u{2014} ".into())];
                pieces.extend(self.callable(&args));
                pieces
            }
            "Bl" => {
                self.flush();
                let kind = args
                    .iter()
                    .find(|arg| arg.starts_with('-') && !matches!(arg.as_str(), "-compact"))
                    .cloned()
                    .unwrap_or_else(|| "-tag".into());
                let block = match kind.as_str() {
                    "-bullet" | "-dash" | "-hyphen" | "-item" => Block::List(List {
                        kind: ListKind::Bullet,
                        items: Vec::new(),
                    }),
                    "-enum" => Block::List(List {
                        kind: ListKind::Ordered { start: 1 },
                        items: Vec::new(),
                    }),
                    "-column" => Block::Table(TableBlock::default()),
                    _ => Block::Definitions(Vec::new()),
                };
                if self.mdoc_lists.len() < MAX_NESTING {
                    self.frames.push(Frame {
                        kind: FrameKind::Indent,
                        blocks: vec![block],
                    });
                    self.mdoc_lists.push(kind);
                }
                Vec::new()
            }
            "It" => {
                self.flush();
                self.close_items();
                let tag = self.callable(&args);
                let kind = self.mdoc_lists.last().cloned().unwrap_or_default();
                let top = self.top();
                match top.last_mut() {
                    Some(Block::List(list)) => list.items.push(ListItem::default()),
                    Some(Block::Definitions(definitions)) => definitions.push(Definition {
                        term: to_inlines(&tag),
                        blocks: Vec::new(),
                    }),
                    Some(Block::Table(table)) if kind == "-column" => {
                        let cells = split_columns(&tag);
                        table.rows.push(cells);
                        return Vec::new();
                    }
                    _ => {}
                }
                self.open_item();
                Vec::new()
            }
            "El" => {
                self.flush();
                self.close_items();
                if self.mdoc_lists.pop().is_some() {
                    // Unwrap the list from the indent frame `.Bl` opened.
                    if let Some(frame) = self.frames.pop() {
                        self.top().extend(frame.blocks);
                    }
                }
                Vec::new()
            }
            "Bd" => {
                self.flush();
                let literal = args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-literal" | "-unfilled"));
                let literal = literal && args.iter().any(|arg| arg == "-literal");
                let filled = args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-filled" | "-ragged" | "-centered"));
                if !filled {
                    self.display = Some((literal, Vec::new()));
                }
                Vec::new()
            }
            "Ed" => {
                if let Some((literal, lines)) = self.display.take() {
                    self.push_display(literal, lines);
                } else {
                    self.flush();
                }
                Vec::new()
            }
            "D1" | "Dl" => {
                self.flush();
                let pieces = self.callable(&args);
                let block = if name == "Dl" {
                    Block::Code(CodeBlock {
                        code: plain(&pieces),
                        ..CodeBlock::default()
                    })
                } else {
                    Block::Paragraph(to_inlines(&pieces))
                };
                self.top().push(Block::Indent {
                    amount: INDENT,
                    blocks: vec![block],
                });
                Vec::new()
            }
            "Ex" => {
                let utility = args
                    .iter()
                    .find(|arg| *arg != "-std")
                    .cloned()
                    .or_else(|| self.mdoc_name.clone())
                    .unwrap_or_else(|| "this".into());
                vec![
                    Piece::Run(Font::Roman, "The ".into()),
                    Piece::Run(Font::Bold, utility),
                    Piece::Run(
                        Font::Roman,
                        " utility exits\u{a0}0 on success, and\u{a0}>0 if an error occurs.".into(),
                    ),
                ]
            }
            "Rv" => {
                let function = args
                    .iter()
                    .find(|arg| *arg != "-std")
                    .cloned()
                    .unwrap_or_else(|| "this".into());
                vec![
                    Piece::Run(Font::Roman, "The ".into()),
                    Piece::Run(Font::Bold, format!("{function}()")),
                    Piece::Run(
                        Font::Roman,
                        " function returns the value\u{a0}0 if successful; otherwise the value\u{a0}-1 is returned and the global variable errno is set to indicate the error."
                            .into(),
                    ),
                ]
            }
            "Nm" if self.section == "SYNOPSIS" && !self.paragraph.is_empty() => {
                self.paragraph.push(Piece::Break);
                self.callable_macro(name, &args)
            }
            _ if is_callable(name) => self.callable_macro(name, &args),
            _ => {
                // An unknown macro: its arguments, as text.
                if !self.reported.iter().any(|known| known == name) && self.reported.len() < 32 {
                    self.reported.push(name.to_string());
                    self.info(format!(
                        "unknown macro .{name}; its arguments are shown as text"
                    ));
                }
                let text = args.join(" ");
                self.escapes(&text, None)
            }
        }
    }

    /// The pieces of an mdoc line whose words may name callable macros.
    fn callable(&mut self, args: &[String]) -> Vec<Piece> {
        let mut out = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let word = &args[index];
            if is_callable(word) && self.mdoc_depth < MAX_NESTING {
                self.mdoc_depth += 1;
                let pieces = self.callable_macro(word, &args[index + 1..]);
                self.mdoc_depth -= 1;
                append_words(&mut out, pieces);
                return out;
            }
            let pieces = self.escapes(word, None);
            append_words(&mut out, pieces);
            index += 1;
        }
        out
    }

    /// One callable mdoc macro and the words after it (which may hold more
    /// macros).
    fn callable_macro(&mut self, name: &str, args: &[String]) -> Vec<Piece> {
        // The words this macro takes: up to the next callable macro.
        let split = args
            .iter()
            .position(|word| is_callable(word))
            .unwrap_or(args.len());
        let (own, rest) = args.split_at(split);
        let enclosure = |open: &str, close: &str, this: &mut Man| {
            let mut pieces = vec![Piece::Run(Font::Roman, open.to_string())];
            // Trailing punctuation goes after the closing mark.
            let mut inner: Vec<String> = args.to_vec();
            let mut trailing = Vec::new();
            while inner.last().is_some_and(|word| is_closing(word)) {
                trailing.insert(0, inner.pop().expect("checked"));
            }
            let body = this.callable(&inner);
            pieces.extend(body);
            pieces.push(Piece::Run(Font::Roman, close.to_string()));
            for word in trailing {
                pieces.push(Piece::Run(Font::Roman, word));
            }
            pieces
        };
        let font_words = |font: Font, words: &[String], this: &mut Man| -> Vec<Piece> {
            let mut pieces = Vec::new();
            for word in words {
                let font = if is_closing(word) || is_opening(word) {
                    Font::Roman
                } else {
                    font
                };
                append_words(&mut pieces, this.escapes(word, Some(font)));
            }
            pieces
        };
        let mut out = match name {
            "Op" => return enclosure("[", "]", self),
            "Dq" => return enclosure("\u{201c}", "\u{201d}", self),
            "Sq" => return enclosure("\u{2018}", "\u{2019}", self),
            "Qq" => return enclosure("\"", "\"", self),
            "Pq" => return enclosure("(", ")", self),
            "Bq" => return enclosure("[", "]", self),
            "Brq" => return enclosure("{", "}", self),
            "Aq" => return enclosure("\u{27e8}", "\u{27e9}", self),
            "Ql" => return enclosure("\u{2018}", "\u{2019}", self),
            "Oo" => vec![Piece::Run(Font::Roman, "[".into())],
            "Oc" => vec![Piece::Run(Font::Roman, "]".into())],
            "Do" => vec![Piece::Run(Font::Roman, "\u{201c}".into())],
            "Dc" => vec![Piece::Run(Font::Roman, "\u{201d}".into())],
            "Po" => vec![Piece::Run(Font::Roman, "(".into())],
            "Pc" => vec![Piece::Run(Font::Roman, ")".into())],
            "Bo" => vec![Piece::Run(Font::Roman, "[".into())],
            "Bc" => vec![Piece::Run(Font::Roman, "]".into())],
            "So" => vec![Piece::Run(Font::Roman, "\u{2018}".into())],
            "Sc" => vec![Piece::Run(Font::Roman, "\u{2019}".into())],
            "Qo" => vec![Piece::Run(Font::Roman, "\"".into())],
            "Qc" => vec![Piece::Run(Font::Roman, "\"".into())],
            "Fl" => {
                if own.is_empty() {
                    vec![Piece::Run(Font::Bold, "-".into())]
                } else {
                    let flags: Vec<String> = own
                        .iter()
                        .map(|word| {
                            if is_closing(word) || is_opening(word) {
                                word.clone()
                            } else {
                                format!("-{word}")
                            }
                        })
                        .collect();
                    font_words(Font::Bold, &flags, self)
                }
            }
            "Ar" => {
                if own.iter().all(|word| is_closing(word)) {
                    let mut pieces = vec![Piece::Run(Font::Italic, "file ...".into())];
                    pieces.extend(own.iter().map(|w| Piece::Run(Font::Roman, w.clone())));
                    pieces
                } else {
                    font_words(Font::Italic, own, self)
                }
            }
            "Nm" => {
                if own.iter().any(|word| !is_closing(word)) && self.mdoc_name.is_none() {
                    self.mdoc_name = own.first().cloned();
                }
                if own.iter().all(|word| is_closing(word)) {
                    let mut pieces = vec![Piece::Run(
                        Font::Bold,
                        self.mdoc_name.clone().unwrap_or_default(),
                    )];
                    pieces.extend(own.iter().map(|w| Piece::Run(Font::Roman, w.clone())));
                    pieces
                } else {
                    font_words(Font::Bold, own, self)
                }
            }
            "Cm" | "Ic" | "Sy" | "Fd" => font_words(Font::Bold, own, self),
            "In" => {
                let mut pieces = vec![Piece::Run(Font::Roman, "#include <".into())];
                pieces.extend(font_words(Font::Bold, own, self));
                pieces.push(Piece::Run(Font::Roman, ">".into()));
                pieces
            }
            "Em" | "Va" | "Fa" | "Ft" | "Vt" | "Pa" | "Ad" | "Sx" => {
                font_words(Font::Italic, own, self)
            }
            "Li" | "Ev" | "Dv" | "Er" | "Ms" | "Tn" | "No" | "An" | "Ux" | "Bx" | "Fx" | "Nx"
            | "Ox" | "Bsx" | "At" | "St" | "Lb" | "Ot" | "Mt" | "Ta" => {
                let pieces = font_words(Font::Roman, own, self);
                match name {
                    "Ux" => vec![Piece::Run(Font::Roman, "UNIX".into())],
                    "Bx" if own.is_empty() => vec![Piece::Run(Font::Roman, "BSD".into())],
                    "Fx" if own.is_empty() => vec![Piece::Run(Font::Roman, "FreeBSD".into())],
                    "Nx" if own.is_empty() => vec![Piece::Run(Font::Roman, "NetBSD".into())],
                    "Ox" if own.is_empty() => vec![Piece::Run(Font::Roman, "OpenBSD".into())],
                    "Ta" => vec![Piece::Run(Font::Roman, "\t".into())],
                    _ => pieces,
                }
            }
            "Fn" => {
                let mut pieces = Vec::new();
                if let Some(function) = own.first() {
                    pieces.extend(self.escapes(function, Some(Font::Bold)));
                    pieces.push(Piece::Run(Font::Roman, "(".into()));
                    for (index, argument) in own[1..].iter().enumerate() {
                        if index > 0 {
                            pieces.push(Piece::Run(Font::Roman, ", ".into()));
                        }
                        pieces.extend(self.escapes(argument, Some(Font::Italic)));
                    }
                    pieces.push(Piece::Run(Font::Roman, ")".into()));
                }
                pieces
            }
            "Xr" => {
                let mut pieces = Vec::new();
                if let Some(page) = own.first() {
                    pieces.extend(self.escapes(page, Some(Font::Bold)));
                }
                if let Some(section) = own.get(1).filter(|word| !is_closing(word)) {
                    pieces.push(Piece::Run(Font::Roman, format!("({section})")));
                }
                let skip = 1 + usize::from(own.get(1).is_some_and(|w| !is_closing(w)));
                for word in own.iter().skip(skip) {
                    pieces.push(Piece::Run(Font::Roman, word.clone()));
                }
                pieces
            }
            "Lk" => {
                let url = own.first().cloned().unwrap_or_default();
                let mut pieces = vec![Piece::LinkStart(url.clone())];
                let text: Vec<String> = own
                    .iter()
                    .skip(1)
                    .filter(|w| !is_closing(w))
                    .cloned()
                    .collect();
                if text.is_empty() {
                    pieces.push(Piece::Run(Font::Roman, url));
                } else {
                    pieces.extend(font_words(Font::Roman, &text, self));
                }
                pieces.push(Piece::LinkEnd);
                pieces
            }
            "Ns" | "Ap" | "Pf" => {
                let mut pieces = Vec::new();
                if name == "Ap" {
                    pieces.push(Piece::Run(Font::Roman, "'".into()));
                }
                if name == "Pf" {
                    if let Some(prefix) = own.first() {
                        pieces.extend(self.escapes(prefix, None));
                    }
                    let mut rest_pieces = self.callable(rest);
                    if let Some(Piece::Run(_, text)) = rest_pieces.first_mut() {
                        *text = text.trim_start().to_string();
                    }
                    pieces.extend(rest_pieces);
                    return pieces;
                }
                pieces.push(Piece::Run(Font::Roman, "\u{0}".into()));
                pieces.extend(font_words(Font::Roman, own, self));
                pieces
            }
            _ => font_words(Font::Roman, own, self),
        };
        if !rest.is_empty() {
            let more = self.callable(rest);
            append_words(&mut out, more);
        }
        out
    }

    /// Roff escapes in `text`, as pieces. `font` overrides the current font
    /// for this text only (a font macro's argument); `\f` changes the font
    /// for what follows in either case.
    fn escapes(&mut self, text: &str, font: Option<Font>) -> Vec<Piece> {
        let saved = (self.font, self.previous_font);
        if let Some(font) = font {
            self.previous_font = self.font;
            self.font = font;
        }
        let mut pieces = Vec::new();
        let mut current = String::new();
        self.saw_continuation = false;
        let mut budget = EXPANSION_BUDGET;
        self.expand(
            text,
            &mut pieces,
            &mut current,
            &mut Vec::new(),
            &mut budget,
        );
        if !current.is_empty() {
            pieces.push(Piece::Run(self.font, current));
        }
        if font.is_some() {
            (self.font, self.previous_font) = saved;
        }
        pieces
    }

    /// `stack` holds the `\*` strings being expanded (a name already on it
    /// is not expanded again) and `budget` the bytes of string values one
    /// call of [`escapes`](Self::escapes) may still splice in, so a string
    /// that refers to itself many times cannot grow without bound.
    fn expand(
        &mut self,
        text: &str,
        pieces: &mut Vec<Piece>,
        current: &mut String,
        stack: &mut Vec<String>,
        budget: &mut usize,
    ) {
        let mut chars = text.char_indices().peekable();
        while let Some((_, ch)) = chars.next() {
            if ch != '\\' {
                // Tabs line columns up in no-fill text; filled text is
                // wrapped anyway.
                current.push(if ch == '\t' && self.fill { ' ' } else { ch });
                continue;
            }
            let Some((_, next)) = chars.next() else {
                break;
            };
            match next {
                'f' => {
                    let name = escape_name(&mut chars);
                    if !current.is_empty() {
                        pieces.push(Piece::Run(self.font, std::mem::take(current)));
                    }
                    self.set_font(&name);
                }
                '(' | '[' | 'C' => {
                    let name = match next {
                        '(' => take_chars(&mut chars, 2),
                        '[' => take_until(&mut chars, ']'),
                        _ => {
                            let delimiter = chars.next().map(|(_, c)| c).unwrap_or('\'');
                            take_until(&mut chars, delimiter)
                        }
                    };
                    current.push_str(&special(&name));
                }
                '*' => {
                    let name = escape_name(&mut chars);
                    let value = self
                        .strings
                        .get(&name)
                        .cloned()
                        .or_else(|| predefined_string(&name).map(str::to_string));
                    if let Some(value) = value {
                        if stack.len() < 8 && !stack.contains(&name) && value.len() <= *budget {
                            *budget -= value.len();
                            stack.push(name);
                            self.expand(&value, pieces, current, stack, budget);
                            stack.pop();
                        }
                    }
                }
                'n' | 'g' | 'V' | 'Y' | '$' | 'm' | 'M' | 'F' | 'k' => {
                    // Registers, colours, families, environment: nothing to show.
                    if next == 'n' && matches!(chars.peek(), Some((_, '+' | '-'))) {
                        chars.next();
                    }
                    let _ = escape_name(&mut chars);
                }
                's' => {
                    // `\s+2`, `\s(12`, `\s[12]`, `\s'12'`, `\s0`.
                    if matches!(chars.peek(), Some((_, '+' | '-'))) {
                        chars.next();
                    }
                    match chars.peek().map(|&(_, c)| c) {
                        Some('(') => {
                            chars.next();
                            take_chars(&mut chars, 2);
                        }
                        Some('[') => {
                            chars.next();
                            take_until(&mut chars, ']');
                        }
                        Some('\'') => {
                            chars.next();
                            take_until(&mut chars, '\'');
                        }
                        Some(c) if c.is_ascii_digit() => {
                            chars.next();
                            if matches!(c, '1'..='3')
                                && matches!(chars.peek(), Some((_, d)) if d.is_ascii_digit())
                            {
                                chars.next();
                            }
                        }
                        _ => {}
                    }
                }
                'h' | 'v' | 'w' | 'x' | 'X' | 'N' | 'D' | 'l' | 'L' | 'b' | 'S' | 'H' | 'A'
                | 'B' | 'R' | 'Z' | 'o' => {
                    // A delimited argument: `\h'1n'`, `\w'text'`.
                    let delimiter = chars.next().map(|(_, c)| c).unwrap_or('\'');
                    let argument = take_delimited(&mut chars, delimiter, 0);
                    match next {
                        'Z' | 'o' => current.extend(argument.chars().next()),
                        'h' if !argument.starts_with('-') && !argument.is_empty() => {
                            current.push(' ')
                        }
                        _ => {}
                    }
                }
                '-' => current.push('-'),
                'e' | 'E' | '\\' => current.push('\\'),
                '&' | '|' | '^' | '%' | ':' | '/' | ',' | ')' | '{' | '}' | 'a' | 'd' | 'u'
                | 'r' | 'p' | 'z' | '!' => {}
                ' ' | '0' | '~' => current.push(' '),
                't' => current.push(' '),
                '\'' => current.push('\u{b4}'),
                '`' => current.push('`'),
                '.' => current.push('.'),
                '"' | '#' => break,
                'c' => {
                    self.saw_continuation = true;
                    break;
                }
                '?' => {
                    take_until(&mut chars, '\\');
                    chars.next();
                }
                other => current.push(other),
            }
        }
    }
}

/// The `NAME(SECTION)` a man page's header shows.
fn page_name(title: &Title) -> String {
    match (title.name.is_empty(), title.section.is_empty()) {
        (true, _) => String::new(),
        (false, true) => title.name.clone(),
        (false, false) => format!("{}({})", title.name, title.section),
    }
}

/// The manual a section number belongs to (for mdoc, which does not name it).
fn section_manual(section: &str) -> &'static str {
    match section.chars().next() {
        Some('1') => "General Commands Manual",
        Some('2') => "System Calls Manual",
        Some('3') => "Library Functions Manual",
        Some('4') => "Kernel Interfaces Manual",
        Some('5') => "File Formats Manual",
        Some('6') => "Games Manual",
        Some('7') => "Miscellaneous Information Manual",
        Some('8') => "System Manager's Manual",
        Some('9') => "Kernel Developer's Manual",
        _ => "",
    }
}

/// Lines with roff's escaped newlines joined, each with its line number.
fn join_continuations(source: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (index, line) in source.split('\n').enumerate() {
        // A backslash at the end of a comment is part of the comment.
        let continued = ends_with_escape(line) && strip_comment(line).len() == line.len();
        let text = if continued {
            &line[..line.len() - 1]
        } else {
            line
        };
        match pending.as_mut() {
            Some((_, joined)) => joined.push_str(text),
            None => pending = Some((index + 1, text.to_string())),
        }
        if !continued {
            out.extend(pending.take());
        }
    }
    out.extend(pending);
    out
}

/// Whether `line` ends with an unescaped backslash.
fn ends_with_escape(line: &str) -> bool {
    let run = line.bytes().rev().take_while(|&b| b == b'\\').count();
    run % 2 == 1
}

/// A request's arguments without a trailing `\"` comment.
fn strip_comment(arguments: &str) -> &str {
    let bytes = arguments.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if bytes.get(i + 1) == Some(&b'"') {
                return &arguments[..i];
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    arguments
}

/// Split macro arguments: by spaces, with `"double quoted"` arguments (and
/// `""` for a quote inside them); `\ ` does not split.
fn split_args(arguments: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = arguments.chars().peekable();
    loop {
        while matches!(chars.peek(), Some(' ' | '\t')) {
            chars.next();
        }
        let Some(&first) = chars.peek() else {
            break;
        };
        let mut argument = String::new();
        if first == '"' {
            chars.next();
            while let Some(ch) = chars.next() {
                if ch == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        argument.push('"');
                        continue;
                    }
                    break;
                }
                if ch == '\\' {
                    argument.push(ch);
                    if let Some(next) = chars.next() {
                        argument.push(next);
                    }
                    continue;
                }
                argument.push(ch);
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch == ' ' || ch == '\t' {
                    break;
                }
                chars.next();
                argument.push(ch);
                if ch == '\\' {
                    if let Some(next) = chars.next() {
                        argument.push(next);
                    }
                }
            }
        }
        out.push(argument);
        if out.len() > 4096 {
            break;
        }
    }
    out
}

type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

/// The name after `\f`, `\*` and the like: `(xx`, `[name]` or one character.
fn escape_name(chars: &mut Chars<'_>) -> String {
    match chars.next() {
        Some((_, '(')) => take_chars(chars, 2),
        Some((_, '[')) => take_until(chars, ']'),
        Some((_, c)) => c.to_string(),
        None => String::new(),
    }
}

fn take_chars(chars: &mut Chars<'_>, count: usize) -> String {
    let mut out = String::new();
    for _ in 0..count {
        match chars.next() {
            Some((_, c)) => out.push(c),
            None => break,
        }
    }
    out
}

fn take_until(chars: &mut Chars<'_>, end: char) -> String {
    let mut out = String::new();
    for (_, c) in chars.by_ref() {
        if c == end || out.len() > 256 {
            break;
        }
        out.push(c);
    }
    out
}

/// A delimited escape argument (`'1n'` after `\h`), skipping escapes
/// inside it, which may nest their own delimited arguments.
fn take_delimited(chars: &mut Chars<'_>, delimiter: char, depth: usize) -> String {
    let mut out = String::new();
    while let Some((_, c)) = chars.next() {
        if c == delimiter || out.len() > 4096 {
            break;
        }
        if c == '\\' {
            let Some((_, escape)) = chars.next() else {
                break;
            };
            out.push('\\');
            out.push(escape);
            if depth < 8 && "hvwxXNDlLbSHABRZo".contains(escape) {
                if let Some((_, inner)) = chars.next() {
                    out.push(inner);
                    out.push_str(&take_delimited(chars, inner, depth + 1));
                    out.push(inner);
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// How many more `\{` than `\}` a line opens.
fn braces(line: &str) -> i64 {
    line.matches("\\{").count() as i64 - line.matches("\\}").count() as i64
}

/// Evaluate a conditional as nroff would, returning the result and the
/// body after the condition.
fn condition<'a>(arguments: &'a str, strings: &HashMap<String, String>) -> (bool, &'a str) {
    let arguments = arguments.trim_start();
    let (negate, rest) = match arguments.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, arguments),
    };
    let Some(first) = rest.chars().next() else {
        return (false, "");
    };
    let (value, body) = match first {
        'n' => (true, &rest[1..]),
        't' | 'v' => (false, &rest[1..]),
        'o' => (true, &rest[1..]),
        'e' => (false, &rest[1..]),
        'd' | 'r' | 'c' | 'm' | 'F' | 'S' => {
            let after = rest[1..].trim_start();
            let end = after.find([' ', '\t']).unwrap_or(after.len());
            let name = &after[..end];
            let defined = first == 'd' && strings.contains_key(name);
            (defined, &after[end..])
        }
        '\'' | '"' | '|' | '/' => {
            // `'a'b'`: equal strings.
            let mut parts = rest[1..].splitn(3, first);
            let a = parts.next().unwrap_or("");
            let b = parts.next().unwrap_or("");
            let used = 1 + a.len() + 1 + b.len() + 1;
            (a == b, rest.get(used.min(rest.len())..).unwrap_or(""))
        }
        _ => {
            // A numeric expression: `\n(.g`, `1`, `(\n(.x>1)`.
            let end = rest.find([' ', '\t']).unwrap_or(rest.len());
            let expression = &rest[..end];
            let value = match expression {
                "\\n(.g" | "\\n[.g]" | "1" => true,
                _ => expression.parse::<i64>().is_ok_and(|v| v > 0),
            };
            (value, &rest[end..])
        }
    };
    (value != negate, body)
}

/// The text of pieces, without fonts.
fn plain(pieces: &[Piece]) -> String {
    let mut out = String::new();
    for piece in pieces {
        match piece {
            Piece::Run(_, text) => out.push_str(text),
            Piece::Break => out.push('\n'),
            _ => {}
        }
    }
    out.replace('\u{0}', "")
}

/// As [`to_inlines`], keeping leading spaces (no-fill lines).
fn to_inlines_keep(pieces: &[Piece]) -> Vec<Inline> {
    let leading: String = plain(pieces).chars().take_while(|c| *c == ' ').collect();
    let mut inlines = to_inlines(pieces);
    if !leading.is_empty() {
        inlines.insert(0, Inline::Text(leading));
    }
    inlines
}

/// Append words, with a space between them unless the next one closes
/// (`.`, `)`) or the last one opens (`(`), or `\0` asked for none (`.Ns`).
fn append_words(out: &mut Vec<Piece>, pieces: Vec<Piece>) {
    let first_text = pieces.iter().find_map(|piece| match piece {
        Piece::Run(_, text) => Some(text.clone()),
        _ => None,
    });
    let Some(first_text) = first_text else {
        out.extend(pieces);
        return;
    };
    let no_space_marker = first_text.starts_with('\u{0}');
    let last_text = out.iter().rev().find_map(|piece| match piece {
        Piece::Run(_, text) => Some(text.clone()),
        _ => None,
    });
    let needs_space = match &last_text {
        None => false,
        Some(last) => {
            !no_space_marker
                && !last.ends_with(' ')
                && !last.ends_with('\u{0}')
                && !is_closing(first_text.trim())
                && !is_opening(last.trim())
                && !last.is_empty()
        }
    };
    if needs_space {
        out.push(Piece::Run(Font::Roman, " ".into()));
    }
    for piece in pieces {
        match piece {
            Piece::Run(font, text) => {
                let text = text.replace('\u{0}', "");
                if !text.is_empty() {
                    out.push(Piece::Run(font, text));
                }
            }
            other => out.push(other),
        }
    }
}

/// mdoc's closing delimiters, which attach to the word before them.
fn is_closing(word: &str) -> bool {
    matches!(word, "." | "," | ":" | ";" | ")" | "]" | "?" | "!")
}

fn is_opening(word: &str) -> bool {
    matches!(word, "(" | "[")
}

/// mdoc macros that may be called from another macro's line.
fn is_callable(word: &str) -> bool {
    matches!(
        word,
        "Ad" | "An"
            | "Ap"
            | "Ar"
            | "At"
            | "Bc"
            | "Bo"
            | "Bq"
            | "Brq"
            | "Bsx"
            | "Bx"
            | "Cm"
            | "Dc"
            | "Do"
            | "Dq"
            | "Dv"
            | "Em"
            | "Er"
            | "Ev"
            | "Fa"
            | "Fl"
            | "Fn"
            | "Ft"
            | "Fx"
            | "Ic"
            | "In"
            | "Li"
            | "Lk"
            | "Ms"
            | "Mt"
            | "Nm"
            | "No"
            | "Ns"
            | "Nx"
            | "Oc"
            | "Oo"
            | "Op"
            | "Ox"
            | "Pa"
            | "Pc"
            | "Pf"
            | "Po"
            | "Pq"
            | "Qc"
            | "Ql"
            | "Qo"
            | "Qq"
            | "Sc"
            | "So"
            | "Sq"
            | "St"
            | "Sx"
            | "Sy"
            | "Ta"
            | "Tn"
            | "Ux"
            | "Va"
            | "Vt"
            | "Xr"
            | "Fd"
            | "Lb"
            | "Aq"
    )
}

/// An mdoc `-column` row: cells divided by `Ta` (now tabs).
fn split_columns(pieces: &[Piece]) -> Vec<Vec<Inline>> {
    let mut cells = vec![Vec::new()];
    for piece in pieces {
        match piece {
            Piece::Run(font, text) if text.contains('\t') => {
                let mut parts = text.split('\t');
                if let Some(first) = parts.next() {
                    cells
                        .last_mut()
                        .expect("one cell")
                        .push(Piece::Run(*font, first.to_string()));
                }
                for part in parts {
                    cells.push(vec![Piece::Run(*font, part.to_string())]);
                }
            }
            other => cells.last_mut().expect("one cell").push(other.clone()),
        }
    }
    cells.iter().map(|cell| to_inlines(cell)).collect()
}

/// The predefined strings (`\*(lq`, `\*R`, …).
fn predefined_string(name: &str) -> Option<&'static str> {
    Some(match name {
        "lq" | "Lq" => "\u{201c}",
        "rq" | "Rq" => "\u{201d}",
        "R" => "\u{ae}",
        "Tm" => "\u{2122}",
        "S" | "HF" => "",
        "aq" => "'",
        "dq" => "\"",
        _ => return None,
    })
}

/// A roff special character by name (`em`, `bu`, `u2014`, `char65`).
fn special(name: &str) -> String {
    if let Some(hex) = name.strip_prefix('u') {
        let first = hex.split('_').next().unwrap_or(hex);
        if let Some(c) = u32::from_str_radix(first, 16).ok().and_then(char::from_u32) {
            return c.to_string();
        }
    }
    if let Some(decimal) = name.strip_prefix("char") {
        if let Some(c) = decimal.parse::<u32>().ok().and_then(char::from_u32) {
            return c.to_string();
        }
    }
    SPECIALS
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_string())
        .unwrap_or_default()
}

/// groff's special characters (groff_char(7)), the common ones.
const SPECIALS: &[(&str, &str)] = &[
    ("em", "\u{2014}"),
    ("en", "\u{2013}"),
    ("hy", "-"),
    ("mi", "\u{2212}"),
    ("bu", "\u{2022}"),
    ("ci", "\u{25cb}"),
    ("sq", "\u{25a1}"),
    ("lq", "\u{201c}"),
    ("rq", "\u{201d}"),
    ("oq", "\u{2018}"),
    ("cq", "\u{2019}"),
    ("aq", "'"),
    ("dq", "\""),
    ("Fo", "\u{ab}"),
    ("Fc", "\u{bb}"),
    ("fo", "\u{2039}"),
    ("fc", "\u{203a}"),
    ("co", "\u{a9}"),
    ("rg", "\u{ae}"),
    ("tm", "\u{2122}"),
    ("de", "\u{b0}"),
    ("dg", "\u{2020}"),
    ("dd", "\u{2021}"),
    ("sc", "\u{a7}"),
    ("ps", "\u{b6}"),
    ("ct", "\u{a2}"),
    ("Po", "\u{a3}"),
    ("Ye", "\u{a5}"),
    ("eu", "\u{20ac}"),
    ("Eu", "\u{20ac}"),
    ("mu", "\u{d7}"),
    ("di", "\u{f7}"),
    ("+-", "\u{b1}"),
    ("<=", "\u{2264}"),
    (">=", "\u{2265}"),
    ("!=", "\u{2260}"),
    ("==", "\u{2261}"),
    ("~=", "\u{2245}"),
    ("ap", "\u{223c}"),
    ("->", "\u{2192}"),
    ("<-", "\u{2190}"),
    ("<>", "\u{2194}"),
    ("=>", "\u{21d2}"),
    ("<=>", "\u{21d4}"),
    ("ua", "\u{2191}"),
    ("da", "\u{2193}"),
    ("rA", "\u{21d2}"),
    ("lA", "\u{21d0}"),
    ("if", "\u{221e}"),
    ("pl", "+"),
    ("eq", "="),
    ("**", "*"),
    ("sl", "/"),
    ("rs", "\\"),
    ("ba", "|"),
    ("bv", "|"),
    ("br", "\u{2502}"),
    ("ul", "_"),
    ("ru", "_"),
    ("at", "@"),
    ("sh", "#"),
    ("Do", "$"),
    ("ti", "~"),
    ("ha", "^"),
    ("ga", "`"),
    ("aa", "\u{b4}"),
    ("lB", "["),
    ("rB", "]"),
    ("lC", "{"),
    ("rC", "}"),
    ("la", "\u{27e8}"),
    ("ra", "\u{27e9}"),
    ("fm", "\u{2032}"),
    ("sd", "\u{2033}"),
    ("12", "\u{bd}"),
    ("14", "\u{bc}"),
    ("34", "\u{be}"),
    ("ss", "\u{df}"),
    ("ae", "\u{e6}"),
    ("AE", "\u{c6}"),
    ("oe", "\u{153}"),
    ("OE", "\u{152}"),
    ("/o", "\u{f8}"),
    ("/O", "\u{d8}"),
    ("'a", "\u{e1}"),
    ("'e", "\u{e9}"),
    ("'i", "\u{ed}"),
    ("'o", "\u{f3}"),
    ("'u", "\u{fa}"),
    ("'E", "\u{c9}"),
    ("`a", "\u{e0}"),
    ("`e", "\u{e8}"),
    (":a", "\u{e4}"),
    (":o", "\u{f6}"),
    (":u", "\u{fc}"),
    (":A", "\u{c4}"),
    (":O", "\u{d6}"),
    (":U", "\u{dc}"),
    ("~n", "\u{f1}"),
    ("~N", "\u{d1}"),
    (",c", "\u{e7}"),
    (",C", "\u{c7}"),
    ("^a", "\u{e2}"),
    ("^e", "\u{ea}"),
    ("^o", "\u{f4}"),
    ("oa", "\u{e5}"),
    ("*a", "\u{3b1}"),
    ("*b", "\u{3b2}"),
    ("*g", "\u{3b3}"),
    ("*d", "\u{3b4}"),
    ("*e", "\u{3b5}"),
    ("*l", "\u{3bb}"),
    ("*m", "\u{3bc}"),
    ("*p", "\u{3c0}"),
    ("*s", "\u{3c3}"),
    ("*t", "\u{3c4}"),
    ("*f", "\u{3c6}"),
    ("*w", "\u{3c9}"),
    ("tno", "~"),
    ("rn", "\u{203e}"),
    ("OK", "\u{2713}"),
    ("ho", "\u{2c7}"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::inlines_text;

    fn blocks(source: &str) -> Vec<Block> {
        parse(source).document.blocks
    }

    const LS: &str = r#".\" A comment
.TH LS 1 "March 2024" "GNU coreutils 9.4" "User Commands"
.SH NAME
ls \- list directory contents
.SH SYNOPSIS
.B ls
[\fI\,OPTION\/\fR]... [\fI\,FILE\/\fR]...
.SH DESCRIPTION
List information about the FILEs.
.TP
\fB\-a\fR, \fB\-\-all\fR
do not ignore entries starting with .
.TP
.BR \-A ", " \-\-almost\-all
do not list implied . and ..
.SH "SEE ALSO"
Full documentation
.UR https://www.gnu.org/software/coreutils/ls
.UE
"#;

    #[test]
    fn string_expansion_is_bounded() {
        // Each string refers to the one before 16 times: 16^5 copies of "x"
        // unbounded, plus a string that refers to itself.
        let mut source = String::from(".ds a xxxxxxxx\n.ds s \\*s\\*s\\*s\n");
        for (name, previous) in [("b", "a"), ("c", "b"), ("d", "c"), ("e", "d"), ("f", "e")] {
            source.push_str(&format!(
                ".ds {name} {}\n",
                format!("\\*{previous}").repeat(16)
            ));
        }
        source.push_str("\\*f \\*s\n");
        let text: usize = blocks(&source)
            .iter()
            .map(|block| match block {
                Block::Paragraph(inlines) => inlines_text(inlines).len(),
                _ => 0,
            })
            .sum();
        assert!(text <= EXPANSION_BUDGET + 64, "{text}");
    }

    #[test]
    fn a_man_page_has_its_banner_headings_and_options() {
        let blocks = blocks(LS);
        assert!(
            matches!(&blocks[0], Block::Banner { left, center, .. } if left == "LS(1)" && center == "User Commands")
        );
        assert!(
            matches!(&blocks[1], Block::Heading(h) if inlines_text(&h.content) == "NAME" && h.level == 2)
        );
        assert_eq!(
            blocks[2],
            Block::Paragraph(vec![Inline::text("ls - list directory contents")])
        );
        let definitions = blocks
            .iter()
            .find_map(|block| match block {
                Block::Definitions(definitions) => Some(definitions),
                _ => None,
            })
            .expect("the options");
        assert_eq!(definitions.len(), 2);
        assert_eq!(inlines_text(&definitions[0].term), "-a, --all");
        assert_eq!(inlines_text(&definitions[1].term), "-A, --almost-all");
        assert!(matches!(&definitions[1].term[0], Inline::Strong(_)));
        assert!(matches!(blocks.last(), Some(Block::Banner { .. })));
        let text = Document {
            blocks: blocks.clone(),
        }
        .plain_text();
        assert!(
            text.contains("https://www.gnu.org/software/coreutils/ls"),
            "{text}"
        );
    }

    #[test]
    fn indents_lists_and_examples() {
        let blocks = blocks(
            ".IP \\(bu 2\nOne\n.IP \\(bu 2\nTwo\n.PP\n.RS\nIndented\n.RE\n.EX\n$ ls -l\n.EE",
        );
        assert!(matches!(&blocks[0], Block::List(l) if l.items.len() == 2));
        assert!(matches!(&blocks[1], Block::Indent { .. }));
        assert!(matches!(&blocks[2], Block::Code(code) if code.code == "$ ls -l"));
    }

    #[test]
    fn conditionals_and_strings() {
        let blocks = blocks(
            ".ds Pr shown\n.ie n \\{\\\nnroff \\*(Pr\n.\\}\n.el \\{\\\ntroff\n.\\}\n.if t hidden",
        );
        let text = Document {
            blocks: blocks.clone(),
        }
        .plain_text();
        assert_eq!(text, "nroff shown");
    }

    #[test]
    fn mdoc_pages() {
        let source = ".Dd May 1, 2024\n.Dt CAT 1\n.Os\n.Sh NAME\n.Nm cat\n.Nd concatenate files\n.Sh SYNOPSIS\n.Nm\n.Op Fl benstuv\n.Op Ar\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Fl b\nNumber the lines.\n.El\nSee\n.Xr ls 1 .";
        let blocks = blocks(source);
        let text = Document {
            blocks: blocks.clone(),
        }
        .plain_text();
        assert!(text.contains("cat \u{2014} concatenate files"), "{text}");
        assert!(text.contains("cat [-benstuv] [file ...]"), "{text}");
        assert!(text.contains("See ls(1)."), "{text}");
        assert!(blocks.iter().any(
            |block| matches!(block, Block::Definitions(d) if inlines_text(&d[0].term) == "-b")
        ));
    }

    #[test]
    fn tables() {
        let blocks = blocks(".TS\ntab(;);\nlb lb\nl l.\nName;Size\n_\na;1\nb;2\n.TE");
        let Block::Table(table) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert!(table.header.is_some());
        assert_eq!(table.rows.len(), 2);
    }

    #[test]
    fn sniffing() {
        assert!(sniff(LS) >= 0.9);
        assert!(sniff(".Dd May 1, 2024\n.Dt CAT 1\n.Sh NAME\n") >= 0.9);
        assert!(sniff("int main(void) {\n  printf(\".TH x\");\n}\n") < 0.1);
        assert!(sniff(".PHONY: all\nall:\n\tcc x.c\n") < 0.1);
        assert_eq!(sniff("#!/bin/sh\n.TH X 1\n"), 0.0);
    }
}
