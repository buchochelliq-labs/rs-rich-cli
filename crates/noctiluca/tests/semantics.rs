//! What a page states that Noctiluca draws natively: meters, progress
//! bars and ratings as indicators, screen-reader twins, the labels of
//! things that are not controls, icon buttons' faces, hover-only controls
//! and lazy thumbnails. On snapshots Chrome made of the fixture pages
//! (`meters.html`, `product-grid.html`) and on pages built by
//! [`PageBuilder`]: no browser needed, and nothing here names a site.

use noctiluca::adapt::{Adapt, Rule};
use noctiluca::engine::Rect;
use noctiluca::layout::{Line, Marks, face, project, reader};
use noctiluca::model::{BlockKind, IndicatorKind, Page, Revisions, Role, build, rating_of};
use noctiluca::testing::{PageBuilder, snapshot_file};

fn page(name: &str) -> Page {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/snapshots/{name}.json"));
    build(&snapshot_file(&path).unwrap(), &mut Revisions::new())
}

fn lines(page: &Page, columns: u16, adapt: Adapt) -> Vec<String> {
    let marks = Marks {
        adapt,
        ..Marks::default()
    };
    project(page, columns, (8.0, 18.0), &marks)
        .lines
        .iter()
        .map(Line::plain)
        .collect()
}

fn line_with<'a>(lines: &'a [String], text: &str) -> &'a str {
    lines
        .iter()
        .find(|l| l.contains(text))
        .unwrap_or_else(|| panic!("{text:?} not in {lines:#?}"))
}

#[test]
fn meters_and_progress_bars_are_indicators_drawn_as_bars() {
    let page = page("meters");
    let found: Vec<(IndicatorKind, &str, Option<f64>, f64)> = page
        .indicators
        .iter()
        .map(|i| (i.kind, i.label.as_str(), i.value, i.max))
        .collect();
    assert_eq!(found.len(), 6, "{found:#?}");
    let disk = &page.indicators[0];
    assert_eq!(
        (disk.kind, disk.label.as_str(), disk.value, disk.max),
        (IndicatorKind::Meter, "Disk", Some(72.0), 100.0)
    );
    assert_eq!(disk.bands, Some((60.0, 85.0, 20.0)));
    let upload = &page.indicators[1];
    assert_eq!(
        (upload.kind, upload.label.as_str()),
        (IndicatorKind::Progress, "Upload")
    );
    assert!(
        upload.value.is_some_and(|v| (10.0..=100.0).contains(&v)),
        "{upload:?}"
    );
    let wait = &page.indicators[2];
    assert_eq!(
        (wait.kind, wait.label.as_str(), wait.value),
        (IndicatorKind::Busy, "Waiting", None)
    );
    let download = &page.indicators[3];
    assert_eq!(
        (download.kind, download.label.as_str()),
        (IndicatorKind::Progress, "Download")
    );
    assert!(download.label_in_text, "the word before it is its label");
    let signal = &page.indicators[4];
    assert_eq!(
        (
            signal.kind,
            signal.value,
            signal.max,
            signal.text.as_deref()
        ),
        (IndicatorKind::Meter, Some(3.0), 5.0, Some("3 of 5 bars"))
    );
    let rating = &page.indicators[5];
    assert_eq!(
        (rating.kind, rating.value, rating.max),
        (IndicatorKind::Rating, Some(4.5), 5.0)
    );
    // The slider is still a control, not an indicator.
    assert!(page.controls.iter().any(|c| c.role == Role::Slider));
    // A label of a meter names it: not text of its own as well.
    let text = page.text();
    assert_eq!(text.matches("Disk").count(), 1, "{text}");
    assert!(
        text.contains("Disk 72%") && text.contains("Waiting busy"),
        "{text}"
    );
    assert!(text.contains("4.5 out of 5 stars"), "{text}");

    let shown = lines(&page, 100, Adapt::default());
    let disk = line_with(&shown, "Disk");
    assert!(disk.contains("Disk ━") && disk.ends_with("72%"), "{disk:?}");
    assert!(line_with(&shown, "Upload").contains("━"));
    assert!(line_with(&shown, "Waiting").contains("━ busy"));
    // "Download" once: the text before the bar is its label.
    let download = line_with(&shown, "Download");
    assert_eq!(download.matches("Download").count(), 1, "{download:?}");
    assert!(
        download.contains("━━") && download.contains("%"),
        "{download:?}"
    );
    assert!(line_with(&shown, "Signal").contains("3 of 5 bars"));
    assert!(
        line_with(&shown, "Rating").contains("★★★★½ 4.5"),
        "{shown:#?}"
    );
    // Reader mode draws them the same way.
    let read: Vec<String> = reader(&page, 80, &Marks::default())
        .iter()
        .flat_map(|p| match p {
            noctiluca::layout::Piece::Text(lines) => lines.iter().map(Line::plain).collect(),
            _ => Vec::new(),
        })
        .collect();
    assert!(line_with(&read, "Disk").contains("━"), "{read:#?}");
    assert!(line_with(&read, "★").contains("4.5"));
}

