//! INI and dotenv files as a `section | key | value | comment` table.

use std::borrow::Cow;

use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Table, Text};

use super::table::cell_text;
use super::{escape_controls, style, Node, Path, Redactor, Value};

/// A config file as a table: one row per key, grouped by section.
///
/// Built for the trees [`parse_ini`](super::parse_ini) and
/// [`parse_dotenv`](super::parse_dotenv) produce: root entries that are maps
/// are sections, the rest are keys outside any section. The `section` column
/// only appears when there are sections (never for dotenv), and `comment`
/// only when some entry has one. Set a [`Redactor`] to mask secrets.
///
/// ```
/// use rich::Console;
/// use rich_ext::data::{parse_dotenv, ConfigFileView, Redaction};
///
/// let node = parse_dotenv("# the host\nHOST=example.com\nAPI_TOKEN=abc123\n").unwrap();
/// let view = ConfigFileView::new(&node).redactor(Redaction::secrets());
/// let out = Console::builder().width(50).build().render_export(&view);
/// assert!(out.contains("│ HOST      │ example.com │ the host │"), "{out}");
/// assert!(out.contains("│ API_TOKEN │ ********    │"), "{out}");
/// ```
pub struct ConfigFileView<'a> {
    node: Cow<'a, Node>,
    redactor: Option<Box<dyn Redactor + 'a>>,
    title: Option<String>,
}

impl<'a> ConfigFileView<'a> {
    pub fn new(node: impl Into<Cow<'a, Node>>) -> Self {
        ConfigFileView {
            node: node.into(),
            redactor: None,
            title: None,
        }
    }

    /// Mask values with `redactor` before display.
    pub fn redactor(mut self, redactor: impl Redactor + 'a) -> Self {
        self.redactor = Some(Box::new(redactor));
        self
    }

    /// A title above the table (plain text).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// `(section, path, key, node)` rows.
    fn rows(&self) -> Vec<(Option<&str>, Path, &str, &Node)> {
        let mut rows = Vec::new();
        let Value::Map(entries) = &self.node.value else {
            return rows;
        };
        for (key, node) in entries {
            match &node.value {
                Value::Map(section) => {
                    for (inner, value) in section {
                        let path = Path::root().child_key(key).child_key(inner);
                        rows.push((Some(key.as_str()), path, inner.as_str(), value));
                    }
                }
                _ => rows.push((None, Path::root().child_key(key), key.as_str(), node)),
            }
        }
        rows
    }
}

impl Renderable for ConfigFileView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let rows = self.rows();
        let sections = rows.iter().any(|(section, ..)| section.is_some());
        let comments = rows.iter().any(|(.., node)| node.meta.comment.is_some());
        let mut table = Table::new();
        if let Some(title) = &self.title {
            table = table.title(rich::markup::escape(title));
        }
        if sections {
            table.add_column_text(Text::new("section"), Justify::Left);
        }
        table.add_column_text(Text::new("key"), Justify::Left);
        table.add_column_text(Text::new("value"), Justify::Left);
        if comments {
            table.add_column_text(Text::new("comment"), Justify::Left);
        }
        let mut previous: Option<Option<&str>> = None;
        for (section, path, key, node) in rows {
            let shown: Cow<'_, Node> =
                match self.redactor.as_ref().and_then(|r| r.redact(&path, node)) {
                    Some(masked) => Cow::Owned(masked),
                    None => Cow::Borrowed(node),
                };
            let mut row = Vec::new();
            if sections {
                // Name each section once, on its first row.
                let name = if previous == Some(section) {
                    ""
                } else {
                    section.unwrap_or("")
                };
                row.push(Text::styled(
                    escape_controls(name),
                    style(console, "data.section"),
                ));
            }
            previous = Some(section);
            row.push(Text::styled(
                escape_controls(key),
                style(console, "json.key"),
            ));
            row.push(match &shown.value {
                Value::String(s) => Text::new(escape_controls(s)),
                _ => cell_text(console, &shown, None),
            });
            if comments {
                let comment = node.meta.comment.as_deref().unwrap_or("");
                row.push(Text::styled(
                    escape_controls(&comment.replace('\n', " ")),
                    style(console, "data.comment"),
                ));
            }
            table.add_row_text(row);
        }
        table.rich_render(console, options)
    }
}
