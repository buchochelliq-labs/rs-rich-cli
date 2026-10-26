//! The Frame path's cost, for a profiler: a 200x60 screen of styled text
//! built into a `Frame` and split into cells, as `Screen::write_lines`
//! does for text it cannot write directly and as a graphics plan does for
//! the whole screen each frame.
//!
//!     cargo test --release -p rs-rich-ext --test frame_path -- --ignored --nocapture

use std::time::Instant;

use rich::{Segment, Style};
use rich_ext::frame::Frame;

fn screen() -> Vec<Segment> {
    let styles = [
        None,
        Style::parse("bold cyan").ok(),
        Style::parse("on #202020").ok(),
    ];
    let words = [
        "status",
        "ok",
        "12:00:01",
        "svc-03",
        "héllo",
        "日本",
        "─────",
    ];
    let mut segments = Vec::new();
    for row in 0..60 {
        let mut width = 0;
        let mut i = row;
        while width < 190 {
            let word = words[i % words.len()];
            segments.push(Segment::new(format!("{word} "), styles[i % 3].clone()));
            width += rich::cells::cell_len(word) + 1;
            i += 1;
        }
        segments.push(Segment::line());
    }
    segments
}

#[test]
#[ignore = "a benchmark: run it in release with --ignored"]
fn frame_path_cost() {
    let segments = screen();
    let frames = 200;
    let start = Instant::now();
    let mut cells = 0;
    for _ in 0..frames {
        let frame = Frame::from_segments(&segments);
        for row in 0..frame.height() {
            cells += frame.cells(row).len();
        }
    }
    let per = start.elapsed().as_secs_f64() * 1e6 / frames as f64;
    println!("Frame::from_segments and cells, 200x60: {per:.1} µs a frame ({cells} cells)");
}
