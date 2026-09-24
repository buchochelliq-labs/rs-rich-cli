use rich::{ColorSystem, Console};
use rich_ext::source_view::SourceView;

fn plain(width: usize) -> Console {
    Console::builder().width(width).color_system(None).build()
}

fn lines(view: &SourceView, width: usize) -> Vec<String> {
    plain(width)
        .render_to_string(view)
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect()
}

const CODE: &str = "fn main() {\n    let total = 1;\n    println!(\"{total}\");\n}\n";

#[test]
fn numbers_every_line_with_an_aligned_gutter() {
    let view = SourceView::new(CODE, "rust");
    assert_eq!(
        lines(&view, 60),
        [
            "1 │ fn main() {",
            "2 │     let total = 1;",
            "3 │     println!(\"{total}\");",
            "4 │ }",
        ]
    );
    // The gutter widens with the last number, and an excerpt can start anywhere.
    let excerpt = SourceView::new(CODE, "rust").start_line(98);
    assert_eq!(lines(&excerpt, 60)[0], " 98 │ fn main() {");
    assert_eq!(lines(&excerpt, 60)[3], "101 │ }");
    let bare = SourceView::new(CODE, "rust").line_numbers(false);
    assert_eq!(lines(&bare, 60)[0], "fn main() {");
}

#[test]
fn long_lines_wrap_under_a_blank_gutter() {
    let view = SourceView::new("short\nabcdefghijklmnopqrstuvwxyz\n", "txt");
    assert_eq!(
        lines(&view, 14),
        [
            "1 │ short",
            "2 │ abcdefghij",
            "  │ klmnopqrst",
            "  │ uvwxyz"
        ]
    );
    for line in plain(14).render_to_string(&view).lines() {
        assert!(rich::cells::cell_len(line) <= 14, "{line:?}");
    }
}

#[test]
fn search_is_case_insensitive_and_reports_matches() {
    let view = SourceView::new("Total\nnone\ntotal total\n", "txt").search("TOTAL");
    assert_eq!(view.matches(), vec![(1, 1), (3, 2)]);
    assert!(SourceView::new(CODE, "rust")
        .search("")
        .matches()
        .is_empty());
    // Matches are highlighted and their line numbers marked.
    let console = Console::builder()
        .width(40)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let out = console.render_to_string(&view);
    assert!(out.contains("\x1b[30;43mTotal\x1b[0m"), "{out:?}");
    assert!(out.contains("\x1b[1;33m1\x1b[0m"), "{out:?}");
    assert!(out.contains("\x1b[2m2\x1b[0m"), "{out:?}");
}

#[test]
fn tabs_crlf_empty_files_and_ascii() {
    let view = SourceView::new("a\tb\r\nc\r\n", "txt")
        .tab_size(4)
        .search("b");
    assert_eq!(lines(&view, 40), ["1 │ a   b", "2 │ c"]);
    assert_eq!(view.matches(), vec![(1, 1)]);
    assert!(lines(&SourceView::new("", "txt"), 40).is_empty());
    // A missing final newline still shows the last line.
    assert_eq!(
        lines(&SourceView::new("x\ny", "txt"), 40),
        ["1 │ x", "2 │ y"]
    );
    let ascii = Console::builder()
        .width(40)
        .color_system(None)
        .ascii_only(true)
        .build();
    assert!(ascii
        .render_to_string(&SourceView::new("x\n", "txt"))
        .starts_with("1 | x"));
}

#[test]
fn measures_the_gutter_and_the_widest_line() {
    let console = plain(80);
    let m = rich::measure::Measurement::get(
        &console,
        &console.options(),
        &SourceView::new(CODE, "rust"),
    );
    assert_eq!(
        (m.minimum, m.maximum),
        (5, 4 + "    println!(\"{total}\");".len())
    );
}
