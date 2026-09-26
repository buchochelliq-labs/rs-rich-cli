//! #149: one explicit overflow policy shared by Syntax, JSON and Text.
use rich::cells::cell_len;
use rich::{Console, Json, Renderable, Segment, Syntax, Text};
use rich_ext::layout::{Axis, LayoutNode, OverflowPolicy, Overflowing};

const POLICIES: [OverflowPolicy; 5] = [
    OverflowPolicy::Wrap,
    OverflowPolicy::Fold,
    OverflowPolicy::Crop,
    OverflowPolicy::Ellipsis,
    OverflowPolicy::Visible,
];

fn console() -> Console {
    Console::builder().width(80).color_system(None).build()
}

fn rows(value: &dyn Renderable, width: usize) -> Vec<String> {
    let console = console();
    let segments = value.rich_render(&console, &console.options().update_width(width));
    Segment::split_lines(&segments)
        .into_iter()
        .map(|l| l.iter().map(|s| s.text.as_str()).collect())
        .collect()
}

fn wrap(value: impl Renderable + 'static, policy: OverflowPolicy) -> Overflowing {
    Overflowing::new(Box::new(value), policy)
}

const CODE: &str = "short = 1\na_very_long_identifier = compute(value)\n";
const DOC: &str = r#"{"name": "a long value with words", "n": [1, 2]}"#;

#[test]
fn output_that_fits_is_byte_identical_to_core() {
    let console = Console::builder().width(80).build();
    let options = console.options();
    type Make = fn() -> Box<dyn Renderable>;
    let cases: [Make; 3] = [
        || Box::new(Syntax::new(CODE, "python").padding(1)),
        || Box::new(Json::new(DOC).unwrap()),
        || Box::new(Text::new("plain words")),
    ];
    for make in cases {
        let expected = make().rich_render(&console, &options);
        for policy in POLICIES {
            let wrapped = Overflowing::new(make(), policy);
            assert_eq!(
                wrapped.rich_render(&console, &options),
                expected,
                "{policy:?}"
            );
        }
    }
}

#[test]
fn each_policy_bounds_long_syntax_lines() {
    let expect: [(OverflowPolicy, &[&str]); 5] = [
        (
            OverflowPolicy::Wrap,
            &[
                "short = 1       ",
                "a_very_long_iden",
                "= compute(value)",
                "                ",
            ],
        ),
        (
            OverflowPolicy::Fold,
            &[
                "short = 1       ",
                "a_very_long_iden",
                "tifier =        ",
                "compute(value)  ",
                "                ",
            ],
        ),
        (
            OverflowPolicy::Crop,
            &["short = 1       ", "a_very_long_iden", "                "],
        ),
        (
            OverflowPolicy::Ellipsis,
            &["short = 1       ", "a_very_long_ide…", "                "],
        ),
        (
            OverflowPolicy::Visible,
            &[
                "short = 1                              ",
                "a_very_long_identifier = compute(value)",
                "                                       ",
            ],
        ),
    ];
    for (policy, rows_expected) in expect {
        assert_eq!(
            rows(&wrap(Syntax::new(CODE, "python"), policy), 16),
            rows_expected,
            "{policy:?}"
        );
    }
    // Core alone crops the long line at the width, as upstream Syntax does.
    assert_eq!(cell_len(&rows(&Syntax::new(CODE, "python"), 16)[1]), 16);
}

#[test]
fn json_shares_the_text_policies() {
    assert_eq!(
        rows(&wrap(Json::new(DOC).unwrap(), OverflowPolicy::Ellipsis), 14)[1],
        "  \"name\": \"a …"
    );
    assert_eq!(
        rows(&wrap(Json::new(DOC).unwrap(), OverflowPolicy::Crop), 14)[1],
        "  \"name\": \"a l"
    );
    let folded = rows(&wrap(Json::new(DOC).unwrap(), OverflowPolicy::Fold), 14);
    assert_eq!(
        folded[1..4],
        ["  \"name\": \"a ", "long value ", "with words\","]
    );
}

#[test]
fn wide_glyphs_never_split_or_exceed_the_width() {
    let source = "x = '日本語テキスト' # 👨\u{200d}👩\u{200d}👧 family";
    for width in 1..=24 {
        for policy in POLICIES {
            if policy == OverflowPolicy::Visible {
                continue;
            }
            for value in [
                wrap(Syntax::new(source, "python").padding(1), policy),
                wrap(
                    Json::new(&format!("{{\"k\": \"{source}\"}}")).unwrap(),
                    policy,
                ),
            ] {
                for row in rows(&value, width) {
                    assert!(cell_len(&row) <= width, "{policy:?} {width}: {row:?}");
                    assert!(!row.contains('\u{fffd}'), "{policy:?} {width}: {row:?}");
                }
            }
        }
    }
    assert_eq!(
        rows(
            &wrap(
                Syntax::new("x = '日本語テキスト'", "python").padding(1),
                OverflowPolicy::Ellipsis
            ),
            9
        ),
        ["         ", " x = '日…", "         "]
    );
}

#[test]
fn folded_syntax_rows_keep_the_theme_background() {
    let console = Console::builder().width(80).build();
    let value = wrap(Syntax::new(CODE, "python"), OverflowPolicy::Fold);
    let segments = value.rich_render(&console, &console.options().update_width(16));
    for line in Segment::split_lines(&segments) {
        assert_eq!(line.iter().map(Segment::cell_length).sum::<usize>(), 16);
        let last = line.last().unwrap();
        assert!(
            last.style.as_ref().and_then(|s| s.bgcolor()).is_some(),
            "{line:?}"
        );
    }
}

#[test]
fn measured_syntax_sizes_a_content_width_layout_column() {
    let node = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(wrap(
                Syntax::new("ab = 1", "python"),
                OverflowPolicy::Crop,
            )))
            .content_width(),
            LayoutNode::leaf(Box::new(Text::new("|rest"))),
        ],
    );
    let console = console();
    let segments = node.rich_render(&console, &console.options().update_dimensions(20, 1));
    assert_eq!(rows_of(&segments), ["ab = 1|rest         "]);
}

fn rows_of(segments: &[Segment]) -> Vec<String> {
    Segment::split_lines(segments)
        .into_iter()
        .map(|l| l.iter().map(|s| s.text.as_str()).collect())
        .collect()
}
