//! The page model, from snapshots Chrome made of the fixture pages
//! (`tests/snapshots`, captured from `tests/fixtures` by a live run): no
//! browser needed.

use noctiluca::engine::PageSnapshot;
use noctiluca::model::{BlockKind, Check, Page, Revisions, Role, build};

fn page(name: &str) -> Page {
    let path = format!("{}/tests/snapshots/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let all: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let snapshot = PageSnapshot {
        dom: all["dom"].clone(),
        ax: all["ax"].clone(),
    };
    build(&snapshot, &mut Revisions::new())
}

fn kinds(page: &Page) -> Vec<(BlockKind, String)> {
    page.blocks
        .iter()
        .filter(|b| !matches!(b.kind, BlockKind::Table(_)))
        .map(|b| (b.kind.clone(), b.text()))
        .collect()
}

#[test]
fn a_page_reads_in_order_with_hidden_text_left_out() {
    let page = page("reader");
    assert_eq!(page.title, "Reader fixture");
    let item = |depth, marker: &str, text: &str| {
        (
            BlockKind::ListItem {
                depth,
                marker: marker.into(),
            },
            text.to_string(),
        )
    };
    let blocks = kinds(&page);
    assert_eq!(
        &blocks[..10],
        &[
            (BlockKind::Heading(1), "Plankton".to_string()),
            (
                BlockKind::Paragraph,
                "Tiny drifters of the open sea, with light of their own.".to_string()
            ),
            (BlockKind::Heading(2), "Kinds".to_string()),
            item(1, "•", "Dinoflagellates"),
            item(2, "◦", "Noctiluca"),
            item(2, "◦", "Pyrocystis"),
            item(1, "•", "Copepods"),
            item(1, "1.", "Bloom"),
            item(1, "2.", "Glow"),
            (
                BlockKind::Paragraph,
                "The sea was on fire with light.".to_string()
            ),
        ]
    );
    assert_eq!(page.blocks[9].quote, 1);
    assert!(blocks.contains(&(BlockKind::Code, "glow = touch * 0.1\nsleep()".to_string())));
    let all = page.text();
    assert!(!all.contains("Hidden by"), "{all}");
    // The table, with its header row.
    let table = page
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.caption, "Sizes");
    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|r| r.iter().map(|c| c.text()).collect())
        .collect();
    assert_eq!(
        rows,
        [["Name", "Size"], ["Noctiluca", "1 mm"], ["Copepod", "2 mm"]]
    );
    assert!(table.rows[0][0].header);
    // The image is a media region with its alt text and address.
    assert_eq!(page.media.len(), 1);
    assert_eq!(page.media[0].alt, "A glowing wave");
    assert!(page.media[0].src.ends_with("/fixtures/glow.png"));
    // Links are controls, with their resolved address, and their text is
    // marked as theirs.
    let links: Vec<_> = page
        .controls
        .iter()
        .filter(|c| c.role == Role::Link)
        .collect();
    assert_eq!(links.len(), 2);
    assert!(
        links[0]
            .href
            .as_deref()
            .unwrap()
            .ends_with("/fixtures/second.html")
    );
    let run = page.blocks[1]
        .runs
        .iter()
        .find(|r| r.text == "open sea")
        .unwrap();
    assert_eq!(run.control, Some(0));
}

