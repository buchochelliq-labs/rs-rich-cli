//! `rich-rst` 2.2.0's `RSTVisitor` and `RestructuredText`: the renderables
//! each node of the document tree becomes, and how they print.
//!
//! The visitor keeps a flat list of renderables. Inline markup appends to the
//! last one while it is a `Text`, so text runs on across paragraphs when
//! nothing else stands between them; each `Text` carries upstream's `end`
//! (core's `Text::end`), which the list prints after it.

use std::sync::Arc;

use fancy_regex::Regex;
use rich::r#box::{DOUBLE, ROUNDED, SQUARE};
use rich::{
    Align, Cell, Console, ConsoleOptions, Panel, Renderable, Rule, Segment, Style, StyleType,
    Styled, Syntax, Table, Text,
};

use super::doctree::{Element, Node};
use super::entities::ENTITIES;
use super::parse::{parse, roman};

/// A reStructuredText document, rendered as `rich-rst` 2.2.0 renders it.
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
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct RestructuredText {
    document: Element,
    code_theme: Option<String>,
    default_lexer: String,
}

impl RestructuredText {
    /// Parse `markup`. Code blocks use `Syntax`'s default theme (`monokai`,
    /// or the console's code highlighting) and the `python` lexer by
    /// default, as upstream's do.
    pub fn new(markup: &str) -> Self {
        // A `|page|` with no definition is a page number, 1.
        let document = if markup.contains("|page|") && !markup.contains(".. |page|") {
            parse(&format!("{markup}\n\n.. |page| replace:: 1\n"))
        } else {
            parse(markup)
        };
        RestructuredText {
            document,
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

    /// The parsed document tree, transforms applied.
    pub fn document(&self) -> &Element {
        &self.document
    }
}

/// A renderable the visitor builds: rich's own, described as data so a
/// table cell can hold one.
#[derive(Clone, Debug)]
enum R {
    /// A `Text`, printed with its `end`.
    Text(Text),
    /// `rich.console.NewLine`.
    NewLine,
    /// A `str`: console markup, not highlighted (every string the visitor
    /// shows sits in a panel, which renders with `highlight=False`).
    Str(String),
    Group(Vec<R>),
    Center(Box<R>),
    Left(Box<R>),
    Styled(Box<R>, Style),
    Panel(Box<PanelSpec>),
    Syntax(Box<SyntaxSpec>),
    Rule(Style),
    Table(Box<TableSpec>),
}

#[derive(Clone, Debug)]
struct PanelSpec {
    child: R,
    title: Option<String>,
    subtitle: Option<String>,
    box_set: Option<BoxSet>,
    style: Option<Style>,
    border_style: Option<Style>,
    expand: bool,
}

impl PanelSpec {
    fn new(child: R) -> PanelSpec {
        PanelSpec {
            child,
            title: None,
            subtitle: None,
            box_set: None,
            style: None,
            border_style: None,
            expand: true,
        }
    }
}

#[derive(Clone, Debug)]
struct SyntaxSpec {
    code: String,
    lexer: String,
    theme: Option<String>,
    line_numbers: bool,
    start_line: i64,
    highlight_lines: Vec<i64>,
}

#[derive(Clone, Debug)]
struct TableSpec {
    columns: Vec<String>,
    title: Option<Text>,
    show_header: bool,
    header_style: Option<Style>,
    column_style: Option<Style>,
    rows: Vec<Vec<R>>,
}

type BoxSet = rich::r#box::Box;

impl R {
    fn panel(spec: PanelSpec) -> R {
        R::Panel(Box::new(spec))
    }

    /// The rich renderable this describes.
    fn build(&self, console: &Console) -> Box<dyn Renderable + '_> {
        match self {
            R::Text(text) => Box::new(text.clone()),
            R::NewLine => Box::new(Text::new("")),
            R::Str(markup) => Box::new(console.render_str(markup, Some(false))),
            R::Group(_) | R::Center(_) | R::Left(_) | R::Styled(..) => Box::new(self.clone()),
            R::Panel(spec) => {
                let child: Box<dyn Renderable> = Box::new(spec.child.clone());
                let mut panel = Panel::new(child).expand(spec.expand);
                if let Some(title) = &spec.title {
                    panel = panel.title(title.clone());
                }
                if let Some(subtitle) = &spec.subtitle {
                    panel = panel.subtitle(subtitle.clone());
                }
                if let Some(box_set) = spec.box_set {
                    panel = panel.box_set(box_set);
                }
                if let Some(style) = &spec.style {
                    panel = panel.style(style.clone());
                }
                if let Some(style) = &spec.border_style {
                    panel = panel.border_style(style.clone());
                }
                Box::new(panel)
            }
            R::Syntax(spec) => {
                let mut syntax = Syntax::new(spec.code.clone(), spec.lexer.clone())
                    .line_numbers(spec.line_numbers)
                    .start_line(spec.start_line);
                if let Some(theme) = &spec.theme {
                    syntax = syntax.theme(theme.clone());
                }
                if !spec.highlight_lines.is_empty() {
                    syntax = syntax.highlight_lines(spec.highlight_lines.clone());
                }
                Box::new(syntax)
            }
            R::Rule(style) => Box::new(Rule::line().style(style.clone())),
            R::Table(spec) => {
                let mut table = Table::new().show_lines(true).show_header(spec.show_header);
                if let Some(title) = &spec.title {
                    table = table.title_text(title.clone());
                }
                if let Some(style) = &spec.header_style {
                    table = table.header_style(style.clone());
                }
                for column in &spec.columns {
                    table.add_column(column.clone());
                    if let Some(style) = &spec.column_style {
                        table.column_style(style.clone());
                    }
                }
                for row in &spec.rows {
                    let cells = row
                        .iter()
                        .map(|cell| match cell {
                            R::Text(text) => Cell::Text(text.clone()),
                            other => Cell::Renderable(Arc::new(other.clone())),
                        })
                        .collect();
                    table.add_row_cells(cells);
                }
                Box::new(table)
            }
        }
    }

    /// Its segments as `console.render` gives them, then its `end`: a
    /// `Text`'s own, a line break after anything else but a `NewLine`'s.
    fn render_into(&self, console: &Console, options: &ConsoleOptions, out: &mut Vec<Segment>) {
        match self {
            R::NewLine => out.push(Segment::line()),
            R::Text(text) => {
                out.extend(console.render(text, Some(options)));
                push_end(out, text.get_end());
            }
            R::Group(children) => {
                for child in children {
                    child.render_into(console, options, out);
                }
            }
            other => {
                let built = other.build(console);
                out.extend(console.render(built.as_ref(), Some(options)));
                out.push(Segment::line());
            }
        }
    }
}

fn push_end(out: &mut Vec<Segment>, end: &str) {
    let mut parts = end.split('\n').peekable();
    while let Some(part) = parts.next() {
        if !part.is_empty() {
            out.push(Segment::new(part, None));
        }
        if parts.peek().is_some() {
            out.push(Segment::line());
        }
    }
}

/// Lines in this port are separated, not ended: a renderable's last line
/// break is the container's.
fn trim_last_line(mut segments: Vec<Segment>) -> Vec<Segment> {
    if segments.last().is_some_and(|segment| segment.text == "\n") {
        segments.pop();
    }
    segments
}

impl Renderable for R {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        match self {
            R::Group(_) | R::NewLine => {
                let mut out = Vec::new();
                self.render_into(console, options, &mut out);
                trim_last_line(out)
            }
            R::Text(text) => {
                let mut out = console.render(text, Some(options));
                // A text's own line breaks show; its last is the container's.
                let end = text.get_end();
                push_end(&mut out, end);
                trim_last_line(out)
            }
            R::Center(child) | R::Left(child) => {
                let inner: Box<dyn Renderable> = Box::new(child.as_ref().clone());
                let align = if matches!(self, R::Center(_)) {
                    Align::center(inner)
                } else {
                    Align::left(inner)
                };
                console.render(&align, Some(options))
            }
            R::Styled(child, style) => {
                let inner: Box<dyn Renderable> = Box::new(child.as_ref().clone());
                console.render(&Styled::new(inner, style.clone()), Some(options))
            }
            other => {
                let built = other.build(console);
                console.render(built.as_ref(), Some(options))
            }
        }
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        match self {
            R::Group(children) => {
                let mut minimum = 0;
                let mut maximum = 0;
                for child in children {
                    let m = rich::measure::Measurement::get(console, options, child);
                    minimum = minimum.max(m.minimum);
                    maximum = maximum.max(m.maximum);
                }
                rich::measure::Measurement::new(minimum, maximum)
            }
            R::NewLine => rich::measure::Measurement::new(0, 0),
            R::Center(child) | R::Left(child) | R::Styled(child, _) => {
                rich::measure::Measurement::get(console, options, child.as_ref())
            }
            R::Text(text) => rich::measure::Measurement::get(console, options, text),
            other => {
                let built = other.build(console);
                rich::measure::Measurement::get(console, options, built.as_ref())
            }
        }
    }
}

/// An ancestor of the node being visited: what the visitor asks of
/// `node.parent`.
#[derive(Clone, Copy, Debug)]
struct Ancestor {
    tag: &'static str,
    /// A `contents` topic: its lists are a table of contents.
    contents: bool,
}

fn ancestor(element: &Element) -> Ancestor {
    Ancestor {
        tag: element.tag,
        contents: element.tag == "topic" && element.classes.iter().any(|c| c == "contents"),
    }
}

/// One field table: `Table("Field Name", "Field Value", show_lines=True)`,
/// kept as rows so the next field can add one.
#[derive(Clone, Debug)]
struct Fields {
    title: Option<Text>,
    rows: Vec<(Text, R)>,
}

/// One renderable in the visitor's list.
#[derive(Clone, Debug)]
enum Item {
    Text(Text),
    NewLine,
    Fields(Fields),
    Other(R),
}

