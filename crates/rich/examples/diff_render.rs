use std::io::{self, BufRead};

use rich::{ColorSystem, Console, Justify, Overflow, Style, Text};
use serde_json::{json, Value};

fn color_system(name: &str) -> Result<ColorSystem, String> {
    match name {
        "truecolor" => Ok(ColorSystem::Truecolor),
        "256" => Ok(ColorSystem::EightBit),
        "standard" => Ok(ColorSystem::Standard),
        other => Err(format!("unknown color_system {other:?}")),
    }
}

fn overflow(name: &str) -> Result<Overflow, String> {
    match name {
        "fold" => Ok(Overflow::Fold),
        "crop" => Ok(Overflow::Crop),
        "ellipsis" => Ok(Overflow::Ellipsis),
        "ignore" => Ok(Overflow::Ignore),
        other => Err(format!("unknown overflow {other:?}")),
    }
}

fn justify(name: &str) -> Result<Justify, String> {
    match name {
        "default" => Ok(Justify::Default),
        "left" => Ok(Justify::Left),
        "center" => Ok(Justify::Center),
        "right" => Ok(Justify::Right),
        "full" => Ok(Justify::Full),
        other => Err(format!("unknown justify {other:?}")),
    }
}

fn field<'a>(case: &'a Value, name: &str) -> Result<&'a str, String> {
    case.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string field {name:?}"))
}

fn render(case: &Value) -> Result<String, String> {
    let width = case
        .get("width")
        .and_then(Value::as_u64)
        .ok_or_else(|| "missing integer field \"width\"".to_string())? as usize;
    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(color_system(field(case, "color_system")?)?))
        .width(width)
        .highlight(false)
        .safe_box(
            case.get("safe_box")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .ascii_only(
            case.get("ascii_only")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .no_color(false)
        .build();

    match field(case, "kind")? {
        "markup" => Ok(console.render_str_to_string(field(case, "source")?)),
        "text" => {
            let mut text = if let Some(style) = case.get("style").and_then(Value::as_str) {
                Text::styled(
                    field(case, "source")?,
                    Style::parse(style).map_err(|e| e.to_string())?,
                )
            } else {
                Text::new(field(case, "source")?)
            };
            if let Some(value) = case.get("overflow").and_then(Value::as_str) {
                text.set_overflow(Some(overflow(value)?));
            }
            if let Some(value) = case.get("no_wrap").and_then(Value::as_bool) {
                text.set_no_wrap(Some(value));
            }
            if let Some(value) = case.get("justify").and_then(Value::as_str) {
                text.set_justify(justify(value)?);
            }
            Ok(console.render_to_string(&text))
        }
        other => Err(format!("unknown kind {other:?}")),
    }
}

fn main() {
    for raw in io::stdin().lock().lines() {
        let raw = match raw {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => continue,
            Err(error) => {
                println!("{}", json!({"ok": false, "error": error.to_string()}));
                continue;
            }
        };
        let output = match serde_json::from_str::<Value>(&raw) {
            Ok(case) => match render(&case) {
                Ok(output) => json!({"ok": true, "output": output}),
                Err(error) => json!({"ok": false, "error": error}),
            },
            Err(error) => json!({"ok": false, "error": error.to_string()}),
        };
        println!("{output}");
    }
}
