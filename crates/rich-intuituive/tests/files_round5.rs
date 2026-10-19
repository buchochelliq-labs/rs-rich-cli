//! Round 5's `files` plugin pieces: a search listed as a level of its own,
//! the parent column's marks, a selection and a visual mode per tab, the
//! columns' moves in your own words, links, the file picker and command
//! previewers.

#![cfg(feature = "files")]

use std::path::{Path, PathBuf};

use intuituive::files::*;
use intuituive::prelude::*;
use intuituive::Driver;

/// root/
///   alpha/a.txt  beta/  notes.md  z.txt
fn tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "intuituive-files-round5-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("alpha")).unwrap();
    std::fs::create_dir_all(root.join("beta")).unwrap();
    std::fs::write(root.join("alpha/a.txt"), "in alpha").unwrap();
    std::fs::write(root.join("notes.md"), "# Title\n\nhello notes\n").unwrap();
    std::fs::write(root.join("z.txt"), "zzz").unwrap();
    root.canonicalize().unwrap()
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[test]
fn h_from_a_search_goes_back_to_the_directory_searched() {
    let root = tree("search");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let root = start.clone();
        let header = move || {
            let at = name_of(&m.cwd.get());
            match m.searching() {
                Some(s) => format!("{at} search: {}", s.query()),
                None => format!("{at} browsing"),
            }
        };
        column([text(header).fixed(1), m.navigation(m.view()).flex(1)]).on_key("s", move |_| {
            m.show_search(search(&root, "a.t", SearchKind::Name))
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 6);
    let here = name_of(&root);
    // notes.md hovered, then a search.
    driver.press("j j");
    driver.press("s");
    driver.settle();
    let screen = rows(&driver).join("\n").replace('\\', "/");
    assert!(screen.contains("alpha/a.txt"), "{screen}");
    assert_eq!(rows(&driver)[0], format!("{here} search: a.t"));
    // Up from the results: the directory searched, notes.md hovered again.
    driver.press("h");
    assert_eq!(rows(&driver)[0], format!("{here} browsing"));
    assert!(rows(&driver).join("\n").contains("z.txt"));
    driver.press("l");
    assert!(
        rows(&driver).join("\n").contains("hello notes"),
        "notes.md is hovered and previewed: {:?}",
        rows(&driver)
    );
    // Entering a directory found goes there, and ends the search.
    std::fs::create_dir_all(root.join("beta/deep")).unwrap();
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let root = start.clone();
        let at = move || format!("{} {}", name_of(&m.cwd.get()), m.searching().is_some());
        column([text(at).fixed(1), m.navigation(m.view()).flex(1)]).on_key("s", move |_| {
            m.show_search(search(&root, "deep", SearchKind::Name))
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 6);
    driver.press("s");
    driver.settle();
    driver.press("l");
    assert_eq!(rows(&driver)[0], "deep false");
}

#[test]
fn the_parent_column_marks_the_selection_by_path() {
    let root = tree("parent-marks");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let picked = selection(m);
        m.navigation(m.view()).on_key("space", move |_| {
            if let Some(path) = m.hovered_path() {
                picked.toggle(path);
            }
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 4);
    // Select alpha, then go into it: alpha is marked in the parent column.
    driver.press("space l");
    let styled = intuituive::testing::styled(&driver);
    let first = styled.lines().next().unwrap();
    assert!(first.starts_with("[on yellow] [/]"), "{styled}");
}

#[test]
fn each_tab_keeps_its_selection_and_visual_mode() {
    let root = tree("tab-select");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let picked = selection(m);
        let t = tabs(m);
        let status = move || {
            let mode = if picked.in_visual() {
                "visual"
            } else {
                "normal"
            };
            let names: Vec<String> = m.targets().iter().map(|p| name_of(p)).collect();
            format!("{mode} {}", names.join(" "))
        };
        column([m.navigation(m.view()).flex(1), text(status).fixed(1)])
            .on_key("space", move |_| {
                if let Some(path) = m.hovered_path() {
                    picked.toggle(path);
                }
            })
            .on_key("v", move |_| picked.visual(false))
            .on_key("t", move |_| {
                t.open();
            })
            .on_key("1", move |_| t.switch(0))
            .on_key("2", move |_| t.switch(1))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 5);
    // Tab 1: alpha selected. Tab 2, in beta: visual mode from the top.
    driver.press("space t");
    assert_eq!(rows(&driver)[4], "normal alpha", "a new tab has its own");
    driver.press("j v");
    assert_eq!(rows(&driver)[4], "visual beta");
    driver.press("1");
    assert_eq!(rows(&driver)[4], "normal alpha");
    driver.press("2");
    assert_eq!(rows(&driver)[4], "visual beta", "still selecting");
    // Moving within a tab ends its visual range, as before.
    driver.press("l");
    assert!(
        rows(&driver)[4].starts_with("normal"),
        "{:?}",
        rows(&driver)
    );
}

#[test]
fn moves_take_your_own_wording() {
    let root = tree("moves");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let at = move || m.hovered.get().map(|e| e.name).unwrap_or_default();
        column([
            m.navigation_with(
                m.view(),
                &[
                    (Move::Down, "j down", "Next file"),
                    (Move::Bottom, "G", "Move cursor to bottom"),
                ],
            )
            .flex(1),
            text(at).fixed(1),
        ])
        .bind_sequence("g g", "Move cursor to top", move |_| m.go(Move::Top))
        .on_key("?", |cx| cx.help())
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 16);
    driver.press("G");
    assert_eq!(rows(&driver)[15], "z.txt");
    driver.press("g g");
    assert_eq!(rows(&driver)[15], "alpha");
    driver.press("?");
    let help = rows(&driver).join("\n");
    assert!(
        help.contains("Move cursor to bottom") && help.contains("Next file"),
        "{help}"
    );
}

#[cfg(unix)]
#[test]
fn yanked_files_link_into_a_directory() {
    use rich_ext::file_ops::LinkKind;
    let root = tree("links");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let jobs = tasks();
        let yanked = signal(None::<Yank>);
        let link = move |kind| {
            if let Some(yank) = yanked.get_untracked() {
                jobs.run(yank.link(m.cwd.get_untracked(), kind), |_, _| {});
            }
        };
        m.navigation(m.view())
            .on_key("y", move |_| yanked.set(Some(Yank::copy(m.targets()))))
            .on_key("minus", move |_| {
                link(LinkKind::Symbolic { relative: false })
            })
            .on_key("underscore", move |_| {
                link(LinkKind::Symbolic { relative: true })
            })
            .on_key("ctrl+minus", move |_| link(LinkKind::Hard))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 4);
    // Yank notes.md, go into beta, link it three ways.
    driver.press("j j y");
    driver.press("k l");
    driver.press("minus");
    driver.settle();
    driver.press("underscore");
    driver.settle();
    driver.press("ctrl+minus");
    driver.settle();
    let beta = root.join("beta");
    assert_eq!(
        std::fs::read_link(beta.join("notes.md")).unwrap(),
        root.join("notes.md")
    );
    assert_eq!(
        std::fs::read_link(beta.join("notes_1.md")).unwrap(),
        Path::new("../notes.md")
    );
    let hard = std::fs::symlink_metadata(beta.join("notes_2.md")).unwrap();
    assert!(hard.is_file());
    assert_eq!(
        std::fs::read_to_string(beta.join("notes_2.md")).unwrap(),
        "# Title\n\nhello notes\n"
    );
}

