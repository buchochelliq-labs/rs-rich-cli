//! Edge values from the second core audit: degenerate sizes and extreme
//! numbers render (or error) without panicking, where rich renders or
//! raises. Byte parity for the values rich defines lives in
//! `golden_audit_edges.rs`.

use std::panic::{catch_unwind, AssertUnwindSafe};

use rich::syntax::Syntax;
use rich::{
    Align, ColorSystem, Columns, Console, Layout, LiveRender, Panel, Renderable, Rule, Segment,
    Table, Text, Tree, VerticalAlign,
};

fn console(width: usize, height: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .height(height)
        .highlight(false)
        .no_color(false)
        .legacy_windows(false)
        .build()
}

type Build = Box<dyn Fn() -> Box<dyn Renderable>>;

/// Degenerate inputs must not panic (rich raises Python exceptions for some;
/// others render).
#[test]
fn degenerate_inputs_do_not_panic() {
    let mut panics = Vec::new();
    let cases: Vec<(&str, usize, usize, Build)> = vec![
        (
            "columns_width0",
            10,
            5,
            Box::new(|| Box::new(Columns::new(vec!["a".into(), "b".into()]).width(0))),
        ),
        (
            "columns_width_big",
            10,
            5,
            Box::new(|| Box::new(Columns::new(vec!["a".into(), "b".into()]).width(30))),
        ),
        (
            "columns_pad_big",
            10,
            5,
            Box::new(|| {
                Box::new(Columns::new(vec!["a".into(), "b".into()]).padding((0, 50, 0, 50)))
            }),
        ),
        (
            "columns_w0",
            0,
            5,
            Box::new(|| Box::new(Columns::new(vec!["a".into(), "b".into()]))),
        ),
        (
            "table_pad_big",
            10,
            5,
            Box::new(|| {
                let mut t = Table::new().padding(0, 40, 0, 40);
                t.add_column("a");
                t.add_row(&["x"]);
                Box::new(t)
            }),
        ),
        (
            "table_w0",
            0,
            5,
            Box::new(|| {
                let mut t = Table::new().show_footer(true).title("t");
                t.add_column("a").column_footer("f");
                t.add_row(&["x"]);
                Box::new(t)
            }),
        ),
        (
            "table_ratio0_expand",
            10,
            5,
            Box::new(|| {
                let mut t = Table::new().expand(true);
                t.add_column("a").column_ratio(0);
                t.add_column("b").column_ratio(0);
                t.add_row(&["x", "y"]);
                Box::new(t)
            }),
        ),
        (
            "table_ratio_max",
            10,
            5,
            Box::new(|| {
                let mut t = Table::new().expand(true);
                t.add_column("a").column_ratio(usize::MAX);
                t.add_column("b").column_ratio(usize::MAX);
                t.add_row(&["x", "y"]);
                Box::new(t)
            }),
        ),
        (
            "align_h0",
            10,
            5,
            Box::new(|| {
                Box::new(
                    Align::center(Box::new(Text::new("a\nb")))
                        .vertical(VerticalAlign::Middle)
                        .height(0),
                )
            }),
        ),
        (
            "panel_h0",
            10,
            5,
            Box::new(|| Box::new(Panel::new(Box::new(Text::new("a"))).height(0))),
        ),
        (
            "panel_w0_title",
            0,
            5,
            Box::new(|| {
                Box::new(
                    Panel::new(Box::new(Text::new("a")))
                        .title("t")
                        .subtitle("s"),
                )
            }),
        ),
        (
            "rule_empty_chars",
            10,
            5,
            Box::new(|| Box::new(Rule::new("t").characters(""))),
        ),
        ("rule_w0", 0, 5, Box::new(|| Box::new(Rule::new("t")))),
        (
            "tree_w0",
            0,
            5,
            Box::new(|| {
                let mut t = Tree::new("r");
                t.add("a").add("b");
                Box::new(t)
            }),
        ),
        (
            "layout_ratio0",
            10,
            3,
            Box::new(|| {
                let mut l = Layout::new();
                l.split_row(vec![
                    Layout::with_renderable(Box::new(Text::new("a"))).ratio(0),
                    Layout::with_renderable(Box::new(Text::new("b"))).ratio(0),
                ]);
                Box::new(l)
            }),
        ),
        (
            "layout_w0_h0",
            0,
            0,
            Box::new(|| {
                let mut l = Layout::new();
                l.split_column(vec![Layout::new().name("a"), Layout::new().name("b")]);
                Box::new(l)
            }),
        ),
        (
            "syntax_w0_num",
            0,
            5,
            Box::new(|| Box::new(Syntax::new("a\n", "python").line_numbers(true))),
        ),
        (
            "syntax_pad_big",
            5,
            5,
            Box::new(|| {
                Box::new(
                    Syntax::new("a\n", "python")
                        .padding_sides((1, 40, 1, 40))
                        .line_numbers(true),
                )
            }),
        ),
        (
            "syntax_hl_min",
            20,
            5,
            Box::new(|| {
                Box::new(
                    Syntax::new("a\nb\n", "python")
                        .start_line(i64::MIN)
                        .line_numbers(true),
                )
            }),
        ),
        (
            "live_w0",
            0,
            0,
            Box::new(|| Box::new(LiveRender::new(Box::new(Text::new("a\nb"))))),
        ),
    ];
    for (name, w, h, build) in cases {
        let r = catch_unwind(AssertUnwindSafe(|| {
            let c = console(w, h);
            let r = build();
            c.capture(|c| c.print(r.as_ref()))
        }));
        if let Err(e) = r {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            panics.push(format!("{name}: {msg}"));
        }
    }
    assert!(panics.is_empty(), "panics:\n{}", panics.join("\n"));
}

