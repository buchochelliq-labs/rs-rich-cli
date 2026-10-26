//! A measurement, run by hand: what deflate (level 1, one stream per
//! frame, as the page's `DecompressionStream("deflate-raw")` reads it)
//! does to each frame of a few scenes, for the xterm.js stream and the DOM
//! renderer's JSON: bytes before and after, and the time to encode.
//!
//! ```sh
//! cargo test -p rs-rich-web --test compress_measure \
//!   --config 'profile.dev.package.miniz_oxide.opt-level=3' \
//!   --config 'profile.dev.package.flate2.opt-level=3' -- --ignored --nocapture
//! ```
#![cfg(feature = "server")]

use std::time::{Duration, Instant};

use rich_web::dom::Dom;
use rich_web::intuituive::prelude::*;
use rich_web::intuituive::Driver;

fn deflate(bytes: &[u8]) -> (usize, Duration) {
    use std::io::Write;
    let start = Instant::now();
    let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(1));
    z.write_all(bytes).unwrap();
    let out = z.finish().unwrap();
    (out.len(), start.elapsed())
}

/// Frames of `app` at `size`, each step pressing `key` (or only moving the
/// clock): the xterm.js bytes and the DOM messages of each.
fn frames(
    app: impl Fn() -> App,
    size: (u16, u16),
    key: &str,
    n: usize,
) -> (Vec<String>, Vec<String>) {
    let run = |dom: bool| {
        let mut driver: Driver = app().driver(size.0, size.1);
        let mut d = Dom::new();
        let mut out = Vec::new();
        for step in 0..=n {
            if step > 0 && !key.is_empty() {
                driver.event(rich_web::intuituive::interact::Event::Key(
                    rich_web::intuituive::interact::Key::parse(key).unwrap(),
                ));
            }
            driver.update(Duration::from_millis(50 * step as u64));
            let frame = driver.render().unwrap_or_default();
            if dom {
                out.extend(d.update(&driver));
            } else if !frame.is_empty() {
                out.push(frame);
            }
        }
        out
    };
    (run(false), run(true))
}

fn report(scene: &str, kind: &str, frames: &[String]) {
    let (mut raw, mut z, mut time, mut big) = (0usize, 0usize, Duration::ZERO, 0usize);
    let (mut raw_big, mut z_big) = (0usize, 0usize);
    for f in frames {
        let (n, t) = deflate(f.as_bytes());
        raw += f.len();
        z += n;
        time += t;
        if f.len() >= 1024 {
            big += 1;
            raw_big += f.len();
            z_big += n;
        }
    }
    let count = frames.len().max(1);
    eprintln!(
        "{scene:<22} {kind:<5} {count:>4} frames  {:>7} B/frame -> {:>6} B ({:>3}%)  {:>6.1} µs/frame  | >=1 KiB: {big} frames, {}%",
        raw / count,
        z / count,
        z * 100 / raw.max(1),
        time.as_secs_f64() * 1e6 / count as f64,
        z_big * 100 / raw_big.max(1),
    );
}

#[test]
#[ignore]
fn deflate_on_a_few_scenes() {
    let churn = || {
        App::new(|| {
            let t = signal(0u64);
            every(Duration::from_millis(50), move |_| t.update(|t| *t += 1));
            text(move || {
                let t = t.get();
                let mut s = String::new();
                for y in 0..24u64 {
                    for x in 0..80u64 {
                        let c = (x * 7 + y * 13 + t * 31) % 216;
                        s.push_str(&format!("[on color({})] [/]", 16 + c));
                    }
                    s.push('\n');
                }
                s
            })
        })
    };
    let list_app = || {
        App::new(|| {
            let picked = signal(0usize);
            list(
                || {
                    (0..2000)
                        .map(|i| {
                            format!("[bold]{i:>5}[/] file-{i}.txt  [dim]{} KiB[/]", i * 7 % 900)
                        })
                        .collect()
                },
                picked,
            )
        })
    };
    let log_app = || {
        App::new(|| {
            let lines = signal(Vec::<String>::new());
            every(Duration::from_millis(50), move |_| {
                lines.update(|l| {
                    let n = l.len();
                    l.push(format!(
                        "[green]INFO[/] request {n} served in {} ms",
                        n % 37
                    ))
                })
            });
            scroll(text(move || lines.get().join("\n")))
        })
    };
    for (scene, (x, d)) in [
        ("churn 80x24", frames(churn, (80, 24), "", 40)),
        (
            "list, a row a frame",
            frames(list_app, (80, 24), "down", 40),
        ),
        (
            "list, a page a frame",
            frames(list_app, (80, 24), "pagedown", 40),
        ),
        ("log append", frames(log_app, (80, 24), "", 40)),
        (
            "list 200x60, pages",
            frames(list_app, (200, 60), "pagedown", 40),
        ),
    ] {
        report(scene, "xterm", &x);
        report(scene, "dom", &d);
    }
}
