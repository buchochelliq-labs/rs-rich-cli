//! Video against a real Chrome: the PoC's `motion.webm` (a silent five
//! seconds of a moving test pattern) in `video.html`. Runs when
//! `RICH_EMBED_CHROME` names a browser, as a normal user.
//!
//! - its poster comes as the bytes the page loaded;
//! - it plays, pauses and seeks through the tab's media commands, its
//!   `currentTime` moves, and the frames cut from the screencast change;
//! - a seek while paused brings one new frame;
//! - fed to rs-rich-art's pacing, a region's bytes over two seconds stay
//!   under its share of the budget, and a slow terminal steps it down to
//!   character art;
//! - `about:noctiluca`'s codec table says what `canPlayType` says.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use noctiluca::about;
use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::engine::{Engine, MediaOp, NodeId, Playback, TabId};
use noctiluca::file_url;
use noctiluca::model::{BlockKind, Page};
use noctiluca::tab::{Command, Tab, Update};
use noctiluca::video::{Pacing, VideoFrame};
use rich_art::picture::{count_picture_bytes, picture_bytes, ByteBudget, Pixels};

fn fixture(name: &str) -> String {
    file_url(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

fn start() -> Option<ChromeBackend> {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return None;
    };
    Some(
        ChromeBackend::start(ChromeOptions {
            binary: Some(binary.into()),
            columns: 100,
            rows: 40,
            ..ChromeOptions::default()
        })
        .unwrap(),
    )
}

/// The tab's updates: the newest page, and every frame.
struct Watch {
    tab: Tab,
    updates: Receiver<Update>,
    page: Option<Arc<Page>>,
    frames: Vec<Arc<VideoFrame>>,
}

impl Watch {
    fn new(engine: ChromeBackend) -> Watch {
        let (send, updates) = mpsc::channel();
        let tab = Tab::spawn(Box::new(engine), move |u| {
            let _ = send.send(u);
        });
        Watch {
            tab,
            updates,
            page: None,
            frames: Vec::new(),
        }
    }

    fn pump(&mut self, for_: Duration) {
        let end = Instant::now() + for_;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match self.updates.recv_timeout(left.min(Duration::from_millis(50))) {
                Ok(Update::Page(p)) => self.page = Some(p),
                Ok(Update::Frame(f)) => self.frames.push(f),
                _ => {}
            }
        }
    }

    fn until(&mut self, what: &str, ok: impl Fn(&Watch) -> bool) {
        let start = Instant::now();
        while !ok(self) {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "timed out waiting for {what}: {:?}",
                self.playback()
            );
            self.pump(Duration::from_millis(50));
        }
    }

    fn video(&self) -> Option<&noctiluca::model::Media> {
        self.page.as_ref()?.media.first()
    }

    fn playback(&self) -> Option<Playback> {
        self.video()?.playback
    }

    fn node(&self) -> NodeId {
        self.video().unwrap().node
    }

    fn media(&self, op: MediaOp) {
        let page = self.page.as_ref().unwrap();
        self.tab.send(Command::Media {
            document: page.document,
            node: page.media[0].node,
            op,
        });
    }
}

