use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use std::time::Instant;

use rich::{r#box::SQUARE, ColorSystem, Console, Justify, Panel, Style, Table, Text};
use serde_json::json;

fn digest(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .highlight(false)
        .no_color(false)
        .build()
}

fn table(rows: usize) -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("id");
    table.add_column("name");
    table.add_column("note");
    for index in 0..rows {
        let id = index.to_string();
        let name = format!("item-{index}");
        let note = format!("note {} wraps through the table layout engine", index % 17);
        table.add_row(&[&id, &name, &note]);
    }
    table
}

fn measure<T>(
    name: &str,
    runs: usize,
    warmups: usize,
    mut render: impl FnMut() -> T,
) -> serde_json::Value {
    for _ in 0..warmups {
        black_box(render());
    }
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        black_box(render());
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let mut sorted = samples.clone();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    json!({"name": name, "samples_ms": samples, "median_ms": median})
}

fn parse_count(args: &[String], flag: &str, default: usize) -> usize {
    args.windows(2)
        .find_map(|pair| (pair[0] == flag).then(|| pair[1].parse().ok()).flatten())
        .unwrap_or(default)
}

fn parse_string(args: &[String], flag: &str, default: &str) -> String {
    args.windows(2)
        .find_map(|pair| (pair[0] == flag).then(|| pair[1].clone()))
        .unwrap_or_else(|| default.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runs = parse_count(&args, "--runs", 20);
    let warmups = parse_count(&args, "--warmups", 3);
    let revision = parse_string(&args, "--revision", "unknown");

    let markup = "[bold red]error[/]: [#ff8800]value[/] ".repeat(128);
    let started = Instant::now();
    let parsed = Text::from_markup(&markup).expect("valid benchmark markup");
    let setup_markup_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut markup_case = measure("markup_parse", runs, warmups, || {
        Text::from_markup(black_box(&markup)).expect("valid benchmark markup")
    });
    markup_case["setup_ms"] = json!(setup_markup_ms);
    markup_case["output_hash"] = json!(digest(&markup));

    let render_console = console(100);
    let text = Text::styled(
        "hello world ".repeat(512),
        Style::parse("bold blue").expect("valid style"),
    )
    .justify(Justify::Full);
    let text_output = render_console.render_to_string(&text);
    let mut text_case = measure("text_wrap_justify", runs, warmups, || {
        render_console.render_to_string(black_box(&text))
    });
    text_case["output_bytes"] = json!(text_output.len());
    text_case["output_hash"] = json!(digest(&text_output));

    let started = Instant::now();
    let rows = table(500);
    let setup_table_ms = started.elapsed().as_secs_f64() * 1000.0;
    let table_output = render_console.render_to_string(&rows);
    let mut table_case = measure("table_layout", runs, warmups, || {
        render_console.render_to_string(black_box(&rows))
    });
    table_case["setup_ms"] = json!(setup_table_ms);
    table_case["output_bytes"] = json!(table_output.len());
    table_case["output_hash"] = json!(digest(&table_output));

    let panel = Panel::new(Box::new(parsed))
        .title("[bold]Benchmark[/]")
        .box_set(SQUARE);
    let panel_output = render_console.render_to_string(&panel);
    let mut console_case = measure("console_render", runs, warmups, || {
        render_console.render_to_string(black_box(&panel))
    });
    console_case["output_bytes"] = json!(panel_output.len());
    console_case["output_hash"] = json!(digest(&panel_output));

    println!(
        "{}",
        json!({
            "revision": revision,
            "runs": runs,
            "warmup_runs": warmups,
            "width": 100,
            "color_system": "truecolor",
            "features": {"syntax_cache": cfg!(feature = "syntax-cache")},
            "cases": [markup_case, text_case, table_case, console_case],
        })
    );
}