#[test]
fn every_control_has_its_role_name_and_state() {
    let page = page("form");
    assert!(page.has_ax);
    let summary: Vec<_> = page
        .controls
        .iter()
        .map(|c| {
            (
                c.role.clone(),
                c.name.as_str(),
                c.value.as_deref(),
                c.checked,
                c.disabled,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (Role::TextBox, "Name", Some("Ada"), None, false),
            (Role::Password, "Secret", None, None, false),
            (Role::CheckBox, "Send news", None, Some(Check::Off), false),
            (Role::Radio, "Small", None, Some(Check::Off), false),
            (Role::Radio, "Large", None, Some(Check::On), false),
            (Role::ComboBox, "Mirror", Some("First mirror"), None, false),
            (Role::Slider, "Volume", Some("50"), None, false),
            (Role::Button, "Send", None, None, false),
            (Role::Button, "Unavailable", None, None, true),
            (Role::Button, "Schedule remote update", None, None, false),
        ]
    );
    // The AX tree is complete: every control was found in it.
    assert!(page.controls.iter().all(|c| c.from_ax));
    let mirror = &page.controls[5];
    let labels: Vec<_> = mirror
        .options
        .iter()
        .map(|o| (o.label.as_str(), o.disabled))
        .collect();
    assert_eq!(
        labels,
        [
            ("First mirror", false),
            ("Unavailable mirror", true),
            ("Second mirror", false)
        ]
    );
    let slider = &page.controls[6];
    assert_eq!(
        (slider.min, slider.max, slider.step),
        (Some(0.0), Some(100.0), Some(5.0))
    );
    assert_eq!(page.controls[3].group.as_deref(), Some("size"));
    // The password's value is in the snapshot Chrome made, and nowhere in
    // the model.
    assert!(!format!("{page:?}").contains("hunter2"));
    // Label text is the controls' names, not paragraphs of its own.
    assert!(!kinds(&page).iter().any(|(_, t)| t.contains("Send news")));
}

#[test]
fn no_control_character_reaches_the_model() {
    let page = page("inject");
    let all = format!("{page:?}");
    // Debug escapes them as \u{1b} and the like: none may be left.
    for bad in [
        "\\u{1b}",
        "\\u{9b}",
        "\\u{7f}",
        "\\u{202e}",
        "\\u{2066}",
        "\\u{7}",
    ] {
        assert!(!all.contains(bad), "{bad} in the model");
    }
    assert!(!page.title.chars().any(char::is_control));
    assert_eq!(page.title, "Inject ]0;owned title [2J");
    assert_eq!(page.media[0].alt, "alt[31m text");
    let pick = page
        .controls
        .iter()
        .find(|c| c.role == Role::ComboBox)
        .unwrap();
    assert_eq!(pick.name, "pick[1m");
    assert_eq!(pick.options[1].label, "two");
}

/// The page in `tests/fixtures/hidden-text.html` as Chrome lays it out
/// at 800 CSS pixels: four channel links in a sidebar, each with a hint
/// hidden a different way, and an icon link named by hidden text only.
fn hidden_text() -> Page {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let hint = "Use the Right Arrow Key to show more information.";
    let mut p = PageBuilder::new(
        "file:///noctiluca/fixtures/hidden-text.html",
        "Hidden text fixture",
        800.0,
        400.0,
    );
    let body = p.body();
    let nav = p.element(
        body,
        "NAV",
        &[("aria-label", "Followed channels")],
        &[("position", "absolute")],
        Some(Rect::new(0.0, 0.0, 240.0, 210.0)),
    );
    let names = ["Glowworm", "Tidepool", "Sea Sparkle", "Night Swim"];
    for (k, name) in names.iter().enumerate() {
        let y = 42.0 * k as f64;
        let href = format!("#{k}");
        let a = p.element(
            nav,
            "A",
            &[("href", href.as_str())],
            &[("position", "relative")],
            Some(Rect::new(0.0, y, 240.0, 42.0)),
        );
        let src = format!("file:///noctiluca/fixtures/media/avatar-{}.png", k + 1);
        p.image(a, &src, "", Rect::new(0.0, y + 6.0, 30.0, 30.0));
        p.text(
            a,
            &format!(" {name}"),
            Rect::new(30.0, y + 18.0, 100.0, 18.0),
        );
        match k {
            // A "visually hidden" class: a 1 x 1 box, overflow hidden,
            // clipped to nothing. Its text still has its own boxes, one
            // word a line in the box's one pixel.
            0 => {
                let sr = p.element(
                    a,
                    "SPAN",
                    &[("class", "sr-only")],
                    &[
                        ("display", "inline"),
                        ("position", "absolute"),
                        ("overflow-y", "hidden"),
                        ("clip", "rect(0px, 0px, 0px, 0px)"),
                        ("white-space", "nowrap"),
                    ],
                    Some(Rect::new(130.0, y + 18.0, 1.0, 1.0)),
                );
                p.text(sr, hint, Rect::new(130.0, y + 18.0, 380.0, 18.0));
            }
            // A transparent tooltip beside the sidebar.
            1 => {
                let faded = p.element(
                    a,
                    "DIV",
                    &[("class", "faded")],
                    &[("position", "absolute"), ("opacity", "0")],
                    Some(Rect::new(250.0, y, 390.0, 18.0)),
                );
                let para = p.element(faded, "P", &[], &[], Some(Rect::new(250.0, y, 390.0, 18.0)));
                p.text(para, hint, Rect::new(250.0, y, 380.0, 18.0));
            }
            // Clipped by an inset of half its size.
            2 => {
                let inset = p.element(
                    a,
                    "SPAN",
                    &[("class", "inset")],
                    &[
                        ("display", "inline"),
                        ("position", "absolute"),
                        ("clip-path", "inset(50%)"),
                    ],
                    Some(Rect::new(50.0, y + 20.0, 380.0, 18.0)),
                );
                p.text(inset, hint, Rect::new(50.0, y + 20.0, 380.0, 18.0));
            }
            // `visibility: hidden`, inherited by its text.
            _ => {
                let shut = p.element(
                    a,
                    "SPAN",
                    &[("class", "shut")],
                    &[("display", "inline"), ("visibility", "hidden")],
                    Some(Rect::new(130.0, y + 18.0, 380.0, 18.0)),
                );
                let inner = p.element(
                    shut,
                    "SPAN",
                    &[],
                    &[("display", "inline")],
                    Some(Rect::new(130.0, y + 18.0, 380.0, 18.0)),
                );
                p.text(inner, hint, Rect::new(130.0, y + 18.0, 380.0, 18.0));
            }
        }
    }
    let search = p.element(
        nav,
        "A",
        &[("href", "#search")],
        &[("position", "relative")],
        Some(Rect::new(0.0, 168.0, 240.0, 42.0)),
    );
    let sr = p.element(
        search,
        "SPAN",
        &[("class", "sr-only")],
        &[
            ("display", "inline"),
            ("position", "absolute"),
            ("overflow-y", "hidden"),
            ("clip", "rect(0px, 0px, 0px, 0px)"),
        ],
        Some(Rect::new(0.0, 168.0, 1.0, 1.0)),
    );
    p.text(sr, "Search channels", Rect::new(0.0, 168.0, 8.0, 36.0));
    let main = p.element(
        body,
        "MAIN",
        &[],
        &[],
        Some(Rect::new(260.0, 0.0, 540.0, 100.0)),
    );
    let h1 = p.element(
        main,
        "H1",
        &[],
        &[("font-weight", "700")],
        Some(Rect::new(260.0, 21.0, 540.0, 37.0)),
    );
    p.text(h1, "Live channels", Rect::new(260.0, 21.0, 200.0, 37.0));
    let para = p.element(
        main,
        "P",
        &[],
        &[],
        Some(Rect::new(260.0, 80.0, 540.0, 18.0)),
    );
    p.text(
        para,
        "Four channels are live.",
        Rect::new(260.0, 80.0, 180.0, 18.0),
    );
    build(&p.snapshot(), &mut Revisions::new())
}

#[test]
fn visually_hidden_text_is_left_out_unless_it_names_a_link() {
    use noctiluca::layout::{Line, Marks, Piece, projection, reader};
    let page = hidden_text();
    let all: String = page.blocks.iter().map(|b| b.text() + "\n").collect();
    assert!(!all.contains("Right Arrow"), "{all}");
    for name in [
        "Glowworm",
        "Tidepool",
        "Sea Sparkle",
        "Night Swim",
        "Live channels",
    ] {
        assert!(all.contains(name), "{name}: {all}");
    }
    // The icon link's only text is hidden: it is its name, and it reads.
    assert!(all.contains("Search channels"), "{all}");
    let lines: Vec<String> = projection(&page, 100, (8.0, 18.0), &Marks::default())
        .iter()
        .map(Line::plain)
        .collect();
    assert!(
        !lines.iter().any(|l| l.contains("Right Arrow")),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("Sea Sparkle")),
        "{lines:#?}"
    );
    let read: Vec<String> = reader(&page, 80, &Marks::default())
        .iter()
        .flat_map(|piece| match piece {
            Piece::Text(lines) => lines.iter().map(Line::plain).collect(),
            _ => Vec::new(),
        })
        .collect();
    assert!(!read.iter().any(|l| l.contains("Right Arrow")), "{read:#?}");
    assert!(
        read.iter().any(|l| l.contains("Search channels")),
        "{read:#?}"
    );
}

