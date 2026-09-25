//! Transforms over a data [`Document`]: redact, select, filter and highlight
//! by JSONPath (the `jsonpath` feature). See [`crate::transform`] for the
//! pipeline.
//!
//! ```
//! use rich::{Console, Style};
//! use rich_ext::data::transform::{Document, Filter, Select};
//! use rich_ext::data::{parse, Format};
//! use rich_ext::transform::Pipeline;
//!
//! let node = parse(Format::Json, r#"{"a": {"b": 1, "c": 2}, "d": [1, 2]}"#).unwrap();
//! let pipeline = Pipeline::new().then("filter", Filter::new("$.a.c").unwrap());
//! let document = pipeline.apply(Document::new(node).label("doc")).unwrap();
//! let out = Console::builder().width(40).build().render_export(&document.explorer());
//! assert_eq!(out, "doc\n└── a\n    └── c: 2\n");
//! ```

use std::collections::HashSet;

use rich::Style;

use super::{Explorer, Node, Path, Redactor, SelectError, Selectors, Value};
use crate::transform::{Transform, TransformError};

/// A data tree on its way to being rendered, with what transforms decided
/// about how to show it.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub node: Node,
    /// The root's label: the file name, or the path [`Select`] narrowed to.
    pub label: Option<String>,
    /// Styles for the lines of these paths, from [`Highlight`].
    pub highlights: Vec<(Path, Style)>,
}

impl Document {
    pub fn new(node: Node) -> Self {
        Document {
            node,
            label: None,
            highlights: Vec::new(),
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// An [`Explorer`] over the document, with its label and highlights.
    pub fn explorer(&self) -> Explorer<'_> {
        let mut explorer = Explorer::new(&self.node);
        if let Some(label) = &self.label {
            explorer = explorer.root_label(label.clone());
        }
        for (path, style) in &self.highlights {
            explorer = explorer.highlight(path.clone(), style.clone());
        }
        explorer
    }
}

/// Masks values with a [`Redactor`], such as
/// [`Redaction::secrets`](super::Redaction::secrets).
pub struct Redact<R>(pub R);

impl<R: Redactor + Send + Sync> Transform<Document> for Redact<R> {
    fn apply(&self, mut document: Document) -> Result<Document, TransformError> {
        document.node = document.node.redacted(&self.0);
        Ok(document)
    }
}

/// A compiled JSONPath expression, kept as text so the transform is `Sync`.
#[derive(Clone, Debug)]
struct Expression(String);

impl Expression {
    fn new(expression: &str) -> Result<Self, SelectError> {
        Selectors::default().compile("jsonpath", expression)?;
        Ok(Expression(expression.to_string()))
    }

    fn hits<'a>(&self, node: &'a Node) -> Result<Vec<(Path, &'a Node)>, TransformError> {
        let to_error = |e: SelectError| TransformError::new(e.to_string());
        Selectors::default()
            .compile("jsonpath", &self.0)
            .map_err(to_error)?
            .select(node)
            .map_err(to_error)
    }
}

/// Narrows a document to what a JSONPath selects. One hit becomes the root,
/// labelled with its path; several (or none) become a map keyed by path.
#[derive(Clone, Debug)]
pub struct Select(Expression);

impl Select {
    pub fn new(expression: &str) -> Result<Self, SelectError> {
        Expression::new(expression).map(Select)
    }
}

impl Transform<Document> for Select {
    fn apply(&self, mut document: Document) -> Result<Document, TransformError> {
        let hits: Vec<_> = self
            .0
            .hits(&document.node)?
            .into_iter()
            .map(|(path, hit)| (path.to_string(), hit.clone()))
            .collect();
        document.node = match <[_; 1]>::try_from(hits) {
            Ok([(path, hit)]) => {
                document.label = Some(if path.is_empty() { "$".into() } else { path });
                hit
            }
            Err(hits) => Node::new(Value::Map(hits)),
        };
        // Paths no longer name the same nodes.
        document.highlights.clear();
        Ok(document)
    }
}

/// Keeps what a JSONPath selects and the containers above it, and drops the
/// rest, so the document keeps its shape. Sequences are renumbered.
#[derive(Clone, Debug)]
pub struct Filter(Expression);

impl Filter {
    pub fn new(expression: &str) -> Result<Self, SelectError> {
        Expression::new(expression).map(Filter)
    }
}