#[test]
fn a_moving_progress_bar_is_read_again_at_its_new_value() {
    let build_at = |value: &str| {
        let mut p = PageBuilder::new("file:///noctiluca/up.html", "Up", 800.0, 200.0);
        let body = p.body();
        let label = p.element(
            body,
            "LABEL",
            &[("for", "up")],
            &[("display", "inline")],
            Some(Rect::new(8.0, 8.0, 50.0, 18.0)),
        );
        p.text(label, "Upload", Rect::new(8.0, 8.0, 50.0, 18.0));
        p.element(
            body,
            "PROGRESS",
            &[("id", "up"), ("max", "100"), ("value", value)],
            &[("display", "inline-block")],
            Some(Rect::new(64.0, 8.0, 160.0, 16.0)),
        );
        build(&p.snapshot(), &mut Revisions::new())
    };
    let early = build_at("10");
    let late = build_at("90");
    assert_eq!(early.indicators[0].label, "Upload");
    assert_eq!(early.indicators[0].share(), Some(0.1));
    assert_eq!(late.indicators[0].share(), Some(0.9));
    let (a, b) = (
        lines(&early, 80, Adapt::default()),
        lines(&late, 80, Adapt::default()),
    );
    assert!(line_with(&a, "Upload").ends_with(" 10%"));
    assert!(line_with(&b, "Upload").ends_with(" 90%"));
}

#[test]
fn a_canvas_gauge_with_a_meter_role_is_a_dial_in_its_place() {
    let mut p = PageBuilder::new("file:///noctiluca/gauge.html", "Gauge", 800.0, 400.0);
    let body = p.body();
    p.element(
        body,
        "CANVAS",
        &[
            ("role", "meter"),
            ("aria-label", "Upload"),
            ("aria-valuemin", "0"),
            ("aria-valuemax", "500"),
            ("aria-valuenow", "287.4"),
            ("aria-valuetext", "287.4 Mbps"),
        ],
        &[("display", "inline")],
        Some(Rect::new(8.0, 8.0, 240.0, 200.0)),
    );
    let page = build(&p.snapshot(), &mut Revisions::new());
    assert_eq!(page.media.len(), 1, "the canvas is still a region");
    assert_eq!(page.indicators[0].media, Some(0));
    let dial = lines(&page, 80, Adapt::default());
    assert!(dial.iter().any(|l| l.contains("287.4 Mbps")), "{dial:#?}");
    assert!(dial.iter().any(|l| l.contains('┏')), "{dial:#?}");
    assert!(
        !dial.iter().any(|l| l.contains('░')),
        "no grey box: {dial:#?}"
    );
    // With the rule off, the region, as Chrome draws it.
    let off = lines(&page, 80, Adapt::default().with(Rule::Indicators, false));
    assert!(off.iter().any(|l| l.contains('░')), "{off:#?}");
    assert!(!off.iter().any(|l| l.contains("287.4 Mbps")));
}