#[test]
fn a_root_that_hides_its_overflow_over_a_placed_body_still_shows_the_page() {
    // tests/fixtures/root-overflow.html, reduced from a video site's
    // embedded player page: `html { overflow: hidden }` and a body placed
    // absolutely, so the root's box is 0 pixels tall. The root's overflow
    // clips the viewport, not its box: the page shows, as in Chrome.
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new(
        "file:///noctiluca/fixtures/root-overflow.html",
        "Root overflow fixture",
        800.0,
        0.0,
    );
    for node in [1, p.body()] {
        p.set_style(node, "overflow-y", "hidden");
    }
    p.set_style(p.body(), "position", "absolute");
    let alert = p.element(
        p.body(),
        "DIV",
        &[("role", "alert")],
        &[],
        Some(Rect::new(0.0, 0.0, 800.0, 120.0)),
    );
    let h1 = p.element(
        alert,
        "H1",
        &[],
        &[],
        Some(Rect::new(0.0, 20.0, 800.0, 37.0)),
    );
    p.text(
        h1,
        "Video player configuration error",
        Rect::new(0.0, 20.0, 420.0, 37.0),
    );
    let para = p.element(
        alert,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 80.0, 800.0, 18.0)),
    );
    p.text(para, "Error 153", Rect::new(0.0, 80.0, 72.0, 18.0));
    let page = build(&p.snapshot(), &mut Revisions::new());
    assert_eq!(
        kinds(&page),
        vec![
            (
                BlockKind::Heading(1),
                "Video player configuration error".into()
            ),
            (BlockKind::Paragraph, "Error 153".into()),
        ]
    );
}

