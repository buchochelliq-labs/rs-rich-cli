//! Pictures in the app, on intuiTUIve's driver over the scripted engine
//! (Chrome's saved snapshots of the fixture pages, the pictures from the
//! fixture files): the projection draws each picture over its region,
//! in Sixel, iTerm2 or Kitty, or half blocks; an uncaptioned picture gets
//! its `Image:` line; animated GIFs step within the byte budget, and
//! freezing stops their bytes.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::app::{app, Options};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, Images, View};
use noctiluca::testing::FakeEngine;
use rich_intuituive::Driver;

fn snapshots() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn options(url: &str, images: Images) -> Options {
    options_with(url, images, Config::default())
}

fn options_with(url: &str, images: Images, config: Config) -> Options {
    Options {
        config,
        bookmarks: Bookmarks::default(),
        url: Some(url.to_string()),
        view: Some(View::Projection),
        images: Some(images),
    }
}

fn driver(url: &str, images: Images, columns: u16, rows: u16) -> Driver {
    let engine = FakeEngine::from_dir(&snapshots()).unwrap();
    app(Box::new(engine), options(url, images)).driver(columns, rows)
}

/// The news page, tall enough that its animated GIF is in view, with
/// `bandwidth` bytes a second for moving pictures.
fn news_with_gif(images: Images, bandwidth: u64) -> Driver {
    let engine = FakeEngine::from_dir(&snapshots()).unwrap();
    let config = Config {
        bandwidth: Some(bandwidth),
        ..Config::default()
    };
    let mut driver =
        app(Box::new(engine), options_with("news.html", images, config)).driver(100, 90);
    // Every picture drawn, the GIF among them.
    let (_, ready) = run_until(&mut driver, Duration::from_secs(30), |_, screen| {
        screen.contains("Why the tides") && screen.contains("Image: Lightning")
    });
    assert!(ready, "{}", screen(&driver));
    // Its first pictures out: what follows is the GIF's.
    run_until(&mut driver, Duration::from_millis(1500), |_, _| false);
    driver
}

/// Press `name` without drawing, so what it makes the app write is in
/// the next [`written_for`].
fn key(driver: &mut Driver, name: &str) {
    for key in rich_intuituive::interact::keymap::keys(name) {
        driver.event(rich_intuituive::interact::Event::Key(key));
    }
}

