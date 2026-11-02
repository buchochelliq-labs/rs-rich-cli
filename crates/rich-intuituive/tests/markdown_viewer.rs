//! The Markdown viewer (#805): a table of contents that scrolls a heading
//! to the top, links followed between documents to an anchor, back and
//! forward to the same scroll offset, and a loader's error shown in place.

use intuituive::prelude::*;
use intuituive::{Driver, MarkdownViewer, markdown_viewer};

/// A long document: four sections, each with ten paragraphs.
fn long() -> String {
    let mut doc = String::from(
        "# Guide\n\nSee [the other](b.md#usage) and [the web](https://example.com).\n",
    );
    for section in ["Install", "Configure", "Run", "Debug"] {
        doc.push_str(&format!("\n## {section}\n"));
        for n in 1..=10 {
            doc.push_str(&format!("\n{section} step {n}.\n"));
        }
    }
    doc
}

fn load(path: &str) -> Result<String, String> {
    match path {
        "a.md" => Ok(long()),
        "b.md" => {
            let mut doc = String::from("# Other\n\nIntro.\n\n## Usage\n\n[Back home](a.md)\n");
            for n in 1..=12 {
                doc.push_str(&format!("\nUse it, {n}.\n"));
            }
            Ok(doc)
        }
        _ => Err(format!("no document {path}")),
    }
}

fn app(slot: std::rc::Rc<std::cell::RefCell<Option<MarkdownViewer>>>) -> App {
    App::new(move || {
        let viewer = markdown_viewer(load).open("a.md");
        *slot.borrow_mut() = Some(viewer.clone());
        // Keys for the test to steer it with from inside the app.
        let (go, missing, back, forward, scroll) = (
            viewer.clone(),
            viewer.clone(),
            viewer.clone(),
            viewer.clone(),
            viewer.offset(),
        );
        viewer
            .view()
            .on_key("f1", move |_| go.go("b.md#usage"))
            .on_key("f2", move |_| missing.go("missing.md"))
            .on_key("f3", move |_| assert!(back.back()))
            .on_key("f4", move |_| assert!(forward.forward()))
            .on_key("f5", move |_| scroll.set(3))
    })
    .wait_for_tasks(true)
}

fn viewer() -> (Driver, MarkdownViewer) {
    let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut driver = app(slot.clone()).driver(70, 12);
    driver.settle();
    let viewer = slot.borrow().clone().unwrap();
    (driver, viewer)
}

/// The document's side of row `row`: right of the table of contents.
fn document_row(driver: &Driver, row: usize) -> String {
    let line = &driver.screen().plain()[row];
    line.chars().skip(28).collect::<String>().trim().to_string()
}

/// Let the loads in flight arrive: an app that waits for its tasks waits
/// before each event, so send one nothing uses.
fn wait(driver: &mut Driver) {
    driver.press("f9");
    driver.settle();
}

fn shown(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

#[test]
fn the_table_of_contents_lists_the_headings_and_marks_the_one_in_view() {
    let (driver, _) = viewer();
    let now = shown(&driver);
    assert!(now.contains("Contents"), "{now}");
    assert!(now.contains("▸ Guide"), "{now}");
    assert!(
        now.contains("    Install") && now.contains("    Debug"),
        "{now}"
    );
}

#[test]
fn picking_the_third_heading_scrolls_it_to_the_top() {
    let (mut driver, viewer) = viewer();
    driver.assert_focused("Contents");
    driver.press("down down enter");
    let headings = viewer.headings();
    assert_eq!(headings[2].text, "Configure");
    assert!(
        document_row(&driver, 0).contains("Configure"),
        "{}",
        shown(&driver)
    );
    assert!(
        shown(&driver).contains("▸   Configure"),
        "{}",
        shown(&driver)
    );
}

#[test]
fn a_link_opens_the_other_document_at_its_heading_and_back_returns() {
    let (mut driver, viewer) = viewer();
    // Scrolled a little first, to come back to.
    driver.press("f5");
    wait(&mut driver);
    let before = shown(&driver);
    assert!(before.contains("Install step 5."), "{before}");
    driver.press("f1");
    wait(&mut driver);
    assert_eq!(viewer.location().as_deref(), Some("b.md"));
    assert!(
        document_row(&driver, 0).contains("Usage"),
        "{}",
        shown(&driver)
    );
    driver.press("f3");
    wait(&mut driver);
    assert_eq!(viewer.location().as_deref(), Some("a.md"));
    assert_eq!(viewer.offset().get(), 3);
    assert_eq!(shown(&driver), before);
    driver.press("f4");
    wait(&mut driver);
    assert_eq!(viewer.location().as_deref(), Some("b.md"));
    assert!(
        document_row(&driver, 0).contains("Usage"),
        "{}",
        shown(&driver)
    );
    assert!(!viewer.can_go_forward() && viewer.can_go_back());
}

#[test]
fn enter_on_a_link_follows_it_and_backspace_goes_back() {
    let (mut driver, viewer) = viewer();
    // Tab from the table of contents, past the view, to the paragraph
    // with the links.
    driver.press("tab tab");
    driver.press("enter");
    wait(&mut driver);
    assert_eq!(viewer.location().as_deref(), Some("b.md"));
    assert!(
        document_row(&driver, 0).contains("Usage"),
        "{}",
        shown(&driver)
    );
    driver.press("backspace");
    wait(&mut driver);
    assert_eq!(viewer.location().as_deref(), Some("a.md"));
}

#[test]
fn a_link_with_a_scheme_goes_to_on_link() {
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let seen = opened.clone();
    let app = App::new(move || {
        let seen = seen.clone();
        markdown_viewer(load)
            .on_link(move |url, _| seen.borrow_mut().push(url.to_string()))
            .open("a.md")
            .view()
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(70, 12);
    wait(&mut driver);
    driver.press("tab tab tab enter");
    assert_eq!(
        opened.borrow().as_slice(),
        ["https://example.com".to_string()]
    );
}

#[test]
fn t_hides_the_table_of_contents() {
    let (mut driver, _) = viewer();
    driver.press("t");
    assert!(!shown(&driver).contains("Contents"), "{}", shown(&driver));
    assert!(driver.screen().plain()[0].starts_with("Guide") || shown(&driver).contains("Guide"));
}

#[test]
fn a_loader_that_fails_shows_its_error_and_the_app_goes_on() {
    let (mut driver, _) = viewer();
    driver.press("f2");
    wait(&mut driver);
    assert!(
        shown(&driver).contains("missing.md: no document missing.md"),
        "{}",
        shown(&driver)
    );
    assert!(!driver.is_done());
    driver.press("f3");
    wait(&mut driver);
    assert!(
        !shown(&driver).contains("no document"),
        "{}",
        shown(&driver)
    );
    assert!(
        document_row(&driver, 0).contains("Guide"),
        "{}",
        shown(&driver)
    );
}
