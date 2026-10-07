//! The inspector and theme files reloaded while the app runs.

mod common;

use common::{run, screen};
use intuituive::prelude::*;
use rich_interact::headless::Script;

fn counter() -> App {
    App::new(|| {
        let count = signal(0);
        column([
            text!("count {count}").fixed(1).name("count"),
            label("+ adds").panel("Help"),
        ])
        .on_key("+", move |_| count.update(|c| *c += 1))
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn the_inspector_shows_the_tree_and_what_drew() {
    let rows = screen(&run(
        counter().inspector(true),
        Script::new().keys("+ q"),
        80,
        16,
    ));
    // The app keeps the left 48 columns; the panel takes 32.
    assert_eq!(&rows[0][..7], "count 1");
    let panel: Vec<String> = rows.iter().map(|r| r.chars().skip(48).collect()).collect();
    let text = panel.join("\n");
    assert!(text.contains("inspector"), "{text}");
    // After the key, one node was dirty and drew: the count.
    assert!(text.contains("drew 1 of 1 dirty"), "{text}");
    assert!(text.contains("count (text) 48x1"), "{text}");
    assert!(text.contains("panel \"Help\""), "{text}");
    assert!(text.contains("label 46x"), "{text}");
}

#[test]
fn f12_hides_and_shows_the_inspector() {
    let rows = screen(&run(
        counter().inspector(true),
        Script::new().keys("f12 q"),
        80,
        6,
    ));
    assert!(rows.iter().all(|r| !r.contains("inspector")), "{rows:?}");
    // The panel's border now spans the whole width.
    assert_eq!(rows[1].chars().count(), 80);
}

#[test]
fn a_theme_file_is_reloaded_when_it_changes() {
    let dir = std::env::temp_dir().join(format!("intuituive-theme-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("theme.ini");
    std::fs::write(&path, "[styles]\naccent = red\n").unwrap();
    let edit = path.clone();
    let app = App::new(move || {
        let edit = edit.clone();
        label("[accent]hi[/]")
            .on_key("e", move |_| {
                // A different length, so the change shows without waiting
                // for the file system's clock.
                std::fs::write(&edit, "[styles]\naccent = bold green\n").unwrap();
            })
            .on_key("q", |cx| cx.quit())
    })
    .theme_file(&path);
    let out = run(app, Script::new().keys("e q"), 10, 1).output();
    let red = out
        .find("\x1b[0;31mhi")
        .expect("drawn in the file's first accent");
    let green = out
        .find("\x1b[0;1;32mhi")
        .expect("drawn again after the edit");
    assert!(red < green);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_bad_theme_file_keeps_the_last_good_styles_and_says_why() {
    let dir = std::env::temp_dir().join(format!("intuituive-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("theme.ini");
    std::fs::write(&path, "[styles]\naccent = red\n").unwrap();
    let edit = path.clone();
    let app = App::new(move || {
        let edit = edit.clone();
        label("[accent]hi[/]")
            .on_key("e", move |_| {
                std::fs::write(&edit, "[styles]\naccent = not-a-colour-at-all\n").unwrap();
            })
            .on_key("q", |cx| cx.quit())
    })
    .theme_file(&path)
    .inspector(true);
    let record = run(app, Script::new().keys("e q"), 80, 10);
    let rows = screen(&record);
    assert!(rows[0].starts_with("hi"));
    let text = rows.join("\n");
    assert!(text.contains("theme theme.ini:"), "{text}");
    assert!(!text.contains("theme.ini: loaded"), "{text}");
    // Still red: the last good styles stay.
    let out = record.output();
    assert!(out.contains("\x1b[0;31mhi"));
    assert!(!out.contains("not-a-colour"));
    std::fs::remove_dir_all(dir).ok();
}
