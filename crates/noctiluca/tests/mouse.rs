//! The mouse in the app, on intuiTUIve's driver over the scripted engine:
//! a click on a link or a field uses it as its hint would, the wheel
//! scrolls, the header's buttons and the address row work, the status
//! bar's keys run their actions, and the lists the app opens take clicks.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::app::{Options, app};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, Images, View};
use noctiluca::engine::PageAction;
use noctiluca::testing::FakeEngine;
use rich_intuituive::Driver;

fn snapshots() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn engine() -> FakeEngine {
    let engine = FakeEngine::from_dir(&snapshots()).unwrap();
    {
        let mut state = engine.state.lock().unwrap();
        for (name, snapshot) in state.pages.iter_mut() {
            let doc = &snapshot.dom["documents"][0];
            let at = doc["documentURL"].as_u64().unwrap() as usize;
            snapshot.dom["strings"][at] = format!("https://fixture.example/{name}.html").into();
        }
    }
    engine
}

fn options(view: View, url: &str) -> Options {
    Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: Some(url.to_string()),
        view: Some(view),
        images: Some(Images::Ansi),
        tunnel: None,
    }
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

fn wait(driver: &mut Driver, what: &str, ok: impl Fn(&Driver) -> bool) {
    let start = Instant::now();
    loop {
        driver.settle();
        if ok(driver) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{what} never happened:\n{}",
            screen(driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for(driver: &mut Driver, text: &str) {
    wait(driver, &format!("{text:?}"), |d| screen(d).contains(text));
}

/// Click the first place `text` shows on the screen, below the header.
fn click_page_text(driver: &mut Driver, text: &str) {
    let (column, row) = driver
        .screen()
        .plain()
        .iter()
        .enumerate()
        .skip(2)
        .find_map(|(row, line)| {
            let at = line.find(text)?;
            Some((rich::cells::cell_len(&line[..at]) as u16, row as u16))
        })
        .unwrap_or_else(|| panic!("no {text:?} on the page:\n{}", screen(driver)));
    driver.click(column + 1, row);
}

fn clicks(state: &std::sync::Arc<std::sync::Mutex<noctiluca::testing::FakeState>>) -> usize {
    state
        .lock()
        .unwrap()
        .actions
        .iter()
        .filter(|a| matches!(a, PageAction::Click(_)))
        .count()
}

#[test]
fn a_click_on_a_link_follows_it_and_the_header_goes_back_and_forward() {
    for view in [View::Reader, View::Projection] {
        let engine = engine();
        let state = engine.state.clone();
        let mut driver = app(Box::new(engine), options(view, "reader.html")).driver(80, 30);
        wait_for(&mut driver, "Tiny drifters");
        // Plain text takes no click.
        click_page_text(&mut driver, "Tiny drifters");
        assert_eq!(clicks(&state), 0, "{view:?}");
        click_page_text(&mut driver, "open sea");
        wait_for(&mut driver, "The second page");
        assert_eq!(clicks(&state), 1, "{view:?}");
        // The header's buttons: back, then forward.
        let header = driver.screen().plain()[0].clone();
        assert!(header.contains("◀ ▶ ↻"), "{header}");
        let back = rich::cells::cell_len(&header[..header.find('◀').unwrap()]) as u16;
        driver.click(back, 0);
        wait_for(&mut driver, "Tiny drifters");
        driver.click(back + 2, 0);
        wait_for(&mut driver, "You followed the link");
        // Reload, and nothing else in the band, keeps the page.
        driver.click(back + 4, 0);
        driver.click(back + 1, 0);
        wait_for(&mut driver, "You followed the link");
    }
}

#[test]
fn the_wheel_scrolls_the_page() {
    let mut driver = app(
        Box::new(engine()),
        options(View::Projection, "article.html"),
    )
    .driver(80, 10);
    wait(&mut driver, "the page", |d| screen(d).contains("row 1/"));
    // Three rows a notch.
    driver.wheel(10, 4, 2);
    wait_for(&mut driver, "row 7/");
    driver.wheel(10, 4, -1);
    wait_for(&mut driver, "row 4/");
}

#[test]
fn a_click_on_a_field_gives_it_its_widget() {
    // In the projection, a field opens its widget in a box, as its hint
    // does; in reader mode the widget is in the page, and takes the click.
    let engine = engine();
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options(View::Projection, "form.html")).driver(100, 40);
    wait_for(&mut driver, "Sign up");
    let before = screen(&driver);
    let row = before
        .lines()
        .position(|l| l.contains("Ada"))
        .unwrap_or_else(|| panic!("no name field:\n{before}"));
    let line = &before.lines().nth(row).unwrap().to_string();
    let column = rich::cells::cell_len(&line[..line.find("Ada").unwrap()]) as u16;
    driver.click(column, row as u16);
    wait(&mut driver, "the field's box", |d| {
        d.focused_name().is_some_and(|n| n != "page")
    });
    driver.press("end");
    driver.type_text(" Lovelace");
    driver.press("enter");
    wait(&mut driver, "the commit", |_| {
        state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::SetValue { value, .. } if value.ends_with("Lovelace")))
    });
}

#[test]
fn the_address_row_and_the_status_keys_take_clicks() {
    let mut driver = app(Box::new(engine()), options(View::Reader, "reader.html")).driver(100, 30);
    wait_for(&mut driver, "Tiny drifters");
    // The address row edits the address.
    driver.click(5, 1);
    wait_for(&mut driver, "Open: an address");
    driver.press("esc");
    // The status bar's keys: `r reader` switches the view.
    let keys = driver.screen().plain().last().unwrap().clone();
    let at = keys.find("r reader").unwrap_or_else(|| panic!("{keys}"));
    driver.click(rich::cells::cell_len(&keys[..at]) as u16 + 2, 29);
    wait(&mut driver, "the projection", |d| {
        d.screen().plain()[0].contains("PROJECTION")
    });
    // `F1 actions` opens the actions, and a click there runs one.
    assert!(driver.click_text("actions"));
    wait_for(&mut driver, "Find in page");
    assert!(driver.click_text("Find in page"));
    wait(&mut driver, "the find prompt", |d| {
        screen(d).contains("─ Find in page")
    });
}

#[test]
fn a_click_selects_a_bookmark_and_a_second_opens_it() {
    let mut driver = app(Box::new(engine()), options(View::Reader, "reader.html")).driver(100, 30);
    wait_for(&mut driver, "Tiny drifters");
    driver.press("m");
    click_page_text(&mut driver, "open sea");
    wait_for(&mut driver, "The second page");
    driver.press("m");
    driver.press("M");
    wait_for(&mut driver, "▸");
    let list = screen(&driver);
    // Two rows: the selected one is marked; click the other.
    let marked = list.lines().position(|l| l.contains('▸')).unwrap();
    let other = list
        .lines()
        .enumerate()
        .skip(marked.saturating_sub(1))
        .find(|(row, l)| *row != marked && l.contains("fixture.example"))
        .map(|(row, _)| row)
        .unwrap_or_else(|| panic!("one bookmark:\n{list}"));
    driver.click(10, other as u16);
    wait(&mut driver, "the other row selected", |d| {
        d.screen().plain()[other].contains('▸')
    });
    // A click on the selected row opens it.
    driver.click(10, other as u16);
    wait(&mut driver, "the list closed and a page opened", |d| {
        !screen(d).contains('▸')
    });
}
