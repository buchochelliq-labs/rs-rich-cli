// Neutral DOM snapshots exercise the public extraction/projection boundary.
fn projected_source(
    f: &Fake,
    width: u16,
    marks: &crate::layout::Marks,
) -> (SourcePage, crate::layout::Projection) {
    let source = build_source(
        &PageSnapshot {
            dom: f.snapshot(),
            ax: Value::Null,
        },
        &mut Revisions::new(),
    );
    let projection = crate::layout::project_source(&source, width, (8.0, 18.0), marks);
    (source, projection)
}

fn flow_text(f: &mut Fake, parent: i64, text: &str, bounds: [f64; 4]) -> i64 {
    let p = f.el(parent, "P", &[], "block");
    clip_bounds(f, p, bounds);
    let text = f.text(p, text);
    clip_bounds(f, text, bounds);
    p
}

#[test]
fn source_projection_nested_grids_follow_roots_despite_reversed_boxes_and_overhang() {
    let mut f = Fake::new();
    clip_bounds(&mut f, 0, [0.0, 0.0, 960.0, 900.0]);
    // DOMSnapshot's document layout object has no element styles.
    clip_style(&mut f, 0, 14, "");
    let outer = f.el(0, "DIV", &[], "grid");
    clip_bounds(&mut f, outer, [0.0, 0.0, 960.0, 144.0]);
    for (side, x) in [("first", 480.0), ("second", 0.0)] {
        let card = f.el(outer, "DIV", &[], "block");
        clip_bounds(&mut f, card, [x, 0.0, 480.0, 144.0]);
        let inner = f.el(card, "DIV", &[], "grid");
        clip_bounds(&mut f, inner, [x, 0.0, 480.0, 144.0]);
        for (name, offset) in [("alpha", 240.0), ("beta", 0.0)] {
            let unit = f.el(inner, "DIV", &[], "block");
            clip_bounds(&mut f, unit, [x + offset, 0.0, 240.0, 144.0]);
            let link = f.el(unit, "A", &[("href", "/original")], "inline");
            clip_bounds(&mut f, link, [x + offset, 0.0, 200.0, 18.0]);
            let words = f.text(link, &format!("{side}-{name}"));
            clip_bounds(&mut f, words, [x + offset - 1.0, 0.0, 201.0, 18.0]);
        }
    }
    for width in [40, 80, 120] {
        let (source, p) = projected_source(&f, width, &crate::layout::Marks::default());
        let text = p
            .lines
            .iter()
            .map(crate::layout::Line::plain)
            .collect::<Vec<_>>()
            .join("\n");
        let positions: Vec<_> = ["first-alpha", "first-beta", "second-alpha", "second-beta"]
            .iter()
            .map(|word| {
                assert_eq!(text.matches(word).count(), 1, "{width}: {text}");
                text.find(word).unwrap()
            })
            .collect();
        assert!(
            positions[0] < positions[1]
                && positions[2] < positions[3]
                && positions[0] < positions[2]
                && positions[1] < positions[3],
            "source lanes/order at {width}: {text}"
        );
        for c in 0..4 {
            let control = &source.page().controls[c];
            assert_eq!(
                source
                    .page()
                    .check(&control.binding(source.page()))
                    .unwrap()
                    .node,
                control.node
            );
            assert!(
                p.lines
                    .iter()
                    .flat_map(|l| &l.spans)
                    .any(|s| s.control == Some(c))
            );
        }
        assert!(p.lines.iter().all(|l| l.width() <= width as usize));
    }
}

#[test]
fn source_projection_video_pairs_with_accessible_live_chat_logs_only() {
    for has_entries in [true, false] {
        let mut f = Fake::new();
        let panes = f.el(0, "DIV", &[], "flex");
        clip_bounds(&mut f, panes, [0.0, 0.0, 960.0, 720.0]);
        clip_style(&mut f, panes, 11, "nowrap");
        let video_section = f.el(
            panes,
            "SECTION",
            &[("aria-label", "Video")],
            "block",
        );
        clip_bounds(&mut f, video_section, [0.0, 0.0, 560.0, 720.0]);
        let video = f.el(video_section, "VIDEO", &[], "block");
        clip_bounds(&mut f, video, [0.0, 0.0, 560.0, 315.0]);
        let chat_section = f.el(panes, "SECTION", &[("aria-label", "Chat")], "block");
        clip_bounds(&mut f, chat_section, [560.0, 0.0, 400.0, 720.0]);
        let live = f.el(chat_section, "DIV", &[("aria-live", "polite")], "block");
        clip_bounds(&mut f, live, [560.0, 0.0, 400.0, 600.0]);
        if has_entries {
            for (n, message) in ["first chat message", "second chat message"]
                .into_iter()
                .enumerate()
            {
                let entry = f.el(live, "DIV", &[], "block");
                clip_bounds(&mut f, entry, [560.0, n as f64 * 36.0, 400.0, 36.0]);
                let text = f.text(entry, message);
                clip_bounds(&mut f, text, [560.0, n as f64 * 36.0, 400.0, 18.0]);
            }
        } else {
            let text = f.text(live, "Connection status");
            clip_bounds(&mut f, text, [560.0, 0.0, 400.0, 18.0]);
        }

        let source = build_source(
            &PageSnapshot {
                dom: f.snapshot(),
                ax: Value::Null,
            },
            &mut Revisions::new(),
        );
        assert_eq!(source.page().logs.len(), usize::from(has_entries));
        assert_eq!(
            source.page().grids.len(),
            usize::from(has_entries),
            "only a named video pane beside an actual accessible log is paired"
        );
    }
}

