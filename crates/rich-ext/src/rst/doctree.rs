//! A docutils document tree: the nodes `rich-rst`'s visitor walks.
//!
//! Elements are named by docutils' tag names (`paragraph`, `section`,
//! `bullet_list`, …) and carry the attributes the visitor reads. Text is
//! joined as docutils' `astext()` joins it: a text element's children run
//! together, other elements' are separated by a blank line.

use std::collections::BTreeMap;

/// A node: text, or an element with children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Text(String),
    Element(Element),
}

/// An element: its tag, attributes and children.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    pub tag: &'static str,
    /// Attributes other than `classes` and `names`, as strings.
    pub attrs: BTreeMap<&'static str, String>,
    pub classes: Vec<String>,
    pub names: Vec<String>,
    pub children: Vec<Node>,
    /// The source text of a directive's element (an image's `rawsource`).
    pub rawsource: String,
}

/// Elements whose children's text joins with nothing between.
const TEXT_ELEMENTS: &[&str] = &[
    "title",
    "subtitle",
    "author",
    "organization",
    "address",
    "contact",
    "version",
    "revision",
    "status",
    "date",
    "copyright",
    "paragraph",
    "rubric",
    "attribution",
    "reference",
    "term",
    "classifier",
    "field_name",
    "option_string",
    "option_argument",
    "literal_block",
    "doctest_block",
    "math_block",
    "line",
    "label",
    "caption",
    "comment",
    "substitution_definition",
    "target",
    "raw",
    "abbreviation",
    "acronym",
    "emphasis",
    "generated",
    "inline",
    "literal",
    "strong",
    "subscript",
    "superscript",
    "title_reference",
    "footnote_reference",
    "citation_reference",
    "substitution_reference",
    "math",
    "problematic",
    "option",
];

/// docutils' inline elements (`nodes.Inline`), and text.
const INLINE: &[&str] = &[
    "reference",
    "image",
    "target",
    "raw",
    "abbreviation",
    "acronym",
    "emphasis",
    "generated",
    "inline",
    "literal",
    "strong",
    "subscript",
    "superscript",
    "title_reference",
    "footnote_reference",
    "citation_reference",
    "substitution_reference",
    "math",
    "problematic",
];

impl Node {
    /// docutils' `Node.astext()`.
    pub fn astext(&self) -> String {
        match self {
            Node::Text(text) => text.clone(),
            Node::Element(element) => element.astext(),
        }
    }

    pub fn element(&self) -> Option<&Element> {
        match self {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        }
    }

    pub fn element_mut(&mut self) -> Option<&mut Element> {
        match self {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        }
    }

    /// Whether this is an element with tag `tag`.
    pub fn is(&self, tag: &str) -> bool {
        matches!(self, Node::Element(element) if element.tag == tag)
    }

    /// Text, or an inline element (`isinstance(child, (Inline, Text))`).
    pub fn is_inline(&self) -> bool {
        match self {
            Node::Text(_) => true,
            Node::Element(element) => INLINE.contains(&element.tag),
        }
    }
}

impl From<Element> for Node {
    fn from(element: Element) -> Node {
        Node::Element(element)
    }
}

impl Element {
    pub fn new(tag: &'static str) -> Element {
        Element {
            tag,
            ..Element::default()
        }
    }

    /// An element holding one text node.
    pub fn text(tag: &'static str, text: impl Into<String>) -> Element {
        Element::new(tag).child(Node::Text(text.into()))
    }

    #[must_use]
    pub fn with(mut self, key: &'static str, value: impl Into<String>) -> Element {
        self.attrs.insert(key, value.into());
        self
    }

    #[must_use]
    pub fn class(mut self, class: impl Into<String>) -> Element {
        self.classes.push(class.into());
        self
    }

    #[must_use]
    pub fn child(mut self, child: impl Into<Node>) -> Element {
        self.children.push(child.into());
        self
    }

    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = Node>) -> Element {
        self.children.extend(children);
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }

    /// An integer attribute, `default` when absent.
    pub fn int(&self, key: &str, default: i64) -> i64 {
        self.get(key)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    /// The child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(Node::element)
    }

    /// The first child element with tag `tag`.
    pub fn find(&self, tag: &str) -> Option<&Element> {
        self.elements().find(|element| element.tag == tag)
    }

    /// docutils' `astext()`.
    pub fn astext(&self) -> String {
        match self.tag {
            "image" => self.get("alt").unwrap_or("").to_string(),
            "option_argument" => {
                let delimiter = self.get("delimiter").unwrap_or(" ");
                format!("{delimiter}{}", self.join(""))
            }
            "system_message" => self.join("\n\n"),
            "option_group" => self.join(", "),
            "option_list_item" => self.join("  "),
            tag if TEXT_ELEMENTS.contains(&tag) => self.join(""),
            _ => self.join("\n\n"),
        }
    }

    fn join(&self, separator: &str) -> String {
        self.children
            .iter()
            .map(Node::astext)
            .collect::<Vec<_>>()
            .join(separator)
    }

    /// Every element in this subtree, this one first.
    pub fn walk(&self, f: &mut dyn FnMut(&Element)) {
        f(self);
        for child in self.elements() {
            child.walk(f);
        }
    }

    /// Every element in this subtree, mutably, this one first.
    pub fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Element)) {
        f(self);
        for child in &mut self.children {
            if let Node::Element(element) = child {
                element.walk_mut(f);
            }
        }
    }

    /// The tree in an indented form, one node a line, with the attributes
    /// the visitor reads: what the parity tests compare with docutils' tree.
    pub fn dump(&self) -> String {
        let mut out = Vec::new();
        self.dump_into(0, &mut out);
        out.join("\n")
    }

    fn dump_into(&self, depth: usize, out: &mut Vec<String>) {
        // Messages show only their source, so only those that have one.
        if self.tag == "system_message" && self.find("literal_block").is_none() {
            return;
        }
        let pad = "  ".repeat(depth);
        let mut line = format!("{pad}<{}", self.tag);
        if !self.classes.is_empty() {
            line.push_str(&format!(" classes={}", json_list(&self.classes)));
        }
        for (key, value) in &self.attrs {
            if matches!(*key, "delimiter" | "bullet" | "name" | "cols" | "colwidth" | "style" | "source" | "version")
                || key.starts_with("__")
                || value.is_empty()
                || value == "0"
            {
                continue;
            }
            line.push_str(&format!(" {key}={}", json_value(key, value)));
        }
        if self.tag == "literal_block" && !self.names.is_empty() {
            line.push_str(&format!(" names={}", json_list(&self.names)));
        }
        if self.tag == "image" {
            line.push_str(&format!(" rawsource={}", json_string(&self.rawsource)));
        }
        line.push('>');
        out.push(line);
        for child in &self.children {
            match child {
                Node::Text(text) => out.push(format!("{pad}  {}", json_string(text))),
                Node::Element(element) => element.dump_into(depth + 1, out),
            }
        }
    }
}

const INT_ATTRS: &[&str] = &[
    "morecols",
    "morerows",
    "start",
    "start_line",
    "auto",
    "anonymous",
    "linenos",
];

fn json_value(key: &str, value: &str) -> String {
    if INT_ATTRS.contains(&key) && value.parse::<i64>().is_ok() {
        return value.to_string();
    }
    json_string(value)
}

fn json_list(items: &[String]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|item| json_string(item))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A string as Python's `json.dumps(..., ensure_ascii=False)` writes it.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