#[test]
fn the_picker_enters_directories_refuses_what_the_filter_does_and_cancels() {
    let root = tree("pick");
    let start = root.clone();
    let app = App::new(move || {
        let chosen = signal(String::from("none"));
        let start = start.clone();
        text!("picked {chosen}").on_key("o", move |cx| {
            let text_files = |path: &Path| path.extension().is_some_and(|e| e == "txt");
            pick_file(cx, start.clone(), text_files, move |path, _| {
                chosen.set(name_of(&path))
            });
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 16);
    driver.press("o");
    driver.settle();
    assert!(rows(&driver).join("\n").contains("Pick a file"));
    // notes.md is not a .txt: it stays, with a warning.
    driver.press("j j enter");
    let screen = rows(&driver).join("\n");
    assert!(screen.contains("cannot be picked"), "{screen}");
    assert_eq!(rows(&driver)[0], "picked none");
    // Esc closes it, nothing picked; again, and z.txt is picked.
    driver.press("escape");
    // (The warning's toast is titled so too: the picker's foot is gone.)
    assert!(
        !rows(&driver).join("\n").contains("esc cancel"),
        "{}",
        rows(&driver).join("\n")
    );
    driver.press("o");
    driver.settle();
    driver.press("G enter");
    assert_eq!(rows(&driver)[0], "picked z.txt");
}
