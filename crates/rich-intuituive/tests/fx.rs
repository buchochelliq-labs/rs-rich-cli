//! The `fx` effect presets: frame by frame on the test clock, composed,
//! triggered, ending cleanly, and still in accessible mode.

#[path = "../examples/fx.rs"]
#[allow(dead_code)]
mod gallery;

use std::path::Path;
use std::time::Duration;

use intuituive::fx::{self, Frame, Fx};
use intuituive::prelude::*;
use intuituive::testing::{assert_snapshot, plain};
use intuituive::{Direction, Driver, Easing};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A clock for a driver: `advance` it and it knows the time.
struct Clocked {
    driver: Driver,
    now: Duration,
}

impl Clocked {
    fn new(app: App, width: u16, height: u16) -> Clocked {
        let mut driver = app.driver(width, height);
        driver.settle();
        Clocked {
            driver,
            now: Duration::ZERO,
        }
    }

    /// On to `at` on the app's clock.
    fn to(&mut self, at: Duration) -> &mut Clocked {
        self.driver.advance(at.saturating_sub(self.now));
        self.now = self.now.max(at);
        self
    }

    fn rows(&self) -> Vec<String> {
        self.driver.screen().plain()
    }

    /// The screen at each of `times` (ms), one after another.
    fn frames(&mut self, times: &[u64]) -> String {
        let mut out = String::new();
        for &at in times {
            self.to(ms(at));
            out.push_str(&format!("@ {at} ms\n{}", plain(self.driver.screen())));
        }
        out
    }

    /// Whether the app is idle: nothing to draw and no frame asked for
    /// (the longest wait the driver gives when nothing is due).
    fn idle(&self) -> bool {
        !self.driver.needs_render() && self.driver.timeout(self.now) == ms(50)
    }

    fn fg(&self, x: u16, y: u16) -> Option<String> {
        let screen = self.driver.screen();
        let style = screen.style(screen.cell(x, y).style)?;
        Some(style.color()?.get_truecolor()?.hex())
    }

    fn bg(&self, x: u16, y: u16) -> Option<String> {
        let screen = self.driver.screen();
        let style = screen.style(screen.cell(x, y).style)?;
        Some(style.bgcolor()?.get_truecolor()?.hex())
    }
}

fn snapshot(name: &str, text: &str) {
    assert_snapshot(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("snapshots")
            .join(format!("{name}.txt")),
        text,
    );
}

/// An effect that writes how far through it is (in hundredths) at
/// column `at`: what composition hands each part.
fn probe(at: u16, length: u64) -> Fx {
    Fx::new(ms(length), move |frame: &Frame, canvas, _cx| {
        let shown = format!("{:03}", (frame.progress * 100.0).round() as u32);
        canvas.print(at, 0, &shown, None);
    })
    .easing(Easing::Linear)
}

#[test]
fn fade_in_moves_each_cell_from_the_background_to_its_colour() {
    let app = App::new(|| fx::play(label("[#ffffff on #0000ff]Hi"), fx::fade_in(ms(320))));
    let mut clock = Clocked::new(app, 2, 1);
    // At the start: the background colour, black.
    assert_eq!(clock.fg(0, 0).as_deref(), Some("#000000"));
    assert_eq!(clock.bg(0, 0).as_deref(), Some("#000000"));
    // Half way (eased in and out, so half the colour).
    clock.to(ms(160));
    assert_eq!(clock.fg(0, 0).as_deref(), Some("#808080"));
    assert_eq!(clock.bg(0, 0).as_deref(), Some("#000080"));
    clock.to(ms(320));
    assert_eq!(clock.fg(0, 0).as_deref(), Some("#ffffff"));
    assert_eq!(clock.bg(1, 0).as_deref(), Some("#0000ff"));
    assert_eq!(clock.rows(), ["Hi"]);
}

