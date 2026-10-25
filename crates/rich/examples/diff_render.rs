use std::io::{self, BufRead};

#[path = "../tests/support/pyvalue.rs"]
mod pyvalue;

use rich::json::JsonOptions;
use rich::markdown::Markdown;
use rich::panel::Panel;
use rich::pretty::Pretty;
use rich::r#box::{ASCII, DOUBLE, HEAVY, HEAVY_HEAD, MINIMAL, ROUNDED, SQUARE};
use rich::{
    Align, ColorSystem, Console, HorizontalAlign, Justify, Overflow, Padding, Rule, Style, Table,
    Text,
};
use rich::{Columns, Json, ProgressColumn, Tree};
use serde_json::{json, Value};

fn box_set(name: &str) -> Result<rich::r#box::Box, String> {
    match name {
        "square" => Ok(SQUARE),
        "rounded" => Ok(ROUNDED),
        "heavy" => Ok(HEAVY),
        "double" => Ok(DOUBLE),
        "ascii" => Ok(ASCII),
        "minimal" => Ok(MINIMAL),
        "heavy_head" => Ok(HEAVY_HEAD),
        other => Err(format!("unknown box {other:?}")),
    }
}

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

fn horizontal(name: &str) -> Result<HorizontalAlign, String> {
    match name {
        "left" => Ok(HorizontalAlign::Left),
        "center" => Ok(HorizontalAlign::Center),
        "right" => Ok(HorizontalAlign::Right),
        other => Err(format!("unknown align {other:?}")),
    }
}

fn flag(case: &Value, name: &str, default: bool) -> bool {
    case.get(name).and_then(Value::as_bool).unwrap_or(default)
}

fn table(case: &Value) -> Result<Table, String> {
    let box_type = case
        .get("box")
        .and_then(Value::as_str)
        .unwrap_or("heavy_head");
    // `box: "none"` is upstream's `box=None` (what `Table.grid` uses).
    let base = if box_type == "none" {
        Table::new().without_box()
    } else {
        Table::new().box_set(box_set(box_type)?)
    };
    let mut table = base
        .show_header(flag(case, "show_header", true))
        .show_lines(flag(case, "show_lines", false))
        .show_edge(flag(case, "show_edge", true))
        .pad_edge(flag(case, "pad_edge", true))
        .expand(flag(case, "expand", false));
    if let Some(title) = case.get("title").and_then(Value::as_str) {
        table = table.title(title);
    }
    let columns = case
        .get("columns")
        .and_then(Value::as_array)
        .ok_or("missing array field \"columns\"")?;
    for column in columns {
        let header = column.get("header").and_then(Value::as_str).unwrap_or("");
        let justify = match column.get("justify").and_then(Value::as_str) {
            Some(name) => justify(name)?,
            None => Justify::Left,
        };
        table.add_column_justify(header, justify);
        if flag(column, "no_wrap", false) {
            table.column_no_wrap();
        }
        let number = |name: &str| column.get(name).and_then(Value::as_u64).map(|v| v as usize);
        if let Some(value) = number("min_width") {
            table.column_min_width(value);
        }
        if let Some(value) = number("max_width") {
            table.column_max_width(value);
        }
        if let Some(value) = number("ratio") {
            table.column_ratio(value);
        }
    }
    for row in case
        .get("rows")
        .and_then(Value::as_array)
        .ok_or("missing array field \"rows\"")?
    {
        let cells: Vec<&str> = row
            .as_array()
            .ok_or("row must be an array")?
            .iter()
            .map(|cell| cell.as_str().unwrap_or(""))
            .collect();
        table.add_row(&cells);
    }
    Ok(table)
}

fn tree(spec: &Value) -> Tree {
    let mut tree = Tree::new(spec["label"].as_str().unwrap_or("")).expanded(
        spec.get("expanded")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    );
    add_children(&mut tree, spec);
    tree
}

fn add_children(tree: &mut Tree, spec: &Value) {
    for child in spec["children"].as_array().into_iter().flatten() {
        let node = tree.add(child["label"].as_str().unwrap_or(""));
        node.set_expanded(
            child
                .get("expanded")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        );
        add_children(node, child);
    }
}

