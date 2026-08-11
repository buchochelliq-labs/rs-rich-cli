//! JSON pretty-printing.
//!
//! Port of upstream `rich/json.py`. [`Json`] parses a JSON string and renders it
//! with 2-space indentation (matching Python's `json.dumps(indent=2)`) and the
//! default JSON highlight colors.
//!
//! Non-ASCII strings render as UTF-8, matching upstream (`rich.json.JSON`
//! defaults to `ensure_ascii=False`); object keys keep input order (serde_json's
//! `preserve_order`). The one remaining caveat is **number formatting** for
//! exotic values — exponent notation (`1e+20`, `1e-07`) and integers beyond
//! i64/u64 can differ from CPython's `repr`. Custom indent/sort options are
//! deferred — see docs/DIVERGENCES.md.

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
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        self.render_value(&self.value, 0, &mut segments);
        // Fold rather than let the console crop: a long string value used to be
        // cut mid-token, so the printed document was missing data (and, for
        // JSON specifically, was no longer parseable) at exit 0. Upstream keeps
        // every character by wrapping.
        Segment::fold_lines(&segments, options.max_width)
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

    #[test]
    fn non_ascii_stays_utf8_in_input_order() {
        // Upstream's JSON defaults to ensure_ascii=False, so accented/symbol
        // characters render as UTF-8 (not \uXXXX), and keys keep input order.
        // (Byte-parity is guaranteed by the `json_unicode` golden.)
        let out = render("{\"name\": \"caf\u{e9}\", \"emoji\": \"\u{2764}\"}");
        assert!(out.contains("caf\u{e9}"), "café stays UTF-8: {out:?}");
        assert!(out.contains('\u{2764}'), "heart stays UTF-8");
        let name_at = out.find("name").expect("name key present");
        let emoji_at = out.find("emoji").expect("emoji key present");
        assert!(name_at < emoji_at, "keys keep input order");
    }

    #[test]
    fn a_long_value_is_wrapped_rather_than_cropped() {
        // A long string value used to be cut mid-token, so the printed document
        // was missing data -- and, for JSON, no longer parseable -- at exit 0.
        let payload = format!("{{\"k\": \"{}\"}}", "y".repeat(120));
        let console = Console::builder().width(40).no_color(true).build();
        let json = Json::new(&payload).expect("valid json");
        let out = console.render_to_string(&json);
        assert_eq!(
            out.matches('y').count(),
            120,
            "characters were dropped:
{out}"
        );
    }

    /// serde_json's default float parser takes a fast path that can land 1 ULP
    /// from the value in the file, so the rendered number parsed back to a
    /// *different* double. The `float_roundtrip` feature makes parsing exact.
    #[test]
    fn floats_round_trip_exactly() {
        for literal in [
            "-938371.9565467801",
            "0.1",
            "1.7976931348623157e308",
            "5e-324",
            "3.141592653589793",
        ] {
            let json = Json::new(&format!("{{\"v\": {literal}}}")).expect("valid json");
            let console = Console::builder().width(120).no_color(true).build();
            let out = console.render_to_string(&json);
            let rendered: String = out
                .split(':')
                .nth(1)
                .expect("a value after the key")
                .trim()
                .trim_end_matches(['}', ' ', '\n'])
                .to_string();
            let want: f64 = literal.parse().expect("literal parses");
            let got: f64 = rendered
                .parse()
                .unwrap_or_else(|_| panic!("rendered {rendered:?}"));
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "{literal} rendered as {rendered} — a different double"
            );
        }
    }
}