#[test]
fn source_projection_nested_figure_units_measure_text_and_remove_internal_source_gaps() {
    let mut f = Fake::new();
    clip_bounds(&mut f, 0, [0.0, 0.0, 960.0, 2000.0]);
    let article = f.el(0, "ARTICLE", &[], "block");
    clip_bounds(&mut f, article, [0.0, 0.0, 960.0, 1800.0]);
    flow_text(&mut f, article, "Before", [0.0, 0.0, 960.0, 18.0]);
    let figure = f.el(article, "FIGURE", &[], "block");
    clip_bounds(&mut f, figure, [400.0, 180.0, 560.0, 1100.0]);
    let image = f.el(
        figure,
        "IMG",
        &[("src", "/image.png"), ("alt", "")],
        "block",
    );
    clip_bounds(&mut f, image, [400.0, 180.0, 320.0, 72.0]);
    let caption = f.el(figure, "FIGCAPTION", &[], "block");
    clip_bounds(&mut f, caption, [400.0, 700.0, 560.0, 72.0]);
    let link = f.el(caption, "A", &[("href", "/caption")], "inline");
    clip_bounds(&mut f, link, [400.0, 700.0, 560.0, 72.0]);
    let text = f.text(
        link,
        "Caption café 界 extraordinarylongunbrokenwordabcdefghijklmnopqrstuvwxyz",
    );
    clip_bounds(&mut f, text, [400.0, 700.0, 560.0, 72.0]);
    let nested = f.node(figure, 1, "FIGURE", None, &[], None);
    let nested_image = f.el(
        nested,
        "IMG",
        &[("src", "/nested.png"), ("alt", "")],
        "block",
    );
    clip_bounds(&mut f, nested_image, [400.0, 900.0, 160.0, 36.0]);
    let nested_caption = f.el(nested, "FIGCAPTION", &[], "block");
    clip_bounds(&mut f, nested_caption, [400.0, 1000.0, 560.0, 18.0]);
    let nested_link = f.el(
        nested_caption,
        "A",
        &[("href", "/nested-caption")],
        "inline",
    );
    clip_bounds(&mut f, nested_link, [400.0, 1000.0, 560.0, 18.0]);
    let nested_text = f.text(nested_link, "Nested");
    clip_bounds(&mut f, nested_text, [400.0, 1000.0, 560.0, 18.0]);
    flow_text(&mut f, article, "After", [0.0, 1600.0, 960.0, 18.0]);
    let mut marks = crate::layout::Marks::default();
    marks.adapt.set(crate::adapt::Rule::CollapseGaps, false);
    let mut caption_anchor = None;
    for width in [40, 80, 120] {
        let (_, p) = projected_source(&f, width, &marks);
        let lines: Vec<_> = p.lines.iter().map(crate::layout::Line::plain).collect();
        let text = lines.join("\n");
        for word in ["Before", "Caption", "café", "界", "Nested", "After"] {
            assert_eq!(text.matches(word).count(), 1, "{width}: {text}");
        }
        assert_eq!(p.regions.len(), 2, "nested media must be consumed once");
        let joined: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(
            joined
                .matches("extraordinarylongunbrokenwordabcdefghijklmnopqrstuvwxyz")
                .count(),
            1,
            "long word clipped at {width}: {text}"
        );
        assert!(
            p.lines
                .iter()
                .flat_map(|l| &l.spans)
                .any(|s| s.control == Some(1) && s.text.contains("Nested"))
        );
        assert!(
            p.lines.len() < 18,
            "obsolete source gaps at {width}: {} rows",
            p.lines.len()
        );
        let image = p.regions[0];
        let caption = lines.iter().position(|s| s.contains("Caption")).unwrap();
        assert_eq!(caption, image.row + image.rows);
        assert!(p.y[caption] >= 700.0, "caption source anchor lost");
        if let Some(key) = caption_anchor {
            assert_eq!(
                p.rows.get(&key),
                Some(&caption),
                "resize must restore caption's source key"
            );
        }
        caption_anchor = Some(p.keys[caption]);
        assert!(
            p.lines
                .iter()
                .flat_map(|l| &l.spans)
                .any(|s| s.control == Some(0) && s.text.contains("Caption"))
        );
    }
}