#[test]
fn a_script_value_for_alt_text_is_no_alt_text() {
    // A video site's search results bind a boolean to their thumbnails'
    // `alt`, so each says "true"; the captions would read "Image: true".
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new("file:///noctiluca/alt.html", "Alt", 800.0, 400.0);
    let body = p.body();
    for (k, alt) in ["true", "undefined", "A harbour at night"]
        .iter()
        .enumerate()
    {
        let y = 120.0 * k as f64;
        p.image(
            body,
            "media/thumb-1.jpg",
            alt,
            Rect::new(0.0, y, 160.0, 90.0),
        );
    }
    let page = build(&p.snapshot(), &mut Revisions::new());
    let alts: Vec<&str> = page.media.iter().map(|m| m.alt.as_str()).collect();
    assert_eq!(alts, ["", "", "A harbour at night"]);
}

/// A page 3000 pixels long read with Chrome scrolled 500 pixels down, its
/// viewport 648 tall: an intro, a sticky header stuck at the viewport's
/// top (its static place is 50), a long section, a cookie bar fixed to
/// the bottom, a bar fixed to the top, a menu fixed but slid off screen,
/// and a sidebar fixed down the whole viewport.
fn layered() -> Page {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new(
        "https://fixture.example/layers.html",
        "Layers",
        960.0,
        3090.0,
    );
    p.view(500.0, 648.0);
    let body = p.body();
    let boxed = |p: &mut PageBuilder, position: &str, rect: Rect, text: &str| {
        let at = p.element(body, "DIV", &[], &[("position", position)], Some(rect));
        p.text(at, text, Rect::new(rect.x, rect.y, 100.0, 18.0));
    };
    boxed(&mut p, "static", Rect::new(0.0, 16.0, 960.0, 18.0), "Intro");
    boxed(
        &mut p,
        "sticky",
        Rect::new(0.0, 500.0, 960.0, 40.0),
        "Sticky head",
    );
    boxed(
        &mut p,
        "static",
        Rect::new(0.0, 90.0, 960.0, 3000.0),
        "Long section",
    );
    boxed(
        &mut p,
        "fixed",
        Rect::new(0.0, 1130.0, 960.0, 18.0),
        "Cookie bar",
    );
    boxed(
        &mut p,
        "fixed",
        Rect::new(0.0, 500.0, 100.0, 18.0),
        "Fixed top",
    );
    boxed(
        &mut p,
        "fixed",
        Rect::new(-300.0, 2000.0, 300.0, 18.0),
        "Slid away",
    );
    boxed(
        &mut p,
        "fixed",
        Rect::new(860.0, 500.0, 100.0, 648.0),
        "Side rail",
    );
    build(&p.snapshot(), &mut Revisions::new())
}

