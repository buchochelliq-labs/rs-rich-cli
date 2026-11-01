#![cfg(feature = "avif")]
//! AVIF decoding, behind the `avif` feature (libdav1d): a still AVIF made
//! from `halo-32.png` by `tests/fixtures/make_avif.sh`, at ffmpeg's best quality in 4:4:4,
//! read by content and drawn as art.
use std::path::{Path, PathBuf};

use rich_art::ImageArt;
use rich_art::image::{self, ImageFormat};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn a_still_avif_decodes_by_content_like_the_png_it_was_made_from() {
    let bytes = std::fs::read(fixture("halo-32.avif")).unwrap();
    assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Avif);
    // `reading_enabled` answers for `image`'s AVIF encoder feature, not
    // the decoder's: it is not asked.
    let avif = image::load_from_memory(&bytes).unwrap().to_rgb8();
    let png = image::open(fixture("halo-32.png")).unwrap().to_rgb8();
    assert_eq!(avif.dimensions(), png.dimensions());
    // AV1 through YUV: close to the PNG, not equal to it.
    let (mut total, mut worst) = (0u64, 0u8);
    for (a, b) in avif.pixels().zip(png.pixels()) {
        for (x, y) in a.0.iter().zip(b.0) {
            let d = x.abs_diff(y);
            total += u64::from(d);
            worst = worst.max(d);
        }
    }
    let mean = total as f64 / (avif.len() as f64);
    assert!(mean < 4.0 && worst < 32, "mean {mean}, worst {worst}");
    let art = ImageArt::from_bytes(&bytes).unwrap();
    let console = rich::Console::builder()
        .width(16)
        .force_terminal(false)
        .color_system(None)
        .build();
    assert!(
        !console
            .render_lines(&art, &console.options(), false)
            .is_empty()
    );
}
