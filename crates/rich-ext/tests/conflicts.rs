//! Three-way merge conflicts: the marker parser and `ConflictView`.
use rich::{ColorSystem, Console, Renderable};
use rich_ext::diff::*;

fn plain(r: &dyn Renderable, w: usize) -> String {
    let c = Console::builder()
        .width(w)
        .height(200)
        .no_color(true)
        .build();
    c.render_to_string(r)
}

fn assert_clean(out: &str, width: usize) {
    for line in out.lines() {
        assert_eq!(line, line.trim_end(), "trailing space in {line:?}");
        assert!(
            rich::cells::cell_len(line) <= width,
            "{line:?} is wider than {width}"
        );
    }
}

const MERGE: &str = "\
fn main() {
    let a = 1;
<<<<<<< HEAD
    let b = 2;
=======
    let b = 3;
    let c = 4;
>>>>>>> feature
    println!(\"{a}\");
}
";

const DIFF3: &str = "\
one
<<<<<<< ours
x = 1
||||||| base
x = 0
=======
x = 2
>>>>>>> theirs
two
";

// ---------------------------------------------------------------- parser

#[test]
fn parses_a_merge_conflict() {
    let file = ConflictFile::parse(MERGE).unwrap();
    assert_eq!(file.lines().len(), 10);
    assert!(file.has_conflicts());
    assert!(!file.is_crlf());
    let [conflict] = file.conflicts() else {
        panic!("one conflict");
    };
    assert_eq!(conflict.number, 1);
    assert_eq!((conflict.start_line(), conflict.end_line), (3, 8));
    assert_eq!(conflict.span(), 2..8);
    assert_eq!(conflict.ours.label.as_deref(), Some("HEAD"));
    assert_eq!(conflict.ours.lines, 3..4);
    assert!(conflict.base.is_none());
    assert_eq!(conflict.theirs.label.as_deref(), Some("feature"));
    assert_eq!(conflict.theirs.marker_line, 5);
    assert_eq!(
        file.text(&conflict.theirs),
        "    let b = 3;\n    let c = 4;"
    );
    let picks: Vec<Pick> = conflict.sides().into_iter().map(|(p, _)| p).collect();
    assert_eq!(picks, [Pick::Ours, Pick::Theirs]);
}

#[test]
fn parses_the_diff3_base() {
    let file = ConflictFile::parse(DIFF3).unwrap();
    let conflict = &file.conflicts()[0];
    let base = conflict.base.as_ref().unwrap();
    assert_eq!(base.label.as_deref(), Some("base"));
    assert_eq!(base.marker_line, 4);
    assert_eq!(file.text(base), "x = 0");
    assert_eq!(file.text(&conflict.ours), "x = 1");
    assert_eq!(file.text(&conflict.theirs), "x = 2");
    let picks: Vec<&str> = conflict.sides().iter().map(|(p, _)| p.name()).collect();
    assert_eq!(picks, ["ours", "base", "theirs"]);
}

#[test]
fn labels_are_optional_and_sides_may_be_empty() {
    let file = ConflictFile::parse("<<<<<<<\n|||||||\n=======\nb\n>>>>>>>\n").unwrap();
    let conflict = &file.conflicts()[0];
    assert_eq!(conflict.ours.label, None);
    assert!(conflict.ours.lines.is_empty());
    assert!(conflict.base.as_ref().unwrap().lines.is_empty());
    assert_eq!(conflict.theirs.label, None);
    assert_eq!(file.text(&conflict.theirs), "b");
}

#[test]
fn several_conflicts_are_numbered_in_order() {
    let text = "<<<<<<< a\n1\n=======\n2\n>>>>>>> b\nmid\n<<<<<<< a\n3\n=======\n4\n>>>>>>> b";
    let file = ConflictFile::parse(text).unwrap();
    let numbers: Vec<(usize, usize, usize)> = file
        .conflicts()
        .iter()
        .map(|c| (c.number, c.start_line(), c.end_line))
        .collect();
    assert_eq!(numbers, [(1, 1, 5), (2, 7, 11)]);
}