#[test]
fn ratings_parse_from_their_names_or_not_at_all() {
    assert_eq!(rating_of("4.5 out of 5 stars"), Some((4.5, 5.0)));
    assert_eq!(rating_of("Rated 4 out of 5"), Some((4.0, 5.0)));
    assert_eq!(rating_of("4.5 / 5"), Some((4.5, 5.0)));
    assert_eq!(rating_of("4.5/5 stars"), Some((4.5, 5.0)));
    assert_eq!(rating_of("Kettle"), None);
    assert_eq!(rating_of("12 out of 400 left"), None, "a count, not stars");
    assert_eq!(rating_of("6 out of 5"), None);
    // An image that is not a rating draws nothing new.
    let mut p = PageBuilder::new("file:///noctiluca/img.html", "Img", 800.0, 200.0);
    let body = p.body();
    p.element(
        body,
        "I",
        &[("role", "img"), ("aria-label", "A wave")],
        &[("display", "inline-block")],
        Some(Rect::new(8.0, 8.0, 80.0, 16.0)),
    );
    let page = build(&p.snapshot(), &mut Revisions::new());
    assert!(page.indicators.is_empty());
}

#[test]
fn a_price_split_for_show_reads_as_its_screen_reader_text() {
    let page = page("product-grid");
    let text = page.text();
    for price in ["$29.99", "$24.49", "$49.00"] {
        assert!(text.contains(price), "{price}: {text}");
    }
    assert!(!text.contains("$2999"), "{text}");
    let adapted = lines(&page, 120, Adapt::default());
    let all = adapted.join("\n");
    assert!(all.contains("$29.99") && all.contains("$24.49"), "{all}");
    assert!(!all.contains("$2999"), "{all}");
    // The faithful projection shows the parts, as the page does.
    let faithful = lines(
        &page,
        120,
        Adapt::default().with(Rule::ScreenReaderTwins, false),
    );
    let all = faithful.join("\n");
    assert!(all.contains("$2999") && !all.contains("$29.99"), "{all}");
    // The ratings are stars with their number, not an orange bar.
    let ratings: Vec<Option<f64>> = page
        .indicators
        .iter()
        .filter(|i| i.kind == IndicatorKind::Rating)
        .map(|i| i.value)
        .collect();
    assert_eq!(ratings, [Some(4.5), Some(4.1), Some(4.7)]);
    assert!(
        adapted.iter().any(|l| l.contains("★★★★½ 4.5")),
        "{adapted:#?}"
    );
}

#[test]
fn a_hidden_hint_beside_hidden_parts_stays_hidden() {
    // Twins need the same digits: a hint ("Use the Right Arrow Key…")
    // beside an `aria-hidden` icon is not one.
    let mut p = PageBuilder::new("file:///noctiluca/hint.html", "Hint", 800.0, 200.0);
    let body = p.body();
    let para = p.element(body, "P", &[], &[], Some(Rect::new(0.0, 0.0, 800.0, 18.0)));
    p.text(para, "Channels ", Rect::new(0.0, 0.0, 72.0, 18.0));
    let sr = p.element(
        para,
        "SPAN",
        &[],
        &[
            ("display", "inline"),
            ("position", "absolute"),
            ("overflow-y", "hidden"),
            ("clip", "rect(0px, 0px, 0px, 0px)"),
        ],
        Some(Rect::new(72.0, 0.0, 1.0, 1.0)),
    );
    p.text(
        sr,
        "Use the Right Arrow Key to show more.",
        Rect::new(72.0, 0.0, 300.0, 18.0),
    );
    let icon = p.element(
        para,
        "SPAN",
        &[("aria-hidden", "true")],
        &[("display", "inline")],
        Some(Rect::new(80.0, 0.0, 8.0, 18.0)),
    );
    p.text(icon, "›", Rect::new(80.0, 0.0, 8.0, 18.0));
    let page = build(&p.snapshot(), &mut Revisions::new());
    assert_eq!(page.blocks[0].text(), "Channels ›");
}

