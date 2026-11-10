//! The browser app on intuiTUIve's driver, over a scripted engine that
//! serves Chrome's saved snapshots of the fixture pages: no browser and
//! no terminal. What the live tests check against Chrome, these check
//! for the app: views, hints, history, find, the F1 overlay, keys from
//! config, site keys and binding a key from the overlay.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::app::{Options, app};
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
        tunnel: None,
        control: None,
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
    assert!(
        state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::Click(_)))
    );
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
        screen(&driver).contains("sound is off: --audio turns it on"),
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
    // Tall enough for every rule and the codec table under them.
    .driver(72, 48);
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
    // The growing rules list scrolls independently to the codec table.
    driver.press("end");
    let codecs = screen(&driver);
    assert!(codecs.contains("Codecs"), "{codecs}");
    assert!(codecs.contains("H.264 (MP4)"), "{codecs}");
    driver.press("home");
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

#[test]
fn a_page_opened_in_reader_mode_is_the_one_the_projection_shows() {
    let mut options = options(Config::default(), "second.html");
    options.view = Some(View::Projection);
    let mut driver = app(Box::new(engine()), options).driver(80, 30);
    wait_for(&mut driver, "You followed the link");
    driver.press("r");
    wait_for(&mut driver, "reader");
    // Open another page from the address bar, in reader mode.
    driver.press("o");
    wait_for(&mut driver, "Open: an address");
    driver.type_text("https://fixture.example/docs.html");
    driver.press("enter");
    wait_for(&mut driver, "A lamp that glows");
    driver.press("r");
    wait_for(&mut driver, "projection");
    let text = screen(&driver);
    assert!(text.contains("A lamp that glows"), "{text}");
    assert!(!text.contains("You followed the link"), "{text}");
    // A hint, back and forward still show the right page in each view.
    driver.press("f");
    wait_for(&mut driver, "the guide");
    let label = screen(&driver)
        .split('[')
        .filter_map(|part| part.split_once("]the guide"))
        .map(|(label, _)| label.to_string())
        .next()
        .expect("a hint on the guide link");
    driver.type_text(&label);
    wait_for(&mut driver, "You followed the link");
    driver.press("H");
    wait_for(&mut driver, "A lamp that glows");
    driver.press("r");
    wait_for(&mut driver, "reader");
    assert!(screen(&driver).contains("A lamp that glows"));
    driver.press("L");
    wait_for(&mut driver, "You followed the link");
    driver.press("r");
    wait_for(&mut driver, "projection");
    let text = screen(&driver);
    assert!(text.contains("You followed the link"), "{text}");
    assert!(!text.contains("A lamp that glows"), "{text}");
}

/// `tests/fixtures/no-background.html` (`background` `None`) or
/// `with-background.html`, as Chrome lays them out at 800 CSS pixels.
fn plan(background: Option<&str>) -> noctiluca::engine::PageSnapshot {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let name = if background.is_some() {
        "with-background"
    } else {
        "no-background"
    };
    let mut p = PageBuilder::new(
        &format!("https://fixture.example/{name}.html"),
        "Plan",
        800.0,
        200.0,
    );
    let body = p.body();
    if let Some(colour) = background {
        p.set_style(body, "background-color", colour);
    }
    let h1 = p.element(
        body,
        "H1",
        &[],
        &[("font-weight", "700")],
        Some(Rect::new(8.0, 21.0, 784.0, 37.0)),
    );
    p.text(h1, "Plan", Rect::new(8.0, 21.0, 70.0, 37.0));
    let para = p.element(body, "P", &[], &[], Some(Rect::new(8.0, 80.0, 784.0, 18.0)));
    p.text(
        para,
        "Ship the release on Friday.",
        Rect::new(8.0, 80.0, 210.0, 18.0),
    );
    p.snapshot()
}