impl Item {
    fn into_r(self) -> R {
        match self {
            Item::Text(text) => R::Text(text),
            Item::NewLine => R::NewLine,
            Item::Fields(fields) => fields_table(fields),
            Item::Other(r) => r,
        }
    }
}

fn fields_table(fields: Fields) -> R {
    R::Table(Box::new(TableSpec {
        columns: vec!["Field Name".into(), "Field Value".into()],
        title: fields.title,
        show_header: true,
        header_style: None,
        column_style: None,
        rows: fields
            .rows
            .into_iter()
            .map(|(name, value)| vec![R::Text(name), value])
            .collect(),
    }))
}

struct Visitor<'a> {
    console: &'a Console,
    code_theme: Option<&'a str>,
    default_lexer: &'a str,
    items: Vec<Item>,
    footer: Vec<R>,
    citations: Vec<R>,
    in_docinfo: bool,
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

fn text_end(plain: &str, style: Style, end: &str) -> Text {
    Text::styled(plain, style).end(end)
}

/// `Text.copy()` with a new `end`.
fn with_end(text: &Text, end: &str) -> Text {
    text.clone().end(end)
}

/// `a + b`: `a`'s copy with `b` appended (keeping `a`'s `end`).
fn concat(a: &Text, b: &Text) -> Text {
    let end = a.get_end().to_string();
    a.clone().append_text(b).end(end)
}

/// Strip the trailing whitespace of a text, keeping its `end`.
fn rstrip(text: &mut Text) {
    let end = text.get_end().to_string();
    text.rstrip();
    *text = std::mem::take(text).end(end);
}

/// Append `piece` to `text` (keeping `text`'s `end`).
fn append_text(text: &mut Text, piece: &Text) {
    let end = text.get_end().to_string();
    *text = std::mem::take(text).append_text(piece).end(end);
}

fn append(text: &mut Text, plain: &str, style: Option<Style>) {
    text.append(plain, style.map(StyleType::from));
}

const BULLET_MARKERS: [&str; 3] = [" • ", " ∘ ", " ▪ "];

/// `_merge_bullet_markers_with_text`: a bullet marker on its own joins the
/// text after it.
fn merge_bullet_markers(items: Vec<Item>) -> Vec<Item> {
    let mut merged = Vec::new();
    let mut iter = items.into_iter().peekable();
    while let Some(item) = iter.next() {
        if let Item::Text(marker) = &item {
            if ["•", "∘", "▪"].contains(&marker.plain().trim()) {
                if let Some(Item::Text(next)) = iter.peek() {
                    let mut combined = Text::new("");
                    append_text(&mut combined, marker);
                    append_text(&mut combined, next);
                    iter.next();
                    merged.push(Item::Text(combined));
                    continue;
                }
            }
        }
        merged.push(item);
    }
    merged
}

/// `_clean_body_for_panel`: no trailing blank text or line breaks.
fn clean_body(mut body: Vec<Item>) -> Vec<Item> {
    while let Some(last) = body.last_mut() {
        match last {
            Item::Text(text) => {
                rstrip(text);
                if !text.plain().is_empty() {
                    break;
                }
                body.pop();
            }
            Item::NewLine => {
                body.pop();
            }
            _ => break,
        }
    }
    body
}

fn group(items: Vec<Item>) -> R {
    R::Group(items.into_iter().map(Item::into_r).collect())
}

/// `Group(*body) if body else ""`.
fn group_or_empty(items: Vec<Item>) -> R {
    if items.is_empty() {
        R::Str(String::new())
    } else {
        group(items)
    }
}

