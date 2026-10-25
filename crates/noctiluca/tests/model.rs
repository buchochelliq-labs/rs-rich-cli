//! The page model, from snapshots Chrome made of the fixture pages
//! (`tests/snapshots`, captured from `tests/fixtures` by a live run): no
//! browser needed.

use noctiluca::engine::PageSnapshot;
use noctiluca::model::{build, BlockKind, Check, Page, Revisions, Role};

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
    assert!(links[0]
        .href
        .as_deref()
        .unwrap()
        .ends_with("/fixtures/second.html"));
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
