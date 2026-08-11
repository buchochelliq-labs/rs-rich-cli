//! Parity of [`rich_art::imagediff`] against the Python oracle.
//!
//! The oracle is `scripts/image_diff.py` on the branch of PR #41, which is
//! deliberately never merged — the same arrangement as `capture_golden.py` for
//! render parity. `tests/fixtures/halo-expected.json` records the oracle's
//! output for this exact input at these exact settings, and where those numbers
//! came from.
//!
//! # Why this is a tolerance test and not byte-parity
//!
//! Every stage is reproduced exactly except one: PIL's `ImageFilter.GaussianBlur`
//! is **not** a true Gaussian — it approximates one with three box-blur passes.
//! `imagediff` uses a real separable Gaussian, so blurred pixel values differ
//! slightly, which moves the ΔE threshold boundary by a pixel or two and
//! therefore nudges region edges and areas.
//!
//! Measured on the full-size 1254×1254 pair: identical region count and rank
//! order, positions within 1–3 px, areas within 0.1–2.1%, mean ΔE identical to
//! one decimal place, and `naive_changed_fraction` exact to four.
//!
//! The tolerances below are set just wide enough for that, so a real regression
//! — a wrong connectivity rule, a wrong `share_of_change` denominator, a
//! dropped morphological pass — still fails. Closing the gap entirely means
//! porting PIL's box approximation; until then this pins the agreement rather
//! than pretending it is exact.
#![cfg(feature = "image")]

use rich_art::imagediff::{diff, DiffSettings};

fn fixture(name: &str) -> image::DynamicImage {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    image::open(&path).unwrap_or_else(|e| panic!("cannot open fixture {}: {e}", path.display()))
}

/// Expected values are the oracle's, recorded in `tests/fixtures/halo-expected.json`.
#[test]
fn matches_the_python_oracle_on_the_halo_pair() {
    let before = fixture("halo-before.png");
    let after = fixture("halo-after.png");

    let settings = DiffSettings {
        blur: 6.0,
        threshold: 60.0,
        open_kernel: 11,
        min_region: 400,
        top: 3,
    };
    let report = diff(&before, &after, &settings).expect("fixtures are the same size");

    assert_eq!((report.width, report.height), (418, 418));

    // Exact: a byte-level comparison, so the blur cannot affect it. If this
    // drifts, the pixel comparison itself is wrong.
    assert!(
        (report.naive_changed_fraction - 0.4221).abs() < 5e-4,
        "naive_changed_fraction was {} (oracle: 0.4221)",
        report.naive_changed_fraction
    );

    assert!(
        (report.changed_fraction - 0.0532).abs() < 3e-3,
        "changed_fraction was {} (oracle: 0.0532)",
        report.changed_fraction
    );
    assert!(
        (report.mean_delta_e - 17.75).abs() < 0.2,
        "mean_delta_e was {} (oracle: 17.75)",
        report.mean_delta_e
    );
    assert!(
        (report.max_delta_e - 124.9).abs() < 1.0,
        "max_delta_e was {} (oracle: 124.9)",
        report.max_delta_e
    );

    // The oracle finds exactly one region above `min_region` at this scale.
    // A wrong connectivity rule or a dropped open would change this count.
    assert_eq!(
        report.regions.len(),
        1,
        "expected one region, got {:#?}",
        report.regions
    );

    let r = &report.regions[0];
    assert!((r.x as i64 - 95).abs() <= 3, "x was {} (oracle: 95)", r.x);
    assert!((r.y as i64 - 24).abs() <= 3, "y was {} (oracle: 24)", r.y);
    assert!(
        (r.width as i64 - 174).abs() <= 4,
        "width was {} (oracle: 174)",
        r.width
    );
    assert!(
        (r.height as i64 - 71).abs() <= 4,
        "height was {} (oracle: 71)",
        r.height
    );

    let area_drift = (r.area_px as f64 - 8482.0).abs() / 8482.0;
    assert!(
        area_drift < 0.03,
        "area_px was {} (oracle: 8482, {:.1}% off)",
        r.area_px,
        area_drift * 100.0
    );

    // Mean ΔE is an average over many pixels, so it is far less sensitive to
    // the blur than the boundary is — a loose result here means the colour
    // conversion is wrong, not the blur.
    assert!(
        (r.mean_delta_e - 93.6).abs() < 0.5,
        "mean_delta_e was {} (oracle: 93.6)",
        r.mean_delta_e
    );

    // One region holds essentially all the change; the denominator includes
    // components dropped for being too small, so this is just under 1.0.
    assert!(
        (r.share_of_change - 0.9707).abs() < 0.01,
        "share_of_change was {} (oracle: 0.9707)",
        r.share_of_change
    );
}

/// The pipeline's whole justification, asserted rather than claimed: a byte
/// comparison calls 42% of this canvas changed, the perceptual one 5% — a
/// factor of ~7.9 on this fixture.
///
/// The bound is 5× rather than the measured 7.9× so that ordinary drift in the
/// blur does not fail it, while a pipeline that had stopped denoising at all
/// (which would collapse the ratio towards 1) still would.
#[test]
fn the_perceptual_diff_is_far_quieter_than_a_byte_diff() {
    let report = diff(
        &fixture("halo-before.png"),
        &fixture("halo-after.png"),
        &DiffSettings::default(),
    )
    .unwrap();

    let ratio = report.naive_changed_fraction / report.changed_fraction;
    assert!(
        ratio > 5.0,
        "expected the byte diff to be far noisier, ratio was {ratio:.2} \
         (naive {} vs perceptual {})",
        report.naive_changed_fraction,
        report.changed_fraction
    );
}