#[test]
fn fixed_and_sticky_boxes_are_layers_placed_by_where_they_are_on_screen() {
    use noctiluca::model::Place;
    let page = layered();
    assert_eq!((page.scroll_y, page.view_height), (500.0, 648.0));
    let layer = |text: &str| {
        let block = page.blocks.iter().find(|b| b.text() == text).unwrap();
        page.layers[block.stack.layer.unwrap_or_else(|| panic!("{text}"))]
    };
    assert!(
        page.blocks
            .iter()
            .filter(|b| ["Intro", "Long section"].contains(&b.text().as_str()))
            .all(|b| b.stack.layer.is_none())
    );
    // Stuck at the top now, it sits at its static place, under the intro:
    // the top of the section after it less its height.
    let sticky = layer("Sticky head");
    assert_eq!((sticky.place, sticky.top), (Place::Flow, 50.0));
    assert!(!sticky.fixed);
    assert_eq!(layer("Fixed top").place, Place::Pinned);
    let cookies = layer("Cookie bar");
    assert_eq!((cookies.place, cookies.top), (Place::Docked, 630.0));
    assert_eq!(layer("Slid away").place, Place::Off);
    // Taller than a quarter of the screen: in the flow, as at the top.
    let rail = layer("Side rail");
    assert_eq!((rail.place, rail.top), (Place::Flow, 0.0));
}