#[test]
fn the_theme_sets_the_colour_faded_to() {
    let theme = Theme::dark().style("fx", rich::Style::parse("#102030 on #fafafa").unwrap());
    let app = App::new(|| fx::play(label("plain"), fx::fade_out(ms(320)))).theme(theme);
    let mut clock = Clocked::new(app, 5, 1);
    // Text with no colour of its own fades from the `fx` style's colour
    // to its background.
    clock.to(ms(160));
    assert_eq!(clock.fg(0, 0).as_deref(), Some("#858d95"));
    clock.to(ms(320));
    // Gone at the end: blank cells.
    assert_eq!(clock.rows(), ["     "]);
}

#[test]
fn a_finished_effect_asks_for_no_more_frames() {
    let app = App::new(|| fx::play(label("done"), fx::dissolve(ms(100))));
    let mut clock = Clocked::new(app, 4, 1);
    // While it runs, a frame is due within a frame's time.
    assert!(clock.driver.timeout(clock.now) <= fx::FRAME);
    clock.to(ms(60));
    assert!(clock.driver.timeout(clock.now) <= fx::FRAME);
    clock.to(ms(100));
    assert_eq!(clock.rows(), ["    "]);
    // Over: nothing to draw, nothing asked for, and nothing sent later.
    assert!(clock.idle());
    clock.to(ms(1000));
    assert!(clock.idle());
    assert!(clock.driver.render().is_none());
    // Its end stays.
    assert_eq!(clock.rows(), ["    "]);
}

#[test]
fn an_endless_effect_keeps_asking_until_its_node_goes() {
    let app = App::new(|| {
        let on = signal(true);
        column([
            fx::play(label("shiny"), fx::shimmer(ms(500))).shown_when(move || on.get()),
            label(""),
        ])
        .on_key("x", move |_| on.set(false))
    });
    let mut clock = Clocked::new(app, 5, 2);
    clock.to(ms(2000));
    assert!(!clock.idle());
    clock.driver.press("x");
    clock.to(ms(2100));
    assert!(clock.idle());
}

#[test]
fn accessible_mode_goes_straight_to_the_end() {
    let app = App::new(|| {
        column([
            fx::play(label("one"), fx::coalesce(ms(500))).fixed(1),
            fx::play(label("two"), fx::fade_out(ms(500))).fixed(1),
            fx::play(label("rain"), fx::rain()).fixed(1),
        ])
    })
    .accessible(true);
    let clock = Clocked::new(app, 4, 3);
    assert_eq!(clock.rows(), ["one ", "    ", "rain"]);
    // No motion: no frame asked for.
    assert!(clock.idle());
}

#[test]
fn dissolve_and_coalesce_frames() {
    let text = "dissolve and\ncoalesce cel";
    let app = App::new(move || {
        column([
            fx::play(label(text), fx::dissolve(ms(384))).fixed(2),
            fx::play(label(text), fx::coalesce(ms(384))).fixed(2),
        ])
    });
    let mut clock = Clocked::new(app, 12, 4);
    snapshot("fx_dissolve", &clock.frames(&[0, 96, 192, 288, 384]));
}

#[test]
fn sweep_and_slide_frames() {
    let app = App::new(|| {
        column([
            fx::play(label("sweep right"), fx::sweep(Direction::Right, ms(320))).fixed(1),
            fx::play(
                label("sweep up\nfrom below"),
                fx::sweep(Direction::Up, ms(320)),
            )
            .fixed(2),
            fx::play(
                label("slide left").text_align(TextAlign::Right),
                fx::slide_in(Direction::Left, ms(320)),
            )
            .fixed(1),
            fx::play(label("slide down"), fx::slide_in(Direction::Down, ms(320))).fixed(1),
        ])
    });
    let mut clock = Clocked::new(app, 12, 5);
    snapshot("fx_sweep_slide", &clock.frames(&[0, 80, 160, 240, 320]));
}

