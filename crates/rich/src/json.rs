//! JSON pretty-printing.
//!
//! Port of upstream `rich/json.py`. [`Json`] parses a JSON string and renders it
//! with 2-space indentation (matching Python's `json.dumps(indent=2)`) and the
//! default JSON highlight colors.
//!
//! Slice scope: ASCII input with the default 2-space indent. Non-ASCII escaping
//! (`ensure_ascii`) and custom indent/sort options are deferred — see
//! docs/DIVERGENCES.md.

use serde_json::Value;

use crate::console::{Console, ConsoleOptions};
use crate::errors::{Result, RichError};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// A parsed JSON document, rendered with syntax highlighting. Mirrors `rich.json.JSON`.
pub struct Json {
    value: Value,
    styles: JsonStyles,
}

struct JsonStyles {
    brace: Style,
    key: Style,
    string: Style,
    number: Style,
    bool_true: Style,
    bool_false: Style,
    null: Style,
}

impl JsonStyles {
    fn defaults() -> Self {
        let s = |spec: &str| Style::parse(spec).expect("valid built-in style");
        JsonStyles {
            brace: s("bold"),
            key: s("bold blue"),
            string: s("green"),
            number: s("bold cyan"),
            bool_true: s("italic bright_green"),
            bool_false: s("italic bright_red"),
            null: s("italic magenta"),
        }
    }
}

impl Json {
    /// Parse `text` as JSON. Returns an error if it is not valid JSON.
    pub fn new(text: &str) -> Result<Self> {
        let value = serde_json::from_str(text).map_err(|e| RichError::Json(e.to_string()))?;
        Ok(Json {
            value,
            styles: JsonStyles::defaults(),
        })
    }

    fn render_value(&self, value: &Value, level: usize, out: &mut Vec<Segment>) {
        let brace = |text: &str| Segment::new(text.to_string(), Some(self.styles.brace.clone()));
        let plain = |text: String| Segment::new(text, None);
        match value {
            Value::Null => out.push(Segment::new(
                "null".to_string(),
                Some(self.styles.null.clone()),
            )),
            Value::Bool(true) => out.push(Segment::new(
                "true".to_string(),
                Some(self.styles.bool_true.clone()),
            )),
            Value::Bool(false) => out.push(Segment::new(
                "false".to_string(),
                Some(self.styles.bool_false.clone()),
            )),
            Value::Number(number) => out.push(Segment::new(
                number.to_string(),
                Some(self.styles.number.clone()),
            )),
            Value::String(string) => out.push(Segment::new(
                quote(string),
                Some(self.styles.string.clone()),
            )),
            Value::Array(items) => {
                out.push(brace("["));
                if items.is_empty() {
                    out.push(brace("]"));
                    return;
                }
                out.push(plain("\n".to_string()));
                let last = items.len() - 1;
                for (index, item) in items.iter().enumerate() {
                    out.push(plain("  ".repeat(level + 1)));
                    self.render_value(item, level + 1, out);
                    if index != last {
                        out.push(plain(",".to_string()));
                    }
                    out.push(plain("\n".to_string()));
                }
                out.push(plain("  ".repeat(level)));
                out.push(brace("]"));
            }
            Value::Object(map) => {
                out.push(brace("{"));
                if map.is_empty() {
                    out.push(brace("}"));
                    return;
                }
                out.push(plain("\n".to_string()));
                let last = map.len() - 1;
                for (index, (key, item)) in map.iter().enumerate() {
                    out.push(plain("  ".repeat(level + 1)));
                    out.push(Segment::new(quote(key), Some(self.styles.key.clone())));
                    out.push(plain(": ".to_string()));
                    self.render_value(item, level + 1, out);
                    if index != last {
                        out.push(plain(",".to_string()));
                    }
                    out.push(plain("\n".to_string()));
                }
                out.push(plain("  ".repeat(level)));
                out.push(brace("}"));
            }
        }
    }
}

impl Renderable for Json {
    fn rich_render(&self, _console: &Console, _options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        self.render_value(&self.value, 0, &mut segments);
        segments
    }
}

/// Serialize a string as a JSON string literal (quoted + escaped).
fn quote(string: &str) -> String {
    serde_json::to_string(string).unwrap_or_else(|_| format!("{string:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(text: &str) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .build();
        console.render_to_string(&Json::new(text).unwrap())
    }

    #[test]
    fn empty_collections_stay_inline() {
        assert_eq!(render("{}"), "\x1b[1m{\x1b[0m\x1b[1m}\x1b[0m");
        assert_eq!(render("[]"), "\x1b[1m[\x1b[0m\x1b[1m]\x1b[0m");
    }

    #[test]
    fn object_with_scalars() {
        assert_eq!(
            render(r#"{"ok": false}"#),
            "\x1b[1m{\x1b[0m\n  \x1b[1;34m\"ok\"\x1b[0m: \x1b[3;91mfalse\x1b[0m\n\x1b[1m}\x1b[0m"
        );
    }

    #[test]
    fn invalid_json_errors() {
        assert!(Json::new("{not json}").is_err());
    }
}