#[test]
fn motion_plays_pauses_and_seeks_and_its_frames_change() {
    let Some(engine) = start() else { return };
    let mut w = Watch::new(engine);
    let url = fixture("video.html");
    w.tab.send(Command::Open(url.clone()));
    w.until("the video, ready", |w| {
        w.page.as_ref().is_some_and(|p| p.url == url)
            && w.playback().is_some_and(|p| p.ready >= 2 && p.duration.is_some())
    });
    let video = w.video().unwrap().clone();
    assert_eq!(video.poster, fixture("glow.png"));
    // The poster is the bytes of the page's own picture.
    w.until("the poster", |w| w.video().is_some_and(|v| v.still.is_some()));
    let glow = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/glow.png"))
        .unwrap();
    assert_eq!(&*w.video().unwrap().still.clone().unwrap(), &glow[..]);
    assert_eq!(w.playback().unwrap().duration, Some(5.0));
    assert_eq!(w.playback().unwrap().label(), "PAUSED");

    // Off screen: playing sends no frames.
    w.media(MediaOp::Play);
    w.until("playing", |w| w.playback().is_some_and(|p| p.time > 0.2));
    w.pump(Duration::from_millis(600));
    assert!(w.frames.is_empty(), "frames of a region off screen");

    // On screen: frames, which change as the clock moves.
    w.tab.send(Command::Regions {
        on_screen: vec![w.node()],
        intervals: Vec::new(),
    });
    w.until("frames", |w| w.frames.len() >= 4);
    let first = w.frames.first().unwrap().clone();
    let last = w.frames.last().unwrap().clone();
    assert_eq!(first.node, w.node());
    // The region's own size, cut from the screencast.
    assert_eq!((first.pixels.width, first.pixels.height), (320, 180));
    assert!(last.seq > first.seq);
    assert_ne!(first.pixels.rgb, last.pixels.rgb, "the frames did not change");
    // At most eight a second.
    let count = w.frames.len();
    w.pump(Duration::from_secs(2));
    let in_two = w.frames.len() - count;
    assert!((2..=17).contains(&in_two), "{in_two} frames in two seconds");

    // Paused: the clock holds, and frames stop.
    w.media(MediaOp::Pause);
    w.until("paused", |w| w.playback().is_some_and(|p| p.paused));
    w.pump(Duration::from_millis(400));
    let held = w.playback().unwrap().time;
    let frames = w.frames.len();
    w.pump(Duration::from_millis(1200));
    assert_eq!(w.playback().unwrap().time, held);
    assert!(w.frames.len() <= frames + 1, "frames while paused");

    // A seek while paused: the time moves, it stays paused, and one new
    // frame shows where it went.
    let before = w.frames.last().unwrap().clone();
    w.media(MediaOp::Seek(-100.0));
    w.until("at the start", |w| w.playback().is_some_and(|p| p.time == 0.0));
    w.media(MediaOp::Seek(2.5));
    w.until("at 2.5 s", |w| {
        w.playback().is_some_and(|p| (p.time - 2.5).abs() < 0.01)
    });
    assert!(w.playback().unwrap().paused);
    w.until("a frame of the seek", |w| {
        w.frames.last().is_some_and(|f| f.pixels.rgb != before.pixels.rgb)
    });
    w.media(MediaOp::Seek(100.0));
    w.until("at the end", |w| w.playback().is_some_and(|p| p.time == 5.0));

    // Mute.
    w.media(MediaOp::Mute(true));
    w.until("muted", |w| w.playback().is_some_and(|p| p.muted));
}