#[test]
fn slide_in_moves_wide_graphemes_whole() {
    let app = App::new(|| fx::play(label("界a界"), fx::slide_in(Direction::Right, ms(100))));
    let mut clock = Clocked::new(app, 6, 1);
    for at in [0, 20, 40, 60, 80, 100] {
        clock.to(ms(at));
        let row = &clock.rows()[0];
        assert_eq!(rich::cells::cell_len(row), 6, "{at}: {row:?}");
    }
    assert_eq!(clock.rows()[0].trim_end(), "界a界");
}

#[test]
fn sequence_runs_each_in_turn_and_parallel_together() {
    let app = App::new(|| {
        column([
            fx::play(label(""), fx::sequence([probe(0, 160), probe(4, 160)])).fixed(1),
            fx::play(label(""), fx::parallel([probe(0, 160), probe(4, 320)])).fixed(1),
            fx::play(label(""), probe(0, 160).delay(ms(160))).fixed(1),
            fx::play(label(""), probe(0, 160).ping_pong().repeat(2)).fixed(1),
            fx::play(label(""), probe(0, 160).reversed().hold(ms(80))).fixed(1),
            fx::play(label(""), probe(0, 160).forever().limit(ms(400))).fixed(1),
        ])
    });
    let mut clock = Clocked::new(app, 8, 6);
    snapshot("fx_compose", &clock.frames(&[0, 80, 160, 240, 400, 640]));
    assert!(clock.idle());
}

#[test]
fn easing_a_whole_sequence_eases_its_timeline() {
    let eased = fx::sequence([probe(0, 160), probe(4, 160)]).easing(Easing::EaseIn);
    assert_eq!(eased.length(), Some(ms(320)));
    let app = App::new(move || fx::play(label(""), eased.clone()));
    let mut clock = Clocked::new(app, 8, 1);
    // An eighth of the way at half time: the first part, a quarter in.
    clock.to(ms(160));
    assert_eq!(clock.rows()[0].trim_end(), "025");
}

#[test]
fn play_when_replays_each_time_its_condition_turns_true() {
    let app = App::new(|| {
        let flash = signal(false);
        fx::play_when(label(""), probe(0, 160), move || flash.get())
            .on_key("x", move |_| flash.update(|f| *f = !*f))
    });
    let mut clock = Clocked::new(app, 3, 1);
    // Not yet played: as it is.
    assert_eq!(clock.rows()[0], "   ");
    assert!(clock.idle());
    clock.driver.press("x");
    clock.to(ms(80));
    assert_eq!(clock.rows()[0], "050");
    clock.to(ms(160));
    assert_eq!(clock.rows()[0], "100");
    // Turning false leaves its end; true again plays it afresh.
    clock.driver.press("x");
    assert_eq!(clock.rows()[0], "100");
    clock.driver.press("x");
    clock.to(ms(208));
    assert_eq!(clock.rows()[0], "030");
}

#[test]
fn play_while_runs_only_while_its_condition_holds() {
    let app = App::new(|| {
        let loading = signal(true);
        fx::play_while(label("Ada"), fx::skeleton(ms(1000)), move || loading.get())
            .on_key("x", move |_| loading.update(|l| *l = !*l))
    });
    let mut clock = Clocked::new(app, 3, 1);
    // Dim at the start, bright half way through a pulse.
    assert_eq!(clock.bg(0, 0).as_deref(), Some("#303030"));
    clock.to(ms(500));
    assert_eq!(clock.bg(1, 0).as_deref(), Some("#585858"));
    assert_eq!(clock.rows()[0], "   ");
    clock.driver.press("x");
    assert_eq!(clock.rows()[0], "Ada");
    assert!(clock.idle());
    // Again: from the start of a pulse.
    clock.driver.press("x");
    assert_eq!(clock.bg(0, 0).as_deref(), Some("#303030"));
}