/// `_format_labelled_node`: a footnote or citation as `label: body`.
fn labelled(node: &Element) -> String {
    let label = node
        .find("label")
        .map(|label| label.astext().replace('\n', " ").trim().to_string())
        .unwrap_or_default();
    let body: Vec<String> = node
        .children
        .iter()
        .filter(|child| !child.is("label"))
        .map(|child| child.astext().replace('\n', " ").trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    let body = body.join(" ");
    let body = body.trim();
    match (label.is_empty(), body.is_empty()) {
        (false, false) => format!("{label}: {body}"),
        (false, true) => format!("{label}:"),
        _ => node.astext().replace('\n', " ").trim().to_string(),
    }
}

impl<'a> Visitor<'a> {
    fn new(console: &'a Console, code_theme: Option<&'a str>, default_lexer: &'a str) -> Self {
        Visitor {
            console,
            code_theme,
            default_lexer,
            items: Vec::new(),
            footer: Vec::new(),
            citations: Vec::new(),
            in_docinfo: false,
        }
    }

    /// `_make_sub_visitor`.
    fn sub(&self) -> Visitor<'a> {
        Visitor::new(self.console, self.code_theme, self.default_lexer)
    }

    /// `console.get_style("restructuredtext.<name>", default=default)`.
    fn style(&self, name: &str, default: &str) -> Style {
        self.console
            .get_style(&StyleType::Name(format!("restructuredtext.{name}")))
            .or_else(|_| self.console.get_style(&StyleType::Name(default.into())))
            .unwrap_or_default()
    }

    fn last_text(&mut self) -> Option<&mut Text> {
        match self.items.last_mut() {
            Some(Item::Text(text)) => Some(text),
            _ => None,
        }
    }

    fn push_text(&mut self, text: Text) {
        self.items.push(Item::Text(text));
    }

    fn push(&mut self, r: R) {
        self.items.push(Item::Other(r));
    }

    /// `_append_inline_text`: onto the last `Text`, or a new one ending
    /// with nothing.
    fn append_inline(&mut self, text: &str, style: Style) {
        if let Some(last) = self.last_text() {
            append_text(last, &Text::styled(text, style));
        } else {
            self.push_text(text_end(text, style, ""));
        }
    }

    /// `node.walkabout(visitor)`.
    fn walk(&mut self, node: &Node, ancestors: &mut Vec<Ancestor>) {
        match node {
            Node::Text(text) => self.visit_text(text),
            Node::Element(element) => {
                let skip = self.visit(element, ancestors);
                if !skip {
                    ancestors.push(ancestor(element));
                    for child in &element.children {
                        self.walk(child, ancestors);
                    }
                    ancestors.pop();
                }
                self.depart(element, ancestors);
            }
        }
    }

    /// Walk `children` of `parent` with a fresh visitor: its renderables.
    fn render_children(
        &self,
        children: &[Node],
        parent: &Element,
        ancestors: &[Ancestor],
    ) -> Vec<Item> {
        let mut sub = self.sub();
        let mut chain = ancestors.to_vec();
        chain.push(ancestor(parent));
        for child in children {
            sub.walk(child, &mut chain);
        }
        sub.items
    }

    /// `_render_child_inline`: one child, walked by a fresh visitor.
    fn render_child(&self, child: &Node, parent: &Element, ancestors: &[Ancestor]) -> Vec<Item> {
        self.render_children(std::slice::from_ref(child), parent, ancestors)
    }

    fn visit_text(&mut self, text: &str) {
        let style = self.style("text", "default not bold not italic not underline");
        let text = text.replace('\n', " ");
        if let Some(last) = self.last_text() {
            append_text(last, &Text::styled(text, style));
            return;
        }
        self.push_text(text_end(&text, style, ""));
    }

    /// Visit `element`; whether its children are skipped.
    #[allow(clippy::too_many_lines)]
    fn visit(&mut self, element: &Element, ancestors: &[Ancestor]) -> bool {
        match element.tag {
            "reference" => {
                if matches!(element.children.as_slice(), [Node::Element(image)] if image.tag == "image")
                {
                    return false;
                }
                let mut style = self.style("reference", "blue underline on default");
                if let Some(uri) = element.get("refuri").filter(|uri| !uri.is_empty()) {
                    style = style.update_link(Some(uri.to_string()));
                }
                let text = element.astext().replace('\n', " ");
                self.append_inline(&text, style);
            }
            "target" | "comment" | "substitution_definition" | "pending" => {}
            "paragraph" | "compound" | "container" => return false,
            "title" => {
                let level = ancestors.iter().filter(|a| a.tag == "section").count();
                self.heading(&element.astext(), level);
            }
            "subtitle" => {
                let style = self.style("subtitle", "bold");
                let mut panel = PanelSpec::new(R::Center(Box::new(R::Str(element.astext()))));
                panel.box_set = Some(ROUNDED);
                panel.style = Some(style.clone());
                panel.border_style = Some(style);
                self.push(R::panel(panel));
                self.items.push(Item::NewLine);
            }
            "rubric" => {
                let style = self.style("rubric", "italic dim");
                let mut panel = PanelSpec::new(R::Center(Box::new(R::Str(element.astext()))));
                panel.box_set = Some(ROUNDED);
                panel.style = Some(style.clone());
                panel.border_style = Some(style);
                self.push(R::panel(panel));
            }
            "inline" => {
                let name = match element.classes.first() {
                    Some(class) => format!("inline.{class}"),
                    None => "inline".into(),
                };
                let style = self.style(&name, "none");
                self.append_inline(&element.astext().replace('\n', " "), style);
            }
            "admonition" => {
                let (title, body) = match element.children.first() {
                    Some(Node::Element(title)) if title.tag == "title" => {
                        (title.astext(), &element.children[1..])
                    }
                    _ => ("Admonition".to_string(), &element.children[..]),
                };
                self.admonition(&title, "admonition", "bold white", body, element, ancestors);
            }
            "attention" | "caution" | "danger" | "error" | "hint" | "important" | "note"
            | "tip" | "warning" | "seealso" => {
                let (title, default) = match element.tag {
                    "attention" => ("Attention", "bold black on yellow"),
                    "caution" => ("Caution", "red"),
                    "danger" => ("DANGER", "bold white on red"),
                    "error" => ("ERROR", "bold red"),
                    "hint" => ("Hint", "yellow"),
                    "important" => ("IMPORTANT", "bold blue"),
                    "note" => ("Note", "bold white"),
                    "tip" => ("Tip", "bold green"),
                    "warning" => ("Warning", "bold yellow"),
                    _ => ("See Also", "bold white"),
                };
                self.admonition(
                    title,
                    element.tag,
                    default,
                    &element.children,
                    element,
                    ancestors,
                );
            }
            "versionmodified" => {
                let kind = element.get("type").unwrap_or("versionadded");
                let version = element.get("version").unwrap_or("");
                let (name, default) = match kind {
                    "versionchanged" => ("versionchanged", "bold cyan"),
                    "deprecated" => ("deprecated", "bold yellow"),
                    _ => ("versionadded", "bold green"),
                };
                let title = match kind {
                    "versionchanged" => format!("Changed in version {version}"),
                    "deprecated" => format!("Deprecated since version {version}"),
                    "versionadded" => format!("New in version {version}"),
                    other => format!("{other} {version}"),
                };
                let style = self.style(name, default);
                let body = clean_body(self.render_children(&element.children, element, ancestors));
                let mut panel = PanelSpec::new(group_or_empty(body));
                panel.title = Some(title);
                panel.border_style = Some(style);
                self.push(R::panel(panel));
            }
            "subscript" | "superscript" => {
                let (table, name) = if element.tag == "subscript" {
                    (SUBSCRIPT, "subscript")
                } else {
                    (SUPERSCRIPT, "superscript")
                };
                let style = self.style(name, "none");
                self.append_inline(&translate(&element.astext(), table), style);
            }
            "emphasis" | "strong" | "literal" | "title_reference" => {
                let (name, default) = match element.tag {
                    "emphasis" => ("emphasis", "italic"),
                    "strong" => ("strong", "bold"),
                    "literal" => ("inline_codeblock", "grey78 on grey7"),
                    _ => ("title_reference", "italic"),
                };
                let style = self.style(name, default);
                self.append_inline(&element.astext().replace('\n', " "), style);
            }
            "abbreviation" | "acronym" => {
                let style = self.style(element.tag, "underline");
                let mut text = element.astext().replace('\n', " ");
                if let Some(explanation) = element.get("explanation").filter(|e| !e.is_empty()) {
                    text = format!("{text} ({explanation})");
                }
                self.append_inline(&text, style);
            }
            "image" => {
                let text = image_text(element, None);
                self.push_text(text);
            }
            "figure" => self.visit_figure(element),
            "bullet_list" => {
                self.bullet_list(element, 0, ancestors);
                if !ancestors.iter().any(|a| a.contents) {
                    self.items.push(Item::NewLine);
                }
            }
            "enumerated_list" => {
                self.enumerated_list(element, 0, ancestors);
                if !ancestors.iter().any(|a| a.contents) {
                    self.items.push(Item::NewLine);
                }
            }
            "literal_block" => self.literal_block(element),
            "system_message" => self.system_message(element),
            "field" => {
                let name = element
                    .elements()
                    .next()
                    .map(Element::astext)
                    .unwrap_or_default();
                let body = element.elements().nth(1).cloned().unwrap_or_default();
                let value = self.docinfo_value(&body, element, ancestors);
                self.add_field(&name, value);
            }
            "docinfo" => {
                self.in_docinfo = true;
                return false;
            }
            "author" | "organization" | "address" | "contact" | "version" | "revision"
            | "status" | "date" | "copyright" => {
                let name = match element.tag {
                    "author" => "Author",
                    "organization" => "Organization",
                    "address" => "Address",
                    "contact" => "Contact",
                    "version" => "Version",
                    "revision" => "Revision",
                    "status" => "Status",
                    "date" => "Date",
                    _ => "Copyright",
                };
                let value = self.docinfo_value(element, element, ancestors);
                self.add_field(name, value);
            }
            "authors" => {
                let mut texts: Vec<R> = Vec::new();
                for author in element.elements() {
                    texts.push(self.docinfo_value(author, element, ancestors));
                }
                // `Text("\n").join(author_texts)`.
                let mut joined = Text::new("");
                for (index, text) in texts.iter().enumerate() {
                    if index > 0 {
                        append_text(&mut joined, &Text::new("\n"));
                    }
                    if let R::Text(text) = text {
                        append_text(&mut joined, text);
                    }
                }
                self.add_field("Authors", R::Text(joined));
            }
            "definition_list" => self.definition_list(element, ancestors),
            "option_list" => self.option_list(element),
            "doctest_block" => {
                let border = self.style("literal_block_border", "grey58");
                let mut panel = PanelSpec::new(R::Syntax(Box::new(self.syntax(
                    element.astext(),
                    "pycon".into(),
                    false,
                    1,
                    Vec::new(),
                ))));
                panel.border_style = Some(border);
                panel.box_set = Some(SQUARE);
                panel.title = Some("doctest block".into());
                self.push(R::panel(panel));
            }
            "block_quote" => self.block_quote(element, ancestors),
            "line_block" => self.line_block(element, 0),
            "topic" => {
                let style = self.style("topic", "bold cyan");
                let (title, body) = match element.children.first() {
                    Some(Node::Element(title)) if title.tag == "title" => {
                        (title.astext(), &element.children[1..])
                    }
                    _ => (String::new(), &element.children[..]),
                };
                let mut items = Vec::new();
                for child in body {
                    items.extend(self.render_child(child, element, ancestors));
                }
                let items = clean_body(items);
                let mut panel = PanelSpec::new(group_or_empty(items));
                panel.title = Some(title);
                panel.style = Some(style.clone());
                panel.border_style = Some(style);
                self.push(R::panel(panel));
            }
            "sidebar" => self.sidebar(element, ancestors),
            "transition" => {
                let style = self.style("hr", "yellow");
                self.push(R::Rule(style));
            }
            "math_block" => {
                let border = self.style("literal_block_border", "grey58");
                if let Some(last) = self.last_text() {
                    rstrip(last);
                    append_text(last, &Text::new("\n"));
                }
                let title = match element.get("label") {
                    Some(label) => format!("math - {label}"),
                    None => "math".into(),
                };
                let mut panel =
                    PanelSpec::new(R::Text(Text::new(math_to_unicode(&element.astext()))));
                panel.border_style = Some(border);
                panel.box_set = Some(SQUARE);
                panel.title = Some(title);
                self.push(R::panel(panel));
            }
            "math" => {
                let style = self.style("math", "italic");
                self.append_inline(
                    &math_to_unicode(&element.astext().replace('\n', " ")),
                    style,
                );
            }
            "citation" => self
                .citations
                .push(R::Left(Box::new(R::Str(labelled(element))))),
            "citation_reference" | "substitution_reference" => {
                let style = if element.tag == "citation_reference" {
                    self.style("citation_reference", "grey74")
                } else {
                    self.style("substitution_reference", "none")
                };
                let text = element.astext().replace('\n', " ");
                if let Some(last) = self.last_text() {
                    append(last, &text, Some(style));
                } else {
                    self.push_text(text_end(&text, style, ""));
                }
            }
            "footnote_reference" => {
                let style = self.style("footnote_reference", "grey74");
                let text = format!("[{}]", element.astext().replace('\n', " "));
                if let Some(last) = self.last_text() {
                    append(last, &text, Some(style));
                } else {
                    self.push_text(text_end(&text, style, ""));
                }
            }
            "footnote" => self
                .footer
                .push(R::Left(Box::new(R::Str(labelled(element))))),
            "generated" => self
                .footer
                .push(R::Left(Box::new(R::Str(element.astext())))),
            "problematic" => {
                let style = self.style("problematic", "none");
                let text = element.astext().replace('\n', " ");
                if !text.is_empty() {
                    if let Some(last) = self.last_text() {
                        append(last, &text, Some(style));
                    } else {
                        self.push_text(text_end(&text, style, ""));
                    }
                }
            }
            "raw" => self.raw(element),
            "table" => self.table(element, ancestors),
            // A node the visitor has no method for: its children are visited.
            _ => return false,
        }
        true
    }

    fn depart(&mut self, element: &Element, ancestors: &[Ancestor]) {
        match element.tag {
            "paragraph" => {
                let in_item = ancestors.last().is_some_and(|a| a.tag == "list_item");
                if let Some(last) = self.last_text() {
                    if !last.plain().is_empty() {
                        append(last, if in_item { "\n" } else { "\n\n" }, None);
                    }
                }
            }
            "docinfo" => self.in_docinfo = false,
            _ => {}
        }
    }

    /// `_render_heading`.
    fn heading(&mut self, text: &str, level: usize) {
        const LEVELS: [(&str, &str, Option<BoxSet>); 6] = [
            ("title.level.1", "bold", Some(DOUBLE)),
            ("title.level.2", "bold", Some(ROUNDED)),
            ("title.level.3", "bold underline", None),
            ("title.level.4", "bold", None),
            ("title.level.5", "underline", None),
            ("title.level.6", "italic", None),
        ];
        let (name, default, box_set) = LEVELS[level.min(LEVELS.len() - 1)];
        let style = self.style(name, default);
        let title = R::Center(Box::new(R::Text(Text::styled(text, style.clone()))));
        match box_set {
            None => {
                self.push(title);
                self.items.push(Item::NewLine);
            }
            Some(box_set) => {
                let mut panel = PanelSpec::new(title);
                panel.box_set = Some(box_set);
                panel.style = Some(style.clone());
                panel.border_style = Some(style);
                self.push(R::panel(panel));
            }
        }
    }

    /// `_emit_admonition` in the default `panel` style.
    fn admonition(
        &mut self,
        title: &str,
        name: &str,
        default: &str,
        body: &[Node],
        parent: &Element,
        ancestors: &[Ancestor],
    ) {
        let style = self.style(name, default);
        let body = clean_body(self.render_children(body, parent, ancestors));
        let mut panel = PanelSpec::new(group_or_empty(body));
        panel.title = Some(title.to_string());
        panel.style = Some(match style.bgcolor() {
            Some(color) => Style::from_color(None, Some(color.clone())),
            None => Style::null(),
        });
        panel.border_style = Some(style);
        self.push(R::panel(panel));
    }

    fn visit_figure(&mut self, element: &Element) {
        let reference = element.find("reference");
        let image = element
            .find("image")
            .or_else(|| reference.and_then(|r| r.find("image")));
        let image_text = match image {
            Some(image) => image_text(image, reference.and_then(|r| r.get("refuri"))),
            None => Text::new("🌆 Image"),
        };
        let caption = element.find("caption").map(Element::astext);
        let legend = element
            .find("legend")
            .map(|legend| legend.astext().replace('\n', " "));
        let border = self.style("figure_border", "blue");
        let legend_style = self.style("figure_legend", "dim");
        let body = match legend {
            Some(legend) => R::Group(vec![
                R::Text(image_text),
                R::Text(Text::styled(legend, legend_style)),
            ]),
            None => R::Text(image_text),
        };
        let mut panel = PanelSpec::new(body);
        panel.title = caption;
        panel.border_style = Some(border);
        panel.expand = false;
        self.push(R::panel(panel));
    }

    /// `_render_bullet_list`.
    fn bullet_list(&mut self, list: &Element, level: usize, ancestors: &[Ancestor]) {
        let marker_style = self.style("bullet_list_marker", "bold yellow");
        let text_style = self.style("bullet_list_text", "none");
        let indent = "  ".repeat(level);
        let marker = BULLET_MARKERS[level.min(BULLET_MARKERS.len() - 1)];
        let mut chain = ancestors.to_vec();
        chain.push(ancestor(list));
        for item in list.elements() {
            let mut first = true;
            for child in &item.children {
                match child {
                    Node::Element(nested) if nested.tag == "bullet_list" => {
                        self.bullet_list(nested, level + 1, &with(&chain, item));
                    }
                    Node::Element(nested) if nested.tag == "enumerated_list" => {
                        self.enumerated_list(nested, level + 1, &with(&chain, item));
                    }
                    Node::Element(block) if block.tag == "literal_block" => {
                        if first {
                            self.push_text(text_end(
                                &format!("{indent}{marker}"),
                                marker_style.clone(),
                                "",
                            ));
                            first = false;
                        }
                        self.literal_block(block);
                    }
                    _ => {
                        let mut items = self.render_child(child, item, &chain);
                        if first {
                            let marker_text =
                                text_end(&format!("{indent}{marker}"), marker_style.clone(), "");
                            match items.first_mut() {
                                Some(Item::Text(text)) => {
                                    let mut combined = marker_text;
                                    append_text(&mut combined, text);
                                    *text = combined;
                                }
                                _ => self.push_text(marker_text),
                            }
                            self.items.extend(items);
                            first = false;
                        } else {
                            if let Some(Item::Text(text)) = items.first_mut() {
                                let len = text.plain().len();
                                text.stylize(text_style.clone(), 0, len);
                            }
                            self.items.extend(items);
                        }
                    }
                }
            }
        }
    }

    /// `_render_enumerated_list`.
    fn enumerated_list(&mut self, list: &Element, level: usize, ancestors: &[Ancestor]) {
        let marker_style = self.style("enumerated_list_marker", "bold yellow");
        let text_style = self.style("enumerated_text", "none");
        let indent = "  ".repeat(level);
        let enumtype = list.get("enumtype").unwrap_or("arabic");
        let prefix = list.get("prefix").unwrap_or("");
        let suffix = list.get("suffix").unwrap_or(".");
        let start = list.int("start", 1);
        let mut chain = ancestors.to_vec();
        chain.push(ancestor(list));
        for (index, item) in list.elements().enumerate() {
            let number = start + index as i64;
            let marker = format!("{indent} {prefix}{}{suffix}", enum_marker(enumtype, number));
            let mut first = true;
            for child in &item.children {
                match child {
                    Node::Element(nested) if nested.tag == "bullet_list" => {
                        self.bullet_list(nested, level + 1, &with(&chain, item));
                    }
                    Node::Element(nested) if nested.tag == "enumerated_list" => {
                        self.enumerated_list(nested, level + 1, &with(&chain, item));
                    }
                    Node::Element(block) if block.tag == "literal_block" => {
                        if first {
                            self.push_text(text_end(&marker, marker_style.clone(), " "));
                            first = false;
                        }
                        self.literal_block(block);
                    }
                    _ => {
                        let mut items = self.render_child(child, item, &chain);
                        if first {
                            self.push_text(text_end(&marker, marker_style.clone(), " "));
                            first = false;
                        } else if let Some(Item::Text(text)) = items.first_mut() {
                            let len = text.plain().len();
                            text.stylize(text_style.clone(), 0, len);
                        }
                        self.items.extend(items);
                    }
                }
            }
        }
    }

    fn syntax(
        &self,
        code: String,
        lexer: String,
        line_numbers: bool,
        start_line: i64,
        highlight_lines: Vec<i64>,
    ) -> SyntaxSpec {
        SyntaxSpec {
            code,
            lexer,
            theme: self.code_theme.map(String::from),
            line_numbers,
            start_line,
            highlight_lines,
        }
    }

    /// `_find_lexer`: the second class, else the format; else the default.
    fn find_lexer(&self, element: &Element) -> (String, bool) {
        let lexer = element.classes.get(1).cloned().or_else(|| {
            element
                .get("format")
                .filter(|f| !f.is_empty())
                .map(String::from)
        });
        match lexer {
            Some(lexer) => (lexer, true),
            None => (self.default_lexer.to_string(), false),
        }
    }

    fn literal_block(&mut self, element: &Element) {
        let border = self.style("literal_block_border", "grey58");
        if let Some(last) = self.last_text() {
            rstrip(last);
            append_text(last, &Text::new("\n"));
        }
        let name = element
            .get("name")
            .map(String::from)
            .or_else(|| element.names.first().cloned());
        if element.classes.iter().any(|c| c == "parsed-literal") {
            let content = self.parsed_literal(element, Style::new());
            let mut title = "parsed-literal".to_string();
            if let Some(name) = name {
                title = format!("{title} — {name}");
            }
            let mut panel = PanelSpec::new(R::Text(content));
            panel.border_style = Some(border);
            panel.box_set = Some(SQUARE);
            panel.title = Some(title);
            self.push(R::panel(panel));
            return;
        }
        let (lexer, explicit) = self.find_lexer(element);
        let mut title = if explicit {
            lexer.clone()
        } else {
            format!("{lexer} (default)")
        };
        if let Some(name) = name {
            title = format!("{title} — {name}");
        }
        let highlight: Vec<i64> = element
            .get("highlight_lines")
            .map(|lines| lines.split(',').filter_map(|n| n.parse().ok()).collect())
            .unwrap_or_default();
        let line_numbers = element.get("linenos").is_some() || !highlight.is_empty();
        let start = element.int("start_line", 1);
        let syntax = self.syntax(element.astext(), lexer, line_numbers, start, highlight);
        let mut panel = PanelSpec::new(R::Syntax(Box::new(syntax)));
        panel.border_style = Some(border);
        panel.box_set = Some(SQUARE);
        panel.title = Some(title);
        self.push(R::panel(panel));
    }

    /// `_render_parsed_literal_node`.
    fn parsed_literal(&self, element: &Element, parent: Style) -> Text {
        let mut style = parent;
        match element.tag {
            "emphasis" => style = style.combine(&self.style("emphasis", "italic")),
            "strong" => style = style.combine(&self.style("strong", "bold")),
            "literal" => style = style.combine(&self.style("inline_codeblock", "grey78 on grey7")),
            "title_reference" => style = style.combine(&self.style("title_reference", "italic")),
            "reference" => {
                let mut link = Style::parse("#6088ff underline").unwrap_or_default();
                if let Some(uri) = element.get("refuri").filter(|u| !u.is_empty()) {
                    link = link.update_link(Some(uri.to_string()));
                }
                style = style.combine(&link);
            }
            _ => {}
        }
        let mut out = Text::new("");
        for child in &element.children {
            match child {
                Node::Text(text) => {
                    append_text(&mut out, &Text::styled(text.as_str(), style.clone()))
                }
                Node::Element(child) => {
                    let piece = self.parsed_literal(child, style.clone());
                    append_text(&mut out, &piece);
                }
            }
        }
        out
    }

    fn system_message(&mut self, element: &Element) {
        let message = element.astext().to_lowercase();
        if ["title", "overline", "underline"]
            .iter()
            .any(|k| message.contains(k))
        {
            return;
        }
        for child in element.elements().filter(|c| c.tag == "literal_block") {
            let snippet = child.astext().replace('\n', " ");
            if snippet.is_empty() {
                continue;
            }
            if let Some(last) = self.last_text() {
                append_text(last, &Text::new(snippet));
            } else {
                self.push_text(Text::new(snippet).end(""));
            }
        }
    }

    /// `_render_docinfo_value`.
    fn docinfo_value(&self, node: &Element, parent: &Element, ancestors: &[Ancestor]) -> R {
        let blocks = node.children.iter().any(|child| !child.is_inline());
        let mut chain = ancestors.to_vec();
        if !std::ptr::eq(node, parent) {
            chain.push(ancestor(parent));
        }
        if blocks {
            let mut items = clean_body(self.render_children(&node.children, node, &chain));
            for item in &mut items {
                if let Item::Text(text) = item {
                    rstrip(text);
                    *text = with_end(text, "\n");
                }
            }
            return match items.len() {
                0 => R::Str(String::new()),
                1 => items.pop().map(Item::into_r).unwrap_or(R::NewLine),
                _ => group(items),
            };
        }
        let mut combined = Text::new("");
        for child in &node.children {
            for item in self.render_child(child, node, &chain) {
                if let Item::Text(text) = item {
                    append_text(&mut combined, &text);
                }
            }
        }
        R::Text(combined)
    }

    /// `_add_to_field_table`.
    fn add_field(&mut self, name: &str, value: R) {
        let name_style = self.style("field_name", "bold");
        let name = Text::styled(name, name_style);
        if let Some(Item::Fields(fields)) = self.items.last_mut() {
            fields.rows.push((name, value));
            return;
        }
        let title = self
            .in_docinfo
            .then(|| Text::styled("Document Information", self.style("docinfo_title", "bold")));
        self.items.push(Item::Fields(Fields {
            title,
            rows: vec![(name, value)],
        }));
    }

    fn definition_list(&mut self, list: &Element, ancestors: &[Ancestor]) {
        let term_style = self.style("term_style", "none");
        let classifier_style = self.style("classifier_style", "cyan");
        let definitions_style = self.style("definitions_style", "none");
        let mut chain = ancestors.to_vec();
        chain.push(ancestor(list));
        for item in list.elements() {
            let children: Vec<&Element> = item.elements().collect();
            match children.as_slice() {
                [] => {}
                [term, classifier, definition] => {
                    let header = concat(
                        &concat(
                            &text_end(&term.astext(), term_style.clone(), ""),
                            &Text::new(" : ").end(""),
                        ),
                        &Text::styled(classifier.astext(), classifier_style.clone()),
                    );
                    self.push_text(header);
                    self.push_text(Text::new("\n    ").end(""));
                    let body =
                        self.render_children(&definition.children, definition, &with(&chain, item));
                    self.items.extend(body);
                    self.push_text(Text::new("\n").end(""));
                }
                [term, rest @ ..] if rest.len() >= 3 => {
                    let header = concat(
                        &concat(
                            &text_end(&term.astext(), term_style.clone(), ""),
                            &Text::new(" : ").end(""),
                        ),
                        &Text::styled(rest[0].astext(), classifier_style.clone()),
                    );
                    self.push_text(header);
                    for child in &rest[1..] {
                        match child.tag {
                            "classifier" => self.push_text(Text::styled(
                                format!(" : {}", child.astext()),
                                classifier_style.clone(),
                            )),
                            "definition" => {
                                self.push_text(Text::new("\n    ").end(""));
                                let body = self.render_children(
                                    &child.children,
                                    child,
                                    &with(&chain, item),
                                );
                                self.items.extend(body);
                                self.push_text(Text::new("\n").end(""));
                            }
                            _ => {}
                        }
                    }
                }
                [term, definition] => {
                    let text = concat(
                        &concat(
                            &concat(
                                &Text::styled(term.astext(), term_style.clone()),
                                &Text::new("\n    ").end(""),
                            ),
                            &Text::styled(
                                definition.astext().replace('\n', " "),
                                definitions_style.clone(),
                            ),
                        ),
                        &Text::new("\n      ").end(""),
                    );
                    self.push_text(text);
                }
                [term] => {
                    let text = concat(
                        &Text::styled(term.astext(), term_style.clone()),
                        &Text::new("\n").end(""),
                    );
                    self.push_text(text);
                }
                _ => {}
            }
        }
    }

    fn option_list(&mut self, list: &Element) {
        let string_style = self.style("option_string", "none");
        let argument_style = self.style("option_argument", "none");
        let separator_style = self.style("option_child_text_separator", "none");
        let description_style = self.style("option_description", "none");
        for item in list.elements() {
            let mut parts = item.elements();
            let (Some(group), description) = (parts.next(), parts.next()) else {
                continue;
            };
            let options: Vec<&Element> = group.elements().collect();
            let mut text = Text::new("").end("");
            for option in &options {
                let mut pieces = option.elements();
                let string = pieces.next().map(Element::astext).unwrap_or_default();
                let argument = pieces.next().map(Element::astext);
                let mut piece = Text::styled(string, string_style.clone());
                if let Some(argument) = argument {
                    piece = concat(&piece, &Text::styled(argument, argument_style.clone()));
                }
                let separator = if options.len() > 1 {
                    Text::styled(", ", separator_style.clone())
                } else {
                    Text::new("")
                };
                piece = concat(&piece, &separator);
                text = concat(&text, &piece);
            }
            if let Some(description) = description {
                text = concat(&text, &Text::new("\n    "));
                text = concat(
                    &text,
                    &Text::styled(description.astext(), description_style.clone()),
                );
            }
            let text = concat(&text, &Text::new("\n"));
            self.push_text(text);
        }
    }

    fn block_quote(&mut self, quote: &Element, ancestors: &[Ancestor]) {
        let text_style = self.style("blockquote_text", "white");
        let marker_style = self.style("blockquote_attribution_marker", "bright_magenta");
        let author_style = self.style("blockquote_attribution_text", "grey89");
        let children: Vec<&Node> = quote.children.iter().collect();
        let attribution = match children.last() {
            Some(Node::Element(last)) if last.tag == "attribution" => Some(last),
            _ => None,
        };
        let paragraphs = if attribution.is_some() {
            &children[..children.len() - 1]
        } else {
            &children[..]
        };
        for (index, paragraph) in paragraphs.iter().enumerate() {
            if index > 0 {
                self.push_text(Text::styled("▌", marker_style.clone()));
            }
            let items = merge_bullet_markers(self.render_child(paragraph, quote, ancestors));
            let mut items = items.into_iter();
            match items.next() {
                Some(Item::Text(mut first)) => {
                    rstrip(&mut first);
                    let len = first.plain().len();
                    first.stylize(text_style.clone(), 0, len);
                    let mut combined = Text::styled("▌ ", marker_style.clone());
                    append_text(&mut combined, &first);
                    self.push_text(combined);
                    for item in items {
                        match item {
                            Item::Text(text) => {
                                let mut combined = Text::styled("▌ ", marker_style.clone());
                                append_text(&mut combined, &text);
                                self.push_text(combined);
                            }
                            other => self.items.push(other),
                        }
                    }
                }
                first => {
                    self.push_text(Text::styled("▌ ", marker_style.clone()));
                    self.items.extend(first);
                    self.items.extend(items);
                }
            }
        }
        self.items.push(Item::NewLine);
        if let Some(attribution) = attribution {
            self.push_text(Text::styled(
                format!("  \u{2014} {}", attribution.astext()),
                author_style,
            ));
        }
    }

    fn line_block(&mut self, block: &Element, indent: usize) {
        let prefix = "    ".repeat(indent);
        for child in block.elements() {
            match child.tag {
                "line_block" => self.line_block(child, indent + 1),
                "line" => self.push_text(Text::new(format!("{prefix}{}", child.astext()))),
                _ => {}
            }
        }
    }

    fn sidebar(&mut self, sidebar: &Element, ancestors: &[Ancestor]) {
        let mut body: &[Node] = &sidebar.children;
        let mut title = String::new();
        if let Some(Node::Element(first)) = body.first() {
            if first.tag == "title" {
                title = first.astext();
                body = &body[1..];
            }
        }
        let mut subtitle = String::new();
        if let Some(Node::Element(first)) = body.first() {
            if first.tag == "subtitle" {
                subtitle = first.astext();
                body = &body[1..];
            }
        }
        if let Some(Node::Element(first)) = body.first() {
            if first.tag == "field_list" {
                for field in first.elements() {
                    let mut parts = field.elements();
                    if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                        if name.astext().trim().to_lowercase() == "subtitle" {
                            subtitle = value.astext().trim().to_string();
                        }
                    }
                }
                body = &body[1..];
            }
        }
        let mut items = Vec::new();
        for child in body {
            items.extend(self.render_child(child, sidebar, ancestors));
        }
        let items = clean_body(items);
        let mut panel = PanelSpec::new(group_or_empty(items));
        panel.title = Some(title);
        panel.subtitle = Some(subtitle);
        panel.expand = false;
        self.push(R::panel(panel));
    }

    fn raw(&mut self, element: &Element) {
        let border = self.style("literal_block_border", "grey58");
        let (lexer, _) = self.find_lexer(element);
        let mut text = element.astext();
        let (title, lexer) = if lexer == "html" {
            text = strip_tags(&text);
            ("stripped raw html".to_string(), "text".to_string())
        } else {
            (format!("raw {lexer}"), lexer)
        };
        let syntax = self.syntax(text, lexer, false, 1, Vec::new());
        let mut panel = PanelSpec::new(R::Syntax(Box::new(syntax)));
        panel.border_style = Some(border);
        panel.box_set = Some(SQUARE);
        panel.title = Some(title);
        self.push(R::panel(panel));
    }
}