#[test]
fn text_that_only_looks_like_markers_is_content() {
    // Eight characters, a marker glued to text, markers outside a conflict
    // and a setext heading are all text.
    let text = "\
Title
=======
<<<<<<<< not a marker
<<<<<<<x
>>>>>>> stray
||||||| stray
<<<<<<< HEAD
a <<<<<<< b
======= not a separator
=======
c
>>>>>>>> still theirs
>>>>>>> topic
";
    let file = ConflictFile::parse(text).unwrap();
    let [conflict] = file.conflicts() else {
        panic!("one conflict: {:?}", file.conflicts());
    };
    assert_eq!(conflict.start_line(), 7);
    assert_eq!(
        file.text(&conflict.ours),
        "a <<<<<<< b\n======= not a separator"
    );
    assert_eq!(file.text(&conflict.theirs), "c\n>>>>>>>> still theirs");
}

#[test]
fn crlf_and_a_byte_order_mark_are_handled() {
    let text = "\u{feff}a\r\n<<<<<<< HEAD\r\nb\r\n=======\r\nc\r\n>>>>>>> topic\r\n";
    let file = ConflictFile::parse(text).unwrap();
    assert!(file.is_crlf());
    assert_eq!(file.lines()[0], "a");
    let conflict = &file.conflicts()[0];
    assert_eq!(conflict.ours.label.as_deref(), Some("HEAD"));
    assert_eq!(conflict.theirs.label.as_deref(), Some("topic"));
    assert_eq!(file.text(&conflict.ours), "b");
}

#[test]
fn a_file_without_conflicts_parses_empty() {
    for text in ["", "\n", "plain\ntext", "=======\n>>>>>>>\n"] {
        let file = ConflictFile::parse(text).unwrap();
        assert!(!file.has_conflicts(), "{text:?}");
    }
}

#[test]
fn malformed_markers_name_the_line() {
    let cases: &[(&str, usize, &str)] = &[
        ("a\n<<<<<<< HEAD\nb\n", 2, "never closed"),
        ("<<<<<<< HEAD\nb\n=======\nc\n", 1, "never closed"),
        ("<<<<<<< HEAD\n|||||||\nb", 1, "never closed"),
        ("<<<<<<< HEAD\nb\n>>>>>>> x\n", 3, "before `=======`"),
        (
            "<<<<<<< HEAD\n||||||| b\n>>>>>>> x\n",
            3,
            "before `=======`",
        ),
        ("<<<<<<< a\n<<<<<<< b\n", 2, "opened at line 1"),
        ("<<<<<<< a\n=======\n<<<<<<< b\n", 3, "opened at line 1"),
        (
            "<<<<<<< a\n=======\n=======\n>>>>>>> b\n",
            3,
            "second `=======`",
        ),
        ("<<<<<<< a\n|||||||\n|||||||\n", 3, "second `|||||||`"),
        (
            "<<<<<<< a\n=======\n||||||| b\n>>>>>>> b\n",
            3,
            "after `=======`",
        ),
    ];
    for (text, line, message) in cases {
        let err = ConflictFile::parse(text).unwrap_err();
        assert_eq!(err.line, Some(*line), "{text:?}: {err}");
        assert!(err.message.contains(message), "{text:?}: {err}");
        assert!(err.to_string().starts_with(&format!("line {line}: ")));
    }
}

#[test]
fn input_size_and_conflict_count_are_capped() {
    let big = "x".repeat(MAX_CONFLICT_SOURCE + 1);
    let err = ConflictFile::parse(&big).unwrap_err();
    assert_eq!(err.line, None);
    assert!(err.to_string().contains("bytes"), "{err}");

    let many = "<<<<<<<\n=======\n>>>>>>>\n".repeat(MAX_CONFLICTS + 1);
    let err = ConflictFile::parse(&many).unwrap_err();
    assert_eq!(err.line, Some(MAX_CONFLICTS * 3 + 1));
    assert!(err.message.contains("more than"), "{err}");
}

#[test]
fn parsing_garbage_never_panics() {
    // Every prefix of a tricky input, cut at every character boundary.
    let text = "é\r\n<<<<<<< ü\r\n|||||||\n=======\r\n>>>>>>> \u{1b}[31m\n<<<<<<<";
    for (i, _) in text.char_indices() {
        let _ = ConflictFile::parse(&text[..i]);
    }
}

