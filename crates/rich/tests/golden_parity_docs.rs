//! Parity for documents and `scope.py` (workstream 6 of the 0.0.22 plan),
//! against `golden/parity_docs.tsv`, captured by `scripts/parity_docs.py`
//! from real rich 15.0.0. Each fixture line is
//! `name<TAB>json(case)<TAB>json(expected)`; the case is data, rebuilt here.

#[path = "support/pyvalue.rs"]
mod pyvalue;

use std::sync::Arc;

use rich::json::JsonOptions;
use rich::markdown::Markdown;
use rich::pretty::{pretty_repr, Pretty, ReprLimits};
use rich::scope::{render_scope, ScopeOptions};
use rich::table::Cell;
use rich::{ColorSystem, Console, Json, Justify, Overflow, Panel, Renderable, Syntax, Table};
use serde_json::Value;

use pyvalue::pretty_value;

fn console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .highlight(false)
        .safe_box(false)
        .legacy_windows(false)
        .no_color(false)
        .build()
}

fn opt_usize(options: &Value, key: &str) -> Option<usize> {
    options.get(key).and_then(Value::as_u64).map(|n| n as usize)
}

fn opt_bool(options: &Value, key: &str) -> Option<bool> {
    options.get(key).and_then(Value::as_bool)
}

fn justify(options: &Value) -> Option<Justify> {
    options
        .get("justify")
        .and_then(Value::as_str)
        .map(|j| match j {
            "left" => Justify::Left,
            "center" => Justify::Center,
            "right" => Justify::Right,
            "full" => Justify::Full,
            _ => Justify::Default,
        })
}

fn overflow(options: &Value) -> Option<Overflow> {
    options
        .get("overflow")
        .and_then(Value::as_str)
        .map(|o| match o {
            "crop" => Overflow::Crop,
            "ellipsis" => Overflow::Ellipsis,
            "ignore" => Overflow::Ignore,
            _ => Overflow::Fold,
        })
}

fn limits(options: &Value) -> ReprLimits {
    ReprLimits {
        max_length: opt_usize(options, "max_length"),
        max_string: opt_usize(options, "max_string"),
        max_depth: opt_usize(options, "max_depth"),
    }
}

/// `Pretty(value, **options)`.
pub fn pretty(case: &Value) -> Pretty {
    let options = case.get("options").cloned().unwrap_or(Value::Null);
    let limits = limits(&options);
    let mut pretty = Pretty::from_value(pretty_value(&case["value"]))
        .indent_size(opt_usize(&options, "indent_size").unwrap_or(4))
        .justify(justify(&options))
        .overflow(overflow(&options))
        .indent_guides(opt_bool(&options, "indent_guides").unwrap_or(false))
        .max_length(limits.max_length)
        .max_string(limits.max_string)
        .max_depth(limits.max_depth)
        .expand_all(opt_bool(&options, "expand_all").unwrap_or(false))
        .margin(opt_usize(&options, "margin").unwrap_or(0))
        .insert_line(opt_bool(&options, "insert_line").unwrap_or(false));
    if let Some(no_wrap) = options.get("no_wrap") {
        pretty = pretty.no_wrap(no_wrap.as_bool());
    }
    pretty
}

