//! Generate `cat.gif` — a waving cat — for trying out GIF playback.
//!
//! ```text
//! cargo run -p rich-art --features gif --example make_demo_gif
//! cargo run -p rich-art --features gif --example gif -- cat.gif 3
//! ```
//!
//! Drawn procedurally so the repository needs no binary fixture. Shapes are
//! deliberately bold and high-contrast: the art renderer maps luminance onto a
//! density ramp, so fine detail would wash out at terminal resolution.

use image::codecs::gif::GifEncoder;
use image::{Delay, Frame, Rgba, RgbaImage};

const SIZE: u32 = 96;
const FRAMES: usize = 12;

const FUR: Rgba<u8> = Rgba([255, 176, 59, 255]); // ginger
const DARK: Rgba<u8> = Rgba([20, 16, 28, 255]); // background + eyes
const PINK: Rgba<u8> = Rgba([255, 133, 168, 255]); // nose + inner ears

fn disc(img: &mut RgbaImage, cx: f64, cy: f64, r: f64, color: Rgba<u8>) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
        for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
            if x < 0 || y < 0 || x >= w || y >= h {
                continue;
            }
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}

/// An axis-aligned ellipse — the cat's body.
fn ellipse(img: &mut RgbaImage, cx: f64, cy: f64, rx: f64, ry: f64, color: Rgba<u8>) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    for y in (cy - ry).floor() as i32..=(cy + ry).ceil() as i32 {
        for x in (cx - rx).floor() as i32..=(cx + rx).ceil() as i32 {
            if x < 0 || y < 0 || x >= w || y >= h {
                continue;
            }
            let (dx, dy) = ((x as f64 + 0.5 - cx) / rx, (y as f64 + 0.5 - cy) / ry);
            if dx * dx + dy * dy <= 1.0 {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}

/// A filled triangle — the ears.
fn triangle(img: &mut RgbaImage, p: [(f64, f64); 3], color: Rgba<u8>) {
    let min_x = p.iter().map(|q| q.0).fold(f64::MAX, f64::min).floor() as i32;
    let max_x = p.iter().map(|q| q.0).fold(f64::MIN, f64::max).ceil() as i32;
    let min_y = p.iter().map(|q| q.1).fold(f64::MAX, f64::min).floor() as i32;
    let max_y = p.iter().map(|q| q.1).fold(f64::MIN, f64::max).ceil() as i32;
    let sign = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (a.0 - c.0) * (b.1 - c.1) - (b.0 - c.0) * (a.1 - c.1)
    };
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
                continue;
            }
            let q = (x as f64 + 0.5, y as f64 + 0.5);
            let (d1, d2, d3) = (
                sign(q, p[0], p[1]),
                sign(q, p[1], p[2]),
                sign(q, p[2], p[0]),
            );
            let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            if !(has_neg && has_pos) {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}

/// A thick line — whiskers and the waving arm.
fn stroke(img: &mut RgbaImage, from: (f64, f64), to: (f64, f64), width: f64, color: Rgba<u8>) {
    let steps = 64;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        disc(
            img,
            from.0 + (to.0 - from.0) * t,
            from.1 + (to.1 - from.1) * t,
            width / 2.0,
            color,
        );
    }
}

fn draw_cat(phase: f64) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(SIZE, SIZE, DARK);

    // Body.
    ellipse(&mut img, 48.0, 78.0, 24.0, 20.0, FUR);

    // Ears (outer fur, inner pink).
    triangle(&mut img, [(30.0, 30.0), (26.0, 6.0), (46.0, 20.0)], FUR);
    triangle(&mut img, [(66.0, 30.0), (70.0, 6.0), (50.0, 20.0)], FUR);
    triangle(&mut img, [(33.0, 27.0), (31.0, 14.0), (42.0, 22.0)], PINK);
    triangle(&mut img, [(63.0, 27.0), (65.0, 14.0), (54.0, 22.0)], PINK);

    // Head.
    disc(&mut img, 48.0, 40.0, 22.0, FUR);

    // Eyes — blink on the two frames either side of the wave's peak.
    let blink = phase.sin() > 0.94;
    if blink {
        stroke(&mut img, (34.0, 38.0), (43.0, 38.0), 3.0, DARK);
        stroke(&mut img, (53.0, 38.0), (62.0, 38.0), 3.0, DARK);
    } else {
        disc(&mut img, 38.5, 37.0, 4.5, DARK);
        disc(&mut img, 57.5, 37.0, 4.5, DARK);
    }

    // Nose + mouth.
    triangle(&mut img, [(44.0, 47.0), (52.0, 47.0), (48.0, 52.0)], PINK);

    // Whiskers.
    for (y, spread) in [(48.0, 2.0), (52.0, 0.0), (56.0, -2.0)] {
        stroke(&mut img, (30.0, y), (10.0, y - spread), 1.6, FUR);
        stroke(&mut img, (66.0, y), (86.0, y - spread), 1.6, FUR);
    }

    // The waving arm: shoulder fixed, paw swinging through an arc.
    let swing = phase.sin(); // -1..1
    let paw = (78.0 + 6.0 * swing, 44.0 - 16.0 * swing.abs());
    stroke(&mut img, (66.0, 72.0), paw, 9.0, FUR);
    disc(&mut img, paw.0, paw.1, 7.5, FUR);
    // Toe beans.
    disc(&mut img, paw.0 - 2.5, paw.1 - 3.0, 1.8, PINK);
    disc(&mut img, paw.0 + 2.5, paw.1 - 3.0, 1.8, PINK);

    // Resting paw.
    disc(&mut img, 32.0, 88.0, 7.0, FUR);

    // Tail.
    stroke(&mut img, (24.0, 84.0), (6.0, 68.0), 7.0, FUR);

    img
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut buffer);
        for f in 0..FRAMES {
            let phase = (f as f64 / FRAMES as f64) * std::f64::consts::TAU;
            let frame = Frame::from_parts(draw_cat(phase), 0, 0, Delay::from_numer_denom_ms(80, 1));
            encoder.encode_frame(frame)?;
        }
    }
    std::fs::write("cat.gif", &buffer)?;
    println!("wrote cat.gif ({} frames, {} bytes)", FRAMES, buffer.len());
    Ok(())
}