/// The document renderers: Markdown, Tree, Columns, a Progress frame, Pretty
/// and JSON, built as `document_renderable` in scripts/diff_rich.py builds them.
fn document(case: &Value) -> Result<Box<dyn rich::Renderable>, String> {
    let options = case.get("options").cloned().unwrap_or(Value::Null);
    let opt_usize = |name: &str| {
        options
            .get(name)
            .and_then(Value::as_u64)
            .map(|n| n as usize)
    };
    let opt_bool = |name: &str| options.get(name).and_then(Value::as_bool).unwrap_or(false);
    Ok(match field(case, "kind")? {
        "markdown" => {
            let mut markdown =
                Markdown::new(field(case, "source")?).hyperlinks(flag(case, "hyperlinks", true));
            if let Some(value) = case.get("justify").and_then(Value::as_str) {
                markdown = markdown.justify(justify(value)?);
            }
            Box::new(markdown)
        }
        "tree" => {
            let mut tree = tree(&case["tree"]).hide_root(flag(case, "hide_root", false));
            if let Some(style) = case.get("guide_style").and_then(Value::as_str) {
                tree = tree.guide_style(style);
            }
            Box::new(tree)
        }
        "columns" => {
            let items = case["items"]
                .as_array()
                .ok_or("missing array field \"items\"")?
                .iter()
                .map(|item| item.as_str().unwrap_or("").to_string())
                .collect();
            let mut columns = Columns::new(items)
                .equal(flag(case, "equal", false))
                .expand(flag(case, "expand", false))
                .column_first(flag(case, "column_first", false))
                .right_to_left(flag(case, "right_to_left", false));
            if let Some(width) = case.get("column_width").and_then(Value::as_u64) {
                columns = columns.width(width as usize);
            }
            Box::new(columns)
        }
        "progress" => {
            let mut progress = rich::Progress::new()
                .columns(vec![
                    ProgressColumn::Description,
                    ProgressColumn::Bar,
                    ProgressColumn::Percentage,
                ])
                .expand(flag(case, "expand", false));
            for task in case["tasks"]
                .as_array()
                .ok_or("missing array field \"tasks\"")?
            {
                progress.add_task(
                    task["description"].as_str().unwrap_or(""),
                    task["total"].as_f64().unwrap_or(0.0),
                    task["completed"].as_f64().unwrap_or(0.0),
                );
            }
            Box::new(progress)
        }
        "pretty" => Box::new(
            Pretty::from_value(pyvalue::pretty_value(&case["value"]))
                .indent_guides(opt_bool("indent_guides"))
                .expand_all(opt_bool("expand_all"))
                .max_length(opt_usize("max_length"))
                .max_string(opt_usize("max_string"))
                .max_depth(opt_usize("max_depth")),
        ),
        "json" => {
            let mut json_options = JsonOptions::default();
            match options.get("indent") {
                Some(Value::Null) => json_options.indent = None,
                Some(Value::Number(n)) => {
                    json_options.indent = JsonOptions::indent_spaces(n.as_i64().unwrap_or(0))
                }
                Some(Value::String(s)) => json_options.indent = Some(s.clone()),
                _ => {}
            }
            json_options.sort_keys = opt_bool("sort_keys");
            json_options.ensure_ascii = opt_bool("ensure_ascii");
            Box::new(
                Json::with_options(field(case, "source")?, &json_options)
                    .map_err(|e| e.to_string())?,
            )
        }
        other => return Err(format!("unknown kind {other:?}")),
    })
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
        // The strict parser: upstream's `console.print` raises `MarkupError`,
        // and the lenient fallback is a documented divergence (DIVERGENCES §2).
        "markup" => console
            .try_build_text(field(case, "source")?)
            .map(|text| console.render_to_string(&text))
            .map_err(|error| format!("MarkupError: {error}")),
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
        "panel" => {
            let box_type = case.get("box").and_then(Value::as_str).unwrap_or("rounded");
            let mut panel =
                Panel::new(Box::new(Text::new(field(case, "source")?))).box_set(box_set(box_type)?);
            if let Some(title) = case.get("title").and_then(Value::as_str) {
                panel = panel.title(title);
            }
            Ok(console.render_to_string(&panel))
        }
        "table" => Ok(console.render_to_string(&table(case)?)),
        "rule" => {
            let title = field(case, "source")?;
            let mut rule = if title.is_empty() {
                Rule::line()
            } else {
                Rule::new(title)
            };
            if let Some(characters) = case.get("characters").and_then(Value::as_str) {
                rule = rule.characters(characters);
            }
            if let Some(align) = case.get("align").and_then(Value::as_str) {
                rule = rule.align(horizontal(align)?);
            }
            Ok(console.render_to_string(&rule))
        }
        "padding" => {
            let pad: Vec<usize> = case
                .get("pad")
                .and_then(Value::as_array)
                .ok_or("missing array field \"pad\"")?
                .iter()
                .map(|v| v.as_u64().unwrap_or(0) as usize)
                .collect();
            let [top, right, bottom, left] = pad[..] else {
                return Err("pad must have four entries".into());
            };
            let mut padding = Padding::new(
                Box::new(Text::new(field(case, "source")?)),
                (top, right, bottom, left),
            );
            if let Some(style) = case.get("style").and_then(Value::as_str) {
                padding = padding.style(Style::parse(style).map_err(|e| e.to_string())?);
            }
            Ok(console.render_to_string(&padding))
        }
        "align" => {
            let child = Box::new(Text::new(field(case, "source")?));
            let align = match horizontal(field(case, "align")?)? {
                HorizontalAlign::Left => Align::left(child),
                HorizontalAlign::Center => Align::center(child),
                HorizontalAlign::Right => Align::right(child),
            };
            Ok(console.render_to_string(&align))
        }
        _ => document(case).map(|renderable| console.render_to_string(renderable.as_ref())),
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