fn render(case: &Value) -> String {
    let options = case.get("options").cloned().unwrap_or(Value::Null);
    let kind = case["kind"].as_str().expect("kind");
    if kind == "pretty_repr" {
        return pretty_repr(
            &pretty_value(&case["value"]),
            opt_usize(&options, "max_width").unwrap_or(80),
            opt_usize(&options, "indent_size").unwrap_or(4),
            limits(&options),
            opt_bool(&options, "expand_all").unwrap_or(false),
        );
    }
    let renderable: Box<dyn Renderable> = match kind {
        "pretty" => Box::new(pretty(case)),
        "syntax" => {
            let mut syntax = match case.get("path").and_then(Value::as_str) {
                Some(path) => Syntax::from_path(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../..")
                        .join(path),
                    Some("text"),
                )
                .expect("readable fixture"),
                None => Syntax::new(case["code"].as_str().expect("code"), "text"),
            };
            syntax = syntax
                .theme("ansi_dark")
                .dedent(opt_bool(&options, "dedent").unwrap_or(false))
                .line_numbers(opt_bool(&options, "line_numbers").unwrap_or(false));
            if let Some(tab_size) = opt_usize(&options, "tab_size") {
                syntax = syntax.tab_size(tab_size);
            }
            if let Some(range) = options.get("line_range").and_then(Value::as_array) {
                syntax = syntax.line_range(range[0].as_i64(), range[1].as_i64());
            }
            Box::new(syntax)
        }
        "markdown" => {
            let mut markdown = Markdown::new(case["source"].as_str().expect("source"));
            if let Some(hyperlinks) = opt_bool(&options, "hyperlinks") {
                markdown = markdown.hyperlinks(hyperlinks);
            }
            if let Some(justify) = justify(&options) {
                markdown = markdown.justify(justify);
            }
            Box::new(markdown)
        }
        "json" => {
            let mut json_options = JsonOptions::default();
            match options.get("indent") {
                Some(Value::Null) => json_options.indent = None,
                Some(Value::Number(n)) => {
                    json_options.indent = JsonOptions::indent_spaces(n.as_i64().expect("int"))
                }
                Some(Value::String(s)) => json_options.indent = Some(s.clone()),
                _ => {}
            }
            json_options.sort_keys = opt_bool(&options, "sort_keys").unwrap_or(false);
            json_options.ensure_ascii = opt_bool(&options, "ensure_ascii").unwrap_or(false);
            json_options.highlight = opt_bool(&options, "highlight").unwrap_or(true);
            json_options.allow_nan = opt_bool(&options, "allow_nan").unwrap_or(true);
            let source = case["source"].as_str().expect("source");
            let json = if case.get("from_data").and_then(Value::as_bool) == Some(true) {
                Json::from_data(&serde_json::from_str(source).expect("data"), &json_options)
            } else {
                Json::with_options(source, &json_options)
            };
            match json {
                Ok(json) => Box::new(json),
                Err(_) => return "<error>".to_string(),
            }
        }
        "scope" => {
            let scope = case["scope"].as_array().expect("scope").iter().map(|pair| {
                (
                    pair[0].as_str().expect("name").to_string(),
                    pretty_value(&pair[1]),
                )
            });
            let scope_options = ScopeOptions {
                title: options
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                sort_keys: opt_bool(&options, "sort_keys").unwrap_or(true),
                indent_guides: opt_bool(&options, "indent_guides").unwrap_or(false),
                max_length: opt_usize(&options, "max_length"),
                max_string: opt_usize(&options, "max_string"),
                max_depth: opt_usize(&options, "max_depth"),
                overflow: overflow(&options),
            };
            Box::new(render_scope(scope, &scope_options))
        }
        other => panic!("unknown case kind {other:?}"),
    };
    let console = console(case.get("width").and_then(Value::as_u64).unwrap_or(40) as usize);
    match case.get("wrap").and_then(Value::as_str) {
        Some("panel_fit") => {
            let panel = Panel::fit(renderable);
            console.capture(|c| c.print(&panel))
        }
        Some("table") => {
            let mut table = Table::new();
            table.add_column("k");
            table.add_column("v");
            table.add_row_cells(vec![
                Cell::Markup("x".into()),
                Cell::Renderable(Arc::new(pretty(case))),
            ]);
            console.capture(|c| c.print(&table))
        }
        _ => console.capture(|c| c.print(renderable.as_ref())),
    }
}

#[test]
fn parity_docs_matches_upstream() {
    let data = include_str!("golden/parity_docs.tsv");
    let mut failures = Vec::new();
    let mut checked = 0;
    for raw in data.lines() {
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let mut fields = raw.splitn(3, '\t');
        let name = fields.next().expect("name");
        let case: Value = serde_json::from_str(fields.next().expect("case")).expect("case json");
        let expected: String =
            serde_json::from_str(fields.next().expect("expected")).expect("expected json");
        let got = render(&case);
        if got != expected {
            failures.push(format!(
                "{name}:\n  expected {expected:?}\n  got      {got:?}"
            ));
        }
        checked += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(checked > 40, "expected every docs parity case to run");
}
