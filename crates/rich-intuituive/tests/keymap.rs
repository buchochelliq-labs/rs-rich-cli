//! User keymaps (#806): bindings with ids, remapped from code, a keys
//! file or a handler; the help and the footer show the new keys, and
//! unknown ids and conflicts are reported.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::widgets::key_hints;

/// A counter of saves, its keys `ctrl+s` under the id `editor.save`, the
/// app's quit on `ctrl+q` as `app.quit`, and the help on `?`.
fn editor() -> Node {
    let saved = signal(0);
    column([
        text!("saved {saved}")
            .bind_id("editor.save", "ctrl+s", "Save", move |_| {
                saved.update(|n| *n += 1)
            })
            .focusable()
            .autofocus(),
        key_hints().fixed(1),
    ])
    .bind_id("app.quit", "ctrl+q", "Quit", |cx| cx.quit())
    .on_key("?", |cx| cx.help())
}

fn first_row(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

#[test]
fn a_remapped_key_does_the_binding_and_the_old_one_does_not() {
    let app = App::new(editor).keymap([("editor.save", "f2")]);
    let mut driver = app.driver(50, 14);
    driver.press("ctrl+s");
    assert_eq!(first_row(&driver), "saved 0");
    driver.press("f2");
    assert_eq!(first_row(&driver), "saved 1");
    // The footer and the help list F2.
    let footer = driver.screen().plain().last().unwrap().to_lowercase();
    assert!(
        footer.contains("f2 save") && !footer.contains("ctrl+s"),
        "{footer}"
    );
    driver.press("?");
    let help = driver.screen().plain().join("\n").to_lowercase();
    assert!(help.contains("f2") && !help.contains("ctrl+s"), "{help}");
    assert!(
        driver.keymap_report().is_empty(),
        "{:?}",
        driver.keymap_report()
    );
}

#[test]
fn several_keys_and_none() {
    let app = App::new(editor).keymap([("editor.save", "ctrl+s f2"), ("app.quit", "none")]);
    let mut driver = app.driver(50, 14);
    driver.press("ctrl+s f2 ctrl+q");
    assert_eq!(first_row(&driver), "saved 2");
    assert!(!driver.is_done());
}

#[test]
fn an_unbound_action_is_not_in_the_palette_or_the_footer() {
    // F9 opens the palette here, whatever else the features bind.
    let app = App::new(|| editor().on_key("f9", |cx| cx.palette()))
        .keymap([("editor.save", "none")]);
    let mut driver = app.driver(60, 14);
    let footer = driver.screen().plain().last().unwrap().to_lowercase();
    assert!(!footer.contains("save"), "{footer}");
    driver.press("f9");
    let palette = driver.screen().plain().join("\n");
    assert!(palette.contains("Quit"), "{palette}");
    assert!(!palette.contains("Save"), "{palette}");
}

#[test]
fn an_unknown_id_is_reported_once_as_a_warning() {
    let app = App::new(editor).keymap([("editor.sav", "f2")]);
    let mut driver = app.driver(60, 14);
    driver.settle();
    assert_eq!(driver.keymap_report(), ["no binding has the id editor.sav"]);
    let screen = driver.screen().plain().join("\n");
    assert!(screen.contains("keymap:"), "{screen}");
    // Not again on the next frames.
    driver.press("ctrl+s");
    driver.advance(Duration::from_secs(10));
    let screen = driver.screen().plain().join("\n");
    assert!(!screen.contains("keymap:"), "{screen}");
    assert_eq!(first_row(&driver), "saved 1");
}

#[test]
fn a_conflict_is_reported_and_the_deeper_binding_wins() {
    let app = App::new(editor).keymap([("editor.save", "ctrl+q")]);
    let mut driver = app.driver(70, 14);
    driver.settle();
    assert_eq!(
        driver.keymap_report(),
        ["ctrl+q is both app.quit and editor.save; editor.save wins"]
    );
    driver.press("ctrl+q");
    assert_eq!(first_row(&driver), "saved 1");
    assert!(!driver.is_done());
}

#[test]
fn a_keys_file_edited_while_the_app_runs_takes_effect() {
    let dir = std::env::temp_dir().join(format!("intuituive-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("keys.toml");
    std::fs::write(&path, "[keys]\neditor.save = \"f2\"\n").unwrap();
    let app = App::new(editor).keymap_file(&path);
    let mut driver = app.driver(50, 14);
    driver.press("f2 ctrl+s");
    assert_eq!(first_row(&driver), "saved 1");
    // A longer file, so the change shows whatever the clock's grain.
    std::fs::write(&path, "[keys]\neditor.save = \"f3 f4\"\n").unwrap();
    driver.advance(Duration::from_millis(50));
    driver.press("f2 f3 f4");
    assert_eq!(first_row(&driver), "saved 3");
    // A file that does not parse keeps the last good map, and says so.
    std::fs::write(&path, "editor.save = ctrl+nope\n").unwrap();
    driver.advance(Duration::from_millis(50));
    let screen = driver.screen().plain().join("\n");
    assert!(screen.contains("keymap:"), "{screen}");
    driver.press("f3");
    assert_eq!(first_row(&driver), "saved 4");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn reload_keymap_reads_the_file_now() {
    let dir = std::env::temp_dir().join(format!("intuituive-keys-r-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("keys");
    std::fs::write(&path, "editor.save = f2\n").unwrap();
    let edit = path.clone();
    let app = App::new(move || {
        let edit = edit.clone();
        editor().on_key("r", move |cx| {
            std::fs::write(&edit, "editor.save = f5\n").unwrap();
            cx.reload_keymap();
        })
    })
    .keymap_file(&path);
    let mut driver = app.driver(50, 14);
    driver.press("r f2 f5");
    assert_eq!(first_row(&driver), "saved 1");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn set_keymap_at_run_time_and_bad_keys_are_an_error() {
    let app = App::new(|| {
        editor().on_key("m", |cx| {
            assert!(cx.set_keymap([("editor.save", "ctrl+nope")]).is_err());
            cx.set_keymap([("editor.save", "f9")]).unwrap();
        })
    });
    let mut driver = app.driver(50, 14);
    driver.press("ctrl+s m ctrl+s f9");
    assert_eq!(first_row(&driver), "saved 2");
}

#[test]
fn sequences_and_scaled_bindings_take_ids() {
    let app = App::new(|| {
        let (said, n) = (signal(String::new()), signal(0.0));
        column([text!("{said} {n}")])
            .bind_sequence("g g", "Top", move |_| said.set("top".into()))
            .sequence_id("nav.top")
            .bind_scaled("up", &[("", 1.0), ("shift", 10.0)], "More", move |_, by| {
                n.update(|n| *n += by)
            })
            .binding_id("count.more")
    })
    .keymap([("nav.top", "space t"), ("count.more", "u")]);
    let mut driver = app.driver(20, 1);
    driver.press("g g up u");
    assert_eq!(first_row(&driver), " 1");
    driver.press("space t");
    assert_eq!(first_row(&driver), "top 1");
}

#[test]
fn editing_actions_are_in_the_same_map() {
    let app = App::new(|| input(signal("one two".to_string())))
        .keymap([("text_area.delete-word", "alt+backspace")]);
    let mut driver = app.driver(20, 1);
    driver.press("alt+backspace");
    assert_eq!(first_row(&driver), "one");
    assert!(
        driver.keymap_report().is_empty(),
        "{:?}",
        driver.keymap_report()
    );
}

#[test]
fn the_diff_views_keys_have_ids() {
    use intuituive::widgets::{DiffOptions, diff_view_with};
    let app = App::new(|| {
        diff_view_with(
            signal("a\nb\n".to_string()),
            signal("A\nb\nB\n".to_string()),
            DiffOptions::new(),
        )
    })
    .keymap([("diff.next", "j")]);
    let mut driver = app.driver(30, 6);
    driver.press("j");
    assert!(
        driver.screen().plain()[0].starts_with("▌@@"),
        "{:?}",
        driver.screen().plain()
    );
}
