//! Regressions from the second renderables audit (core side): super-linear
//! renders, degenerate sizes and huge numbers must neither hang nor panic.
//! Byte parity for the cases rich defines lives in the golden suites.

use std::time::{Duration, Instant};

use rich::{ColorSystem, Columns, Console, Padding, Panel, Text};
use rich::{Highlighter, ReprHighlighter};

fn console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .height(25)
        .highlight(false)
        .no_color(false)
        .legacy_windows(false)
        .build()
}

/// `print(list(range(20000)))`: one paragraph with ~40000 spans wrapped into
/// ~1500 lines. Scanning every span per line (and per cut) was quadratic.
#[test]
fn printing_many_spans_is_not_quadratic() {
    let items: Vec<String> = (0..20000).map(|n| n.to_string()).collect();
    let mut text = Text::new(format!("[{}]", items.join(", ")));
    ReprHighlighter::new().highlight(&mut text);
    assert!(text.spans().len() > 20000);
    let start = Instant::now();
    let out = console(100).capture(|c| c.print(&text));
    assert!(out.contains("19999"));
    // Debug build; the quadratic version took minutes.
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "{:?}",
        start.elapsed()
    );
}

/// `Markdown("[" * 20000 + "x" + "]" * 20000)`: pulldown-cmark emits one
/// text event per bracket, and the strikethrough pass rescanned the merged
/// literal for each one.
#[test]
fn markdown_with_many_brackets_is_not_quadratic() {
    let source = format!("{}x{}", "[".repeat(20000), "]".repeat(20000));
    let start = Instant::now();
    let markdown = rich::markdown::Markdown::new(&source);
    let out = console(80).capture(|c| c.print(&markdown));
    assert!(out.contains('x'));
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
}

/// `Columns(width=0, padding=0)` divides by zero upstream
/// (`ZeroDivisionError`); core renders nothing rather than panic.
#[test]
fn zero_width_columns_without_padding_do_not_panic() {
    let columns = Columns::new(vec!["a".to_string()])
        .width(0)
        .padding((0, 0, 0, 0));
    assert_eq!(console(10).capture(|c| c.print(&columns)), "");
    // With padding the divisor is non-zero, as upstream.
    let columns = Columns::new(vec!["a".to_string()])
        .width(0)
        .padding((0, 1, 0, 0));
    assert!(!console(10).capture(|c| c.print(&columns)).is_empty());
}

/// Offsets and sizes near `usize::MAX` that allocate nothing must not
/// overflow the arithmetic (sizes that would allocate are the binding's to
/// cap).
#[test]
fn huge_offsets_do_not_overflow() {
    for n in [usize::MAX, 1 << 63] {
        let mut text = Text::new("a  ");
        text.rstrip_end(n);
        text.stylize("bold", n, n);
        text.stylize("bold", 1, n);
        let _ = text.divide(&[1, n]);
        text.right_crop(n);
        assert_eq!(text.plain(), "");
        let console = console(30);
        let panel = Panel::new(Box::new(Text::new("x"))).width(n);
        let _ = console.render_to_string(&panel);
        let padding = Padding::new(Box::new(Text::new("x")), (0, n, 0, n));
        let _ = console.render_to_string(&padding);
    }
}