#[test]
fn source_projection_offscreen_canonical_ranges_follow_only_their_known_visible_twin() {
    let mut f = Fake::new();
    let p = f.el(0, "P", &[], "block");
    clip_bounds(&mut f, p, [0.0, 36.0, 640.0, 18.0]);
    let before = f.text(p, "Before ");
    clip_bounds(&mut f, before, [0.0, 36.0, 80.0, 18.0]);
    let spoken = f.el(p, "SPAN", &[], "inline");
    clip_bounds(&mut f, spoken, [-10000.0, -10000.0, 80.0, 18.0]);
    clip_style(&mut f, spoken, 8, "0");
    let said = f.text(spoken, "$29.99");
    clip_bounds(&mut f, said, [-10000.0, -10000.0, 80.0, 18.0]);
    let shown = f.el(p, "SPAN", &[("aria-hidden", "true")], "inline");
    clip_bounds(&mut f, shown, [160.0, 36.0, 80.0, 18.0]);
    let visual = f.text(shown, "$2999");
    clip_bounds(&mut f, visual, [160.0, 36.0, 80.0, 18.0]);
    let after = f.text(p, " After");
    clip_bounds(&mut f, after, [320.0, 36.0, 80.0, 18.0]);
    let (source, projected) = projected_source(&f, 80, &crate::layout::Marks::default());
    assert_eq!(source.relations.twins.len(), 1);
    let text = projected
        .lines
        .iter()
        .map(crate::layout::Line::plain)
        .collect::<Vec<_>>()
        .join("\n");
    for word in ["Before", "$29.99", "After"] {
        assert_eq!(text.matches(word).count(), 1, "{text}");
    }
    assert!(!text.contains("$2999"));
    let price = projected
        .lines
        .iter()
        .find(|l| l.plain().contains("$29.99"))
        .unwrap()
        .plain();
    assert_eq!(
        price.find("$29.99"),
        Some(20),
        "canonical must follow shown x: {price}"
    );
    clip_bounds(&mut f, shown, [-10000.0, -10000.0, 80.0, 18.0]);
    clip_bounds(&mut f, visual, [-10000.0, -10000.0, 80.0, 18.0]);
    let (_, clipped) = projected_source(&f, 80, &crate::layout::Marks::default());
    assert!(!clipped.lines.iter().any(|l| l.plain().contains("29.99")));
}

#[test]
fn source_projection_linked_image_placeholder_uses_ancestor_action_not_overlap() {
    let mut f = Fake::new();
    let link = f.node(0, 1, "A", None, &[("href", "/image-story")], None);
    let image = f.el(
        link,
        "IMG",
        &[("src", "/image.png"), ("alt", "Photograph")],
        "block",
    );
    clip_bounds(&mut f, image, [0.0, 0.0, 240.0, 90.0]);
    let unrelated = f.el(0, "A", &[("href", "/other")], "inline");
    clip_bounds(&mut f, unrelated, [0.0, 0.0, 48.0, 18.0]);
    let text = f.text(unrelated, "Other");
    clip_bounds(&mut f, text, [0.0, 0.0, 48.0, 18.0]);
    let mut dom = f.snapshot();
    dom["documents"][0]["layout"]["paintOrders"] = json!(
        f.layout
            .iter()
            .map(|&node| if node >= unrelated { 2 } else { 1 })
            .collect::<Vec<_>>()
    );
    let source = build_source(
        &PageSnapshot {
            dom,
            ax: Value::Null,
        },
        &mut Revisions::new(),
    );
    let p =
        crate::layout::project_source(&source, 40, (8.0, 18.0), &crate::layout::Marks::default());
    let actual = source
        .page()
        .controls
        .iter()
        .position(|c| c.node == 100 + link)
        .unwrap();
    let region = p.regions[0];
    assert_eq!(
        crate::layout::control_at(&p.lines[region.row + 2], region.column + 8),
        Some(actual)
    );
    let other = source
        .page()
        .controls
        .iter()
        .position(|c| c.node == 100 + unrelated)
        .unwrap();
    assert!(
        !p.overlays.is_empty(),
        "explicit foreground should overlay its image"
    );
    assert_eq!(
        crate::layout::control_at(&p.lines[region.row], region.column + 1),
        Some(other)
    );
    assert!(
        p.lines
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| s.control == Some(other) && s.text.contains("Other"))
    );
    let control = &source.page().controls[actual];
    assert_eq!(
        source
            .page()
            .check(&control.binding(source.page()))
            .unwrap()
            .node,
        100 + link
    );
    let mut presentation = crate::presentation::Presentation::default();
    let presented = presentation.update_source(
        &source,
        crate::config::View::Projection,
        40,
        &crate::layout::Marks::default(),
    );
    assert_eq!(
        presented.as_ref(),
        &p,
        "app must consume the exact paired projection"
    );
    let mut decoded = source.page().as_ref().clone();
    decoded.media[0].still = Some(Arc::from([1u8, 2, 3]));
    decoded.media[0].picture = rich_intuituive::Picture::from_rgba(1, 1, vec![10, 20, 30, 255]);
    let decoded = source.clone().republish(Arc::new(decoded));
    let updated = presentation.update_source(
        &decoded,
        crate::config::View::Projection,
        40,
        &crate::layout::Marks::default(),
    );
    assert!(
        std::rc::Rc::ptr_eq(&presented, &updated),
        "decode must reuse layout/hits"
    );
    assert_eq!(
        crate::layout::control_at(&updated.lines[region.row + 2], region.column + 8),
        Some(actual)
    );
    assert!(
        decoded
            .page()
            .check(&control.binding(source.page()))
            .is_ok()
    );
}