#[test]
fn a_label_for_something_that_is_not_a_control_stays_text() {
    let mut p = PageBuilder::new("file:///noctiluca/labels.html", "Labels", 800.0, 300.0);
    let body = p.body();
    let row = |p: &mut PageBuilder, y: f64, for_id: &str, text: &str| {
        let para = p.element(body, "P", &[], &[], Some(Rect::new(0.0, y, 800.0, 18.0)));
        let label = p.element(
            para,
            "LABEL",
            &[("for", for_id)],
            &[("display", "inline")],
            Some(Rect::new(0.0, y, 80.0, 18.0)),
        );
        p.text(label, text, Rect::new(0.0, y, 80.0, 18.0));
        para
    };
    // A label for an output (not a control): its text stays.
    let total = row(&mut p, 0.0, "total", "Total");
    p.element(
        total,
        "OUTPUT",
        &[("id", "total")],
        &[("display", "inline")],
        Some(Rect::new(90.0, 0.0, 40.0, 18.0)),
    );
    // A label for nothing on the page.
    row(&mut p, 30.0, "gone", "Orphan");
    // A label for a text box: the box's name, not text.
    let name = row(&mut p, 60.0, "name", "Your name");
    p.element(
        name,
        "INPUT",
        &[("id", "name"), ("aria-label", "Your name")],
        &[("display", "inline-block")],
        Some(Rect::new(90.0, 60.0, 160.0, 18.0)),
    );
    let page = build(&p.snapshot(), &mut Revisions::new());
    let texts: Vec<String> = page
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Paragraph)
        .map(|b| b.text())
        .collect();
    assert_eq!(texts, ["Total", "Orphan"]);
    assert_eq!(page.controls[0].name, "Your name");
}

#[test]
fn icon_buttons_take_a_name_and_a_glyph_from_their_icon() {
    let mut p = PageBuilder::new("file:///noctiluca/icons.html", "Icons", 800.0, 300.0);
    let body = p.body();
    let mut y = 0.0;
    let mut button = |p: &mut PageBuilder, attrs: &[(&str, &str)]| {
        y += 30.0;
        p.element(
            body,
            "BUTTON",
            attrs,
            &[("display", "inline-block")],
            Some(Rect::new(0.0, y, 32.0, 24.0)),
        )
    };
    // An icon by its symbol's name, nothing else.
    let close = button(&mut p, &[]);
    let svg = p.element(
        close,
        "SVG",
        &[],
        &[("display", "inline")],
        Some(Rect::new(4.0, 34.0, 16.0, 16.0)),
    );
    p.element(
        svg,
        "USE",
        &[("href", "#icon-close")],
        &[],
        Some(Rect::new(4.0, 34.0, 16.0, 16.0)),
    );
    // A drawing's <title>, never laid out.
    let search = button(&mut p, &[]);
    let svg = p.element(
        search,
        "SVG",
        &[],
        &[("display", "inline")],
        Some(Rect::new(4.0, 64.0, 16.0, 16.0)),
    );
    let title = p.element(svg, "TITLE", &[], &[], None);
    p.text(title, "Search", Rect::default());
    // aria-labelledby, resolved in the page.
    button(&mut p, &[("aria-labelledby", "more-tip")]);
    let tip = p.element(
        body,
        "SPAN",
        &[("id", "more-tip")],
        &[("display", "none")],
        None,
    );
    p.text(tip, "More actions", Rect::default());
    // A title of no known icon: its name, in brackets.
    button(&mut p, &[("title", "Prime offers")]);
    let page = build(&p.snapshot(), &mut Revisions::new());
    let faces: Vec<(String, String, bool)> = page
        .controls
        .iter()
        .map(|c| (c.name.clone(), face(c), c.icon_only))
        .collect();
    assert_eq!(
        faces,
        [
            ("Close".to_string(), "[✕]".to_string(), true),
            ("Search".to_string(), "[🔍]".to_string(), true),
            ("More actions".to_string(), "[⋯]".to_string(), true),
            (
                "Prime offers".to_string(),
                "[ Prime offers ]".to_string(),
                true
            ),
        ]
    );
}