/// The background colour of the cells of the row that shows `text`, in
/// the faithful projection (`--no-adapt`), with or without
/// `--transparent`.
fn page_backgrounds(transparent: bool, background: Option<&str>) -> Vec<Option<String>> {
    let dir = std::env::temp_dir().join(format!("noctiluca-bg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = FakeEngine::from_dir(&dir).unwrap();
    let name = if background.is_some() {
        "with-background"
    } else {
        "no-background"
    };
    engine
        .state
        .lock()
        .unwrap()
        .pages
        .insert(name.into(), plan(background));
    let config = Config {
        transparent,
        adapt: noctiluca::adapt::Adapt::none(),
        ..Config::default()
    };
    let url = format!("https://fixture.example/{name}.html");
    let mut driver = app(Box::new(engine), projection(config, &url)).driver(100, 20);
    wait_for(&mut driver, "Ship the release");
    let lines = driver.screen().lines();
    let row = driver
        .screen()
        .plain()
        .iter()
        .position(|l| l.contains("Ship the release"))
        .unwrap();
    lines[row]
        .iter()
        .map(|s| {
            s.style
                .as_ref()
                .and_then(|s| s.bgcolor())
                .map(|c| format!("{c:?}"))
        })
        .collect()
}

#[test]
fn transparent_lets_the_terminals_background_show_where_the_page_sets_none() {
    // Without it, the faithful projection is drawn on its backdrop.
    let backdrop = page_backgrounds(false, None);
    assert!(backdrop.iter().all(Option::is_some), "{backdrop:?}");
    // With it, a page with no background leaves the cells at the
    // terminal's default.
    let none = page_backgrounds(true, None);
    assert!(none.iter().all(Option::is_none), "{none:?}");
    // A page with its own background keeps it.
    let own = page_backgrounds(true, Some("rgb(255, 250, 230)"));
    assert!(own.iter().all(Option::is_some), "{own:?}");
}

#[test]
fn transparent_is_read_from_the_config() {
    let config = Config::parse("[startup]\ntransparent = true\n").unwrap();
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    assert!(config.transparent);
    assert!(!Config::default().transparent);
}

/// A page `height` pixels long with a top bar and a cookie bar fixed to
/// the screen (a viewport 360 pixels tall: the driver's 24 rows less the
/// header and status bar's four, 18 pixels each), and a paragraph every
/// 100 pixels.
fn barred(height: f64) -> noctiluca::engine::PageSnapshot {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new("https://fixture.example/barred.html", "Bars", 800.0, height);
    p.view(0.0, 360.0);
    let body = p.body();
    let mut y = 60.0;
    let mut n = 1;
    while y + 18.0 < height {
        let para = p.element(body, "P", &[], &[], Some(Rect::new(8.0, y, 784.0, 18.0)));
        p.text(
            para,
            &format!("Paragraph {n}"),
            Rect::new(8.0, y, 100.0, 18.0),
        );
        y += 100.0;
        n += 1;
    }
    let top = p.element(
        body,
        "DIV",
        &[],
        &[("position", "fixed"), ("background-color", "rgb(0, 0, 0)")],
        Some(Rect::new(0.0, 0.0, 800.0, 36.0)),
    );
    p.paint(top, 2);
    p.text(top, "Top bar", Rect::new(8.0, 9.0, 60.0, 18.0));
    let bottom = p.element(
        body,
        "DIV",
        &[],
        &[
            ("position", "fixed"),
            ("background-color", "rgb(255, 255, 255)"),
        ],
        Some(Rect::new(0.0, 324.0, 800.0, 36.0)),
    );
    p.paint(bottom, 3);
    p.text(bottom, "We use cookies", Rect::new(8.0, 333.0, 110.0, 18.0));
    p.snapshot()
}

fn barred_engine(height: f64) -> FakeEngine {
    let dir = std::env::temp_dir().join(format!("noctiluca-bars-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = FakeEngine::from_dir(&dir).unwrap();
    {
        let mut state = engine.state.lock().unwrap();
        state.pages.insert("barred".into(), barred(height));
        state.viewport_height = 360.0;
    }
    engine
}

#[test]
fn a_top_bar_stays_pinned_and_a_cookie_bar_docked_while_the_page_scrolls() {
    let engine = barred_engine(3000.0);
    let url = "https://fixture.example/barred.html";
    let mut driver = app(Box::new(engine), projection(Config::default(), url)).driver(80, 24);
    wait_for(&mut driver, "Paragraph 1");
    let rows = driver.screen().plain();
    // Under the address line, above the status bar.
    assert!(rows[2].contains("Top bar"), "{rows:#?}");
    assert!(rows[24 - 3].contains("We use cookies"), "{rows:#?}");
    assert_eq!(rows.iter().filter(|r| r.contains("Top bar")).count(), 1);
    // To the end of the page: both bars are where they were.
    driver.press("G");
    wait_for(&mut driver, "Paragraph 30");
    let rows = driver.screen().plain();
    assert!(
        !rows.iter().any(|r| r.trim_end().ends_with("Paragraph 1")),
        "{rows:#?}"
    );
    assert!(rows[2].contains("Top bar"), "{rows:#?}");
    assert!(rows[24 - 3].contains("We use cookies"), "{rows:#?}");
}

#[test]
fn a_page_that_fits_in_two_screens_is_not_scrolled_to_its_end() {
    let engine = barred_engine(600.0);
    let state = engine.state.clone();
    let url = "https://fixture.example/barred.html";
    let mut driver = app(Box::new(engine), projection(Config::default(), url)).driver(80, 24);
    wait_for(&mut driver, "Paragraph 5");
    driver.press("G");
    for _ in 0..20 {
        driver.advance(Duration::from_millis(40));
        std::thread::sleep(Duration::from_millis(5));
    }
    let actions = state.lock().unwrap().actions.clone();
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, PageAction::ScrollTo(y) if *y == f64::MAX)),
        "{actions:?}"
    );
}

#[test]
fn hints_in_the_reader_leave_no_old_cells_behind() {
    // The root is a layers stack; the reader's scroll view draws into a
    // buffer of its own, which must be cleared where a row no longer draws
    // (the hints move each control's face right).
    let mut driver =
        app(Box::new(engine()), options(Config::default(), "form.html")).driver(100, 40);
    wait_for(&mut driver, "Sign up");
    driver.press("f");
    wait_for(&mut driver, "[d]");
    let shown = screen(&driver);
    for stale in ["nSend", "newsews", "dAda", "Small Small"] {
        assert!(!shown.contains(stale), "{stale:?} in\n{shown}");
    }
}