#[test]
fn source_projection_unsupported_grids_keep_source_geometry() {
    for unsupported in [
        "boxless",
        "unequal",
        "positioned",
        "unknown",
        "positioned-parent",
    ] {
        let mut f = Fake::new();
        clip_bounds(&mut f, 0, [0.0, 0.0, 640.0, 900.0]);
        let parent = f.el(0, "DIV", &[], "block");
        clip_bounds(&mut f, parent, [0.0, 0.0, 640.0, 500.0]);
        if unsupported == "positioned-parent" {
            clip_style(&mut f, parent, 14, "absolute");
        }
        let grid = f.el(parent, "DIV", &[], "grid");
        clip_bounds(&mut f, grid, [0.0, 0.0, 640.0, 500.0]);
        if unsupported == "positioned" {
            clip_style(&mut f, grid, 14, "absolute");
        }
        if unsupported == "unknown" {
            clip_style(&mut f, grid, 14, "");
        }
        for (name, x, height) in [
            ("Primary", 320.0, 90.0),
            (
                "Aside",
                0.0,
                if unsupported == "unequal" {
                    400.0
                } else {
                    90.0
                },
            ),
        ] {
            let item = if unsupported == "boxless" {
                f.node(grid, 1, "DIV", None, &[], None)
            } else {
                let i = f.el(grid, "DIV", &[], "block");
                clip_bounds(&mut f, i, [x, 0.0, 320.0, height]);
                i
            };
            flow_text(&mut f, item, name, [x, 0.0, 120.0, 18.0]);
        }
        let mut marks = crate::layout::Marks::default();
        marks.adapt.set(crate::adapt::Rule::CollapseNav, false);
        let (source, result) = projected_source(&f, 80, &marks);
        marks.adapt.set(crate::adapt::Rule::SnapGrids, false);
        assert_eq!(
            result,
            crate::layout::project_source(&source, 80, (8.0, 18.0), &marks),
            "unsupported {unsupported} must bypass equal cards"
        );
    }
}

