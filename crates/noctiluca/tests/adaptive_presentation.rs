//! Source semantics and terminal-safe presentation regression tests.
use noctiluca::adapt::Adapt;
use noctiluca::engine::Rect;
use noctiluca::layout::{Marks, Piece, project, reader};
use noctiluca::model::{Page, Revisions, build};
use noctiluca::testing::PageBuilder;

fn heading_page() -> Page {
    build(&heading_snapshot(), &mut Revisions::new())
}
fn heading_snapshot() -> noctiluca::engine::PageSnapshot {
    let mut p = PageBuilder::new("https://fixture.example/headings", "Headings", 1280., 900.);
    for (i, name) in ["H1", "H2", "H3"].iter().enumerate() {
        let r = Rect::new(0., i as f64 * 54., 320., 36.);
        let h = p.element(
            p.body(),
            name,
            &[],
            &[("color", "rgb(15, 17, 17)")],
            Some(r),
        );
        p.text(h, ["Title", "Brand", "Price"][i], r);
    }
    p.snapshot()
}
#[test]
fn neutral_headings_inherit_surface_foreground_in_both_views() {
    let p = heading_page();
    for width in [40, 80, 120, 160] {
        let m = Marks::default();
        let mut lines = project(&p, width, (8., 18.), &m).lines;
        for piece in reader(&p, width, &m) {
            if let Piece::Text(rows) = piece {
                lines.extend(rows);
            }
        }
        for name in ["Title", "Brand", "Price"] {
            let styles: Vec<_> = lines
                .iter()
                .flat_map(|l| &l.spans)
                .filter(|s| s.text.contains(name))
                .collect();
            assert_eq!(styles.len(), 2);
            for s in styles {
                let style = rich::style::Style::parse(&s.style).unwrap();
                assert!(style.color().is_none(), "{name}: {}", s.style);
                assert_eq!(style.bold(), Some(true));
            }
        }
    }
}
fn figure(alt: &str, caption: &str) -> Page {
    let mut p = PageBuilder::new("https://fixture.example/figure", "Figure", 640., 600.);
    let f = p.element(
        p.body(),
        "FIGURE",
        &[],
        &[],
        Some(Rect::new(0., 0., 400., 250.)),
    );
    p.image(f, "photo.png", alt, Rect::new(0., 0., 400., 180.));
    let c = p.element(
        f,
        "FIGCAPTION",
        &[],
        &[],
        Some(Rect::new(0., 180., 400., 54.)),
    );
    p.text(c, caption, Rect::new(0., 180., 400., 54.));
    build(&p.snapshot(), &mut Revisions::new())
}
#[test]
fn figure_caption_must_retain_distinct_alt_meaning() {
    for (alt, caption, equivalent) in [
        ("Blue boat side view", "Blue boat", false),
        ("海の光 blue boat", "海の光 blue boat", true),
        ("Blue  boat", "Blue boat", true),
        ("Boat", "Boat at night", false),
    ] {
        let p = figure(alt, caption);
        assert_eq!(p.media[0].captioned, equivalent, "{alt} / {caption}");
        let lines = project(&p, 80, (8., 18.), &Marks::default()).lines;
        let text = lines
            .iter()
            .map(|l| l.plain())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text.contains("Image:"), !equivalent, "{text}");
        assert!(!p.media[0].alt.is_empty());
        let off = Marks {
            adapt: Adapt::none(),
            ..Marks::default()
        };
        assert!(!project(&p, 80, (8., 18.), &off).lines.is_empty());
    }
}