/// `size` / `minimum_size` of `usize::MAX` resolve without overflowing the
/// region offsets (they saturate). Rendering such a region would need a
/// `usize::MAX`-wide line, which upstream cannot allocate either.
#[test]
fn layout_huge_sizes_resolve() {
    let leaf = |s: &str| Layout::with_renderable(Box::new(Text::new(s)));
    let mut layout = Layout::new();
    layout.split_row(vec![leaf("a").size(usize::MAX), leaf("b").size(5)]);
    let regions = layout.region_map(10, 3);
    assert_eq!(regions.len(), 3);
    assert!(regions
        .iter()
        .any(|(_, r)| r.x == usize::MAX && r.width == 5));

    let mut layout = Layout::new();
    layout.split_column(vec![leaf("a").minimum_size(usize::MAX), leaf("b")]);
    let regions = layout.region_map(10, 3);
    assert_eq!(regions.len(), 3);
}

/// Upstream's `ratio_resolve` reads `edge.size or None`: `size=0` is
/// flexible, not zero wide.
#[test]
fn layout_size_zero_is_flexible() {
    let c = console(10, 1);
    let mut l = Layout::new();
    l.split_row(vec![
        Layout::with_renderable(Box::new(Text::new("a"))).size(0),
        Layout::with_renderable(Box::new(Text::new("b"))),
        Layout::with_renderable(Box::new(Text::new("c"))).size(0),
    ]);
    // rich 15.0.0: "a  b  c   \n"
    assert_eq!(c.capture(|c| c.print(&l)), "a  b  c   \n");
}

fn syntax_render(s: Syntax) -> std::thread::Result<String> {
    catch_unwind(AssertUnwindSafe(|| {
        let c = console(30, 25);
        c.capture(|c| c.print(&s))
    }))
}

/// Python ints are unbounded, so any `start_line` / `line_range` renders.
#[test]
fn syntax_extreme_lines_do_not_overflow() {
    for s in [
        Syntax::new("a\nb\n", "python")
            .start_line(i64::MAX)
            .line_numbers(true),
        Syntax::new("a\nb\n", "python")
            .start_line(i64::MIN)
            .line_numbers(true),
        Syntax::new("a\nb\n", "python").line_range(Some(i64::MIN), None),
        Syntax::new("a\nb\n", "python").line_range(Some(i64::MIN), Some(i64::MIN)),
    ] {
        assert!(syntax_render(s).is_ok());
    }
    let out = syntax_render(
        Syntax::new("a\nb\n", "text")
            .start_line(i64::MAX)
            .line_numbers(true),
    )
    .unwrap();
    assert!(out.contains("9223372036854775809"), "{out:?}");
    let mut s = Syntax::new("a\nb\n", "text");
    s.stylize_range("bold", (i64::MIN, 0), (i64::MAX, i64::MAX), false);
    s.stylize_range("bold", (1, i64::MIN), (i64::MIN, i64::MIN), false);
    assert!(syntax_render(s).is_ok());
}