#[test]
fn source_projection_figure_rule_faithful_popup_and_positioned_precedence() {
    let mut f = Fake::new();
    clip_bounds(&mut f, 0, [0.0, 0.0, 640.0, 1800.0]);
    let article = f.el(0, "ARTICLE", &[], "block");
    clip_bounds(&mut f, article, [0.0, 0.0, 640.0, 1800.0]);
    flow_text(&mut f, article, "Before", [0.0, 0.0, 640.0, 18.0]);
    let figure = f.el(article, "FIGURE", &[], "block");
    clip_bounds(&mut f, figure, [0.0, 600.0, 640.0, 300.0]);
    flow_text(&mut f, figure, "Figure", [0.0, 600.0, 640.0, 18.0]);
    flow_text(&mut f, article, "After", [0.0, 1200.0, 640.0, 18.0]);
    let mut marks = crate::layout::Marks::default();
    marks.adapt.set(crate::adapt::Rule::CollapseGaps, false);
    let (source, adapted) = projected_source(&f, 80, &marks);
    assert_eq!(adapted.lines.len(), 3);
    marks.adapt.set(crate::adapt::Rule::FigureFlow, false);
    let legacy = crate::layout::project(source.page(), 80, (8.0, 18.0), &marks);
    assert_eq!(
        crate::layout::project_source(&source, 80, (8.0, 18.0), &marks),
        legacy
    );
    marks.adapt = crate::adapt::Adapt::none();
    assert_eq!(
        crate::layout::project_source(&source, 80, (8.0, 18.0), &marks),
        crate::layout::project(source.page(), 80, (8.0, 18.0), &marks)
    );
    // A positioned ancestor forbids reflow even though FIGURE itself is static.
    clip_style(&mut f, article, 14, "absolute");
    let (_, positioned) = projected_source(
        &f,
        80,
        &crate::layout::Marks {
            adapt: crate::adapt::Adapt::default().with(crate::adapt::Rule::CollapseGaps, false),
            ..Default::default()
        },
    );
    assert!(positioned.lines.len() > 30);
    clip_style(&mut f, article, 14, "static");
    let dialog = f.el(0, "DIV", &[("role", "dialog")], "block");
    clip_bounds(&mut f, dialog, [320.0, 36.0, 240.0, 180.0]);
    clip_style(&mut f, dialog, 14, "fixed");
    clip_style(&mut f, dialog, 3, "rgb(255,255,255)");
    flow_text(&mut f, dialog, "Popup", [320.0, 36.0, 240.0, 18.0]);
    let (source, _) = projected_source(&f, 80, &crate::layout::Marks::default());
    assert!(!source.page().overlays.is_empty());
    let mut presentation = crate::presentation::Presentation::default();
    let shown = presentation.update_source(
        &source,
        crate::config::View::Projection,
        80,
        &crate::layout::Marks::default(),
    );
    assert!(
        shown.lines.len() > 30,
        "source popup must disable FigureFlow in its underlay"
    );
}

#[test]
fn source_projection_dialog_image_keeps_actual_action_and_source_coordinates() {
    let mut f = Fake::new();
    clip_bounds(&mut f, 0, [0.0, 0.0, 640.0, 800.0]);
    let dialog = f.el(0, "DIV", &[("role", "dialog")], "block");
    clip_bounds(&mut f, dialog, [80.0, 180.0, 320.0, 180.0]);
    clip_style(&mut f, dialog, 14, "fixed");
    clip_style(&mut f, dialog, 3, "rgb(255,255,255)");
    let link = f.node(dialog, 1, "A", None, &[("href", "/inside")], None);
    let image = f.el(link, "IMG", &[("src", "/image.png"), ("alt", "")], "block");
    clip_bounds(&mut f, image, [80.0, 180.0, 240.0, 90.0]);
    let (source, _) = projected_source(&f, 80, &crate::layout::Marks::default());
    assert_eq!(source.page().overlays.len(), 1);
    let marks = crate::layout::Marks::default();
    let popup = crate::layout::project_source_overlay(&source, 0, 40, (8.0, 18.0), &marks).unwrap();
    assert_eq!(popup.regions[0].row, 0);
    assert_eq!(popup.regions[0].column, 0);
    assert_eq!(crate::layout::control_at(&popup.lines[2], 8), Some(0));
}

#[test]
fn source_projection_figure_keeps_table_rows_as_measured_units() {
    let mut f = Fake::new();
    clip_bounds(&mut f, 0, [0.0, 0.0, 640.0, 1200.0]);
    let article = f.el(0, "ARTICLE", &[], "block");
    clip_bounds(&mut f, article, [0.0, 0.0, 640.0, 1200.0]);
    let figure = f.el(article, "FIGURE", &[], "block");
    clip_bounds(&mut f, figure, [0.0, 180.0, 640.0, 800.0]);
    let table = f.el(figure, "TABLE", &[], "table");
    clip_bounds(&mut f, table, [0.0, 180.0, 640.0, 800.0]);
    let row = f.el(table, "TR", &[], "table-row");
    clip_bounds(&mut f, row, [0.0, 180.0, 640.0, 18.0]);
    for (word, x) in [("Left", 0.0), ("Right", 320.0)] {
        let cell = f.el(row, "TD", &[], "table-cell");
        clip_bounds(&mut f, cell, [x, 180.0, 320.0, 18.0]);
        let text = f.text(cell, word);
        clip_bounds(&mut f, text, [x, 180.0, 120.0, 18.0]);
    }
    flow_text(&mut f, article, "After", [0.0, 1100.0, 640.0, 18.0]);
    let (_, p) = projected_source(&f, 40, &crate::layout::Marks::default());
    assert!(
        p.lines
            .iter()
            .any(|l| l.plain().contains("Left") && l.plain().contains("Right")),
        "table row split: {:?}",
        p.lines
    );
    assert!(p.lines.len() < 5);
}