/// `ancestors` with `element` after them.
fn with(ancestors: &[Ancestor], element: &Element) -> Vec<Ancestor> {
    let mut chain = ancestors.to_vec();
    chain.push(ancestor(element));
    chain
}

/// `_make_enum_marker`.
fn enum_marker(enumtype: &str, number: i64) -> String {
    let letter = |base: u8| {
        u8::try_from(number - 1)
            .ok()
            .and_then(|offset| base.checked_add(offset))
            .map_or_else(String::new, |c| char::from(c).to_string())
    };
    match enumtype {
        "loweralpha" => letter(b'a'),
        "upperalpha" => letter(b'A'),
        "lowerroman" => roman(number).to_lowercase(),
        "upperroman" => roman(number),
        _ => number.to_string(),
    }
}

/// `_make_image_text`.
fn image_text(image: &Element, link_override: Option<&str>) -> Text {
    let raw = &image.rawsource;
    let target = raw.contains(":target:").then(|| {
        raw.rsplit(":target:")
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    });
    let alt_option = raw
        .contains(":alt:")
        .then(|| raw.rsplit(":alt:").next().unwrap_or("").trim().to_string());
    let link = link_override
        .filter(|l| !l.is_empty())
        .map(String::from)
        .or_else(|| image.get("target").map(String::from))
        .unwrap_or_else(|| {
            target
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Image".into())
        });
    let alt = image
        .get("alt")
        .map(String::from)
        .or(alt_option.filter(|a| !a.is_empty()))
        .unwrap_or_else(|| "Image".into());
    let style = Style::parse("#6088ff")
        .unwrap_or_default()
        .update_link(Some(link));
    concat(&Text::new("🌆 "), &Text::styled(alt, style))
}