#[test]
fn a_transition_plays_back_and_hides_at_its_start() {
    let app = App::new(|| {
        let open = signal(false);
        column([
            fx::transition(
                label("menu options"),
                fx::sweep(Direction::Right, ms(192)),
                move || open.get(),
            )
            .fixed(1),
            label("rest"),
        ])
        .on_key("x", move |_| open.update(|o| *o = !*o))
    });
    let mut clock = Clocked::new(app, 12, 2);
    let rows = |clock: &Clocked| -> Vec<String> {
        clock
            .rows()
            .iter()
            .map(|r| r.trim_end().to_string())
            .collect()
    };
    // Hidden to begin with.
    assert_eq!(rows(&clock)[0], "rest");
    clock.driver.press("x");
    clock.to(ms(200));
    assert_eq!(rows(&clock), ["menu options", "rest"]);
    assert!(clock.idle());
    // Going: still there, sweeping back…
    clock.driver.press("x");
    clock.to(ms(296));
    assert_eq!(rows(&clock)[1], "rest");
    let half = rows(&clock)[0].clone();
    assert!(
        half.starts_with("menu") && half != "menu options",
        "{half:?}"
    );
    // …turned round half way, back from where it was…
    clock.driver.press("x");
    clock.to(ms(400));
    assert_eq!(rows(&clock), ["menu options", "rest"]);
    // …and gone once back at its start.
    clock.driver.press("x");
    clock.to(ms(560));
    assert_eq!(rows(&clock)[1], "rest");
    clock.to(ms(600));
    assert_eq!(rows(&clock)[0], "rest");
    assert!(clock.idle());
}

#[test]
fn a_transition_in_accessible_mode_shows_and_hides_at_once() {
    let app = App::new(|| {
        let open = signal(true);
        column([
            fx::transition(label("menu"), fx::fade_in(ms(200)), move || open.get()).fixed(1),
            label("rest"),
        ])
        .on_key("x", move |_| open.update(|o| *o = !*o))
    })
    .accessible(true);
    let mut clock = Clocked::new(app, 4, 2);
    assert_eq!(clock.rows(), ["menu", "rest"]);
    clock.driver.press("x");
    assert_eq!(clock.rows()[0], "rest");
}

#[test]
fn glitch_settles_back_to_the_node() {
    let text = "glitching text\nover two rows";
    let app = App::new(move || fx::play(label(text), fx::glitch(ms(400))));
    let mut clock = Clocked::new(app, 14, 2);
    clock.to(ms(200));
    let mid = clock.rows();
    assert_ne!(mid, ["glitching text", "over two rows "]);
    snapshot("fx_glitch", &clock.frames(&[200, 400]));
    assert_eq!(clock.rows(), ["glitching text", "over two rows "]);
    assert!(clock.idle());
}

#[test]
fn rain_falls_over_the_node() {
    let app = App::new(|| fx::play(fill(".", ""), fx::rain()));
    let mut clock = Clocked::new(app, 10, 6);
    // Nothing has fallen at the start.
    assert_eq!(clock.rows(), vec![".".repeat(10); 6]);
    snapshot("fx_rain", &clock.frames(&[400, 800]));
    // Its colour, the theme's `fx.rain` (a bright green unless set).
    let screen = clock.driver.screen();
    let greens = (0..6u16)
        .flat_map(|y| (0..10u16).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            screen
                .style(screen.cell(x, y).style)
                .and_then(|s| s.color()?.get_truecolor())
                .is_some_and(|c| c.green > c.red && c.green > c.blue)
        })
        .count();
    assert!(greens > 3, "{greens}");
    assert!(!clock.idle());
}

