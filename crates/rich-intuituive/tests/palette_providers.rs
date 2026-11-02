//! Command palette providers (#803): a provider's hits merge into the
//! palette by score as they arrive, a new query cancels the searches still
//! running, the matched characters are highlighted, and the system
//! commands (Theme, Keys, Maximize, Screenshot, Quit) can be turned off.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use intuituive::prelude::*;
use intuituive::{Driver, Hit, HitSink, Provider, SystemCommands};

fn shown(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

/// Settle until `done` holds, for up to five seconds.
fn wait_until(driver: &mut Driver, done: impl Fn(&Driver) -> bool) {
    let start = Instant::now();
    while !done(driver) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{}",
            shown(driver)
        );
        std::thread::sleep(Duration::from_millis(10));
        driver.settle();
    }
}

/// Finds "Slow <query>" after 200 ms, on a thread of its own, and notes
/// each search: its query and whether it was cancelled by then.
struct Slow(Arc<Mutex<Vec<(String, bool)>>>);

impl Provider for Slow {
    fn name(&self) -> &str {
        "slow"
    }

    fn search(&self, query: &str, sink: &HitSink) {
        let (query, log) = (query.to_string(), self.0.clone());
        sink.background(move |sink| {
            std::thread::sleep(Duration::from_millis(200));
            let cancelled = sink.cancelled();
            let label = format!("Slow {query}");
            sink.send(Hit::new("slow", label, |_| {}).score(1_000));
            log.lock().unwrap().push((query, cancelled));
        });
    }
}

fn slow_app(log: Arc<Mutex<Vec<(String, bool)>>>) -> App {
    App::new(|| label("main").bind("ctrl+a", "Archive", |_| {}).focusable())
        .provider(Slow(log))
        .palette_key("ctrl+p")
}

#[test]
fn the_bindings_show_at_once_and_a_slow_providers_hits_arrive_later() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut driver = slow_app(log.clone()).driver(120, 24);
    driver.press("ctrl+p a");
    let now = shown(&driver);
    assert!(now.contains("Archive"), "{now}");
    assert!(!now.contains("Slow a"), "{now}");
    assert!(now.contains("searching"), "{now}");
    wait_until(&mut driver, |d| shown(d).contains("Slow a"));
    // Its score puts it first.
    let rows = driver.screen().plain();
    let slow = rows.iter().position(|r| r.contains("Slow a")).unwrap();
    let archive = rows.iter().position(|r| r.contains("Archive")).unwrap();
    assert!(slow < archive, "{}", shown(&driver));
    wait_until(&mut driver, |d| !shown(d).contains("searching"));
}

#[test]
fn typing_again_cancels_the_older_search() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut driver = slow_app(log.clone()).driver(60, 12);
    driver.press("ctrl+p a r");
    wait_until(&mut driver, |_| log.lock().unwrap().len() == 2);
    wait_until(&mut driver, |d| shown(d).contains("Slow ar"));
    let mut log = log.lock().unwrap().clone();
    log.sort();
    assert_eq!(
        log,
        vec![("a".to_string(), true), ("ar".to_string(), false)]
    );
    // The cancelled search's hit was dropped.
    assert!(!shown(&driver).contains("Slow a\n"), "{}", shown(&driver));
    assert_eq!(shown(&driver).matches("Slow").count(), 1);
}

fn themed() -> App {
    App::new(|| label("[accent]hi[/]").focusable()).palette_key("ctrl+p")
}

#[test]
fn the_picks_theme_and_enter_opens_the_picker() {
    let mut driver = themed().driver(80, 40);
    driver.press("ctrl+p t h e");
    let focused = driver
        .screen()
        .plain()
        .into_iter()
        .find(|row| row.contains('❯'))
        .unwrap_or_default();
    assert!(focused.contains("Theme"), "{}", shown(&driver));
    driver.press("enter");
    let picker = shown(&driver);
    assert!(picker.contains("Theme ›"), "{picker}");
    assert!(
        picker.contains("theme dark") && picker.contains("theme nord"),
        "{picker}"
    );
    driver.press("d r a c enter");
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[#ff79c6]hi[/]"), "{styled}");
}

#[test]
fn the_matched_characters_are_highlighted() {
    let mut driver = themed().driver(60, 14);
    driver.press("ctrl+p q u");
    let styled = intuituive::testing::styled(&driver);
    let row = styled
        .lines()
        .find(|line| line.contains("uit"))
        .unwrap_or_default()
        .to_string();
    // "Qu" of Quit is matched, "it" is not.
    assert!(row.contains("magenta]Qu[/"), "{styled}");
    assert!(!row.contains("magenta]Quit"), "{styled}");
}

#[test]
fn system_commands_can_be_turned_off() {
    let app = themed().system_commands(SystemCommands::default().without("theme").without("quit"));
    let mut driver = app.driver(60, 14);
    driver.press("ctrl+p");
    let now = shown(&driver);
    assert!(!now.contains("Theme") && !now.contains("Quit"), "{now}");
    assert!(now.contains("Keys") && now.contains("Screenshot"), "{now}");
    let none = themed().system_commands(SystemCommands::none());
    let mut driver = none.driver(60, 14);
    driver.press("ctrl+p");
    assert!(
        shown(&driver).contains("no matching commands"),
        "{}",
        shown(&driver)
    );
}

#[test]
fn quit_from_the_palette_ends_the_app() {
    let mut driver = themed().driver(60, 14);
    driver.press("ctrl+p q u i t enter");
    assert!(driver.is_done());
}

#[test]
fn maximize_gives_way_to_minimize() {
    let app = App::new(|| {
        row([
            label("left").focusable().maximizable(),
            label("right").focusable(),
        ])
    })
    .palette_key("ctrl+p");
    let mut driver = app.driver(60, 14);
    driver.press("ctrl+p M a x i m enter");
    assert!(!shown(&driver).contains("right"), "{}", shown(&driver));
    driver.press("ctrl+p");
    let now = shown(&driver);
    assert!(
        now.contains("Minimize") && !now.contains("Maximize"),
        "{now}"
    );
    driver.press("M i n i enter");
    assert!(shown(&driver).contains("right"), "{}", shown(&driver));
}

/// A provider registered with the app: what it discovers shows before
/// anything is typed, and a hit runs its closure.
struct Places;

impl Provider for Places {
    fn name(&self) -> &str {
        "place"
    }

    fn discover(&self) -> Vec<Hit> {
        vec![Hit::new("home", "Home", |cx| cx.toast("went home")).help("the start")]
    }

    fn search(&self, query: &str, sink: &HitSink) {
        for hit in self.discover() {
            if let Some(hit) = hit.matching(query) {
                sink.send(hit);
            }
        }
    }
}

#[test]
fn a_providers_discovered_hits_show_and_run() {
    let app = App::new(|| label("main").focusable())
        .provider(Places)
        .palette_key("ctrl+p");
    let mut driver = app.driver(80, 30);
    driver.press("ctrl+p");
    let now = shown(&driver);
    assert!(now.contains("place Home  the start"), "{now}");
    driver.press("h o m enter");
    assert!(shown(&driver).contains("went home"), "{}", shown(&driver));
}