/// `_convert_math_to_unicode`.
fn math_to_unicode(text: &str) -> String {
    use std::sync::LazyLock;
    static LEFT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\left\s*").unwrap());
    static RIGHT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\right\s*").unwrap());
    static FRAC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\\frac\{([^{}]*)\}\{([^{}]*)\}").unwrap());
    static SQRT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\sqrt\{([^{}]*)\}").unwrap());
    static SUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\^\{([^{}]*)\}").unwrap());
    static SUB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_\{([^{}]*)\}").unwrap());
    let mut out = LEFT.replace_all(text, "").into_owned();
    out = RIGHT.replace_all(&out, "").into_owned();
    out = FRAC.replace_all(&out, "($1/$2)").into_owned();
    out = SQRT.replace_all(&out, "√($1)").into_owned();
    out = SUP.replace_all(&out, "^$1").into_owned();
    out = SUB.replace_all(&out, "_$1").into_owned();
    out = out.replace(['{', '}'], "");
    let mut table: Vec<(&str, &str)> = LATEX.to_vec();
    table.sort_by_key(|(latex, _)| std::cmp::Reverse(latex.len()));
    for (latex, unicode) in table {
        out = out.replace(latex, unicode);
    }
    out
}

/// `_LATEX_TO_UNICODE`, in its order (stable-sorted by length on use).
const LATEX: &[(&str, &str)] = &[
    ("\\alpha", "α"),
    ("\\beta", "β"),
    ("\\gamma", "γ"),
    ("\\delta", "δ"),
    ("\\epsilon", "ε"),
    ("\\varepsilon", "ε"),
    ("\\zeta", "ζ"),
    ("\\eta", "η"),
    ("\\theta", "θ"),
    ("\\vartheta", "ϑ"),
    ("\\iota", "ι"),
    ("\\kappa", "κ"),
    ("\\lambda", "λ"),
    ("\\mu", "μ"),
    ("\\nu", "ν"),
    ("\\xi", "ξ"),
    ("\\pi", "π"),
    ("\\varpi", "ϖ"),
    ("\\rho", "ρ"),
    ("\\varrho", "ϱ"),
    ("\\sigma", "σ"),
    ("\\varsigma", "ς"),
    ("\\tau", "τ"),
    ("\\upsilon", "υ"),
    ("\\phi", "φ"),
    ("\\varphi", "φ"),
    ("\\chi", "χ"),
    ("\\psi", "ψ"),
    ("\\omega", "ω"),
    ("\\Gamma", "Γ"),
    ("\\Delta", "Δ"),
    ("\\Theta", "Θ"),
    ("\\Lambda", "Λ"),
    ("\\Xi", "Ξ"),
    ("\\Pi", "Π"),
    ("\\Sigma", "Σ"),
    ("\\Upsilon", "Υ"),
    ("\\Phi", "Φ"),
    ("\\Psi", "Ψ"),
    ("\\Omega", "Ω"),
    ("\\times", "×"),
    ("\\div", "÷"),
    ("\\pm", "±"),
    ("\\mp", "∓"),
    ("\\cdot", "·"),
    ("\\ldots", "…"),
    ("\\cdots", "⋯"),
    ("\\vdots", "⋮"),
    ("\\ddots", "⋱"),
    ("\\leq", "≤"),
    ("\\le", "≤"),
    ("\\geq", "≥"),
    ("\\ge", "≥"),
    ("\\neq", "≠"),
    ("\\ne", "≠"),
    ("\\approx", "≈"),
    ("\\equiv", "≡"),
    ("\\sim", "∼"),
    ("\\simeq", "≃"),
    ("\\cong", "≅"),
    ("\\propto", "∝"),
    ("\\subset", "⊂"),
    ("\\supset", "⊃"),
    ("\\subseteq", "⊆"),
    ("\\supseteq", "⊇"),
    ("\\in", "∈"),
    ("\\notin", "∉"),
    ("\\cup", "∪"),
    ("\\cap", "∩"),
    ("\\emptyset", "∅"),
    ("\\varnothing", "∅"),
    ("\\neg", "¬"),
    ("\\wedge", "∧"),
    ("\\vee", "∨"),
    ("\\oplus", "⊕"),
    ("\\forall", "∀"),
    ("\\exists", "∃"),
    ("\\nexists", "∄"),
    ("\\to", "→"),
    ("\\rightarrow", "→"),
    ("\\leftarrow", "←"),
    ("\\gets", "←"),
    ("\\leftrightarrow", "↔"),
    ("\\Rightarrow", "⇒"),
    ("\\Leftarrow", "⇐"),
    ("\\Leftrightarrow", "⇔"),
    ("\\uparrow", "↑"),
    ("\\downarrow", "↓"),
    ("\\updownarrow", "↕"),
    ("\\Uparrow", "⇑"),
    ("\\Downarrow", "⇓"),
    ("\\mapsto", "↦"),
    ("\\infty", "∞"),
    ("\\partial", "∂"),
    ("\\nabla", "∇"),
    ("\\sum", "∑"),
    ("\\prod", "∏"),
    ("\\int", "∫"),
    ("\\oint", "∮"),
    ("\\hbar", "ℏ"),
    ("\\ell", "ℓ"),
    ("\\wp", "℘"),
    ("\\Re", "ℜ"),
    ("\\Im", "ℑ"),
    ("\\aleph", "ℵ"),
    ("\\angle", "∠"),
    ("\\perp", "⊥"),
    ("\\parallel", "∥"),
    ("\\prime", "′"),
    ("\\dagger", "†"),
    ("\\ddagger", "‡"),
    ("\\langle", "⟨"),
    ("\\rangle", "⟩"),
    ("\\quad", "  "),
    ("\\qquad", "    "),
    ("\\ ", " "),
];