// ---------------------------------------------------------------- view

#[test]
fn stacked_view_at_40() {
    let view = ConflictView::parse(MERGE)
        .unwrap()
        .layout(ConflictLayout::Stacked)
        .context(1);
    let out = plain(&view, 40);
    assert_eq!(
        out,
        "\
conflict 1 of 1, lines 3-8
 2       let a = 1;
   ours: HEAD
 4 <     let b = 2;
   theirs: feature
 6 >     let b = 3;
 7 >     let c = 4;
 9       println!(\"{a}\");"
    );
    assert_clean(&out, 40);
}

#[test]
fn side_by_side_view_at_60() {
    let view = ConflictView::parse(MERGE)
        .unwrap()
        .layout(ConflictLayout::SideBySide)
        .context(1);
    let out = plain(&view, 60);
    assert_eq!(
        out,
        "\
conflict 1 of 1, lines 3-8
 2       let a = 1;
   ours: HEAD                 │    theirs: feature
 4 <     let b = 2;           │  6 >     let b = 3;
                              │  7 >     let c = 4;
 9       println!(\"{a}\");"
    );
    assert_clean(&out, 60);
}

#[test]
fn diff3_side_by_side_has_three_columns() {
    let view = ConflictView::parse(DIFF3).unwrap();
    let out = plain(&view, 80);
    assert_eq!(
        out,
        "\
conflict 1 of 1, lines 2-8
1   one
  ours: ours              │   base: base              │   theirs: theirs
3 < x = 1                 │ 5 | x = 0                 │ 7 > x = 2
9   two"
    );
    assert_clean(&out, 80);
}

#[test]
fn auto_layout_stacks_when_narrow() {
    let view = ConflictView::parse(DIFF3).unwrap();
    let out = plain(&view, 40);
    assert_eq!(
        out,
        "\
conflict 1 of 1, lines 2-8
1   one
  ours: ours
3 < x = 1
  base: base
5 | x = 0
  theirs: theirs
7 > x = 2
9   two"
    );
    // Without the base, two columns fit.
    let out = plain(&ConflictView::parse(DIFF3).unwrap().base(false), 60);
    assert!(out.contains("│"), "{out}");
    assert!(!out.contains("x = 0"), "{out}");
}

#[test]
fn context_never_repeats_between_close_conflicts() {
    let text = "a\n<<<<<<<\n1\n=======\n2\n>>>>>>>\nshared\n<<<<<<<\n3\n=======\n4\n>>>>>>>\nz\n";
    let view = ConflictView::parse(text)
        .unwrap()
        .layout(ConflictLayout::Stacked);
    let out = plain(&view, 40);
    assert_eq!(out.matches("shared").count(), 1, "{out}");
    assert!(out.contains("conflict 2 of 2, lines 8-12"), "{out}");
}

#[test]
fn empty_sides_and_no_line_numbers() {
    let view = ConflictView::parse("<<<<<<< HEAD\n=======\nb\n>>>>>>> x\n")
        .unwrap()
        .layout(ConflictLayout::Stacked)
        .line_numbers(false);
    assert_eq!(
        plain(&view, 30),
        "conflict 1 of 1, lines 1-4\nours: HEAD\n< (empty)\ntheirs: x\n> b"
    );
}

#[test]
fn no_conflicts_says_so() {
    let view = ConflictView::parse("just text\n").unwrap();
    assert_eq!(plain(&view, 30), "no conflicts");
}

#[test]
fn long_lines_wrap_or_truncate_within_the_width() {
    let long = format!(
        "<<<<<<< HEAD\n{}\n=======\nb\n>>>>>>> x\n",
        "word ".repeat(30)
    );
    for layout in [ConflictLayout::Stacked, ConflictLayout::SideBySide] {
        for wrap in [true, false] {
            let view = ConflictView::parse(&long)
                .unwrap()
                .layout(layout)
                .wrap(wrap);
            for width in [12, 30, 61] {
                assert_clean(&plain(&view, width), width);
            }
        }
    }
}

#[test]
fn measure_fits_the_widest_line() {
    let console = Console::builder().width(200).build();
    let stacked = ConflictView::parse(MERGE)
        .unwrap()
        .layout(ConflictLayout::Stacked);
    let m = stacked.measure(&console, &console.options());
    // The heading is wider than `" 9       println!(\"{a}\");"`.
    assert_eq!(m.maximum, "conflict 1 of 1, lines 3-8".len());
    let side = ConflictView::parse(MERGE)
        .unwrap()
        .layout(ConflictLayout::SideBySide);
    assert!(side.measure(&console, &console.options()).maximum > m.maximum);
}

#[cfg(feature = "syntax")]
#[test]
fn sides_are_syntax_highlighted() {
    let view = ConflictView::parse(MERGE).unwrap().path("main.rs");
    let console = Console::builder()
        .width(80)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let segments = view.rich_render(&console, &console.options());
    // `let` is a keyword in every version, and styled differently from the
    // plain text around it.
    let lets: Vec<_> = segments.iter().filter(|s| s.text == "let").collect();
    assert!(lets.len() >= 3, "{segments:?}");
    let plain_style = segments
        .iter()
        .find(|s| s.text.contains("b ="))
        .and_then(|s| s.style.clone());
    assert_ne!(lets[0].style, plain_style);
}

#[test]
fn styles_come_from_the_theme_keys() {
    let view = ConflictView::parse(MERGE)
        .unwrap()
        .layout(ConflictLayout::Stacked);
    let console = Console::builder()
        .width(60)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let segments = view.rich_render(&console, &console.options());
    let marker = |m: &str| {
        segments
            .iter()
            .find(|s| s.text.starts_with(m))
            .and_then(|s| s.style.clone())
            .unwrap_or_else(|| panic!("no {m:?} segment"))
    };
    assert_eq!(marker("< ").color().map(|c| c.name.as_str()), Some("green"));
    assert_eq!(marker("> ").color().map(|c| c.name.as_str()), Some("blue"));
}

#[test]
fn a_large_file_renders_its_conflicts_plain() {
    // Over the 1 MiB highlighting limit: each version would be highlighted
    // whole, so the view falls back to plain text.
    let filler = "let x = 1;\n".repeat(100_000);
    let text =
        format!("{filler}<<<<<<< HEAD\nlet a = 1;\n=======\nlet a = 2;\n>>>>>>> x\n{filler}");
    let view = ConflictView::parse(&text).unwrap().path("big.rs");
    let console = Console::builder()
        .width(80)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let segments = view.rich_render(&console, &console.options());
    assert!(!segments.iter().any(|s| s.text == "let"), "highlighted");
    let out = plain(&view, 80);
    assert!(
        out.starts_with("conflict 1 of 1, lines 100001-100005"),
        "{out}"
    );
}

#[test]
fn a_longer_conflict_marker_size_is_honoured() {
    // `conflict-marker-size=10` in .gitattributes: git writes ten.
    let text = "\
a
<<<<<<<<<< HEAD
b
<<<<<<< not a marker at this size
==========
c
>>>>>>>>>> topic
d
";
    let file = ConflictFile::parse(text).unwrap();
    let [conflict] = file.conflicts() else {
        panic!("one conflict: {:?}", file.conflicts());
    };
    assert_eq!(conflict.ours.label.as_deref(), Some("HEAD"));
    assert_eq!(
        file.text(&conflict.ours),
        "b\n<<<<<<< not a marker at this size"
    );
    assert_eq!(file.text(&conflict.theirs), "c");
    assert_eq!((conflict.start_line(), conflict.end_line), (2, 7));
}

#[test]
fn forced_columns_fall_back_to_stacked_when_too_narrow() {
    for width in 6..=40 {
        let view = ConflictView::parse(DIFF3)
            .unwrap()
            .layout(ConflictLayout::SideBySide);
        let out = plain(&view, width);
        assert_clean(&out, width);
    }
}

#[test]
fn a_huge_context_shows_the_rest_of_the_file() {
    let view = ConflictView::parse(MERGE).unwrap().context(usize::MAX);
    let out = plain(&view, 60);
    assert!(out.contains("println!"), "{out}");
}
