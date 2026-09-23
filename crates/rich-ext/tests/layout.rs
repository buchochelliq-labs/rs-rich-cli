use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text, Theme};
use rich_ext::layout::{fit_segments, Alignment, Axis, Constraint, LayoutNode, OverflowPolicy};
fn strings(lines: Vec<Vec<Segment>>) -> Vec<String> {
    lines
        .into_iter()
        .map(|l| l.iter().map(|s| s.text.as_str()).collect())
        .collect()
}
#[test]
fn overflow_policies_handle_long_words_tabs_and_graphemes() {
    let src = vec![Segment::new("abcdef", Some(Style::parse("red").unwrap()))];
    assert_eq!(
        strings(fit_segments(&src, 3, OverflowPolicy::Fold)),
        ["abc", "def"]
    );
    assert_eq!(
        strings(fit_segments(&src, 3, OverflowPolicy::Wrap)),
        ["abc"]
    );
    assert_eq!(
        strings(fit_segments(&src, 3, OverflowPolicy::Ellipsis)),
        ["ab…"]
    );
    assert_eq!(
        strings(fit_segments(&src, 3, OverflowPolicy::Visible)),
        ["abcdef"]
    );
    let src = vec![Segment::new("界e\u{301}\tX", None)];
    for w in [1, 2, 20, 80] {
        let lines = fit_segments(&src, w, OverflowPolicy::Fold);
        for line in lines {
            assert!(line.iter().map(Segment::cell_length).sum::<usize>() <= w);
        }
    }
    assert!(fit_segments(&src, 0, OverflowPolicy::Fold).is_empty());
    assert_eq!(
        strings(fit_segments(
            &[Segment::new("e\u{301}X", None)],
            1,
            OverflowPolicy::Fold
        )),
        ["e\u{301}", "X"]
    );
}
struct PanicChild;
impl Renderable for PanicChild {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        panic!("zero child")
    }
}
#[test]
fn composition_is_bounded_and_alignment_positions_content() {
    let console = Console::builder().width(8).height(2).no_color(true).build();
    let node = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(Text::new("L"))).width(Constraint::fixed(3)),
            LayoutNode::leaf(Box::new(Text::new("R"))).align(Alignment::End, Alignment::End),
        ],
    );
    assert_eq!(
        strings(Segment::split_lines(&node.rich_render(
            &console,
            &console.options().update_dimensions(8, 2)
        ))),
        ["L       ", "       R"]
    );
    for width in [0, 1, 2, 20, 80] {
        for height in [0, 1, 10] {
            let rows = Segment::split_lines(&node.rich_render(
                &console,
                &console.options().update_dimensions(width, height),
            ));
            assert!(rows.len() <= height);
            assert!(rows
                .iter()
                .all(|r| r.iter().map(Segment::cell_length).sum::<usize>() <= width));
        }
    }
    let zero = LayoutNode::leaf(Box::new(PanicChild));
    assert!(zero
        .rich_render(&console, &console.options().update_dimensions(0, 8))
        .is_empty());
    let bad = LayoutNode::leaf(Box::new(Text::new("bad"))).width(Constraint {
        min: 8,
        max: Some(2),
        preferred: None,
        flex: 1,
    });
    assert!(bad.validate().is_err());
    assert!(bad.rich_render(&console, &console.options()).is_empty());
    let _ = Theme::default_theme();
}

#[test]
fn node_overflow_is_applied_before_text_wrap_and_content_width_is_measured() {
    let c = Console::builder().width(3).height(2).no_color(true).build();
    let node = LayoutNode::leaf(Box::new(Text::new("abcdef"))).overflow(OverflowPolicy::Ellipsis);
    assert_eq!(
        strings(Segment::split_lines(
            &node.rich_render(&c, &c.options().update_dimensions(3, 2))
        )),
        ["ab…", "   "]
    );
    let node = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(Text::new("XX"))).content_width(),
            LayoutNode::leaf(Box::new(Text::new("R"))),
        ],
    );
    assert_eq!(
        strings(Segment::split_lines(
            &node.rich_render(&c, &c.options().update_dimensions(6, 1))
        )),
        ["XXR   "]
    );
}