/// A table cell: its content, and the columns and rows it spans past its
/// own.
type GridCell = Option<(R, usize, usize)>;

impl Visitor<'_> {
    /// `_render_entry_content`.
    fn entry_content(&self, entry: &Element, ancestors: &[Ancestor]) -> R {
        let cell_style = self.style("table_cell", "none");
        let mut items = self.render_children(&entry.children, entry, ancestors);
        if items.is_empty() {
            return R::Text(Text::styled("", cell_style));
        }
        let has_list = entry
            .elements()
            .any(|child| matches!(child.tag, "bullet_list" | "enumerated_list"));
        if has_list {
            let items: Vec<Item> = merge_bullet_markers(items)
                .into_iter()
                .filter(|item| !matches!(item, Item::NewLine))
                .collect();
            if items.is_empty() {
                return R::Text(Text::styled("", cell_style));
            }
            let options = self.console.options().update_width(2048);
            let lines = self.console.render_lines(&group(items), &options, false);
            let mut compact: Vec<R> = Vec::new();
            for line in lines {
                let mut text = Text::new("");
                for segment in line.iter().filter(|s| !s.is_control()) {
                    append(&mut text, &segment.text, segment.style.clone());
                }
                rstrip(&mut text);
                if !text.plain().trim().is_empty() {
                    compact.push(R::Text(text));
                }
            }
            return match compact.len() {
                0 => R::Text(Text::styled("", cell_style)),
                1 => compact.pop().unwrap_or(R::NewLine),
                _ => R::Group(compact),
            };
        }
        for item in &mut items {
            if let Item::Text(text) = item {
                rstrip(text);
                let leading = text.plain().len() - text.plain().trim_start().len();
                if leading > 0 {
                    let end = text.get_end().to_string();
                    let len = text.plain().len();
                    *text = text.slice(leading, len).end(end);
                }
            }
        }
        if items.len() == 1 {
            return items.pop().map(Item::into_r).unwrap_or(R::NewLine);
        }
        group(items)
    }

    #[allow(clippy::too_many_lines)]
    fn table(&mut self, table: &Element, ancestors: &[Ancestor]) {
        let header_style = self.style("table_header", "bold");
        let cell_style = self.style("table_cell", "none");
        let mut title = None;
        let mut tgroup = None;
        for child in table.elements() {
            match child.tag {
                "title" | "caption" => title = Some(child.astext()),
                "tgroup" => tgroup = Some(child),
                _ => {}
            }
        }
        let Some(tgroup) = tgroup else {
            return;
        };
        let mut columns = tgroup.elements().filter(|c| c.tag == "colspec").count();
        let thead = tgroup.find("thead");
        let Some(tbody) = tgroup.find("tbody") else {
            return;
        };
        let chain = with(&with(ancestors, table), tgroup);
        if columns == 0 {
            let first_row = thead
                .and_then(|head| head.elements().next())
                .or_else(|| tbody.elements().next());
            if let Some(row) = first_row {
                columns = row
                    .elements()
                    .map(|e| 1 + e.int("morecols", 0) as usize)
                    .sum();
            }
        }
        let spans = |section: &Element| {
            section.elements().any(|row| {
                row.elements()
                    .any(|e| e.int("morecols", 0) != 0 || e.int("morerows", 0) != 0)
            })
        };
        let has_spans = spans(tbody) || thead.is_some_and(spans);
        if has_spans {
            let header_rows = thead.map_or(0, |head| head.elements().count());
            let mut grid: Vec<Vec<GridCell>> = Vec::new();
            let mut active: std::collections::BTreeMap<usize, i64> =
                std::collections::BTreeMap::new();
            let rows = thead
                .into_iter()
                .flat_map(|head| head.elements().map(move |row| (head, row)))
                .chain(tbody.elements().map(|row| (tbody, row)));
            for (section, row) in rows {
                let mut grid_row: Vec<GridCell> = vec![None; columns];
                let mut col = 0;
                let mut entries = row.elements();
                let row_chain = with(&with(&chain, section), row);
                while col < columns {
                    if let Some(remaining) = active.get_mut(&col) {
                        *remaining -= 1;
                        if *remaining <= 0 {
                            active.remove(&col);
                        }
                        col += 1;
                        continue;
                    }
                    let Some(entry) = entries.next() else {
                        col += 1;
                        continue;
                    };
                    let more_cols = entry.int("morecols", 0).max(0) as usize;
                    let more_rows = entry.int("morerows", 0).max(0) as usize;
                    let content = self.entry_content(entry, &row_chain);
                    grid_row[col] = Some((content, more_cols, more_rows));
                    if more_rows > 0 {
                        for span in col..=col + more_cols {
                            active.insert(span, more_rows as i64);
                        }
                    }
                    col += 1 + more_cols;
                }
                grid.push(grid_row);
            }
            let widths = self.span_widths(&grid, columns);
            let lines = spanning_table(
                &grid,
                &widths,
                header_rows,
                title.as_deref(),
                &header_style,
                &cell_style,
                self.console,
            );
            self.push(R::Group(lines.into_iter().map(R::Text).collect()));
            return;
        }
        let has_header = thead.is_some_and(|head| head.elements().next().is_some());
        let mut headers: Vec<String> = Vec::new();
        match thead.and_then(|head| head.elements().next()) {
            Some(row) => {
                for entry in row.elements() {
                    headers.push(entry.astext().replace('\n', " ").trim().to_string());
                    for _ in 0..entry.int("morecols", 0) {
                        headers.push(String::new());
                    }
                }
                while headers.len() < columns {
                    headers.push(String::new());
                }
            }
            None => headers = vec![String::new(); columns],
        }
        let mut rows: Vec<Vec<R>> = Vec::new();
        let mut remaining: std::collections::BTreeMap<usize, i64> =
            std::collections::BTreeMap::new();
        for row in tbody.elements() {
            let occupied: Vec<usize> = remaining
                .iter()
                .filter(|(_, rem)| **rem > 0)
                .map(|(col, _)| *col)
                .collect();
            let row_chain = with(&with(&chain, tbody), row);
            let mut cells = Vec::new();
            let mut new_spans = Vec::new();
            let mut col = 0;
            let mut entries = row.elements();
            while col < columns {
                if occupied.contains(&col) {
                    cells.push(R::Text(Text::styled("", cell_style.clone())));
                    col += 1;
                    continue;
                }
                let Some(entry) = entries.next() else {
                    cells.push(R::Text(Text::styled("", cell_style.clone())));
                    col += 1;
                    continue;
                };
                let more_cols = entry.int("morecols", 0).max(0) as usize;
                let more_rows = entry.int("morerows", 0);
                cells.push(self.entry_content(entry, &row_chain));
                if more_rows > 0 {
                    for span in col..=col + more_cols {
                        new_spans.push((span, more_rows));
                    }
                }
                for _ in 0..more_cols {
                    cells.push(R::Text(Text::styled("", cell_style.clone())));
                }
                col += 1 + more_cols;
            }
            for col in &occupied {
                if let Some(rem) = remaining.get_mut(col) {
                    *rem -= 1;
                    if *rem <= 0 {
                        remaining.remove(col);
                    }
                }
            }
            remaining.extend(new_spans);
            rows.push(cells);
        }
        self.push(R::Table(Box::new(TableSpec {
            columns: headers,
            title: title.map(Text::new),
            show_header: has_header,
            header_style: Some(header_style),
            column_style: Some(cell_style),
            rows,
        })));
    }

    /// The column widths of a table with spans: each column as wide as its
    /// widest unspanned cell, widened for spanning cells, then shrunk to fit
    /// the console.
    fn span_widths(&self, grid: &[Vec<GridCell>], columns: usize) -> Vec<usize> {
        let options = self.console.options().update_width(2048);
        let plain_lines = |content: &R| -> Vec<String> {
            self.console
                .render_lines(content, &options, false)
                .into_iter()
                .map(|line| {
                    line.iter()
                        .filter(|s| !s.is_control())
                        .map(|s| s.text.as_str())
                        .collect::<String>()
                })
                .collect()
        };
        let plain_width = |content: &R| {
            plain_lines(content)
                .iter()
                .map(|line| rich::cells::cell_len(line))
                .max()
                .unwrap_or(0)
        };
        let min_token = |content: &R| {
            let mut widest = 1;
            for line in plain_lines(content) {
                for token in line.split_whitespace() {
                    widest = widest.max(rich::cells::cell_len(token));
                }
            }
            widest.min(20)
        };
        let mut widths = vec![1; columns];
        let mut floors = vec![1; columns];
        for row in grid {
            for (c, cell) in row.iter().enumerate() {
                if let Some((content, 0, _)) = cell {
                    widths[c] = widths[c].max(plain_width(content));
                    floors[c] = floors[c].max(min_token(content));
                }
            }
        }
        for row in grid {
            for (c, cell) in row.iter().enumerate() {
                if let Some((content, more_cols, _)) = cell {
                    if *more_cols > 0 {
                        let available: usize =
                            widths[c..=c + more_cols].iter().sum::<usize>() + 3 * more_cols;
                        let needed = plain_width(content);
                        if needed > available {
                            widths[c + more_cols] += needed - available;
                        }
                    }
                }
            }
        }
        let max_total = self.console.options().max_width.max(1);
        let total = |widths: &[usize]| widths.iter().sum::<usize>() + 3 * widths.len() + 1;
        let overflow = total(&widths) as i64 - max_total as i64;
        let shrink = |widths: &mut Vec<usize>, floors: &[usize], mut remaining: i64| -> i64 {
            while remaining > 0 {
                let mut reducible: Vec<usize> = (0..widths.len())
                    .filter(|&i| widths[i] > floors[i])
                    .collect();
                if reducible.is_empty() {
                    break;
                }
                // `sort(key=width, reverse=True)`: stable, widest first.
                reducible.sort_by(|a, b| widths[*b].cmp(&widths[*a]));
                let per_column = (remaining / reducible.len() as i64).max(1);
                let mut changed = 0;
                for index in reducible {
                    if remaining <= 0 {
                        break;
                    }
                    let max_cut = (widths[index] - floors[index]) as i64;
                    let cut = max_cut.min(per_column).min(remaining);
                    if cut <= 0 {
                        continue;
                    }
                    widths[index] -= cut as usize;
                    remaining -= cut;
                    changed += cut;
                }
                if changed == 0 {
                    break;
                }
            }
            remaining
        };
        let overflow = shrink(&mut widths, &floors, overflow);
        if overflow > 0 {
            shrink(&mut widths, &vec![1; columns], overflow);
        }
        widths
    }
}

