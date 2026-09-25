//! Parity for edge values found by the second core audit, against
//! `golden/audit_edges.tsv` (captured by `AUDIT_EDGE_CASES` in
//! `scripts/capture_golden.py` from real rich 15.0.0): Layout `size=0` is
//! flexible, Syntax line numbers past 64 bits, LiveRender's ellipsis at
//! height 0, a zero-width Panel, lone surrogates in JSON under
//! `ensure_ascii`, and extreme SVG `font_aspect_ratio`s. Each fixture line is
//! `name<TAB>json(expected)`; every name has a Rust builder below.

use rich::json::JsonOptions;
use rich::terminal_theme::SVG_EXPORT_THEME;
use rich::{
    ColorSystem, Console, Json, Layout, LiveRender, Panel, Renderable, Syntax, Text,
    VerticalOverflow,
};

/// `_cg_console`: truecolor, no highlighting.
fn builder(width: usize) -> rich::console::ConsoleBuilder {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .highlight(false)
        .no_color(false)
        .legacy_windows(false)
}

fn print(console: &Console, renderable: &dyn Renderable) -> String {
    console.capture(|c| c.print(renderable))
}

const UF_CODE: &str =
    "def f(x):\n    if x:\n        return 'a very long line of code'\n\n    return x\n";

/// `_ag_syntax(**options)`.
fn ag_syntax() -> Syntax {
    Syntax::new(UF_CODE, "text").theme("ansi_dark")
}

fn ae_layout(row: bool) -> String {
    let leaf = |s: &str| Layout::with_renderable(Box::new(Text::new(s)));
    let children = vec![leaf("a").size(0), leaf("b"), leaf("c").size(0)];
    let mut layout = Layout::new();
    if row {
        layout.split_row(children);
    } else {
        layout.split_column(children);
    }
    print(&builder(10).height(4).build(), &layout)
}

fn ae_syntax_extremes() -> String {
    let (max, min) = (i64::MAX, i64::MIN);
    let options: Vec<Box<dyn Fn(Syntax) -> Syntax>> = vec![
        Box::new(move |s| s.line_numbers(true).start_line(max)),
        Box::new(move |s| s.line_numbers(true).start_line(max).highlight_lines([max])),
        Box::new(move |s| s.line_numbers(true).start_line(min)),
        Box::new(move |s| s.line_numbers(true).line_range(Some(min), None)),
        Box::new(move |s| s.line_numbers(true).line_range(None, Some(max))),
        Box::new(move |s| s.line_numbers(true).line_range(Some(max), None)),
        Box::new(move |s| s.line_range(Some(min), Some(min))),
        Box::new(move |s| s.line_range(Some(2), Some(min)).word_wrap(true)),
    ];
    let console = builder(30).build();
    let mut out: String = options
        .iter()
        .map(|option| print(&console, &option(ag_syntax())))
        .collect();
    let mut syntax = ag_syntax().line_numbers(true);
    syntax.stylize_range("reverse", (1, 2), (2, max), false);
    syntax.stylize_range("bold", (2, max), (3, max), false);
    syntax.stylize_range("underline", (max, 0), (max, 1), false);
    out.push_str(&print(&console, &syntax));
    out
}

fn ae_live(overflow: VerticalOverflow) -> String {
    let live = LiveRender::new(Box::new(Text::new("1\n2\n3\n4"))).vertical_overflow(overflow);
    print(&builder(10).height(0).build(), &Panel::new(Box::new(live)))
}

fn ae_svg(ratio: f64) -> String {
    let console = builder(20).build();
    match console.export_svg_with(&SVG_EXPORT_THEME, "t", "U", None, ratio, |c| {
        c.print_str("[red]hi[/] [b]x[/]")
    }) {
        Ok(svg) => svg,
        // Upstream's `ceil` raises `OverflowError` for infinity and
        // `ValueError` for NaN.
        Err(error) if error.0.contains("infinity") => "<OverflowError>".to_string(),
        Err(_) => "<ValueError>".to_string(),
    }
}

fn build(name: &str) -> String {
    let text = |s: &str| -> Box<dyn Renderable> { Box::new(Text::new(s)) };
    match name {
        "layout_size_zero_row" => ae_layout(true),
        "layout_size_zero_column" => ae_layout(false),
        "syntax_extreme_lines" => ae_syntax_extremes(),
        "live_ellipsis_h0" => ae_live(VerticalOverflow::Ellipsis),
        "live_crop_h0" => ae_live(VerticalOverflow::Crop),
        "live_visible_h0" => ae_live(VerticalOverflow::Visible),
        "panel_width0" => {
            let console = builder(10).build();
            let panels = [
                Panel::new(text("a")).width(0),
                Panel::fit(text("a")).width(0),
                Panel::new(text("a")).width(0).title("T").subtitle("S"),
                Panel::new(text("a")).width(1),
                Panel::new(text("a")).width(2).border_style("red"),
            ];
            panels.iter().map(|panel| print(&console, panel)).collect()
        }
        "json_lone_surrogate_ascii" => {
            let source = r#"["\ud800", "\udc00x\ud83d\ude00", "\ud800\u0041\udbff", {"\ue000": 1, "\ud800": 2, "\ud7ff": 3}]"#;
            let options = JsonOptions {
                ensure_ascii: true,
                sort_keys: true,
                ..Default::default()
            };
            let json = Json::with_options(source, &options).expect("Python's json accepts it");
            print(&builder(40).build(), &json)
        }
        "svg_font_aspect_ratio" => [1e300, 1e20, -1.0, 1e-7, 0.0, 2.5]
            .into_iter()
            .map(|ratio| ae_svg(ratio) + "\n")
            .collect(),
        "svg_font_aspect_ratio_errors" => [1e308, f64::INFINITY, f64::NEG_INFINITY, f64::NAN]
            .into_iter()
            .map(ae_svg)
            .collect(),
        other => panic!("no Rust builder for audit edge case {other:?}"),
    }
}

#[test]
fn audit_edges_parity() {
    let data = include_str!("golden/audit_edges.tsv");
    let mut failures = Vec::new();
    let mut checked = 0;
    for raw in data.lines() {
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let (name, expected) = raw.split_once('\t').expect("name<TAB>json");
        let expected: String = serde_json::from_str(expected).expect("json string");
        let got = build(name);
        if got != expected {
            let at = expected
                .char_indices()
                .zip(got.chars())
                .find(|((_, a), b)| a != b)
                .map_or(expected.len().min(got.len()), |((at, _), _)| at);
            let from = expected.floor_char_boundary(at.saturating_sub(160));
            let near = |text: &str| {
                let start = text.floor_char_boundary(from.min(text.len()));
                let end = text.floor_char_boundary((at + 160).min(text.len()));
                text[start..end].to_string()
            };
            failures.push(format!(
                "{name} (first difference at byte {at}):\n  expected …{:?}…\n  got      …{:?}…",
                near(&expected),
                near(&got)
            ));
        }
        checked += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(checked, 10, "expected every audit edge case to run");
}
