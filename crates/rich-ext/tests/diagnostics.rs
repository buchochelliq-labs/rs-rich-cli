use rich::{Console, Renderable, Segment};
use rich_ext::diagnostic::{Diagnostic, SourceSnippet};
use rich_ext::event::{EventView, Message, StructuredEvent};
fn output(d: &dyn Renderable, w: usize) -> String {
    let c = Console::builder()
        .width(w)
        .height(100)
        .no_color(true)
        .build();
    d.rich_render(&c, &c.options())
        .iter()
        .map(|s| s.text.as_str())
        .collect()
}
#[test]
fn source_spans_validate_utf8_without_reading_paths() {
    assert!(SourceSnippet::new("missing.rs".into(), "é界".into(), 1..2, 1).is_err());
    assert!(SourceSnippet::new("missing.rs".into(), "é界".into(), 2..5, 1).is_ok());
    let reversed = std::ops::Range { start: 2, end: 1 };
    assert!(SourceSnippet::new("x".into(), "abc".into(), reversed, 1).is_err());
    assert!(SourceSnippet::new("x".into(), "abc".into(), 3..3, 1).is_ok());
    let d = Diagnostic::new("bad value")
        .view(EventView::Expanded)
        .snippet(SourceSnippet::new("missing.rs".into(), "é界".into(), 2..5, 1).unwrap());
    let text = output(&d, 80);
    assert!(text.contains("1 | é界"), "{text}");
    assert!(text.contains("  |  ^^"), "{text}");
    for width in [1, 2, 20, 80] {
        let c = Console::builder().width(width).build();
        let rows = Segment::split_lines(&d.rich_render(&c, &c.options()));
        assert!(rows
            .iter()
            .all(|r| r.iter().map(Segment::cell_length).sum::<usize>() <= width));
    }
}
#[derive(Debug)]
struct Cycle;
impl std::fmt::Display for Cycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cycle error")
    }
}
impl std::error::Error for Cycle {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self)
    }
}
#[test]
fn error_cycles_and_depth_limits_are_visible_and_attachments_are_separate() {
    let d = Diagnostic::from_error(&Cycle, 8);
    assert!(output(&d, 80).contains("[cycle]"));
    assert!(output(&Diagnostic::from_error(&Cycle, 0), 80).contains("[truncated]"));
    let event = StructuredEvent::new(Message::Literal("event".into()))
        .diagnostic(Diagnostic::new("first"))
        .diagnostic(Diagnostic::new("second"));
    assert_eq!(output(&event, 80), "event\nfirst\nsecond");
}

#[test]
fn tabs_multiline_spans_and_eof_have_bounded_annotations() {
    let d = Diagnostic::new("tab")
        .view(EventView::Expanded)
        .snippet(SourceSnippet::new("supplied".into(), "\tX\n界é".into(), 1..9, 0).unwrap())
        .note("note")
        .help("help");
    let text = output(&d, 80);
    assert!(text.contains("1 |         X"));
    assert!(text.contains("2 | 界é"));
    assert!(text.contains("note: note"));
    let d = Diagnostic::new("eof")
        .view(EventView::Expanded)
        .snippet(SourceSnippet::new("x".into(), "abc".into(), 3..3, 0).unwrap());
    assert!(output(&d, 80).contains("  |    ^"));
}

#[test]
fn a_span_inside_a_combining_cluster_marks_its_visible_cell() {
    let d = Diagnostic::new("accent")
        .view(EventView::Expanded)
        .snippet(SourceSnippet::new("x".into(), "e\u{301}X".into(), 1..3, 0).unwrap());
    let text = output(&d, 80);
    assert!(text.contains("1 | e\u{301}X\n  | ^"), "{text}");
    assert!(!text.contains("  |  ^"));
}

#[test]
fn crlf_sources_render_without_carriage_returns() {
    let source = "let a = 1;\r\nlet b = oops;\r\nlet c = 3;\r\n";
    let start = source.find("oops").unwrap();
    let d = Diagnostic::new("crlf")
        .view(EventView::Expanded)
        .snippet(SourceSnippet::new("x".into(), source.into(), start..start + 4, 1).unwrap());
    let text = output(&d, 80);
    assert!(!text.contains('\r'), "{text:?}");
    assert!(
        text.contains("2 | let b = oops;\n  |         ^^^^"),
        "{text}"
    );
    assert!(text.contains("1 | let a = 1;\n"), "{text}");
    assert!(text.contains("3 | let c = 3;"), "{text}");
    // A span covering only the line terminator still gets one bounded marker.
    let cr = source.find('\r').unwrap();
    let d = Diagnostic::new("eol")
        .view(EventView::Expanded)
        .snippet(SourceSnippet::new("x".into(), source.into(), cr..cr + 2, 0).unwrap());
    let text = output(&d, 80);
    assert!(!text.contains('\r'), "{text:?}");
    assert!(text.contains("1 | let a = 1;\n  |           ^"), "{text}");
}

/// A real three-level `std::error::Error` chain: each cause is its own row, in
/// source order, after the headline (#146/#151).
#[test]
fn multi_level_error_chains_render_one_ordered_cause_per_row() {
    #[derive(Debug)]
    struct Leaf;
    impl std::fmt::Display for Leaf {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "disk full")
        }
    }
    impl std::error::Error for Leaf {}
    #[derive(Debug)]
    struct Middle(Leaf);
    impl std::fmt::Display for Middle {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "cannot write cache")
        }
    }
    impl std::error::Error for Middle {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }
    #[derive(Debug)]
    struct Top(Middle);
    impl std::fmt::Display for Top {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "build failed")
        }
    }
    impl std::error::Error for Top {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    let error = Top(Middle(Leaf));
    let text = output(&Diagnostic::from_error(&error, 8), 80);
    assert_eq!(
        text,
        "build failed\ncaused by: cannot write cache\ncaused by: disk full"
    );
    // The depth limit cuts the chain after the given number of causes.
    let text = output(&Diagnostic::from_error(&error, 1), 80);
    assert_eq!(
        text,
        "build failed\ncaused by: cannot write cache\ncaused by: [truncated]"
    );
    // Narrow widths wrap each cause within the width, keeping source order.
    let text = output(&Diagnostic::from_error(&error, 8), 12);
    assert!(
        text.lines().all(|l| rich::cells::cell_len(l) <= 12),
        "{text}"
    );
    let middle = text.find("cannot write").expect("middle cause");
    let leaf = text.find("disk full").expect("leaf cause");
    assert!(
        text.starts_with("build failed\n") && middle < leaf,
        "{text}"
    );
}