/// Huge screen coordinates are written in full, as upstream writes its
/// unbounded ints (`x + 1` would overflow `usize`).
#[test]
fn update_screen_lines_huge_coordinates() {
    let c = console(10, 4);
    let out = c.capture(|c| {
        c.set_alt_screen(true);
        c.update_screen_lines(&[vec![Segment::new("x", None)]], usize::MAX, usize::MAX)
            .unwrap();
        c.set_alt_screen(false);
    });
    assert!(
        out.contains("\x1b[18446744073709551616;18446744073709551616Hx"),
        "{out:?}"
    );
    assert_eq!(
        rich::Control::move_to(u32::MAX, u32::MAX).as_str(),
        "\x1b[4294967296;4294967296H"
    );
    assert_eq!(
        rich::Control::move_to_column(u32::MAX, 0).as_str(),
        "\x1b[4294967296G"
    );
}

/// SVG export with a non-finite cell width is an error, like upstream's
/// `ceil` (`OverflowError` / `ValueError`), not a panic or NaN output.
#[test]
fn svg_non_finite_aspect_ratio_is_an_error() {
    use rich::terminal_theme::SVG_EXPORT_THEME;
    let c = console(20, 25);
    for ratio in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e308] {
        let result = c.export_svg_with(&SVG_EXPORT_THEME, "t", "U", None, ratio, |c| {
            c.print_str("hi")
        });
        assert!(result.is_err(), "ratio {ratio}");
    }
}

/// Python's `json` accepts a lone surrogate escape. Under `ensure_ascii` it
/// prints escaped (golden parity); otherwise upstream prints the surrogate
/// itself, which a Rust string cannot hold, so it is U+FFFD (DIVERGENCES).
#[test]
fn json_lone_surrogate_without_ensure_ascii() {
    let json = rich::Json::new(r#"["\ud800", {"\udfff": "\ud83d\ude00"}]"#).unwrap();
    let c = Console::builder().width(40).color_system(None).build();
    let out = c.capture(|c| c.print(&json));
    assert_eq!(
        out,
        "[\n  \"\u{fffd}\",\n  {\n    \"\u{fffd}\": \"\u{1f600}\"\n  }\n]\n"
    );
}

/// Refresh / update_screen without the alternate screen is an error, not a
/// panic, and writes nothing.
#[test]
fn refresh_screen_without_alt_screen() {
    let c = console(10, 4);
    let mut l = Layout::new();
    l.split_row(vec![
        Layout::with_renderable(Box::new(Text::new("a"))).name("a"),
        Layout::with_renderable(Box::new(Text::new("b"))).name("b"),
    ]);
    let _ = c.capture(|c| c.print(&l));
    let out = c.capture(|c| {
        assert!(l.refresh_screen(c, "a").is_err());
        assert_eq!(l.refresh_screen(c, "nope").ok(), Some(false));
    });
    assert_eq!(out, "");
}

/// A very deep tree renders (upstream walks a stack) without overflowing.
#[test]
fn deep_tree_renders() {
    let h = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let mut root = Tree::new("r");
            {
                let mut n = &mut root;
                for i in 0..3000 {
                    n = n.add(format!("n{i}"));
                }
            }
            let c = console(20000, 25);
            let out = c.capture(|c| c.print(&root));
            let lines = out.lines().count();
            drop(root);
            lines
        })
        .unwrap();
    assert_eq!(h.join().unwrap(), 3001);
}

/// Deeply nested JSON is an error or renders; it must not overflow the stack.
#[test]
fn json_deep_nesting_does_not_overflow() {
    for depth in [900, 200_000] {
        let src = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let c = console(40, 25);
        if let Ok(j) = rich::Json::new(&src) {
            let _ = c.capture(|c| c.print(&j));
        }
        let obj = format!("{}1{}", "{\"a\":".repeat(depth), "}".repeat(depth));
        if let Ok(j) = rich::Json::new(&obj) {
            let _ = c.capture(|c| c.print(&j));
        }
    }
}