#[test]
fn cross_axis_constraints_bound_children_and_nested_intrinsic_content_survives() {
    let c = Console::builder().width(10).height(3).build();
    let vertical = LayoutNode::split(
        Axis::Vertical,
        vec![LayoutNode::leaf(Box::new(Text::new("ABCDEFGHIJ")))
            .width(Constraint::fixed(3))
            .overflow(OverflowPolicy::Crop)],
    );
    let rows = strings(Segment::split_lines(
        &vertical.rich_render(&c, &c.options().update_dimensions(10, 2)),
    ));
    assert_eq!(rows, ["ABC       ", "          "]);
    let horizontal = LayoutNode::split(
        Axis::Horizontal,
        vec![LayoutNode::leaf(Box::new(Text::new("A\nB\nC"))).height(Constraint::fixed(1))],
    );
    assert_eq!(
        strings(Segment::split_lines(
            &horizontal.rich_render(&c, &c.options().update_dimensions(3, 3))
        )),
        ["A  ", "   ", "   "]
    );
    let nested = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(Text::new("ABC"))),
            LayoutNode::leaf(Box::new(Text::new("DEF"))),
        ],
    )
    .content_width();
    let outer = LayoutNode::split(
        Axis::Horizontal,
        vec![nested, LayoutNode::leaf(Box::new(Text::new("X")))],
    );
    assert_eq!(
        strings(Segment::split_lines(
            &outer.rich_render(&c, &c.options().update_dimensions(10, 1))
        )),
        ["ABCDEFX   "]
    );
}

#[test]
fn intrinsic_height_uses_descendant_allocated_widths() {
    let c = Console::builder().width(6).height(3).build();
    let row = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(Text::new("ABCDEF")))
                .width(Constraint::fixed(3))
                .content_height(),
            LayoutNode::leaf(Box::new(Text::new("XY")))
                .width(Constraint::fixed(3))
                .content_height(),
        ],
    )
    .content_height();
    let node = LayoutNode::split(
        Axis::Vertical,
        vec![row, LayoutNode::leaf(Box::new(Text::new("tail")))],
    );
    assert_eq!(
        strings(Segment::split_lines(
            &node.rich_render(&c, &c.options().update_dimensions(6, 3))
        )),
        ["ABCXY ", "DEF   ", "tail  "]
    );
}

/// #134's nested-panel criterion: panels as layout leaves, a split nested inside
/// a split, and a layout inside a panel, all bounded and cell-exact.
#[test]
fn nested_panels_compose_inside_and_around_layouts() {
    use rich::Panel;
    let console = Console::builder()
        .width(30)
        .height(8)
        .no_color(true)
        .build();
    let node = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(
                Panel::new(Box::new(Text::new("nav"))).title("Side"),
            ))
            .width(Constraint::fixed(12)),
            LayoutNode::split(
                Axis::Vertical,
                vec![
                    LayoutNode::leaf(Box::new(Panel::new(Box::new(Text::new("top")))))
                        .height(Constraint::fixed(3)),
                    LayoutNode::leaf(Box::new(Panel::new(Box::new(Text::new("bottom"))))),
                ],
            ),
        ],
    );
    let rows = strings(Segment::split_lines(
        &node.rich_render(&console, &console.options().update_dimensions(30, 8)),
    ));
    assert_eq!(rows.len(), 8, "{rows:#?}");
    for row in &rows {
        assert_eq!(rich::cells::cell_len(row), 30, "{rows:#?}");
    }
    // Like upstream `Layout`, leaves receive their region's height, so panels
    // fill it (byte-identical to rich 15.0.0's `Layout` for the same tree).
    // Left: a 12-cell panel. Right: a 3-row panel above the rest of the column.
    assert_eq!(
        rows,
        [
            "╭── Side ──╮╭────────────────╮",
            "│ nav      ││ top            │",
            "│          │╰────────────────╯",
            "│          │╭────────────────╮",
            "│          ││ bottom         │",
            "│          ││                │",
            "│          ││                │",
            "╰──────────╯╰────────────────╯",
        ]
    );

    // `.content_height()` opts a leaf back into its natural height.
    let natural = LayoutNode::leaf(Box::new(Panel::new(Box::new(Text::new("x"))))).content_height();
    let rows = strings(Segment::split_lines(
        &natural.rich_render(&console, &console.options().update_dimensions(6, 5)),
    ));
    assert_eq!(rows, ["╭────╮", "│ x  │", "╰────╯", "      ", "      "]);

    // A layout inside a panel takes the panel's inner width.
    let inner = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(Text::new("a"))).width(Constraint::fixed(4)),
            LayoutNode::leaf(Box::new(Text::new("b"))).align(Alignment::End, Alignment::Start),
        ],
    );
    let panel = Panel::new(Box::new(inner));
    let rows = strings(Segment::split_lines(
        &panel.rich_render(&console, &console.options().update_dimensions(14, 3)),
    ));
    assert_eq!(rows[1], "│ a        b │", "{rows:#?}");
    for width in [0, 1, 2, 5, 12, 29] {
        for height in [0, 1, 3, 8] {
            let rows = Segment::split_lines(&node.rich_render(
                &console,
                &console.options().update_dimensions(width, height),
            ));
            assert!(rows.len() <= height);
            assert!(rows
                .iter()
                .all(|r| r.iter().map(Segment::cell_length).sum::<usize>() <= width));
        }
    }
}