#[test]
fn a_box_painted_above_covers_what_it_holds_and_a_badge_lies_over_its_picture() {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new("https://fixture.example/stack.html", "Stack", 800.0, 600.0);
    let body = p.body();
    p.paint(body, 1);
    // A thumbnail with a duration badge painted above it.
    let thumb = p.element(
        body,
        "DIV",
        &[],
        &[("position", "relative")],
        Some(Rect::new(0.0, 0.0, 320.0, 180.0)),
    );
    p.paint(thumb, 2);
    p.image(
        thumb,
        "thumb.png",
        "A thumbnail",
        Rect::new(0.0, 0.0, 320.0, 180.0),
    );
    let badge = p.element(
        thumb,
        "SPAN",
        &[],
        &[
            ("position", "absolute"),
            ("background-color", "rgb(0, 0, 0)"),
        ],
        Some(Rect::new(270.0, 158.0, 44.0, 18.0)),
    );
    p.paint(badge, 3);
    p.text(badge, "12:34", Rect::new(272.0, 158.0, 40.0, 17.0));
    // An error panel under an opaque interstitial painted above it.
    let panel = p.element(
        body,
        "DIV",
        &[],
        &[("position", "relative")],
        Some(Rect::new(0.0, 200.0, 800.0, 60.0)),
    );
    p.paint(panel, 4);
    let error = p.element(
        panel,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 200.0, 800.0, 18.0)),
    );
    p.text(
        error,
        "Hidden error panel",
        Rect::new(0.0, 200.0, 120.0, 17.0),
    );
    let cover = p.element(
        panel,
        "DIV",
        &[],
        &[
            ("position", "absolute"),
            ("background-color", "rgb(255, 255, 255)"),
        ],
        Some(Rect::new(0.0, 200.0, 800.0, 60.0)),
    );
    p.paint(cover, 5);
    let shown = p.element(
        cover,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 216.0, 800.0, 18.0)),
    );
    p.text(shown, "Interstitial", Rect::new(0.0, 216.0, 70.0, 17.0));
    // A translucent box painted above text covers nothing.
    let veil = p.element(
        body,
        "DIV",
        &[],
        &[
            ("position", "absolute"),
            ("background-color", "rgba(0, 0, 0, 0)"),
        ],
        Some(Rect::new(0.0, 280.0, 800.0, 40.0)),
    );
    p.paint(veil, 6);
    let under = p.element(
        body,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 290.0, 800.0, 18.0)),
    );
    p.set_style(under, "position", "static");
    p.paint(under, 1);
    p.text(under, "Under a veil", Rect::new(0.0, 290.0, 80.0, 17.0));
    let page = build(&p.snapshot(), &mut Revisions::new());
    let stack = |text: &str| {
        page.blocks
            .iter()
            .find(|b| b.text() == text)
            .unwrap_or_else(|| panic!("{text}: {:#?}", page.blocks))
            .stack
    };
    assert!(stack("Hidden error panel").covered);
    assert!(!stack("Interstitial").covered);
    assert!(!stack("Under a veil").covered);
    let over = stack("12:34");
    assert!(!over.covered);
    assert_eq!(over.over, Some(0));
    assert_eq!(stack("Interstitial").over, None);
}

#[test]
fn an_app_shells_bars_above_and_under_its_scrolling_main_box_are_layers() {
    // The page does not scroll; its main box does (a streaming site's
    // directory): the top bar above it is pinned, the sign-up bar under it
    // docked, the sidebar beside it stays in the flow.
    use noctiluca::engine::Rect;
    use noctiluca::model::Place;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new("https://fixture.example/shell.html", "Shell", 800.0, 648.0);
    p.view(0.0, 648.0);
    let body = p.body();
    let top = p.element(
        body,
        "NAV",
        &[],
        &[],
        Some(Rect::new(0.0, 0.0, 800.0, 50.0)),
    );
    p.text(top, "Browse", Rect::new(8.0, 16.0, 60.0, 18.0));
    let middle = p.element(
        body,
        "DIV",
        &[],
        &[],
        Some(Rect::new(0.0, 50.0, 800.0, 541.0)),
    );
    let side = p.element(
        middle,
        "DIV",
        &[],
        &[],
        Some(Rect::new(0.0, 50.0, 50.0, 541.0)),
    );
    p.text(side, "Side", Rect::new(4.0, 60.0, 40.0, 18.0));
    let main = p.element(
        middle,
        "MAIN",
        &[],
        &[("overflow-y", "auto")],
        Some(Rect::new(50.0, 50.0, 750.0, 541.0)),
    );
    for k in 0..20 {
        let y = 60.0 + 100.0 * f64::from(k);
        let para = p.element(main, "P", &[], &[], Some(Rect::new(60.0, y, 700.0, 18.0)));
        p.text(
            para,
            &format!("Channel {k}"),
            Rect::new(60.0, y, 100.0, 18.0),
        );
    }
    let foot = p.element(
        body,
        "FOOTER",
        &[],
        &[],
        Some(Rect::new(0.0, 591.0, 800.0, 57.0)),
    );
    p.text(
        foot,
        "Join the community!",
        Rect::new(16.0, 610.0, 150.0, 18.0),
    );
    let page = build(&p.snapshot(), &mut Revisions::new());
    let place = |text: &str| {
        let block = page.blocks.iter().find(|b| b.text() == text).unwrap();
        block.stack.layer.map(|l| page.layers[l].place)
    };
    assert_eq!(place("Browse"), Some(Place::Pinned));
    assert_eq!(place("Join the community!"), Some(Place::Docked));
    assert_eq!(place("Side"), None);
    assert_eq!(place("Channel 12"), None);
}