fn session_snapshot(related: bool) -> noctiluca::engine::PageSnapshot {
    session_options(related, false, true)
}
fn session_options(
    related: bool,
    third: bool,
    named_group: bool,
) -> noctiluca::engine::PageSnapshot {
    let mut p = PageBuilder::new(
        "https://fixture.example/session.html",
        "Session",
        1280.,
        900.,
    );
    let attrs = if related && named_group {
        vec![("role", "group"), ("aria-label", "Live session")]
    } else {
        vec![]
    };
    let group = p.element(
        p.body(),
        "DIV",
        &attrs,
        &[("display", "flex")],
        Some(Rect::new(0., 0., 1280., 180.)),
    );
    let stage = p.element(
        group,
        "SECTION",
        &if related {
            vec![("aria-label", "Stage")]
        } else {
            vec![]
        },
        &[],
        Some(Rect::new(0., 0., 640., 180.)),
    );
    p.element(
        stage,
        "VIDEO",
        &[("aria-label", "Live video"), ("controls", "")],
        &[],
        Some(Rect::new(0., 0., 640., 144.)),
    );
    let chat = p.element(
        group,
        "SECTION",
        &if related {
            vec![("aria-label", "Chat")]
        } else {
            vec![]
        },
        &[],
        Some(Rect::new(648., 0., 632., 180.)),
    );
    let log = p.element(
        chat,
        "DIV",
        &[("role", "log"), ("aria-label", "Messages")],
        &[],
        Some(Rect::new(648., 0., 632., 108.)),
    );
    for i in 0..6 {
        let r = Rect::new(648., i as f64 * 18., 632., 18.);
        let e = p.element(log, "DIV", &[], &[], Some(r));
        p.text(e, &format!("message {i}"), r);
    }
    p.element(
        chat,
        "INPUT",
        &[("aria-label", "Draft"), ("value", "")],
        &[],
        Some(Rect::new(648., 126., 300., 18.)),
    );
    let send = p.element(
        chat,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(960., 126., 120., 18.)),
    );
    p.text(send, "Send", Rect::new(960., 126., 120., 18.));
    if third {
        let r = Rect::new(1000., 0., 100., 18.);
        let x = p.element(
            group,
            "SECTION",
            &[("aria-label", "Unrelated")],
            &[],
            Some(r),
        );
        p.text(x, "Other", r);
    }
    let r = Rect::new(0., 216., 1280., 18.);
    let outside = p.element(p.body(), "P", &[], &[], Some(r));
    p.text(outside, "Unrelated outside region", r);
    p.snapshot()
}
#[test]
fn explicit_session_group_stacks_without_grouping_unrelated_neighbors() {
    use noctiluca::adapt::Rule;
    for related in [false, true] {
        let p = build(&session_snapshot(related), &mut Revisions::new());
        assert_eq!(p.grids.len(), usize::from(related));
        for w in [40, 80, 120, 160] {
            let projection = project(&p, w, (8., 18.), &Marks::default());
            assert_eq!(projection.logs.len(), 1);
            let video = &projection.regions[0];
            let log = &projection.logs[0];
            if related && w == 40 {
                assert!(log.1 >= video.row + video.rows, "{projection:#?}");
            }
            let outside = projection
                .lines
                .iter()
                .position(|l| l.plain().contains("Unrelated outside"))
                .unwrap();
            assert!(outside >= log.1 + log.2);
        }
        if related {
            let m = Marks {
                adapt: Adapt::default().with(Rule::SnapGrids, false),
                ..Marks::default()
            };
            let raw = project(&p, 40, (8., 18.), &m);
            let mut without = p.clone();
            without.grids.clear();
            assert_eq!(raw, project(&without, 40, (8., 18.), &m));
        }
    }
}