#[test]
fn shimmer_lights_a_band_across_the_text() {
    let app = App::new(|| fx::play(label("[#404040]shimmering text"), fx::shimmer(ms(1000))));
    let mut clock = Clocked::new(app, 15, 1);
    assert_eq!(clock.fg(0, 0).as_deref(), Some("#404040"));
    // Half way the band is at the middle: brightest there, as is at the ends.
    clock.to(ms(496));
    let light = |x| u8::from_str_radix(&clock.fg(x, 0).unwrap()[1..3], 16).unwrap();
    let brightest = (0..15).max_by_key(|&x| light(x)).unwrap();
    assert!((6..=9).contains(&brightest), "{brightest}");
    assert!(light(brightest) > 0xc0, "{}", light(brightest));
    assert_eq!(light(0), 0x40);
    assert_eq!(light(14), 0x40);
    assert_eq!(clock.rows()[0], "shimmering text");
}

#[test]
fn hsl_shift_turns_the_colours_and_ping_pong_brings_them_back() {
    let app = App::new(|| {
        fx::play(
            label("[#ff0000]red"),
            fx::hsl_shift(120.0, 0.0, 0.0, ms(100)).ping_pong(),
        )
    });
    let mut clock = Clocked::new(app, 3, 1);
    clock.to(ms(100));
    assert_eq!(clock.fg(0, 0).as_deref(), Some("#00ff00"));
    clock.to(ms(200));
    assert_eq!(clock.fg(0, 0).as_deref(), Some("#ff0000"));
}

#[test]
fn a_splash_shows_its_cover_then_the_app_and_a_key_skips_it() {
    let build = || {
        App::new(|| {
            let cover = label("LOGO");
            let app = label("app").on_key("a", |_| {});
            fx::splash(cover, ms(500), app)
        })
    };
    let mut clock = Clocked::new(build(), 4, 1);
    clock.to(ms(400));
    assert_eq!(clock.rows()[0], "LOGO");
    clock.to(ms(1300));
    assert_eq!(clock.rows()[0], "app ");
    // The app fades in, and then all is still.
    clock.to(ms(1700));
    assert!(clock.idle());
    let mut clock = Clocked::new(build(), 4, 1);
    clock.driver.press("enter");
    clock.to(ms(400));
    assert_eq!(clock.rows()[0], "app ");
}

#[test]
fn a_placeholder_reads_as_loading() {
    let app = App::new(|| fx::placeholder([4, 2]));
    let clock = Clocked::new(app, 4, 2);
    let tree = clock.driver.accessibility();
    assert!(
        tree.iter().any(|node| node.describe().contains("Loading")),
        "{:?}",
        tree.iter().map(|n| n.describe()).collect::<Vec<_>>()
    );
    assert!(tree.iter().all(|node| !node.describe().contains("xxxx")));
    assert!(clock.bg(3, 0).is_some() && clock.bg(2, 1).is_none());
}

#[test]
fn the_gallery_example_shows_every_preset_after_its_splash() {
    let mut clock = Clocked::new(gallery::app(), 90, 24);
    assert!(clock.rows().iter().any(|r| r.contains("intuiTUIve")));
    clock.driver.press("enter");
    clock.to(ms(1000));
    let screen = clock.rows().join("\n");
    for title in [
        "fade_in / fade_out",
        "coalesce / dissolve",
        "sweep",
        "slide_in",
        "hsl_shift",
        "glitch",
        "rain",
        "shimmer",
        "skeleton (l)",
    ] {
        assert!(screen.contains(title), "{title} in\n{screen}");
    }
    // The drawer opens on `t`.
    assert!(!screen.contains("transition (t)"));
    clock.driver.press("t");
    clock.to(ms(1400));
    assert!(clock.rows().join("\n").contains("transition (t)"));
    // The skeleton stops on `l`: the names show.
    clock.driver.press("l");
    assert!(clock.rows().join("\n").contains("Ada Lovelace"));
}

#[test]
fn the_gallery_in_accessible_mode_is_still() {
    let mut clock = Clocked::new(gallery::app().accessible(true), 90, 24);
    clock.driver.press("enter");
    clock.to(ms(100));
    // The endless effects rest at their start, the rest at their end:
    // nothing asks for frames.
    assert!(clock.idle());
}
