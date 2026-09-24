//! Masking secrets before a document is shown.

use super::search::{find, wildcard};
use super::{Node, Path, Value};

/// Decides whether (and how) to replace a node before display.
///
/// Called for every node, parents first; a replacement is used as-is and its
/// children are not visited. [`Redaction`] implements it; implement it
/// yourself for other rules (by path, by value shape, …).
pub trait Redactor {
    /// The node to show instead of `node`, or `None` to keep it.
    fn redact(&self, path: &Path, node: &Node) -> Option<Node>;
}

impl<F: Fn(&Path, &Node) -> Option<Node>> Redactor for F {
    fn redact(&self, path: &Path, node: &Node) -> Option<Node> {
        self(path, node)
    }
}

/// Mask string and number leaves whose key matches a pattern.
///
/// A leaf's key is the last key on its path, so the items of a `tokens`
/// list are masked too. Patterns match case-insensitively: as a substring,
/// or as a whole-key glob when they contain `*` or `?`. Masked leaves become
/// the mask string and keep their metadata; booleans and nulls are left
/// alone, and so is the structure.
///
/// ```
/// use rich_ext::data::{parse, Format, Redaction, Value};
///
/// let node = parse(Format::Json, r#"{"db": {"user": "u", "Password": "hunter2"}}"#).unwrap();
/// let safe = node.redacted(&Redaction::secrets());
/// assert_eq!(safe.get("db").unwrap().get("Password").unwrap().value, Value::String("********".into()));
/// assert_eq!(safe.get("db").unwrap().get("user").unwrap().value, Value::String("u".into()));
/// ```
#[derive(Clone, Debug)]
pub struct Redaction {
    patterns: Vec<String>,
    mask: String,
}

/// The key fragments [`Redaction::secrets`] masks. Shared with the text
/// detectors in [`crate::redact`], where it is defined so that builds
/// without the `data` feature have it too.
pub use crate::redact::SECRET_KEYS;

impl Default for Redaction {
    fn default() -> Self {
        Redaction {
            patterns: Vec::new(),
            mask: "********".into(),
        }
    }
}

impl Redaction {
    /// No patterns, mask `********`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Common secret key names (see [`SECRET_KEYS`]).
    pub fn secrets() -> Self {
        Self::new().patterns(SECRET_KEYS.iter().copied())
    }

    /// Add a key pattern.
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.push(pattern.into());
        self
    }

    /// Add key patterns.
    pub fn patterns<S: Into<String>>(mut self, patterns: impl IntoIterator<Item = S>) -> Self {
        self.patterns.extend(patterns.into_iter().map(Into::into));
        self
    }

    /// The replacement text.
    pub fn mask(mut self, mask: impl Into<String>) -> Self {
        self.mask = mask.into();
        self
    }

    /// Whether `key` matches a pattern.
    pub fn matches_key(&self, key: &str) -> bool {
        self.patterns.iter().any(|pattern| {
            if pattern.contains(['*', '?']) {
                wildcard(pattern, key, true)
            } else {
                find(key, pattern, true).is_some()
            }
        })
    }
}

impl Redactor for Redaction {
    fn redact(&self, path: &Path, node: &Node) -> Option<Node> {
        let maskable = matches!(
            node.value,
            Value::String(_)
                | Value::DateTime(_)
                | Value::Int(_)
                | Value::UInt(_)
                | Value::Float(_)
        );
        (maskable && path.last_key().is_some_and(|key| self.matches_key(key)))
            .then(|| Node::with_meta(Value::String(self.mask.clone()), node.meta.clone()))
    }
}

/// `node` with `redactor` applied throughout.
pub(crate) fn apply(node: &Node, redactor: &dyn Redactor) -> Node {
    fn go(node: &Node, path: &mut Path, redactor: &dyn Redactor) -> Node {
        if let Some(replacement) = redactor.redact(path, node) {
            return replacement;
        }
        let value = match &node.value {
            Value::Seq(items) => Value::Seq(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        path.push(super::PathSegment::Index(i));
                        let out = go(item, path, redactor);
                        path.0.pop();
                        out
                    })
                    .collect(),
            ),
            Value::Map(entries) => Value::Map(
                entries
                    .iter()
                    .map(|(key, value)| {
                        path.push(super::PathSegment::Key(key.clone()));
                        let out = go(value, path, redactor);
                        path.0.pop();
                        (key.clone(), out)
                    })
                    .collect(),
            ),
            other => other.clone(),
        };
        Node::with_meta(value, node.meta.clone())
    }
    go(node, &mut Path::root(), redactor)
}
