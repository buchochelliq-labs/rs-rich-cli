//! Save Chrome's snapshot and accessibility tree of fixture pages for the
//! model's browserless tests: `cargo run -p noctiluca --example capture --
//! OUT_DIR PAGE.html ...` (the browser from `RICH_EMBED_CHROME`, else
//! found; run as a normal user). Each page is saved as `OUT_DIR/NAME.json`,
//! `{"dom": …, "ax": …}`. With `--cost`, prints instead what each reading
//! costs: the median of five snapshots and five AX trees, and their sizes.
//! `--columns N` lays the pages out N cells wide (100 by default).
//! `--frames OUT_DIR PAGE.html` records instead the frames of the page's
//! first video as Chrome paints them, playing, cut to its box:
//! `OUT_DIR/STEM-NN.jpg`, STEM the video file's name without its
//! extension, twelve frames an eighth of a second apart. The scripted
//! engine plays them for a video with that source in the docs' tapes.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::engine::{Engine, TabId};
use noctiluca::file_url;
use serde_json::json;

fn median(mut times: Vec<Duration>) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

/// Record the first video's frames of `page` into `out`.
fn frames(out: &std::path::Path, page: &std::path::Path) -> std::io::Result<()> {
    use noctiluca::engine::{MediaOp, PageAction};
    use noctiluca::model::{build, MediaKind, Revisions};
    let mut engine = ChromeBackend::start(ChromeOptions {
        binary: std::env::var_os("RICH_EMBED_CHROME").map(PathBuf::from),
        columns: 100,
        rows: 40,
        ..ChromeOptions::default()
    })?;
    engine.open(TabId(0), &file_url(page)?)?;
    let start = Instant::now();
    let video = loop {
        std::thread::sleep(Duration::from_millis(200));
        if let Ok(snapshot) = engine.snapshot(TabId(0)) {
            let model = build(&snapshot, &mut Revisions::new());
            if let Some(v) = model.media.iter().find(|m| m.kind == MediaKind::Video) {
                if engine
                    .playback(TabId(0), v.node)
                    .is_ok_and(|p| p.ready >= 2)
                {
                    break v.clone();
                }
            }
        }
        if start.elapsed() > Duration::from_secs(30) {
            return Err(std::io::Error::other("no video ready"));
        }
    };
    let stem = video
        .src
        .rsplit('/')
        .next()
        .and_then(|name| name.split('.').next())
        .unwrap_or("video")
        .to_string();
    engine.act(
        TabId(0),
        &PageAction::Media {
            node: video.node,
            op: MediaOp::Play,
        },
    )?;
    engine.set_screencast(TabId(0), true)?;
    std::fs::create_dir_all(out)?;
    let mut saved = 0;
    let mut next = Instant::now();
    while saved < 12 && start.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(10));
        let Some(frame) = engine.screencast_frame(TabId(0)) else {
            continue;
        };
        if Instant::now() < next {
            continue;
        }
        let r = video.rect;
        let Some(cut) = frame.crop(r.x, r.y, r.width, r.height) else {
            continue;
        };
        let image = image::RgbImage::from_raw(cut.width, cut.height, cut.rgb)
            .ok_or_else(|| std::io::Error::other("a frame of the wrong size"))?;
        let path = out.join(format!("{stem}-{saved:02}.jpg"));
        let mut file = std::fs::File::create(&path)?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 70)
            .encode_image(&image)
            .map_err(std::io::Error::other)?;
        saved += 1;
        next = Instant::now() + Duration::from_millis(125);
    }
    println!("{saved} frames of {stem} in {}", out.display());
    Ok(())
}

fn main() -> std::io::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--frames") {
        return frames(&PathBuf::from(&args[1]), &PathBuf::from(&args[2]));
    }
    let cost = args.first().is_some_and(|a| a == "--cost");
    if cost {
        args.remove(0);
    }
    let mut columns = 100;
    if args.first().is_some_and(|a| a == "--columns") {
        args.remove(0);
        columns = args.remove(0).parse().unwrap_or(100);
    }
    let out = PathBuf::from(if cost {
        ".".to_string()
    } else {
        args.remove(0)
    });
    let mut engine = ChromeBackend::start(ChromeOptions {
        binary: std::env::var_os("RICH_EMBED_CHROME").map(PathBuf::from),
        columns,
        rows: 40,
        ..ChromeOptions::default()
    })?;
    if cost {
        println!("page | snapshot ms | DOM nodes | layout boxes | snapshot KiB | AX ms | AX nodes | AX KiB");
    }
    for page in args {
        let page = PathBuf::from(page);
        let url = file_url(&page)?;
        engine.open(TabId(0), &url)?;
        let start = Instant::now();
        let snapshot = loop {
            if let Ok(snapshot) = engine.snapshot(TabId(0)) {
                let strings = &snapshot.dom["strings"];
                let at = snapshot.dom["documents"][0]["documentURL"]
                    .as_u64()
                    .unwrap_or(0);
                if !engine.state(TabId(0)).loading && strings[at as usize] == url.as_str() {
                    break snapshot;
                }
            }
            if start.elapsed() > Duration::from_secs(60) {
                return Err(std::io::Error::other(format!("{url} did not load")));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        // Let the page's own script finish.
        std::thread::sleep(Duration::from_millis(300));
        let name = page
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !cost {
            let snapshot = engine.snapshot(TabId(0))?;
            let all = json!({"dom": snapshot.dom, "ax": snapshot.ax});
            std::fs::write(out.join(format!("{name}.json")), all.to_string())?;
            continue;
        }
        let (mut snap, mut ax) = (Vec::new(), Vec::new());
        let devtools = engine.devtools().clone();
        let mut last = snapshot;
        for _ in 0..5 {
            let start = Instant::now();
            last.dom = devtools.call(
                "DOMSnapshot.captureSnapshot",
                json!({"computedStyles": noctiluca::cdp::SNAPSHOT_STYLES,
                       "includeDOMRects": true, "includePaintOrder": true}),
                Duration::from_secs(30),
            )?;
            snap.push(start.elapsed());
            let start = Instant::now();
            last.ax = devtools.call(
                "Accessibility.getFullAXTree",
                json!({}),
                Duration::from_secs(30),
            )?;
            ax.push(start.elapsed());
        }
        let count = |v: &serde_json::Value| v.as_array().map_or(0, Vec::len);
        let document = &last.dom["documents"][0];
        println!(
            "{name} | {:.1} | {} | {} | {} | {:.1} | {} | {}",
            median(snap),
            count(&document["nodes"]["parentIndex"]),
            count(&document["layout"]["nodeIndex"]),
            last.dom.to_string().len() / 1024,
            median(ax),
            count(&last.ax["nodes"]),
            last.ax.to_string().len() / 1024,
        );
    }
    Ok(())
}