#[test]
fn a_control_shown_only_on_hover_is_listed_but_not_drawn() {
    let mut p = PageBuilder::new("file:///noctiluca/card.html", "Card", 800.0, 300.0);
    let body = p.body();
    let card = p.element(
        body,
        "DIV",
        &[],
        &[],
        Some(Rect::new(0.0, 0.0, 320.0, 240.0)),
    );
    let title = p.element(
        card,
        "A",
        &[("href", "#v")],
        &[("display", "inline")],
        Some(Rect::new(0.0, 200.0, 200.0, 18.0)),
    );
    p.text(title, "A video", Rect::new(0.0, 200.0, 200.0, 18.0));
    let menu = p.element(
        card,
        "DIV",
        &[],
        &[("opacity", "0")],
        Some(Rect::new(280.0, 200.0, 40.0, 40.0)),
    );
    p.element(
        menu,
        "BUTTON",
        &[("aria-label", "More actions")],
        &[("display", "inline-block")],
        Some(Rect::new(280.0, 200.0, 40.0, 40.0)),
    );
    let page = build(&p.snapshot(), &mut Revisions::new());
    let more = page.find("button", "More actions");
    assert_eq!(more.len(), 1, "on the page's lists: {:?}", page.controls);
    assert!(more[0].hover_only);
    assert!(
        !page
            .blocks
            .iter()
            .any(|b| matches!(b.kind, BlockKind::Control(c) if page.controls[c].hover_only)),
        "not drawn"
    );
    let shown = lines(&page, 80, Adapt::default()).join("\n");
    assert!(!shown.contains("More") && !shown.contains('⋯'), "{shown}");
}

#[test]
fn a_lazy_thumbnail_gets_a_placeholder_with_its_title() {
    let mut p = PageBuilder::new("file:///noctiluca/lazy.html", "Lazy", 800.0, 400.0);
    let body = p.body();
    let link = p.element(
        body,
        "A",
        &[("href", "#v"), ("title", "Making of the open movie")],
        &[("display", "block")],
        Some(Rect::new(0.0, 0.0, 320.0, 180.0)),
    );
    // No source yet, transparent until it loads, in its box.
    p.element(
        link,
        "IMG",
        &[("alt", "")],
        &[("display", "inline"), ("opacity", "0")],
        Some(Rect::new(0.0, 0.0, 320.0, 180.0)),
    );
    // No source and no size of its own yet: its parent's box.
    let frame = p.element(
        body,
        "DIV",
        &[],
        &[],
        Some(Rect::new(0.0, 200.0, 320.0, 180.0)),
    );
    p.element(
        frame,
        "IMG",
        &[("alt", "Second")],
        &[("display", "inline")],
        Some(Rect::new(0.0, 200.0, 0.0, 0.0)),
    );
    let page = build(&p.snapshot(), &mut Revisions::new());
    let found: Vec<(bool, &str, f64)> = page
        .media
        .iter()
        .map(|m| (m.pending, m.alt.as_str(), m.rect.height))
        .collect();
    assert_eq!(
        found,
        [
            (true, "Making of the open movie", 180.0),
            (true, "Second", 180.0)
        ]
    );
    let shown = lines(&page, 80, Adapt::default()).join("\n");
    assert!(
        shown.contains('░') && shown.contains("Making of"),
        "{shown}"
    );
}