#[test]
fn source_projection_table_canonical_ranges_use_exact_cell_addresses() {
    let mut f = Fake::new();
    clip_bounds(&mut f, 0, [0.0, 0.0, 640.0, 800.0]);
    let article = f.el(0, "ARTICLE", &[], "block");
    clip_bounds(&mut f, article, [0.0, 0.0, 640.0, 800.0]);
    let figure = f.el(article, "FIGURE", &[], "block");
    clip_bounds(&mut f, figure, [0.0, 0.0, 640.0, 180.0]);
    let table = f.el(figure, "TABLE", &[], "table");
    clip_bounds(&mut f, table, [0.0, 36.0, 640.0, 18.0]);
    let row = f.el(table, "TR", &[], "table-row");
    clip_bounds(&mut f, row, [0.0, 36.0, 640.0, 18.0]);
    let cell = f.el(row, "TD", &[], "table-cell");
    clip_bounds(&mut f, cell, [0.0, 36.0, 640.0, 18.0]);
    let before = f.text(cell, "Before ");
    clip_bounds(&mut f, before, [0.0, 36.0, 80.0, 18.0]);
    let spoken = f.el(cell, "SPAN", &[], "inline");
    clip_bounds(&mut f, spoken, [-10000.0, -10000.0, 80.0, 18.0]);
    clip_style(&mut f, spoken, 8, "0");
    let said = f.text(spoken, "$29.99");
    clip_bounds(&mut f, said, [-10000.0, -10000.0, 80.0, 18.0]);
    let shown = f.el(cell, "SPAN", &[("aria-hidden", "true")], "inline");
    clip_bounds(&mut f, shown, [160.0, 36.0, 80.0, 18.0]);
    let visual = f.text(shown, "$2999");
    clip_bounds(&mut f, visual, [160.0, 36.0, 80.0, 18.0]);
    let (_, p) = projected_source(&f, 80, &crate::layout::Marks::default());
    let price = p
        .lines
        .iter()
        .find(|l| l.plain().contains("$29.99"))
        .unwrap()
        .plain();
    assert_eq!(price.find("$29.99"), Some(20), "exact cell alias: {price}");
    assert!(price.contains("Before"));
}

#[test]
fn source_projection_figure_does_not_flatten_a_descendant_source_grid() {
    let mut f = Fake::new();
    clip_bounds(&mut f, 0, [0.0, 0.0, 640.0, 1200.0]);
    let article = f.el(0, "ARTICLE", &[], "block");
    clip_bounds(&mut f, article, [0.0, 0.0, 640.0, 1200.0]);
    let figure = f.el(article, "FIGURE", &[], "block");
    clip_bounds(&mut f, figure, [0.0, 0.0, 640.0, 180.0]);
    flow_text(&mut f, figure, "Caption", [0.0, 0.0, 640.0, 18.0]);
    let grid = f.el(article, "DIV", &[], "grid");
    clip_bounds(&mut f, grid, [0.0, 360.0, 640.0, 180.0]);
    for (word, x) in [("Leftpane", 0.0), ("Rightpane", 320.0)] {
        let item = f.el(grid, "DIV", &[], "block");
        clip_bounds(&mut f, item, [x, 360.0, 320.0, 180.0]);
        flow_text(&mut f, item, word, [x, 360.0, 160.0, 18.0]);
    }
    flow_text(&mut f, article, "Aftergrid", [0.0, 1100.0, 640.0, 18.0]);
    for width in [40, 80, 120] {
        let marks = crate::layout::Marks {
            adapt: crate::adapt::Adapt::default().with(crate::adapt::Rule::CollapseGaps, false),
            ..Default::default()
        };
        let (_, p) = projected_source(&f, width, &marks);
        assert_eq!(
            p.lines.len(),
            3,
            "the grid's measured height must stack with its neighbors"
        );
        assert!(
            p.lines
                .iter()
                .any(|l| l.plain().contains("Leftpane") && l.plain().contains("Rightpane")),
            "source grid must retain its lanes: {:?}",
            p.lines
        );
    }
}

#[test]
fn source_projection_faithful_keeps_mixed_boxless_figure_runs_in_their_original_block() {
    let mut f = Fake::new();
    clip_bounds(&mut f, 0, [0.0, 0.0, 640.0, 800.0]);
    let p = f.el(0, "DIV", &[], "block");
    clip_bounds(&mut f, p, [0.0, 0.0, 640.0, 18.0]);
    let before = f.text(p, "Before ");
    clip_bounds(&mut f, before, [0.0, 0.0, 80.0, 18.0]);
    let figure = f.node(p, 1, "FIGURE", None, &[], None);
    let middle = f.text(figure, "Middle ");
    clip_bounds(&mut f, middle, [80.0, 0.0, 80.0, 18.0]);
    let after = f.text(p, "After");
    clip_bounds(&mut f, after, [160.0, 0.0, 80.0, 18.0]);
    let marks = crate::layout::Marks {
        adapt: crate::adapt::Adapt::none(),
        ..Default::default()
    };
    let (source, result) = projected_source(&f, 80, &marks);
    let legacy = crate::layout::project(source.page(), 80, (8.0, 18.0), &marks);
    assert_eq!(
        result.lines, legacy.lines,
        "faithful must not split source run owners into new rows"
    );
}