/// `_spanning_table`: a table with merged cells, drawn line by line.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn spanning_table(
    grid: &[Vec<GridCell>],
    widths: &[usize],
    header_rows: usize,
    title: Option<&str>,
    header_style: &Style,
    cell_style: &Style,
    console: &Console,
) -> Vec<Text> {
    let rows = grid.len();
    let columns = widths.len();
    let (vh, vb) = ("┃", "│");
    let origin = |r: usize, c: usize| -> Option<(usize, usize)> {
        for rr in (0..=r).rev() {
            for cc in (0..=c).rev() {
                if let Some((_, csp, rsp)) = &grid[rr][cc] {
                    if rr <= r && r <= rr + rsp && cc <= c && c <= cc + csp {
                        return Some((rr, cc));
                    }
                }
            }
        }
        None
    };
    let rspan_continues = |above: usize, c: usize| -> bool {
        if above + 1 >= rows {
            return false;
        }
        for rr in (0..=above).rev() {
            for cc in (0..=c).rev() {
                if let Some((_, csp, rsp)) = &grid[rr][cc] {
                    if cc <= c && c <= cc + csp && rr <= above && above <= rr + rsp {
                        return above < rr + rsp;
                    }
                }
            }
        }
        false
    };
    let cspan_continues = |r: usize, c: usize| {
        c > 0 && {
            let left = origin(r, c - 1);
            left.is_some() && left == origin(r, c)
        }
    };
    let is_header = |r: usize| r < header_rows;
    let has_vborder = |r: usize, c: usize| origin(r, c) != origin(r, c + 1);
    let row_style = |r: usize| {
        if is_header(r) {
            header_style
        } else {
            cell_style
        }
    };
    let mut cache: std::collections::HashMap<(usize, usize), Vec<Text>> =
        std::collections::HashMap::new();
    let mut cell_lines = |r: usize, c: usize| -> Vec<Text> {
        if let Some(lines) = cache.get(&(r, c)) {
            return lines.clone();
        }
        let Some((content, csp, _)) = &grid[r][c] else {
            return Vec::new();
        };
        let available = widths[c..=c + csp].iter().sum::<usize>() + 3 * csp;
        let style = row_style(r).clone();
        if available == 0 {
            return vec![Text::new("")];
        }
        let options = console.options().update_width(available);
        let target = R::Styled(Box::new(content.clone()), style.clone());
        let rendered = console.render_lines(&target, &options, true);
        let mut lines: Vec<Text> = Vec::new();
        for line in rendered {
            let mut current = vec![Text::new("")];
            for segment in line
                .iter()
                .filter(|s| !s.is_control() && !s.text.is_empty())
            {
                let parts: Vec<&str> = segment.text.split('\n').collect();
                for (index, part) in parts.iter().enumerate() {
                    if !part.is_empty() {
                        if let Some(last) = current.last_mut() {
                            append(last, part, segment.style.clone());
                        }
                    }
                    if index + 1 < parts.len() {
                        current.push(Text::new(""));
                    }
                }
            }
            lines.extend(current);
        }
        if lines.is_empty() {
            lines = vec![Text::styled(" ".repeat(available), style.clone())];
        }
        let lines: Vec<Text> = lines
            .into_iter()
            .map(|mut line| {
                let width = rich::cells::cell_len(line.plain());
                if width > available {
                    line.truncate(available, Some(rich::Overflow::Crop), false);
                } else if width < available {
                    append(
                        &mut line,
                        &" ".repeat(available - width),
                        Some(style.clone()),
                    );
                }
                line
            })
            .collect();
        cache.insert((r, c), lines.clone());
        lines
    };
    let separator = |above: Option<usize>, below: Option<usize>| -> Text {
        let top = above.is_none();
        let bottom = below.is_none();
        let head_sep = matches!((above, below), (Some(a), Some(b)) if a + 1 == header_rows && b == header_rows);
        let inner_header =
            matches!((above, below), (Some(a), Some(b)) if is_header(a) && is_header(b));
        let (left, horizontal, _, right) = if top {
            ("┏", "━", "┳", "┓")
        } else if bottom {
            ("└", "─", "┴", "┘")
        } else if head_sep {
            ("┡", "━", "╇", "┩")
        } else if inner_header {
            ("┣", "━", "╋", "┫")
        } else {
            ("├", "─", "┼", "┤")
        };
        let continues = |c: usize| above.is_some_and(|a| !top && rspan_continues(a, c));
        let v_sep = if inner_header { vh } else { vb };
        let mut s = String::new();
        s.push_str(if continues(0) && !top && !bottom {
            v_sep
        } else {
            left
        });
        for (c, width) in widths.iter().enumerate() {
            let rc = continues(c);
            if rc {
                s.push_str(&" ".repeat(width + 2));
            } else {
                s.push_str(&horizontal.repeat(width + 2));
            }
            if c + 1 < columns {
                let lrc = rc;
                let rrc = continues(c + 1);
                if lrc && rrc {
                    let same = above.is_some_and(|a| {
                        let o = origin(a, c);
                        o.is_some() && o == origin(a, c + 1)
                    });
                    s.push_str(if same { " " } else { v_sep });
                } else if lrc || rrc {
                    s.push_str(match (inner_header, lrc) {
                        (true, true) => "┣",
                        (true, false) => "┫",
                        (false, true) => "├",
                        (false, false) => "┤",
                    });
                } else {
                    let up = above.is_some_and(|a| !top && has_vborder(a, c));
                    let down = below.is_some_and(|b| has_vborder(b, c));
                    s.push_str(match (up, down) {
                        (true, true) if head_sep => "╇",
                        (true, true) if inner_header => "╋",
                        (true, true) => "┼",
                        (true, false) if head_sep || inner_header => "┻",
                        (true, false) => "┴",
                        (false, true) if top => "┳",
                        (false, true) if head_sep => "┯",
                        (false, true) if inner_header => "┳",
                        (false, true) => "┬",
                        (false, false) => horizontal,
                    });
                }
            }
        }
        let rcn = continues(columns - 1);
        s.push_str(if rcn && !top && !bottom { v_sep } else { right });
        Text::new(s)
    };
    let row_height = |r: usize, cell_lines: &mut dyn FnMut(usize, usize) -> Vec<Text>| {
        let mut height = 1;
        let mut c = 0;
        while c < columns {
            match &grid[r][c] {
                None => c += 1,
                Some((_, csp, _)) => {
                    height = height.max(cell_lines(r, c).len());
                    c += 1 + csp;
                }
            }
        }
        height
    };
    let placeholder_row =
        |r: usize| (0..columns).all(|c| grid[r][c].is_none() && origin(r, c).is_some());
    let mut lines: Vec<Text> = Vec::new();
    if let Some(title) = title.filter(|t| !t.is_empty()) {
        let total = if rows > 0 {
            rich::cells::cell_len(separator(None, Some(0)).plain())
        } else {
            widths.iter().sum::<usize>() + 3 * columns + 1
        };
        lines.push(Text::styled(center(title, total), "italic"));
    }
    #[allow(clippy::needless_range_loop)] // rows are read by index everywhere
    for r in 0..rows {
        let above = r.checked_sub(1);
        let placeholder = placeholder_row(r);
        let sep = separator(above, Some(r));
        if !(placeholder
            && sep
                .plain()
                .chars()
                .all(|ch| ch == ' ' || ch == '│' || ch == '┃'))
        {
            lines.push(sep);
        }
        if placeholder {
            continue;
        }
        let height = row_height(r, &mut cell_lines);
        for line_no in 0..height {
            let v = if is_header(r) { vh } else { vb };
            let style = row_style(r).clone();
            let mut line = Text::new(v);
            let mut c = 0;
            while c < columns {
                match &grid[r][c] {
                    Some((_, csp, _)) => {
                        let available = widths[c..=c + csp].iter().sum::<usize>() + 3 * csp;
                        let rendered = cell_lines(r, c);
                        let inner = rendered
                            .get(line_no)
                            .cloned()
                            .unwrap_or_else(|| Text::styled(" ".repeat(available), style.clone()));
                        append(&mut line, " ", Some(style.clone()));
                        append_text(&mut line, &inner);
                        append(&mut line, " ", Some(style.clone()));
                        if c + csp < columns - 1 {
                            append(&mut line, v, None);
                        }
                        c += 1 + csp;
                    }
                    None => {
                        let here = origin(r, c);
                        if let Some(o) = here.filter(|o| o.1 == c) {
                            let mut span_end = c;
                            while span_end + 1 < columns && origin(r, span_end + 1) == Some(o) {
                                span_end += 1;
                            }
                            let csp = span_end - c;
                            let available = widths[c..=span_end].iter().sum::<usize>() + 3 * csp;
                            append(&mut line, &" ".repeat(available + 2), Some(style.clone()));
                            if span_end < columns - 1 {
                                append(&mut line, v, None);
                            }
                            c = span_end + 1;
                        } else {
                            append(&mut line, &" ".repeat(widths[c] + 2), Some(style.clone()));
                            if c + 1 < columns && !cspan_continues(r, c + 1) {
                                append(&mut line, v, None);
                            }
                            c += 1;
                        }
                    }
                }
            }
            append(&mut line, v, None);
            lines.push(line);
        }
    }
    lines.push(separator(rows.checked_sub(1), None));
    lines
}

