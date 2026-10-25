//! What reading a page costs: `cargo run -p noctiluca --example
//! snapshot_cost -- PAGE.html ...` (the browser from `RICH_EMBED_CHROME`,
//! else found). For each page, the median of five
//! `DOMSnapshot.captureSnapshot` calls (styles, rectangles, paint order)
//! and five `Accessibility.getFullAXTree` calls, with their sizes.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use noctiluca::{blocks, file_url, Reader};
use serde_json::json;

fn median(mut times: Vec<Duration>) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

fn main() -> std::io::Result<()> {
    let chrome = std::env::var_os("RICH_EMBED_CHROME").map(PathBuf::from);
    let mut reader = Reader::new(chrome, 100, 40)?;
    println!("page | snapshot ms | nodes | layout boxes | snapshot KiB | AX ms | AX nodes | AX KiB | blocks");
    for page in std::env::args_os().skip(1) {
        let page = PathBuf::from(page);
        reader.load(&file_url(&page)?, Duration::from_secs(60))?;
        let (mut snap_times, mut ax_times) = (Vec::new(), Vec::new());
        let (mut snapshot, mut ax) = (json!(null), json!(null));
        for _ in 0..5 {
            let start = Instant::now();
            snapshot = reader.snapshot()?;
            snap_times.push(start.elapsed());
            let start = Instant::now();
            ax = reader.devtools().call(
                "Accessibility.getFullAXTree",
                json!({}),
                Duration::from_secs(30),
            )?;
            ax_times.push(start.elapsed());
        }
        let document = &snapshot["documents"][0];
        let count = |v: &serde_json::Value| v.as_array().map_or(0, Vec::len);
        println!(
            "{} | {:.1} | {} | {} | {} | {:.1} | {} | {} | {}",
            page.file_name().unwrap_or_default().to_string_lossy(),
            median(snap_times),
            count(&document["nodes"]["parentIndex"]),
            count(&document["layout"]["nodeIndex"]),
            snapshot.to_string().len() / 1024,
            median(ax_times),
            count(&ax["nodes"]),
            ax.to_string().len() / 1024,
            blocks(&snapshot).len(),
        );
    }
    Ok(())
}