impl Transform<Document> for Filter {
    fn apply(&self, mut document: Document) -> Result<Document, TransformError> {
        let keep: HashSet<Path> = self
            .0
            .hits(&document.node)?
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        let root = &document.node;
        document.node = prune(root, &Path::root(), &keep).unwrap_or_else(|| {
            let empty = match root.value {
                Value::Seq(_) => Value::Seq(Vec::new()),
                _ => Value::Map(Vec::new()),
            };
            Node {
                value: empty,
                meta: root.meta.clone(),
            }
        });
        document.highlights.clear();
        Ok(document)
    }
}

fn prune(node: &Node, path: &Path, keep: &HashSet<Path>) -> Option<Node> {
    if keep.contains(path) {
        return Some(node.clone());
    }
    let value = match &node.value {
        Value::Seq(items) => {
            let kept: Vec<Node> = items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| prune(item, &path.child_index(i), keep))
                .collect();
            (!kept.is_empty()).then_some(Value::Seq(kept))?
        }
        Value::Map(entries) => {
            let kept: Vec<(String, Node)> = entries
                .iter()
                .filter_map(|(key, item)| {
                    prune(item, &path.child_key(key), keep).map(|item| (key.clone(), item))
                })
                .collect();
            (!kept.is_empty()).then_some(Value::Map(kept))?
        }
        _ => return None,
    };
    Some(Node {
        value,
        meta: node.meta.clone(),
    })
}

/// Styles the tree lines of what a JSONPath selects.
#[derive(Clone, Debug)]
pub struct Highlight {
    expression: Expression,
    style: Style,
}

impl Highlight {
    pub fn new(expression: &str, style: Style) -> Result<Self, SelectError> {
        Ok(Highlight {
            expression: Expression::new(expression)?,
            style,
        })
    }
}

impl Transform<Document> for Highlight {
    fn apply(&self, mut document: Document) -> Result<Document, TransformError> {
        let paths: Vec<Path> = self
            .expression
            .hits(&document.node)?
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        document
            .highlights
            .extend(paths.into_iter().map(|path| (path, self.style.clone())));
        Ok(document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{parse, Format, Redaction};
    use crate::transform::Pipeline;

    fn doc(json: &str) -> Document {
        Document::new(parse(Format::Json, json).unwrap())
    }

    #[test]
    fn select_narrows_and_labels() {
        let one = Select::new("$.a.b")
            .unwrap()
            .apply(doc(r#"{"a": {"b": [1]}}"#).label("file"))
            .unwrap();
        assert_eq!(one.label.as_deref(), Some("a.b"));
        assert_eq!(one.node.to_json(), serde_json::json!([1]));

        let root = Select::new("$").unwrap().apply(doc("[1]")).unwrap();
        assert_eq!(root.label.as_deref(), Some("$"));

        let many = Select::new("$..x")
            .unwrap()
            .apply(doc(r#"{"x": 1, "y": {"x": 2}}"#).label("file"))
            .unwrap();
        assert_eq!(many.label.as_deref(), Some("file"));
        assert_eq!(many.node.to_json(), serde_json::json!({"x": 1, "y.x": 2}));
        assert!(Select::new("$[").is_err());
    }

    #[test]
    fn filter_keeps_shape() {
        let filtered = Filter::new("$..name")
            .unwrap()
            .apply(doc(
                r#"{"a": [{"name": "x", "age": 1}, {"age": 2}], "b": 3}"#,
            ))
            .unwrap();
        assert_eq!(
            filtered.node.to_json(),
            serde_json::json!({"a": [{"name": "x"}]})
        );

        let none = Filter::new("$.missing")
            .unwrap()
            .apply(doc("[1, 2]"))
            .unwrap();
        assert_eq!(none.node.to_json(), serde_json::json!([]));
    }

    #[test]
    fn highlight_styles_selected_lines() {
        let pipeline = Pipeline::new()
            .then("redact", Redact(Redaction::secrets()))
            .then(
                "highlight",
                Highlight::new("$.b", Style::parse("reverse").unwrap()).unwrap(),
            );
        let document = pipeline
            .apply(doc(r#"{"a": 1, "b": 2, "password": "x"}"#).label("doc"))
            .unwrap();
        assert_eq!(document.highlights.len(), 1);
        let console = rich::Console::builder()
            .width(40)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Standard))
            .build();
        let out = console.render_to_string(&document.explorer());
        let line = out.lines().find(|l| l.contains('b')).unwrap();
        assert!(line.contains("\x1b[7m"), "{line:?}");
        assert!(!out
            .lines()
            .find(|l| l.contains("a"))
            .unwrap()
            .contains("\x1b[7m"));
        assert!(out.contains("********"));
    }
}