/// Python's `str.center`.
fn center(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if width <= len {
        return text.to_string();
    }
    let margin = width - len;
    // Python puts the odd space on the left when the width is odd.
    let left = margin / 2 + (margin & width & 1);
    let right = margin - left;
    format!("{}{text}{}", " ".repeat(left), " ".repeat(right))
}

impl RestructuredText {
    fn renderables<'a>(&'a self, console: &'a Console) -> (Vec<Item>, Vec<R>, Vec<R>) {
        let mut visitor = Visitor::new(console, self.code_theme.as_deref(), &self.default_lexer);
        let mut ancestors = Vec::new();
        visitor.walk(&Node::Element(self.document.clone()), &mut ancestors);
        let mut items = visitor.items;
        // Strip trailing line breaks and empty text; the last text ends its
        // line.
        loop {
            match items.last_mut() {
                Some(Item::Text(text)) => {
                    rstrip(text);
                    *text = with_end(text, "\n");
                    if !text.plain().is_empty() {
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
        (items, visitor.citations, visitor.footer)
    }
}

impl Renderable for RestructuredText {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (items, citations, footer) = self.renderables(console);
        let mut segments = Vec::new();
        for item in items {
            let r = match item {
                // Trailing line breaks print as the text's end.
                Item::Text(text) if text.plain().ends_with('\n') => {
                    let trailing = text.plain().len() - text.plain().trim_end_matches('\n').len();
                    let end = format!("{}{}", "\n".repeat(trailing), text.get_end());
                    let mut fixed = text.clone();
                    fixed.rstrip();
                    R::Text(fixed.end(end))
                }
                other => other.into_r(),
            };
            r.render_into(console, options, &mut segments);
        }
        let mut footer_panel = |title: &str, children: Vec<R>, style: &str, border: &str| {
            let style = console
                .get_style(&StyleType::Name(format!("restructuredtext.{style}")))
                .or_else(|_| console.get_style(&StyleType::Name("none".into())))
                .unwrap_or_default();
            let border = console
                .get_style(&StyleType::Name(format!("restructuredtext.{border}")))
                .or_else(|_| console.get_style(&StyleType::Name("grey74".into())))
                .unwrap_or_default();
            let mut panel = PanelSpec::new(R::Group(children));
            panel.title = Some(title.into());
            panel.box_set = Some(SQUARE);
            panel.border_style = Some(border);
            panel.style = Some(style);
            R::panel(panel).render_into(console, options, &mut segments);
        };
        if !citations.is_empty() {
            footer_panel("citation", citations, "citation", "citation_border");
        }
        if !footer.is_empty() {
            footer_panel("Footer", footer, "footer", "footer_border");
        }
        trim_last_line(segments)
    }
}

/// `rich_rst.strip_tags`: the data an `html.parser.HTMLParser` with
/// `convert_charrefs` reports, fed `html` and closed. Tags, comments,
/// declarations and processing instructions go; character references are
/// decoded as `html.unescape` decodes them; `script` and `style` bodies are
/// kept as written. An unclosed tag at the end is dropped.
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let n = html.len();
    let mut i = 0;
    while i < n {
        let rest = &html[i..];
        let j = match rest.find('<') {
            Some(offset) => i + offset,
            // `close()` hands over the text held back for a reference
            // that might go on.
            None => n,
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
