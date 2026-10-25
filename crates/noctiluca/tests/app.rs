//! The browser app on intuiTUIve's driver, over a scripted engine that
//! serves Chrome's saved snapshots of the fixture pages: no browser and
//! no terminal. What the live tests check against Chrome, these check
//! for the app: views, hints, history, find, the F1 overlay, keys from
//! config, site keys and binding a key from the overlay.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::app::{app, Options};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, View};
use noctiluca::engine::PageAction;
use noctiluca::testing::FakeEngine;
use rich_intuituive::Driver;

fn snapshots() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

/// The fake engine, its pages moved to `https://fixture.example/NAME.html`
/// so they have a host for site settings.
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
    // A click on a link goes where it points (the fake engine's default).
    engine
}

fn options(config: Config, url: &str) -> Options {
    Options {
        config,
        bookmarks: Bookmarks::default(),
        url: Some(url.to_string()),
        view: Some(View::Reader),
        images: Some(noctiluca::config::Images::Ansi),
    }
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

/// Settle the driver until the screen holds `text`.
fn wait_for(driver: &mut Driver, text: &str) {
    let start = Instant::now();
    loop {
        driver.settle();
        if screen(driver).contains(text) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{text:?} never showed:\n{}",
            screen(driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_page_reads_in_reader_mode_and_the_projection_is_one_key_away() {
    let mut driver = app(
        Box::new(engine()),
        options(Config::default(), "reader.html"),
    )
    .driver(80, 30);
    wait_for(&mut driver, "Tiny drifters of the open sea");
    let text = screen(&driver);
    assert!(text.contains("• Dinoflagellates"), "{text}");
    assert!(text.contains("reader"), "{text}");
    assert!(!text.contains("Hidden by"), "{text}");
    driver.press("r");
    wait_for(&mut driver, "projection");
    assert!(screen(&driver).contains("Plankton"));
}

#[test]
fn a_hint_follows_a_link_and_history_returns() {
    let engine = engine();
    let state = engine.state.clone();
    let mut driver =
        app(Box::new(engine), options(Config::default(), "reader.html")).driver(80, 30);
    wait_for(&mut driver, "Tiny drifters");
    driver.press("f");
    wait_for(&mut driver, "[a]open sea");
    driver.press("a");
    wait_for(&mut driver, "The second page");
    assert!(state
        .lock()
        .unwrap()
        .actions
        .iter()
        .any(|a| matches!(a, PageAction::Click(_))));
    driver.press("H");
    wait_for(&mut driver, "Tiny drifters");
    driver.press("L");
    wait_for(&mut driver, "You followed the link");
}

#[test]
fn find_highlights_and_moves_between_matches() {
    let mut driver = app(
        Box::new(engine()),
        options(Config::default(), "reader.html"),
    )
    .driver(80, 30);
    wait_for(&mut driver, "Tiny drifters");
    driver.press("/");
    driver.type_text("glow");
    driver.press("enter");
    wait_for(&mut driver, "match 1 of");
    driver.press("n");
    wait_for(&mut driver, "match 2 of");
}

#[test]
fn the_f1_overlay_lists_the_registry_with_reasons_and_follows_the_page() {
    let engine = engine();
    let state = engine.state.clone();
    let mut driver =
        app(Box::new(engine), options(Config::default(), "reader.html")).driver(100, 40);
    wait_for(&mut driver, "Tiny drifters");
    driver.press("f1");
    wait_for(&mut driver, "Link hints");
    let text = screen(&driver);
    assert!(text.contains("Back (Nothing to go back to.)"), "{text}");
    assert!(text.contains("One tab for now"), "{text}");
    // The page moves while the overlay is open: it follows.
    state.lock().unwrap().go("second.html");
    wait_for(&mut driver, "Second page");
    let text = screen(&driver);
    assert!(
        text.lines()
            .any(|l| l.contains(" Back ") && l.trim_end().ends_with("H │")),
        "{text}"
    );
    assert!(!text.contains("Nothing to go back to"), "{text}");
    driver.press("esc");
}

#[test]
fn a_key_rebound_in_config_shows_in_f1_and_runs() {
    let config = Config::parse("[keys]\nhints = \"ctrl+g\"\n").unwrap();
    let mut driver = app(Box::new(engine()), options(config, "reader.html")).driver(100, 40);
    wait_for(&mut driver, "Tiny drifters");
    driver.press("f1");
    wait_for(&mut driver, "Link hints");
    assert!(
        screen(&driver)
            .lines()
            .any(|l| l.contains("Link hints") && l.contains("ctrl+g")),
        "{}",
        screen(&driver)
    );
    driver.press("esc");
    driver.press("ctrl+g");
    wait_for(&mut driver, "[a]open sea");
}

#[test]
fn a_site_key_overrides_a_global_one_only_on_its_site() {
    let config = Config::parse(
        r#"
        [site."https://fixture.example/reader.html"]
        keys = { f = { role = "link", name = "the second page" }, x = { role = "button", name = "Next page" } }
        "#,
    )
    .unwrap();
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    let mut driver = app(Box::new(engine()), options(config, "reader.html")).driver(220, 40);
    wait_for(&mut driver, "Tiny drifters");
    // On its site, f is the site's: it follows "the second page".
    driver.press("f");
    wait_for(&mut driver, "You followed the link");
    // Elsewhere, f is link hints again.
    driver.press("f");
    wait_for(&mut driver, "[a]Back to the start");
    driver.press("esc");
    driver.press("H");
    wait_for(&mut driver, "Tiny drifters");
    // A site key whose element is not on the page is unavailable, and
    // says why, in F1 and when pressed.
    driver.press("f1");
    wait_for(&mut driver, "[site https://fixture.example/reader.html]");
    assert!(
        screen(&driver).contains("button \"Next page\" [site https://fixture.example/reader.html] (No button named \"Next page\" on this page.)"),
        "{}",
        screen(&driver)
    );
}

#[test]
fn binding_a_key_from_f1_writes_the_config_and_works_at_once() {
    let dir = std::env::temp_dir().join(format!("noctiluca-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "# Mine.\n[startup]\nview = \"reader\"\n").unwrap();
    let config = Config::load(Some(&path)).unwrap();
    let engine = engine();
    let mut driver = app(Box::new(engine), options(config, "reader.html")).driver(100, 40);
    wait_for(&mut driver, "Tiny drifters");
    // Choose "Bind a key on this site…", then the link, then the key.
    driver.press("ctrl+k");
    wait_for(&mut driver, "to what?");
    // The first rows are the page's controls: the two links.
    driver.press("down enter");
    wait_for(&mut driver, "Press the key");
    driver.press("z");
    wait_for(&mut driver, "Enter binds it on fixture.example");
    driver.press("enter");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# Mine.\n"), "{text}");
    assert!(
        text.contains(
            "[site.\"fixture.example\".keys]\nz = { role = \"link\", name = \"the second page\" }"
        ),
        "{text}"
    );
    // It works at once.
    driver.press("z");
    wait_for(&mut driver, "You followed the link");
    // Binding a key that is taken says so, and asks.
    driver.press("H");
    wait_for(&mut driver, "Tiny drifters");
    driver.press("ctrl+k");
    wait_for(&mut driver, "to what?");
    driver.press("enter");
    wait_for(&mut driver, "Press the key");
    driver.press("z");
    wait_for(
        &mut driver,
        "already does link \"the second page\" here: Enter replaces it.",
    );
    driver.press("esc");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_form_field_commits_its_draft_and_a_password_stays_masked() {
    let engine = engine();
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options(Config::default(), "form.html")).driver(100, 40);
    wait_for(&mut driver, "Sign up");
    // Focus the name field by its hint, type, apply.
    driver.press("f");
    wait_for(&mut driver, "hints");
    driver.press("a");
    driver.press("end");
    driver.type_text(" Lovelace");
    wait_for(&mut driver, "edited");
    driver.press("enter");
    let start = Instant::now();
    loop {
        driver.settle();
        let actions = state.lock().unwrap().actions.clone();
        if actions
            .iter()
            .any(|a| matches!(a, PageAction::SetValue { value, .. } if value.ends_with("Lovelace")))
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no commit: {actions:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!screen(&driver).contains("hunter2"));
}

#[test]
fn a_select_is_keyed_by_option_and_its_disabled_option_cannot_be_chosen() {
    let engine = engine();
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options(Config::default(), "form.html")).driver(100, 40);
    wait_for(&mut driver, "Mirror");
    // The hint of the Mirror select focuses it (the disabled button has
    // no hint: a, s, d, f, g, then h).
    driver.press("f");
    wait_for(&mut driver, "[h]");
    driver.press("h");
    driver.press("enter down enter");
    let start = Instant::now();
    loop {
        driver.settle();
        let actions = state.lock().unwrap().actions.clone();
        if let Some(PageAction::SelectOption { option, .. }) = actions
            .iter()
            .find(|a| matches!(a, PageAction::SelectOption { .. }))
        {
            // The third option, "Second mirror": the disabled second one,
            // which has the first one's value, was skipped by its key.
            assert_eq!(*option, 199);
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no choice: {actions:?}\n{}",
            screen(&driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The fake engine with the audio page's element ready to play.
fn audio_engine() -> FakeEngine {
    let engine = engine();
    engine.state.lock().unwrap().media.insert(
        3,
        noctiluca::engine::Playback {
            paused: true,
            muted: false,
            volume: 1.0,
            time: 0.0,
            duration: Some(2.0),
            ended: false,
            ready: 4,
            failed: false,
        },
    );
    engine
}

#[test]
fn media_controls_follow_the_element_and_run_from_keys_and_f1() {
    let engine = audio_engine();
    let state = engine.state.clone();
    let mut driver =
        app(Box::new(engine), options(Config::default(), "audio.html")).driver(100, 30);
    wait_for(&mut driver, "audio: Test tone");
    wait_for(&mut driver, "Play");
    // Sound is off unless asked for, and the widget says so.
    assert!(
        screen(&driver).contains("sound is off: start with --audio"),
        "{}",
        screen(&driver)
    );
    driver.press("p");
    wait_for(&mut driver, "Pause");
    driver.press("alt+m");
    wait_for(&mut driver, "Unmute");
    driver.press("-");
    let start = Instant::now();
    while state.lock().unwrap().media[&3].volume > 0.95 {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no volume change"
        );
        driver.settle();
        std::thread::sleep(Duration::from_millis(10));
    }
    let playback = state.lock().unwrap().media[&3];
    assert!(!playback.paused && playback.muted && (playback.volume - 0.9).abs() < 1e-9);
    // F1 says what the media actions would do now.
    driver.press("f1");
    driver.type_text("media");
    wait_for(&mut driver, "Pause the media");
    assert!(
        screen(&driver).contains("Unmute the media"),
        "{}",
        screen(&driver)
    );
}

#[test]
fn a_site_with_audio_off_keeps_its_media_muted() {
    let config = Config::parse("[site.\"fixture.example\"]\naudio = false\n").unwrap();
    let engine = audio_engine();
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options(config, "audio.html")).driver(100, 30);
    wait_for(&mut driver, "audio: Test tone");
    wait_for(&mut driver, "Unmute");
    assert!(state.lock().unwrap().media[&3].muted);
}

fn projection(config: Config, url: &str) -> Options {
    Options {
        view: Some(View::Projection),
        ..options(config, url)
    }
}

#[test]
fn a_key_toggles_the_adapted_and_the_faithful_projection_and_about_lists_the_rules() {
    // adapt.html was laid out at 72 columns: a narrow terminal.
    let mut driver = app(
        Box::new(engine()),
        projection(Config::default(), "adapt.html"),
    )
    .driver(72, 40);
    wait_for(&mut driver, "≡ Sections: News Sea Sky About");
    driver.press("a");
    wait_for(&mut driver, "faithful");
    let text = screen(&driver);
    assert!(!text.contains("≡ Sections"), "{text}");
    driver.press("?");
    wait_for(&mut driver, "About Noctiluca");
    let text = screen(&driver);
    assert!(text.contains("off, the faithful projection"), "{text}");
    driver.press("esc");
    driver.press("a");
    wait_for(&mut driver, "≡ Sections");
    driver.press("?");
    wait_for(&mut driver, "active rules");
    let text = screen(&driver);
    // One about page: the rules and the browser's codec table.
    assert!(text.contains("Codecs"), "{text}");
    assert!(text.contains("H.264 (MP4)"), "{text}");
    for rule in ["decorative-images", "collapse-nav", "live-tail"] {
        let line = text.lines().find(|l| l.contains(rule)).unwrap();
        assert!(line.contains("on "), "{line}");
    }
    let line = text
        .lines()
        .find(|l| l.contains("text-over-pictures"))
        .unwrap();
    assert!(line.contains("off"), "{line}");
}

#[test]
fn the_config_switches_a_rule_off() {
    let config = Config::parse("[adapt]\ncollapse-nav = false\n").unwrap();
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    let mut driver = app(Box::new(engine()), projection(config, "adapt.html")).driver(72, 40);
    wait_for(&mut driver, "Before the");
    assert!(!screen(&driver).contains('≡'));
    driver.press("?");
    wait_for(&mut driver, "About Noctiluca");
    let text = screen(&driver);
    let line = text
        .lines()
        .find(|l| l.contains("collapse-nav"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(line.contains("off"), "{line}");
}

#[test]
fn a_live_log_scrolls_up_and_holds_then_follows_again() {
    let mut driver = app(
        Box::new(engine()),
        projection(Config::default(), "log-div.html"),
    )
    .driver(100, 30);
    wait_for(&mut driver, "ada: so blue");
    driver.press("{");
    wait_for(&mut driver, "paused");
    let text = screen(&driver);
    assert!(
        text.contains("ada: first light") && !text.contains("so blue"),
        "{text}"
    );
    driver.press("}");
    wait_for(&mut driver, "following its newest entries");
    assert!(screen(&driver).contains("ada: so blue"));
}
