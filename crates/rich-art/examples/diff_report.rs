//! Print a [`rich_art::imagediff`] report as JSON, for comparison against the
//! Python oracle (`scripts/image_diff.py` on the branch of PR #41).
//!
//! ```bash
//! cargo run -p rs-rich-art --features image --example diff_report -- before.png after.png
//! ```
//!
//! The field names and rounding deliberately match the oracle's `*.json`
//! sidecar so the two can be diffed directly.

use rich_art::imagediff::{diff, DiffSettings};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: diff_report <before> <after> [blur] [threshold] [open_kernel]");
        std::process::exit(2);
    }

    let settings = DiffSettings {
        blur: args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6.0),
        threshold: args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60.0),
        open_kernel: args.get(4).and_then(|s| s.parse().ok()).unwrap_or(11),
        ..DiffSettings::default()
    };

    let before = image::open(&args[0]).unwrap_or_else(|e| {
        eprintln!("cannot open {}: {e}", args[0]);
        std::process::exit(1);
    });
    let after = image::open(&args[1]).unwrap_or_else(|e| {
        eprintln!("cannot open {}: {e}", args[1]);
        std::process::exit(1);
    });

    let report = match diff(&before, &after, &settings) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    // Hand-rolled JSON: this example exists to be diffed against the oracle's
    // output, and pulling in serde for one debug helper is not worth it.
    println!("{{");
    println!("  \"size\": [{}, {}],", report.width, report.height);
    println!("  \"settings\": {{");
    println!("    \"blur\": {},", settings.blur);
    println!("    \"delta_e_threshold\": {},", settings.threshold);
    println!("    \"open_kernel\": {}", settings.open_kernel);
    println!("  }},");
    println!("  \"changed_fraction\": {:.4},", report.changed_fraction);
    println!(
        "  \"naive_changed_fraction\": {:.4},",
        report.naive_changed_fraction
    );
    println!("  \"mean_delta_e\": {:.2},", report.mean_delta_e);
    println!("  \"max_delta_e\": {:.2},", report.max_delta_e);
    println!("  \"regions\": [");
    for (i, r) in report.regions.iter().enumerate() {
        let comma = if i + 1 == report.regions.len() {
            ""
        } else {
            ","
        };
        println!("    {{");
        println!("      \"x\": {},", r.x);
        println!("      \"y\": {},", r.y);
        println!("      \"width\": {},", r.width);
        println!("      \"height\": {},", r.height);
        println!("      \"area_px\": {},", r.area_px);
        println!("      \"share_of_change\": {:.4},", r.share_of_change);
        println!("      \"mean_delta_e\": {:.1}", r.mean_delta_e);
        println!("    }}{comma}");
    }
    println!("  ]");
    println!("}}");
}