fn native_source() -> SourcePage {
    let mut snapshot = PageSnapshot {
        dom: serde_json::from_str(include_str!("../../tests/snapshots/source-native/dom.json"))
            .unwrap(),
        ax: serde_json::from_str(include_str!("../../tests/snapshots/source-native/ax.json"))
            .unwrap(),
    };
    snapshot.dom["sourceFormatting"] = serde_json::from_str(include_str!(
        "../../tests/snapshots/source-native/formatting.json"
    ))
    .unwrap();
    let mut source = build_source(&snapshot, &mut Revisions::new());
    let page = Arc::make_mut(&mut source.page);
    for media in &mut page.media {
        media.picture = Some(rich_intuituive::Picture::new(
            image::load_from_memory(include_bytes!(
                "../../tests/snapshots/source-native/glow.png"
            ))
            .unwrap(),
        ));
    }
    source
}

fn native_projection(source: &SourcePage) -> crate::layout::Projection {
    let mut marks = crate::layout::Marks::default();
    marks.adapt.set(crate::adapt::Rule::CollapseNav, false);
    let projection = crate::layout::project_source(source, 40, (8., 18.), &marks);
    if let Ok(path) = std::env::var("NOCT_SOURCE_EVIDENCE") {
        std::fs::write(
            path,
            format!(
                "SOURCE {source:#?}\nREGIONS {:#?}\nLINES\n{}",
                projection.regions,
                projection
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{i}: {}", l.plain()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        )
        .unwrap();
    }
    projection
}

#[test]
fn source_native_captured_figure_images_keep_broad_reservations_and_hits() {
    let source = native_source();
    let p = native_projection(&source);
    let mut undecoded = source.clone();
    for media in &mut Arc::make_mut(&mut undecoded.page).media {
        media.picture = None;
    }
    assert_eq!(
        p,
        native_projection(&undecoded),
        "decode must preserve reservations, control hits and anchors"
    );
    assert_eq!(p.regions.len(), 2);
    for r in &p.regions {
        let media = &source.page().media[r.media];
        let expected = (media.rect.width / 8.).round() as usize;
        assert_eq!(
            r.cols, expected,
            "decoded image must retain its source aspect/reservation: {r:?}"
        );
        let control = source.relations.media_controls[r.media].unwrap();
        for row in r.row..r.row + r.rows {
            let covered: usize = p.lines[row]
                .spans
                .iter()
                .filter(|s| s.control == Some(control))
                .map(|s| rich::cells::cell_len(&s.text))
                .sum();
            assert_eq!(
                covered, r.cols,
                "image control must cover reserved cells at {row}"
            );
        }
    }
}

#[test]
fn source_native_captured_boxless_spanning_grid_keeps_complete_words_and_links() {
    let source = native_source();
    let p = native_projection(&source);
    let words = p.lines.iter().map(|l| l.plain()).collect::<String>();
    let compact: String = words.chars().filter(|c| !c.is_whitespace()).collect();
    for word in [
        "ASIDE-01",
        "TAIL-01",
        "MAIN-01",
        "INNER-B-01",
        "INNER-A-01",
        "MAIN-TEXT-02",
        "SourceRelationshipWordWithoutNaturalBreaks0123456789",
        "AFTER-01",
    ] {
        assert_eq!(compact.matches(word).count(), 1, "{word}: {words}");
    }
    for (control, c) in source.page().controls.iter().enumerate() {
        if c.href.as_deref().is_some_and(|h| {
            ["#target-aside", "#target-tail", "#target-a", "#target-b"]
                .iter()
                .any(|end| h.ends_with(end))
        }) {
            let rendered: String = p
                .lines
                .iter()
                .flat_map(|l| &l.spans)
                .filter(|s| s.control == Some(control))
                .flat_map(|s| s.text.chars())
                .filter(|c| !c.is_whitespace())
                .collect();
            assert_eq!(
                rendered,
                c.name
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
            );
        }
    }
    assert!(
        words.find("MAIN-TEXT-02").unwrap() < words.find("AFTER-01").unwrap(),
        "outside neighbor must follow the whole grid: {words}"
    );
    assert!(p.lines.iter().all(|l| l.width() <= 40));
}


#[test]
fn source_review_code_rewraps_at_final_lane_width_without_losing_characters() {
    for x in [0.0, 320.0] {
        let mut f = Fake::new();
        clip_bounds(&mut f, 0, [0.0, 0.0, 640.0, 900.0]);
        clip_style(&mut f, 0, 14, "");
        let grid = f.el(0, "DIV", &[], "grid");
        clip_bounds(&mut f, grid, [0.0, 0.0, 640.0, 144.0]);
        let code = "  ABCDEFGHIJKLMNOPQRSTUVWXYZ0123\n    second_line\n\nlast_line";
        for lane_x in [0.0, 320.0] {
            let card = f.el(grid, "DIV", &[], "block");
            clip_bounds(&mut f, card, [lane_x, 0.0, 320.0, 144.0]);
            if lane_x == x {
                let pre = f.el(card, "PRE", &[], "block");
                clip_bounds(&mut f, pre, [lane_x, 0.0, 320.0, 72.0]);
                let text = f.text(pre, code);
                clip_bounds(&mut f, text, [lane_x, 0.0, 320.0, 72.0]);
                flow_text(&mut f, card, "CODE-FOLLOWER", [lane_x, 72.0, 320.0, 18.0]);
            } else {
                flow_text(&mut f, card, "OTHER-LANE", [lane_x, 0.0, 320.0, 18.0]);
            }
        }
        for width in [40, 80, 120] {
            let (_, p) = projected_source(&f, width, &crate::layout::Marks::default());
            let code_rows: Vec<_> = p.lines.iter().enumerate().filter_map(|(i,l)| {
                let text: String = l.spans.iter().filter(|s| s.style == "cyan").map(|s|s.text.as_str()).collect();
                (!text.is_empty()).then_some((i,text))
            }).collect();
            let actual: String = code_rows.iter().map(|(_,s)|s.as_str()).collect();
            assert_eq!(actual, code.replace('\n', ""), "x={x}, width={width}: {:?}", p.lines);
            assert!(code_rows.iter().any(|(_,s)|s.starts_with("    second_line")), "original indentation/line boundary");
            assert!(code_rows.iter().any(|(_,s)|s=="last_line"), "original last line boundary");
            let follower=p.lines.iter().position(|l|l.plain().contains("CODE-FOLLOWER")).unwrap();
            assert!(follower>code_rows.last().unwrap().0, "follower must use rewrapped height");
            assert!(code_rows.len() <= 6, "right-side source clipping must not prewrap at one cell");
        }
    }
}

#[test]
fn source_review_expanding_figure_keeps_all_internal_units_before_outside_sibling() {
    let mut f=Fake::new();
    clip_bounds(&mut f,0,[0.0,0.0,640.0,900.0]);
    clip_style(&mut f,0,14,"");
    let article=f.el(0,"ARTICLE",&[],"block");
    clip_bounds(&mut f,article,[0.0,0.0,640.0,72.0]);
    let figure=f.el(article,"FIGURE",&[],"block");
    clip_bounds(&mut f,figure,[0.0,0.0,640.0,36.0]);
    let caption=f.el(figure,"FIGCAPTION",&[],"block");
    clip_bounds(&mut f,caption,[0.0,0.0,640.0,36.0]);
    let link=f.el(caption,"A",&[("href","/original-caption")],"inline");
    clip_bounds(&mut f,link,[0.0,0.0,640.0,36.0]);
    let text=f.text(link,"A long caption with enough words to occupy four full terminal rows when narrowed and keep its original source link intact.");
    clip_bounds(&mut f,text,[0.0,0.0,640.0,36.0]);
    flow_text(&mut f,article,"INSIDE-FIRST",[0.0,36.0,640.0,18.0]);
    flow_text(&mut f,article,"INSIDE-LAST",[0.0,54.0,640.0,18.0]);
    flow_text(&mut f,0,"OUTSIDE-FOLLOWING",[0.0,72.0,640.0,18.0]);
    let marks=crate::layout::Marks::default();
    let mut anchor=None;
    for width in [40,80,120,40] {
        let (source,p)=projected_source(&f,width,&marks);
        let row=|word:&str|p.lines.iter().position(|l|l.plain().contains(word)).unwrap();
        assert!(row("INSIDE-FIRST")<row("INSIDE-LAST"));
        assert!(row("INSIDE-LAST")<row("OUTSIDE-FOLLOWING"), "width={width}: {:?}",p.lines);
        assert!(p.lines.iter().flat_map(|l|&l.spans).any(|s|s.control==Some(0)));
        assert_eq!(source.page().controls[0].node,100+link);
        let key=p.keys[row("INSIDE-LAST")];
        if let Some(old)=anchor {assert_eq!(key,old,"source anchor must survive resizing");}
        anchor=Some(key);
    }
}