#[test]
fn source_named_sections_qualify_without_named_parent_but_third_child_does_not() {
    for (named, third, expected) in [(false, false, 1), (true, false, 1), (false, true, 0)] {
        let p = build(&session_options(true, third, named), &mut Revisions::new());
        assert_eq!(p.grids.len(), expected);
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/stream-with-log.json");
    let p = build(
        &noctiluca::testing::snapshot_file(&root).unwrap(),
        &mut Revisions::new(),
    );
    assert!(
        p.grids.iter().any(|g| g.items.len() == 2),
        "original unnamed .page must qualify"
    );
}

fn wait(d: &mut rich_intuituive::Driver, ok: impl Fn(&rich_intuituive::Driver) -> bool) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        d.settle();
        if ok(d) {
            return;
        }
        assert!(
            std::time::Instant::now() < until,
            "{}",
            d.screen().plain().join("\n")
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[test]
fn heading_cells_keep_theme_foreground_and_reader_original_alt_route() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        testing::FakeEngine,
    };
    use rich_intuituive::Theme;
    for (theme, fg) in [(Theme::light(), "black"), (Theme::dark(), "white")] {
        for transparent in [false, true] {
            for view in [View::Reader, View::Projection] {
                let engine = FakeEngine::from_dir(
                    &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
                )
                .unwrap();
                engine
                    .state
                    .lock()
                    .unwrap()
                    .pages
                    .insert("headings".into(), heading_snapshot());
                let config = Config {
                    transparent,
                    ..Config::default()
                };
                let mut d = app(
                    Box::new(engine),
                    Options {
                        config,
                        bookmarks: Bookmarks::default(),
                        url: Some("headings.html".into()),
                        view: Some(view),
                        images: Some(Images::None),
                        tunnel: None,
                        control: None,
                    },
                )
                .theme(theme.clone())
                .driver(80, 24);
                wait(&mut d, |d| d.find_text("Price").is_some());
                for name in ["Title", "Brand", "Price"] {
                    let (x, y) = d.find_text(name).unwrap();
                    let cell = d.screen().cell(x, y);
                    let style = d.screen().style(cell.style).unwrap();
                    assert_eq!(style.bold(), Some(true));
                    // Native default fg is deliberately unspecified. An owned
                    // surface theme may supply a color; it must not force bright white.
                    assert!(
                        style.color().is_none_or(|c| c.name != "bright_white"),
                        "{fg} {view:?} {name}: {style:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn equivalent_caption_keeps_reader_alt_with_loaded_and_missing_pixels() {
    use noctiluca::layout::Piece;
    let alt = "海の光 blue boat";
    let original = figure(alt, alt);
    for loaded in [false, true] {
        let mut p = original.clone();
        if loaded {
            p.media[0].picture = Some(rich_intuituive::Picture::new(
                image::DynamicImage::new_rgba8(40, 20),
            ));
        }
        for width in [40, 80, 120, 160] {
            let lines = project(&p, width, (8., 18.), &Marks::default()).lines;
            assert!(!lines.iter().any(|l| l.plain().contains("Image:")));
            let text = reader(&p, width, &Marks::default())
                .into_iter()
                .filter_map(|p| match p {
                    Piece::Text(l) => Some(
                        l.into_iter()
                            .map(|l| l.plain())
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            assert!(text.contains(&format!("image: {alt}")), "{text}");
            assert_eq!(p.media[0].alt, alt);
        }
    }
}

#[test]
fn keyed_log_scrollback_survives_reflow_append_edit_and_reorder() {
    use noctiluca::{
        layout::{LogScroll, log_lines, scroll_log},
        model::{Entry, Run},
    };
    let mut p = build(&session_snapshot(true), &mut Revisions::new());
    p.logs[0].entries = (1..=30)
        .map(|key| Entry {
            key,
            runs: vec![Run::plain(format!("message {key}"))],
        })
        .collect();
    let held = scroll_log(&p.logs[0], LogScroll::Follow, -8);
    let mut marks = Marks::default();
    marks.logs.insert(p.logs[0].node, held);
    for width in [80, 120, 160, 40, 80] {
        let projection = project(&p, width, (8., 18.), &marks);
        assert_eq!(marks.logs[&p.logs[0].node], held);
        assert_eq!(projection.logs[0].0, p.logs[0].node);
        let lines = log_lines(&p.logs[0], usize::from(width), 4, &p, &marks);
        assert!(lines.last().unwrap().plain().contains("22"));
        p.logs[0].entries.push(Entry {
            key: 100 + u64::from(width),
            runs: vec![Run::plain("appended")],
        });
        p.logs[0].entries[0].runs = vec![Run::plain("edited earlier")];
        p.logs[0].entries.swap(0, 1);
    }
    let resume = scroll_log(&p.logs[0], held, 1000);
    assert_eq!(resume, LogScroll::Follow);
    marks.logs.insert(p.logs[0].node, resume);
    assert!(
        log_lines(&p.logs[0], 40, 4, &p, &marks)
            .last()
            .unwrap()
            .plain()
            .contains("appended")
    );
}

#[test]
fn product_grid_keeps_images_controls_and_keys_together_at_all_widths() {
    use noctiluca::layout::control_at;
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/product-grid.json");
    let p = build(
        &noctiluca::testing::snapshot_file(&root).unwrap(),
        &mut Revisions::new(),
    );
    let mut surviving = std::collections::HashSet::new();
    for width in [40, 80, 120, 160, 40] {
        let projection = project(&p, width, (8., 18.), &Marks::default());
        assert!(!projection.regions.is_empty());
        for region in &projection.regions {
            assert!(region.column + region.cols <= usize::from(width));
            assert!(region.row + region.rows <= projection.lines.len());
        }
        for (i, c) in p
            .controls
            .iter()
            .enumerate()
            .filter(|(_, c)| c.role == noctiluca::model::Role::Link)
        {
            let hits: Vec<_> = projection
                .lines
                .iter()
                .enumerate()
                .flat_map(|(y, l)| {
                    (0..l.width())
                        .filter_map(move |x| (control_at(l, x) == Some(i)).then_some((x, y)))
                })
                .collect();
            if hits.is_empty() {
                continue;
            }
            assert!(hits.iter().all(|(x, _)| *x < usize::from(width)));
            assert_eq!(p.check(&c.binding(&p)).unwrap().node, c.node);
            for (_, y) in hits {
                assert_eq!(projection.rows.get(&projection.keys[y]), Some(&y));
                surviving.insert(projection.keys[y]);
            }
        }
    }
    assert!(!surviving.is_empty());
}

#[test]
fn hidden_caption_does_not_replace_a_visible_alt_caption() {
    let mut p = PageBuilder::new("https://fixture.example/hidden", "Hidden", 640., 400.);
    let f = p.element(
        p.body(),
        "FIGURE",
        &[],
        &[],
        Some(Rect::new(0., 0., 400., 250.)),
    );
    p.image(
        f,
        "photo.png",
        "Original alt",
        Rect::new(0., 0., 400., 180.),
    );
    let c = p.element(
        f,
        "FIGCAPTION",
        &[("hidden", "")],
        &[("display", "none")],
        None,
    );
    p.text(c, "Original alt", Rect::default());
    let page = build(&p.snapshot(), &mut Revisions::new());
    assert!(!page.media[0].captioned);
}

#[test]
fn caption_equivalence_uses_visible_rendered_descendant_text() {
    for case in [
        "hidden",
        "transparent",
        "clipped",
        "invisible",
        "missing text",
        "rendered text",
        "visible",
    ] {
        let mut p = PageBuilder::new("https://fixture.example/caption", "Caption", 640., 400.);
        let f = p.element(
            p.body(),
            "FIGURE",
            &[],
            &[],
            Some(Rect::new(0., 0., 400., 250.)),
        );
        p.image(f, "photo.png", "Blue boat", Rect::new(0., 0., 400., 180.));
        let c = p.element(
            f,
            "FIGCAPTION",
            &[],
            &[],
            Some(Rect::new(0., 180., 400., 54.)),
        );
        p.text(c, "Blue ", Rect::new(0., 180., 40., 18.));
        let styles: &[(&str, &str)] = match case {
            "hidden" => &[("display", "none")],
            "transparent" => &[("opacity", "0")],
            "clipped" => &[
                ("position", "absolute"),
                ("clip", "rect(0px, 0px, 0px, 0px)"),
            ],
            "invisible" => &[("visibility", "hidden")],
            _ => &[],
        };
        let attrs: &[(&str, &str)] = if case == "hidden" {
            &[("hidden", "")]
        } else {
            &[]
        };
        let span = p.element(
            c,
            "SPAN",
            attrs,
            styles,
            (case != "hidden").then_some(Rect::new(40., 180., 40., 18.)),
        );
        let text_node = p.text(span, "boat", Rect::new(40., 180., 40., 18.));
        let mut snapshot = p.snapshot();
        if matches!(case, "missing text" | "rendered text") {
            let layout = &snapshot.dom["documents"][0]["layout"];
            let at = layout["nodeIndex"]
                .as_array()
                .unwrap()
                .iter()
                .position(|n| n.as_u64() == Some(text_node as u64))
                .unwrap();
            let text = if case == "missing text" {
                -1
            } else {
                let strings = snapshot.dom["strings"].as_array_mut().unwrap();
                strings.push(serde_json::json!("ship"));
                (strings.len() - 1) as i64
            };
            snapshot.dom["documents"][0]["layout"]["text"][at] = serde_json::json!(text);
        }
        let original = build(&snapshot, &mut Revisions::new());
        let equivalent = case == "visible";
        assert_eq!(original.media[0].captioned, equivalent, "{case}");
        for loaded in [false, true] {
            let mut page = original.clone();
            if loaded {
                page.media[0].picture = Some(rich_intuituive::Picture::new(
                    image::DynamicImage::new_rgba8(40, 20),
                ));
            }
            let projection = project(&page, 80, (8., 18.), &Marks::default());
            let text = projection
                .lines
                .iter()
                .map(|l| l.plain())
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(
                text.contains("Image: Blue boat"),
                !equivalent,
                "{case}, loaded={loaded}: {text}"
            );
            let reader_text = reader(&page, 80, &Marks::default())
                .into_iter()
                .filter_map(|piece| match piece {
                    Piece::Text(lines) => Some(
                        lines
                            .iter()
                            .map(|l| l.plain())
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                reader_text.contains("image: Blue boat"),
                "{case}, loaded={loaded}: {reader_text}"
            );
            assert_eq!(page.media[0].alt, "Blue boat");
        }
    }
}

#[test]
fn adapted_source_width_floor_tracks_views_and_adaptation_without_changing_terminal() {
    use noctiluca::{
        app::{Options, app_with_presentation},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        testing::FakeEngine,
    };
    for (adapt, adapted) in [
        (Adapt::none(), false),
        (
            Adapt::default().with(noctiluca::adapt::Rule::SnapGrids, false),
            false,
        ),
        (Adapt::default(), true),
    ] {
        let engine = FakeEngine::from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        )
        .unwrap();
        let state = engine.state.clone();
        let config = Config {
            adapt,
            ..Config::default()
        };
        let mut d = app_with_presentation(
            Box::new(engine),
            Options {
                config,
                bookmarks: Bookmarks::default(),
                url: Some("stream-with-log.html".into()),
                view: Some(View::Projection),
                images: Some(Images::Kitty),
                tunnel: None,
                control: None,
            },
            rich_intuituive::ImagePresentation::new(
                rich_intuituive::Pixels::Kitty,
                rich_intuituive::KittyFeatures {
                    unicode_placeholders: true,
                    native_animation: true,
                },
                rich_intuituive::PixelFormats::cells().with(rich_intuituive::Pixels::Kitty),
            ),
        )
        .driver(40, 50);
        wait(&mut d, |d| d.find_text("Lanternfish").is_some());
        let width = || state.lock().unwrap().viewport_width;
        wait(&mut d, |_| width() == if adapted { 640. } else { 320. });
        assert_eq!(d.screen().area().width, 40);
        if adapted {
            for (keys, expected) in [
                ("a", 320.),
                ("a", 640.),
                ("r", 320.),
                ("r", 640.),
                ("alt+x", 320.),
                ("alt+x", 640.),
            ] {
                d.press(keys);
                wait(&mut d, |_| width() == expected);
            }
        }
        for w in [80, 120, 160, 40] {
            d.resize(w, 50);
            wait(&mut d, |_| {
                width() == f64::from(if adapted { w.max(80) } else { w }) * 8.
            });
            assert_eq!(d.screen().area().width, w);
        }
    }
}

#[test]
fn captured_chrome_narrow_chat_has_readable_rows_and_reachable_send() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        testing::FakeEngine,
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let engine = FakeEngine::from_dir(&root).unwrap();
    let state = engine.state.clone();
    let snapshot =
        noctiluca::testing::snapshot_file(&root.join("stream-narrow-source.json")).unwrap();
    let page = build(&snapshot, &mut Revisions::new());
    let send = page.find("button", "Send")[0].node;
    engine
        .state
        .lock()
        .unwrap()
        .pages
        .insert("stream-with-log".into(), snapshot);
    let mut d = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("stream-with-log.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(40, 24);
    wait(&mut d, |d| d.find_text("Lanternfish").is_some());
    d.press("pagedown");
    d.settle();
    assert!(
        d.screen().plain().join("\n").contains("tide is coming"),
        "{}",
        d.screen().plain().join("\n")
    );
    d.press("G");
    d.settle();
    assert!(d.find_text("Send a message").is_some());
    assert!(d.find_text("[Send]").is_some());
    let (x, y) = d.find_text("[Send]").unwrap();
    d.click(x + 2, y);
    wait(&mut d, |_| {
        state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a,noctiluca::engine::PageAction::Click(n) if *n==send))
    });
}

#[test]
fn faithful_dark_source_page_keeps_neutral_text_readable_on_its_owned_backdrop() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        testing::FakeEngine,
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let snapshot =
        noctiluca::testing::snapshot_file(&root.join("stream-narrow-source.json")).unwrap();
    let page = build(&snapshot, &mut Revisions::new());
    let marks = Marks {
        adapt: Adapt::none(),
        ..Marks::default()
    };
    let projection = project(&page, 120, (8., 18.), &marks);
    assert!(
        projection.inline.is_empty(),
        "faithful disables inline-picture adaptation"
    );
    assert!(
        projection
            .lines
            .iter()
            .any(|l| l.plain().contains("moderator"))
    );
    for transparent in [false, true] {
        let engine = FakeEngine::from_dir(&root).unwrap();
        engine
            .state
            .lock()
            .unwrap()
            .pages
            .insert("stream-with-log".into(), snapshot.clone());
        let config = Config {
            adapt: Adapt::none(),
            transparent,
            ..Config::default()
        };
        let mut d = app(
            Box::new(engine),
            Options {
                config,
                bookmarks: Bookmarks::default(),
                url: Some("stream-with-log.html".into()),
                view: Some(View::Projection),
                images: Some(Images::None),
                tunnel: None,
                control: None,
            },
        )
        .driver(120, 40);
        wait(&mut d, |d| d.find_text("tide is coming").is_some());
        for text in ["Lanternfish", "tide is coming"] {
            let (x, y) = d.find_text(text).unwrap();
            let cell = d.screen().cell(x, y);
            let style = d.screen().style(cell.style).unwrap();
            let fg = style.color().unwrap().get_truecolor().unwrap();
            let bg = style.bgcolor().unwrap().get_truecolor().unwrap();
            let luma = |c: rich::color::ColorTriplet| {
                0.2126 * f64::from(c.red) + 0.7152 * f64::from(c.green) + 0.0722 * f64::from(c.blue)
            };
            assert!(
                (luma(fg) - luma(bg)).abs() > 100.,
                "{text} transparent={transparent}: {style:?}"
            );
        }
    }
}

#[test]
fn source_linked_image_cells_dispatch_the_retained_ancestor_action() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        testing::FakeEngine,
    };
    let mut p = PageBuilder::new(
        "https://fixture.example/image-link.html",
        "Image link",
        640.,
        400.,
    );
    let link = p.element(
        p.body(),
        "A",
        &[("href", "/original-story")],
        &[],
        Some(Rect::new(0., 0., 240., 36.)),
    );
    p.image(
        link,
        "photo.png",
        "Photograph",
        Rect::new(0., 0., 240., 36.),
    );
    let snapshot = p.snapshot();
    let model = build(&snapshot, &mut Revisions::new());
    let target = model
        .controls
        .iter()
        .find(|c| {
            c.href
                .as_deref()
                .is_some_and(|h| h.ends_with("/original-story"))
        })
        .unwrap()
        .node;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let engine = FakeEngine::from_dir(&root).unwrap();
    let state = engine.state.clone();
    state
        .lock()
        .unwrap()
        .pages
        .insert("image-link".into(), snapshot);
    let mut d = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("https://fixture.example/image-link.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(40, 16);
    wait(&mut d, |d| d.find_text("Photograph").is_some());
    let (x, y) = d.find_text("Photograph").unwrap();
    // The shaded placeholder cells, beyond the alt label, carry the same source link.
    d.click(x + 20, y);
    wait(&mut d, |_| {
        state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, noctiluca::engine::PageAction::Click(node) if *node == target))
    });
}
