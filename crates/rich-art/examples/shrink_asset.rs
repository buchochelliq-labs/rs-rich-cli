//! Re-encode a GIF smaller, for use as a repository demo asset.
//!
//! ```text
//! cargo run -p rich-art --features gif --example shrink_asset -- in.gif out.gif [width] [step]
//! ```
//!
//! Terminal art samples down to a few dozen columns anyway, so a large source
//! GIF is wasted bytes in the repo. Pixels are scaled to `width`, and `step`
//! keeps every Nth frame — with the kept frames' delays scaled up so the
//! animation still runs at its original speed.

use image::codecs::gif::{GifDecoder, GifEncoder};
use image::imageops::FilterType;
use image::{AnimationDecoder, Delay, Frame};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (input, output) = match (args.next(), args.next()) {
        (Some(i), Some(o)) => (i, o),
        _ => {
            eprintln!("usage: shrink_asset <in.gif> <out.gif> [width]");
            std::process::exit(2);
        }
    };
    let target: u32 = args.next().and_then(|w| w.parse().ok()).unwrap_or(160);
    let step: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);

    let decoder = GifDecoder::new(std::io::BufReader::new(std::fs::File::open(&input)?))?;
    let frames = decoder.into_frames().collect_frames()?;
    println!("{input}: {} frames", frames.len());

    let mut kept = 0usize;
    let mut buffer = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut buffer);
        encoder.set_repeat(image::codecs::gif::Repeat::Infinite)?;
        for frame in frames.into_iter().step_by(step) {
            // Hold each kept frame for the time its dropped neighbours would
            // have taken, so the animation keeps its original pace.
            let (numer, denom) = frame.delay().numer_denom_ms();
            let delay = Delay::from_numer_denom_ms(numer * step as u32, denom.max(1));
            let (left, top) = (frame.left(), frame.top());
            let image = frame.into_buffer();
            let (w, h) = (image.width(), image.height());
            let height = ((h as f64) * (target as f64) / (w as f64)).round() as u32;
            let scaled =
                image::imageops::resize(&image, target, height.max(1), FilterType::Triangle);
            encoder.encode_frame(Frame::from_parts(scaled, left, top, delay))?;
            kept += 1;
        }
    }
    println!("kept {kept} frames (every {step})");
    std::fs::write(&output, &buffer)?;
    println!("{output}: {} bytes ({target}px wide)", buffer.len());
    Ok(())
}
