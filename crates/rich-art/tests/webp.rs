#![cfg(feature = "image")]
//! WebP decoding: a still and an animated WebP, made from `ball.gif` by
//! `tests/fixtures/make_webp.py` (lossless), read by content and by
//! extension, and an animation read like the GIF it was made from.
use std::path::{Path, PathBuf};

use rich_art::graphics::{decode_animation, FrameBudget};
use rich_art::image::{self, ImageFormat, RgbaImage};
use rich_art::{AsciiArt, ImageArt};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn ball_gif() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/assets/ball.gif")
}

/// Equal where it shows, within `tolerance` a channel: the same alpha
/// everywhere, the same colour where a pixel is not fully transparent
/// (lossless WebP may change the colour under alpha 0).
fn assert_close_picture(webp: &RgbaImage, gif: &RgbaImage, tolerance: u8) {
    assert_eq!(webp.dimensions(), gif.dimensions());
    for (a, b) in webp.pixels().zip(gif.pixels()) {
        assert!(a.0[3].abs_diff(b.0[3]) <= tolerance, "{a:?} {b:?}");
        if a.0[3] != 0 {
            assert!(
                a.0.iter().zip(b.0).all(|(x, y)| x.abs_diff(y) <= tolerance),
                "{a:?} {b:?}"
            );
        }
    }
}

fn assert_same_picture(webp: &RgbaImage, gif: &RgbaImage) {
    assert_close_picture(webp, gif, 0);
}

#[test]
fn a_still_webp_decodes_by_content_and_by_extension() {
    let bytes = std::fs::read(fixture("ball.webp")).unwrap();
    assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::WebP);
    assert!(ImageFormat::WebP.reading_enabled());
    // By content: from memory, no name to go on.
    let from_bytes = image::load_from_memory(&bytes).unwrap().to_rgba8();
    // By extension: `.webp`.
    let from_path = image::open(fixture("ball.webp")).unwrap().to_rgba8();
    assert_eq!(from_bytes, from_path);
    // The GIF's first frame, pixel for pixel.
    let gif = image::open(ball_gif()).unwrap().to_rgba8();
    assert_same_picture(&from_bytes, &gif);
}

#[test]
fn a_webp_renders_as_art() {
    let webp = AsciiArt::from_path(fixture("ball.webp"))
        .unwrap()
        .to_text(24);
    let gif = AsciiArt::from_path(ball_gif()).unwrap().to_text(24);
    assert!(!webp.trim().is_empty());
    assert_eq!(webp, gif);
    let bytes = std::fs::read(fixture("ball.webp")).unwrap();
    let art = ImageArt::from_bytes(&bytes).unwrap();
    let console = rich::Console::builder()
        .width(24)
        .force_terminal(false)
        .color_system(None)
        .build();
    let lines = console.render_lines(&art, &console.options(), false);
    assert!(!lines.is_empty());
}

#[test]
fn an_animated_webp_decodes_its_frames_like_the_gif() {
    let budget = FrameBudget::default();
    let webp = decode_animation(
        &std::fs::read(fixture("ball-animated.webp")).unwrap(),
        budget,
    )
    .unwrap();
    let gif = decode_animation(&std::fs::read(ball_gif()).unwrap(), budget).unwrap();
    assert_eq!(webp.len(), 16);
    assert_eq!(webp.len(), gif.len());
    for (w, g) in webp.iter().zip(&gif) {
        assert_eq!(w.delay, g.delay);
        // An animation's frames are composited by the decoder, which can
        // round a channel by one.
        assert_close_picture(&w.image, &g.image, 2);
    }
    // A still WebP is one frame.
    let still = decode_animation(&std::fs::read(fixture("ball.webp")).unwrap(), budget).unwrap();
    assert_eq!(still.len(), 1);
    // `image::open` on the animation gives its first frame.
    let first = image::open(fixture("ball-animated.webp"))
        .unwrap()
        .to_rgba8();
    assert_close_picture(&first, &gif[0].image, 2);
}

#[cfg(feature = "gif")]
#[test]
fn animated_art_plays_an_animated_webp() {
    use rich_art::AnimatedArt;
    let webp = AnimatedArt::from_path(fixture("ball-animated.webp")).unwrap();
    let gif = AnimatedArt::from_path(ball_gif()).unwrap();
    assert_eq!(webp.frame_count(), 16);
    assert_eq!(webp.duration(), gif.duration());
    assert_eq!(
        AnimatedArt::from_path(fixture("ball.webp"))
            .unwrap()
            .frame_count(),
        1
    );
    // Not a WebP and not a GIF: still an error, not a panic.
    assert!(AnimatedArt::from_bytes(b"RIFF\0\0\0\0WEBPjunk").is_err());
}

#[cfg(feature = "picture")]
#[test]
fn a_picture_keeps_an_animated_webp_animated() {
    let bytes = std::fs::read(fixture("ball-animated.webp")).unwrap();
    let picture = rich_art::picture::Picture::decode_animated(&bytes).unwrap();
    assert!(picture.is_animated());
}

#[test]
fn a_still_image_over_the_budget_is_refused_before_it_is_decoded() {
    // A still WebP or PNG used to return its one frame without the budget
    // check every animation frame gets.
    let tight = FrameBudget {
        max_bytes: 1,
        ..FrameBudget::default()
    };
    let webp = std::fs::read(fixture("ball.webp")).unwrap();
    assert!(decode_animation(&webp, tight).is_err());
    assert_eq!(
        decode_animation(&webp, FrameBudget::default())
            .unwrap()
            .len(),
        1
    );

    let png = std::fs::read(fixture("halo-before.png")).unwrap();
    assert!(decode_animation(&png, tight).is_err());
    assert_eq!(
        decode_animation(&png, FrameBudget::default())
            .unwrap()
            .len(),
        1
    );
}