#[test]
fn source_dialog_semantics_are_preserved_without_inference_from_geometry() {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    for (role, modal, expected) in [
        ("dialog", false, true),
        ("dialog", true, true),
        ("", false, false),
    ] {
        let mut p = PageBuilder::new("https://example.test/", "Layers", 960.0, 720.0);
        p.view(0.0, 720.0);
        let panel = p.element(
            p.body(),
            "DIV",
            &[],
            &[("position", "fixed")],
            Some(Rect::new(196.0, 72.0, 567.0, 532.0)),
        );
        let attrs = if modal {
            vec![("role", role), ("aria-modal", "true")]
        } else {
            vec![("role", role)]
        };
        let dialog = p.element(
            panel,
            "DIV",
            &attrs,
            &[],
            Some(Rect::new(196.0, 72.0, 567.0, 532.0)),
        );
        p.text(dialog, "Panel", Rect::new(210.0, 200.0, 100.0, 18.0));
        let page = build(&p.snapshot(), &mut Revisions::new());
        assert_eq!(!page.overlays.is_empty(), expected, "role={role:?}");
        if expected {
            assert_eq!(page.overlays[0].modal, modal);
        }
    }
}

#[test]
fn visual_fixed_overlays_respect_opacity_paint_order_and_size_without_implying_dialog() {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    for (position, height, colour, opacity, order, expected) in [
        ("fixed", 532.0, "rgb(55, 57, 82)", "1", 100, true),
        ("fixed", 900.0, "rgb(55, 57, 82)", "1", 100, true),
        ("fixed", 80.0, "rgb(55, 57, 82)", "1", 100, false),
        ("sticky", 532.0, "rgb(55, 57, 82)", "1", 100, false),
        ("fixed", 532.0, "rgba(55, 57, 82, 0.5)", "1", 100, false),
        ("fixed", 532.0, "rgb(55, 57, 82)", "0.5", 100, false),
        ("fixed", 532.0, "rgb(55, 57, 82)", "1", 5, false),
    ] {
        let mut p = PageBuilder::new("https://example.test/", "Layers", 960.0, 720.0);
        p.view(0.0, 720.0);
        let under = p.element(
            p.body(),
            "P",
            &[],
            &[],
            Some(Rect::new(220.0, 100.0, 100.0, 18.0)),
        );
        p.paint(under, 10);
        p.text(under, "Underlay", Rect::new(220.0, 100.0, 100.0, 18.0));
        let panel = p.element(
            p.body(),
            "DIV",
            &[],
            &[
                ("position", position),
                ("background-color", colour),
                ("opacity", opacity),
            ],
            Some(Rect::new(196.0, 72.0, 567.0, height)),
        );
        p.paint(panel, order);
        p.text(panel, "Visual layer", Rect::new(210.0, 100.0, 120.0, 18.0));
        let page = build(&p.snapshot(), &mut Revisions::new());
        assert_eq!(
            !page.overlays.is_empty(),
            expected,
            "{position}, height={height}, colour={colour}, opacity={opacity}, order={order}"
        );
        assert!(
            page.dialogs().next().is_none(),
            "geometry inferred dialog semantics"
        );
    }
}

