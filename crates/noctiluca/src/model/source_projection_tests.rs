// Neutral DOM snapshots exercise the public extraction/projection boundary.
fn projected_source(f: &Fake, width: u16, marks: &crate::layout::Marks) -> (SourcePage, crate::layout::Projection) {
    let source = build_source(&PageSnapshot { dom: f.snapshot(), ax: Value::Null }, &mut Revisions::new());
    // RED baseline: replaced with the companion entry point once implemented.
    let projection = crate::layout::project(source.page(), width, (8.0, 18.0), marks);
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
        let text = p.lines.iter().map(crate::layout::Line::plain).collect::<Vec<_>>().join("\n");
        let positions: Vec<_> = ["first-alpha", "first-beta", "second-alpha", "second-beta"].iter().map(|word| {
            assert_eq!(text.matches(word).count(), 1, "{width}: {text}");
            text.find(word).unwrap()
        }).collect();
        assert!(positions.windows(2).all(|p| p[0] < p[1]), "source order at {width}: {text}");
        for c in 0..4 {
            let control = &source.page().controls[c];
            assert_eq!(source.page().check(&control.binding(source.page())).unwrap().node, control.node);
            assert!(p.lines.iter().flat_map(|l| &l.spans).any(|s| s.control == Some(c)));
        }
        assert!(p.lines.iter().all(|l| l.width() <= width as usize));
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
    let image = f.el(figure, "IMG", &[("src", "/image.png"), ("alt", "")], "block");
    clip_bounds(&mut f, image, [400.0, 180.0, 320.0, 72.0]);
    let caption = f.el(figure, "FIGCAPTION", &[], "block");
    clip_bounds(&mut f, caption, [400.0, 700.0, 560.0, 72.0]);
    let link = f.el(caption, "A", &[("href", "/caption")], "inline");
    clip_bounds(&mut f, link, [400.0, 700.0, 560.0, 72.0]);
    let text = f.text(link, "Caption café 界 extraordinarylongunbrokenwordabcdefghijklmnopqrstuvwxyz");
    clip_bounds(&mut f, text, [400.0, 700.0, 560.0, 72.0]);
    let nested = f.node(figure, 1, "FIGURE", None, &[], None);
    flow_text(&mut f, nested, "Nested", [400.0, 1000.0, 560.0, 18.0]);
    flow_text(&mut f, article, "After", [0.0, 1600.0, 960.0, 18.0]);
    let mut marks = crate::layout::Marks::default();
    marks.adapt.set(crate::adapt::Rule::CollapseGaps, false);
    for width in [40, 80, 120] {
        let (_, p) = projected_source(&f, width, &marks);
        let lines: Vec<_> = p.lines.iter().map(crate::layout::Line::plain).collect();
        let text = lines.join("\n");
        for word in ["Before", "Caption", "café", "界", "Nested", "After"] {
            assert_eq!(text.matches(word).count(), 1, "{width}: {text}");
        }
        assert!(p.lines.len() < 18, "obsolete source gaps at {width}: {} rows", p.lines.len());
        let image = p.regions[0];
        let caption = lines.iter().position(|s| s.contains("Caption")).unwrap();
        assert_eq!(caption, image.row + image.rows);
        assert!(p.y[caption] >= 700.0, "caption source anchor lost");
        assert!(p.lines.iter().flat_map(|l| &l.spans).any(|s| s.control == Some(0) && s.text.contains("Caption")));
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
    let text = projected.lines.iter().map(crate::layout::Line::plain).collect::<Vec<_>>().join("\n");
    for word in ["Before", "$29.99", "After"] { assert_eq!(text.matches(word).count(), 1, "{text}"); }
    assert!(!text.contains("$2999"));
    let price = projected.lines.iter().find(|l| l.plain().contains("$29.99")).unwrap().plain();
    assert_eq!(price.find("$29.99"), Some(20), "canonical must follow shown x: {price}");
    clip_bounds(&mut f, shown, [-10000.0, -10000.0, 80.0, 18.0]);
    clip_bounds(&mut f, visual, [-10000.0, -10000.0, 80.0, 18.0]);
    let (_, clipped) = projected_source(&f, 80, &crate::layout::Marks::default());
    assert!(!clipped.lines.iter().any(|l| l.plain().contains("29.99")));
}

#[test]
fn source_projection_linked_image_placeholder_uses_ancestor_action_not_overlap() {
    let mut f = Fake::new();
    let link = f.node(0, 1, "A", None, &[("href", "/image-story")], None);
    let image = f.el(link, "IMG", &[("src", "/image.png"), ("alt", "Photograph")], "block");
    clip_bounds(&mut f, image, [0.0, 0.0, 240.0, 90.0]);
    let unrelated = f.el(0, "A", &[("href", "/other")], "inline");
    clip_bounds(&mut f, unrelated, [0.0, 0.0, 48.0, 18.0]);
    let text = f.text(unrelated, "Other");
    clip_bounds(&mut f, text, [0.0, 0.0, 48.0, 18.0]);
    let (source, p) = projected_source(&f, 40, &crate::layout::Marks::default());
    let actual = source.page().controls.iter().position(|c| c.node == 100 + link).unwrap();
    let region = p.regions[0];
    assert_eq!(crate::layout::control_at(&p.lines[region.row + 2], region.column + 8), Some(actual));
    let other = source.page().controls.iter().position(|c| c.node == 100 + unrelated).unwrap();
    assert!(p.lines.iter().flat_map(|l| &l.spans).any(|s| s.control == Some(other) && s.text.contains("Other")));
    let control = &source.page().controls[actual];
    assert_eq!(source.page().check(&control.binding(source.page())).unwrap().node, 100 + link);
}