/// Everything written over `time`, updating every 10 ms.
fn written_for(driver: &mut Driver, time: Duration) -> String {
    run_until(driver, time, |_, _| false).0
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

/// The real time since the first call: the app's clock, which the
/// driver takes as an absolute time.
fn clock() -> Duration {
    static BORN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    BORN.get_or_init(Instant::now).elapsed()
}

/// Update and render until `done` holds on what was written and the
/// screen, or `deadline` passes; what was written.
fn run_until(
    driver: &mut Driver,
    deadline: Duration,
    done: impl Fn(&str, &str) -> bool,
) -> (String, bool) {
    let start = Instant::now();
    let mut out = String::new();
    while start.elapsed() < deadline {
        // The app's clock follows the real one, so its timers fire.
        driver.update(clock());
        out.push_str(&driver.render().unwrap_or_default());
        if done(&out, &screen(driver)) {
            return (out, true);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    (out, false)
}

#[test]
fn the_projection_draws_each_picture_over_its_region() {
    let mut driver = driver("news.html", Images::Sixel, 100, 40);
    let (out, drawn) = run_until(&mut driver, Duration::from_secs(20), |out, screen| {
        out.contains("\x1bP") && screen.contains('▀')
    });
    assert!(drawn, "no picture drawn:\n{}", screen(&driver));
    // The picture's cells are half blocks under the Sixel; its alt text
    // is under it, not in it.
    let text = screen(&driver);
    let (above, _) = text
        .split_once("Image: Boats at anchor")
        .expect("its caption");
    assert!(!above.contains('░'), "a placeholder left:\n{text}");
    assert!(out.contains("\x1bP"));
}

#[test]
fn an_animated_gif_moves_within_its_budget() {
    let mut driver = news_with_gif(Images::Sixel, 400_000);
    let out = written_for(&mut driver, Duration::from_secs(1));
    // At least two different frames within a second, at most the ceiling.
    let frames: Vec<&str> = out.split("\x1bP").skip(1).collect();
    let distinct: std::collections::HashSet<&str> = frames.iter().copied().collect();
    assert!(distinct.len() >= 2, "{} frames", frames.len());
    assert!(frames.len() <= 9, "{} frames in a second", frames.len());
    // Within its byte budget, counted over two seconds.
    let out = written_for(&mut driver, Duration::from_secs(2));
    assert!(
        out.len() as f64 <= 2.0 * 400_000.0 * 1.25,
        "{} bytes",
        out.len()
    );
}

#[test]
fn freezing_stops_the_bytes() {
    let mut driver = news_with_gif(Images::Sixel, 400_000);
    assert!(written_for(&mut driver, Duration::from_millis(600)).contains("\x1bP"));
    key(&mut driver, "f4");
    written_for(&mut driver, Duration::from_millis(200));
    assert!(screen(&driver).contains("frozen"), "{}", screen(&driver));
    let frozen = written_for(&mut driver, Duration::from_secs(1));
    assert!(!frozen.contains("\x1bP"), "a frame while frozen");
    // And it moves again.
    key(&mut driver, "f4");
    assert!(written_for(&mut driver, Duration::from_secs(1)).contains("\x1bP"));
}

#[test]
fn a_slow_terminal_steps_the_gif_down_to_character_art() {
    // 20 kB a second: too few Sixel frames, so it steps down to half
    // blocks, which still move.
    let mut driver = news_with_gif(Images::Sixel, 20_000);
    written_for(&mut driver, Duration::from_secs(1));
    let out = written_for(&mut driver, Duration::from_secs(2));
    assert!(!out.contains("\x1bP"), "still Sixel");
    // The half blocks move: their colours are written again (at least one
    // frame's worth, however slow the machine running the test).
    assert!(
        out.matches('▀').count() >= 100,
        "{} cells",
        out.matches('▀').count()
    );
    assert!(
        out.len() as f64 <= 2.0 * 20_000.0 * 1.5,
        "{} bytes",
        out.len()
    );
}

#[test]
fn kitty_plays_the_gif_itself_and_a_freeze_sends_its_still() {
    let mut driver = news_with_gif(Images::Kitty, 400_000);
    let mut out = String::new();
    // Its frames went up once, and Kitty loops them: nothing more is sent.
    let quiet = written_for(&mut driver, Duration::from_secs(1));
    out.push_str(&quiet);
    assert!(!quiet.contains("a=f,"), "frames sent again");
    key(&mut driver, "f4");
    let frozen = written_for(&mut driver, Duration::from_millis(500));
    assert!(frozen.contains("a=T,U=1"), "no still sent");
    assert!(!frozen.contains("s=3"), "still animated");
}

#[test]
fn small_pictures_in_a_logs_lines_are_drawn_in_their_cells() {
    // The stream page's log: each message's badge and emotes are small
    // pictures in its line, drawn in half blocks in place of their alt
    // text.
    let mut driver = driver("stream-with-log.html", Images::Ansi, 120, 40);
    let (_, drawn) = run_until(&mut driver, Duration::from_secs(20), |_, screen| {
        screen
            .lines()
            .any(|l| l.contains("the water is glowing") && l.contains('▀'))
    });
    let text = screen(&driver);
    assert!(drawn, "no small picture drawn:\n{text}");
    // A badge in front of its author, also a picture.
    assert!(
        text.lines().any(|l| l.contains("▀ ada")),
        "no badge drawn:\n{text}"
    );
    assert!(!text.contains(":glow:"), "an emote left as text:\n{text}");
}

/// A page of one tall picture, 400 x 600 CSS pixels, with a long alt text
/// and no caption of its own.
fn tall_picture() -> noctiluca::model::Page {
    use noctiluca::engine::Rect;
    use noctiluca::model::{Block, BlockKind, Media, MediaKind, Page};
    let rect = Rect::new(0.0, 0.0, 400.0, 600.0);
    let mut page = Page::default();
    page.media.push(Media {
        node: 1,
        kind: MediaKind::Image,
        rect,
        alt: "A lighthouse on a cliff above a bay where the water glows blue at night".into(),
        src: "lighthouse.jpg".into(),
        still: None,
        picture: None,
        captioned: false,
        playback: None,
        poster: String::new(),
        controls: false,
        autoplay: false,
        empty_alt: false,
        aria_hidden: false,
        in_label: false,
    });
    page.blocks.push(Block {
        kind: BlockKind::Media(0),
        runs: Vec::new(),
        quote: 0,
        rect,
        frame: rect,
        node: 1,
        region: None,
    });
    page
}

#[test]
fn a_capped_pictures_caption_wraps_to_its_box_and_the_rule_switches_it_off() {
    use noctiluca::adapt::{Adapt, Rule};
    use noctiluca::layout::{project, Marks};
    let page = tall_picture();
    let marks = Marks::default();
    let projection = project(&page, 100, (8.0, 18.0), &marks);
    let region = projection.regions[0];
    // Capped: fewer rows, narrowed to keep its shape.
    assert!(region.rows < 600 / 18, "{region:?}");
    assert!(region.cols < 50, "{region:?}");
    let caption: Vec<String> = projection.lines[region.row + region.rows..]
        .iter()
        .map(|l| l.plain().trim_end().to_string())
        .take_while(|l| !l.is_empty())
        .collect();
    assert!(caption[0].starts_with("Image: A lighthouse"), "{caption:?}");
    // Wrapped to the picture's box (50 cells), not to the narrowed picture.
    assert!(
        caption
            .iter()
            .any(|l| rich::cells::cell_len(l) > region.cols),
        "{caption:?}"
    );
    assert!(caption.iter().all(|l| rich::cells::cell_len(l) <= 50));
    // Without the captions rule, no line of ours.
    let mut adapt = Adapt::default();
    adapt.set(Rule::Captions, false);
    let marks = Marks {
        adapt,
        ..Marks::default()
    };
    let plain = project(&page, 100, (8.0, 18.0), &marks);
    assert!(!plain.lines.iter().any(|l| l.plain().contains("Image:")));
}

#[test]
fn a_video_plays_chromes_recorded_frames_in_its_region() {
    // The scripted engine replays Chrome's own frames of motion.webm, as
    // the docs' tapes do: playing, the region's picture changes.
    let engine = FakeEngine::from_dir(&snapshots())
        .unwrap()
        .replaying(&snapshots().join("frames"));
    let mut driver = app(Box::new(engine), options("news.html", Images::Sixel)).driver(100, 40);
    let (_, ready) = run_until(&mut driver, Duration::from_secs(20), |_, screen| {
        screen.contains("PAUSED")
    });
    assert!(ready, "{}", screen(&driver));
    written_for(&mut driver, Duration::from_secs(3));
    // Paused, its poster's play mark is drawn into the pixels (a cell
    // under a Sixel would be covered), so no mark cell shows.
    assert!(
        !screen(&driver).lines().any(|l| l.contains(" ▶ ")),
        "{}",
        screen(&driver)
    );
    key(&mut driver, "p");
    let out = written_for(&mut driver, Duration::from_secs(3));
    assert!(screen(&driver).contains("PLAYING"), "{}", screen(&driver));
    let frames = out.matches("\x1bP").count();
    assert!(frames >= 3, "{frames} frames in three seconds");
}

#[test]
fn a_replayed_video_shows_the_frame_at_its_clock() {
    use noctiluca::engine::{Engine, MediaOp, PageAction, TabId};
    use noctiluca::model::{build, MediaKind, Revisions};
    // Four recorded frames over the five seconds, one colour each.
    let dir = std::env::temp_dir().join(format!("noctiluca-replay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let colours = [[200, 0, 0], [0, 200, 0], [0, 0, 200], [200, 200, 200]];
    for (k, colour) in colours.iter().enumerate() {
        image::RgbImage::from_pixel(32, 18, image::Rgb(*colour))
            .save(dir.join(format!("motion-{k:02}.jpg")))
            .unwrap();
    }
    let mut engine = FakeEngine::from_dir(&snapshots()).unwrap().replaying(&dir);
    engine
        .open(TabId(0), "file:///noctiluca/fixtures/news.html")
        .unwrap();
    let page = build(&engine.snapshot(TabId(0)).unwrap(), &mut Revisions::new());
    let video = page
        .media
        .iter()
        .find(|m| m.kind == MediaKind::Video)
        .unwrap()
        .clone();
    engine.set_screencast(TabId(0), true).unwrap();
    // The colour at the middle of the video's box in the newest frame,
    // and the clock the status line shows.
    let mut seen = |engine: &mut FakeEngine| {
        let frame = engine.screencast_frame(TabId(0)).unwrap();
        let r = video.rect;
        let cut = frame.crop(r.x, r.y, r.width, r.height).unwrap();
        let at = ((cut.height / 2 * cut.width + cut.width / 2) * 3) as usize;
        let rgb = &cut.rgb[at..at + 3];
        let colour = colours
            .iter()
            .position(|c| c.iter().zip(rgb).all(|(a, b)| a.abs_diff(*b) < 40))
            .unwrap_or_else(|| panic!("{rgb:?}"));
        (colour, engine.playback(TabId(0), video.node).unwrap().time)
    };
    let media = |engine: &mut FakeEngine, op| {
        engine
            .act(
                TabId(0),
                &PageAction::Media {
                    node: video.node,
                    op,
                },
            )
            .unwrap();
    };
    // Each second of the clock shows the frame recorded at it: 2.6 s is
    // the third of four frames over five seconds (a replay at a fixed
    // eight frames a second showed the first, at frame 20 of a loop of 4).
    media(&mut engine, MediaOp::Seek(2.6));
    media(&mut engine, MediaOp::Play);
    let (colour, time) = seen(&mut engine);
    assert_eq!(colour, 2, "at {time}");
    assert!((2.6..2.9).contains(&time), "{time}");
    media(&mut engine, MediaOp::Pause);
    media(&mut engine, MediaOp::Seek(1.4));
    let held = engine.playback(TabId(0), video.node).unwrap().time;
    assert!((4.0..4.3).contains(&held), "{held}");
    media(&mut engine, MediaOp::Play);
    let (colour, time) = seen(&mut engine);
    assert_eq!(colour, 3, "at {time}");
    std::fs::remove_dir_all(&dir).ok();
}