#[test]
fn semantic_dialogs_do_not_require_fixed_geometry() {
    use noctiluca::{
        engine::Rect,
        layout::{Marks, project, project_overlay},
        testing::PageBuilder,
    };
    for (tag, position, attrs, modal) in [
        (
            "DIV",
            "absolute",
            vec![("role", "dialog"), ("aria-modal", "true")],
            true,
        ),
        ("DIV", "static", vec![("role", "alertdialog")], false),
        ("DIALOG", "static", vec![("open", "")], false),
    ] {
        let mut p = PageBuilder::new("https://example.test/", "Dialogs", 800.0, 1200.0);
        p.view(160.0, 640.0);
        let under = p.element(
            p.body(),
            "P",
            &[],
            &[],
            Some(Rect::new(8.0, 180.0, 100.0, 20.0)),
        );
        p.text(under, "Underlay", Rect::new(8.0, 180.0, 100.0, 20.0));
        let dialog = p.element(
            p.body(),
            tag,
            &attrs,
            &[("position", position)],
            Some(Rect::new(80.0, 240.0, 400.0, 160.0)),
        );
        p.text(
            dialog,
            "Independent popup",
            Rect::new(88.0, 256.0, 180.0, 20.0),
        );
        let page = build(&p.snapshot(), &mut Revisions::new());
        let overlay = page.dialogs().next().expect(position);
        assert_eq!(overlay.modal, modal);
        assert!(!page.layers[overlay.layer].fixed);
        assert_eq!(page.layers[overlay.layer].top, 240.0);
        let main = project(&page, 80, (8.0, 16.0), &Marks::default());
        assert!(
            !main
                .lines
                .iter()
                .any(|l| l.plain().contains("Independent popup"))
        );
        let popup = project_overlay(&page, 0, 50, (8.0, 16.0), &Marks::default()).unwrap();
        assert!(
            popup
                .lines
                .iter()
                .any(|l| l.plain().contains("Independent popup"))
        );
        assert!(main.lines.iter().any(|l| l.plain().contains("Underlay")));
    }
}

#[test]
fn opacity_requires_supported_positive_evidence_and_never_removes_semantic_dialogs() {
    use noctiluca::{engine::Rect, testing::PageBuilder};
    for (colour, solid) in [
        ("rgba(10, 20, 30, 1)", true),
        ("rgba(10, 20, 30, 0.5)", false),
        ("rgb(10 20 30 / 1)", true),
        ("rgb(10 20 30 / 50%)", false),
        ("color(display-p3 1 0 0 / 0.5)", false),
        ("color(display-p3 1 0 0 / 0)", false),
        ("unrecognized", false),
    ] {
        for semantic in [false, true] {
            let mut p = PageBuilder::new("https://example.test/", "Opacity", 800.0, 640.0);
            p.view(0.0, 640.0);
            let under = p.element(
                p.body(),
                "P",
                &[],
                &[],
                Some(Rect::new(90.0, 100.0, 100.0, 20.0)),
            );
            p.text(under, "Underlay", Rect::new(90.0, 100.0, 100.0, 20.0));
            let attrs = if semantic {
                vec![("role", "dialog")]
            } else {
                vec![]
            };
            let dialog = p.element(
                p.body(),
                "DIV",
                &attrs,
                &[("position", "fixed"), ("background-color", colour)],
                Some(Rect::new(80.0, 80.0, 400.0, 400.0)),
            );
            p.paint(dialog, 20);
            p.text(dialog, "Popup", Rect::new(88.0, 96.0, 100.0, 20.0));
            let page = build(&p.snapshot(), &mut Revisions::new());
            assert_eq!(
                !page.overlays.is_empty(),
                semantic || solid,
                "{colour}, semantic={semantic}"
            );
            if semantic {
                assert_eq!(page.overlays[0].background.is_some(), solid, "{colour}");
            }
        }
    }
}