/// Frames from a playing `motion.webm`, fed to `pacing` as the app does,
/// each drawn picture's escapes counted as the painter counts them.
/// Returns the region's node and the pacing's two-second byte count.
fn pace(pacing: &mut Pacing, seconds: f64) -> (NodeId, u64, Vec<u64>) {
    let engine = start().unwrap();
    let mut w = Watch::new(engine);
    let url = fixture("video.html");
    w.tab.send(Command::Open(url.clone()));
    w.until("the video, ready", |w| {
        w.page.as_ref().is_some_and(|p| p.url == url) && w.playback().is_some_and(|p| p.ready >= 2)
    });
    w.tab.send(Command::Regions {
        on_screen: vec![w.node()],
        intervals: Vec::new(),
    });
    w.media(MediaOp::Play);
    let node = w.node();
    let (cols, rows) = (40, 10);
    let mut window: Vec<(Instant, u64)> = Vec::new();
    let mut peaks = Vec::new();
    let end = Instant::now() + Duration::from_secs_f64(seconds);
    let mut seen = 0;
    while Instant::now() < end {
        w.pump(Duration::from_millis(20));
        for frame in &w.frames[seen..] {
            let now = Instant::now();
            if let Some(picture) = pacing.offer(frame, cols, rows, now, picture_bytes) {
                // The painter: the picture's escapes in its protocol, or
                // its cells, counted under its id.
                let pixels = pacing.pixels(node);
                let bytes = match picture.encode(pixels, cols, rows) {
                    Some(escape) => escape.len() as u64,
                    None => {
                        let console = rich::Console::new();
                        let view = rich_art::picture::ImageView::new().pixels(pixels);
                        picture
                            .lines(&console, view, cols as u16, rows as u16)
                            .iter()
                            .flatten()
                            .map(|s| s.text.len() as u64 + 20)
                            .sum()
                    }
                };
                count_picture_bytes(picture.id(), bytes);
                window.push((now, bytes));
                window.retain(|(at, _)| now.duration_since(*at) <= Duration::from_secs(2));
                peaks.push(window.iter().map(|(_, b)| b).sum());
            }
        }
        seen = w.frames.len();
    }
    (node, pacing.recent_bytes(node), peaks)
}

#[test]
fn a_regions_bytes_over_two_seconds_stay_under_its_cap() {
    if std::env::var_os("RICH_EMBED_CHROME").is_none() {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    }
    // 400 kB a second, Kitty: the pacer learns what a frame costs.
    let budget = ByteBudget::new(400_000);
    let mut pacing = Pacing::new(budget, Pixels::Kitty);
    let (node, _, peaks) = pace(&mut pacing, 4.0);
    let cap = pacing.cap() * 2.0;
    assert!(!peaks.is_empty(), "no frame drawn");
    // After the first frame (its cost was a guess), every two-second
    // window stays under the cap.
    let worst = peaks.iter().skip(1).copied().max().unwrap_or(0);
    assert!(
        (worst as f64) <= cap,
        "{worst} bytes in two seconds over a cap of {cap} ({:?})",
        pacing.pixels(node)
    );
}

#[test]
fn a_slow_terminal_steps_video_down_to_character_art() {
    if std::env::var_os("RICH_EMBED_CHROME").is_none() {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    }
    // 20 kB a second: fewer than two Kitty, iTerm2 or Sixel frames of a
    // 40 x 10 region a second.
    let mut pacing = Pacing::new(ByteBudget::new(20_000), Pixels::Kitty);
    let (node, _, peaks) = pace(&mut pacing, 2.0);
    assert_eq!(pacing.pixels(node), Pixels::Cells);
    assert!(!peaks.is_empty(), "no frame drawn");
}

#[test]
fn the_about_pages_codec_table_matches_can_play_type() {
    let Some(mut engine) = start() else { return };
    let answers = engine.can_play(TabId(0), &about::types()).unwrap();
    // Chrome plays VP9 in WebM, whatever its build.
    assert_eq!(answers[4], "probably", "{answers:?}");
    let mut w = Watch::new(engine);
    w.tab.send(Command::Open(about::ADDRESS.into()));
    w.until("the about page", |w| {
        w.page.as_ref().is_some_and(|p| p.title == about::TITLE)
    });
    let page = w.page.clone().unwrap();
    assert!(about::is_about(&page.url, &page.title));
    let table = page
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Table(t) => Some(t.clone()),
            _ => None,
        })
        .expect("a codec table");
    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|r| r.iter().map(|c| c.text()).collect())
        .collect();
    // The header, then one row a codec, each saying what Chrome said.
    assert_eq!(rows.len(), about::CODECS.len() + 1, "{rows:?}");
    for (((name, kind), answer), row) in about::CODECS.iter().zip(&answers).zip(&rows[1..]) {
        assert_eq!(row[0], *name);
        assert_eq!(row[1], *kind);
        assert_eq!(row[2], about::verdict(answer), "{name}");
    }
}
