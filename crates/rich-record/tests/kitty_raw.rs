//! rs-rich-art's raw Kitty transmissions (`f=24`, `f=32`, `o=z`, chunked)
//! decode in the emulator to the pixels sent.

use rich_art::kitty::{placeholder_row, transmit_raw, RawFormat, RawImage, Transmit};
use rich_record::terminal::Terminal;
use rich_record::Theme;

/// The pixels of the one image `escape` sends, shown in 4x2 placeholder
/// cells, as RGBA.
fn shown(escape: &str, id: u32) -> Vec<u8> {
    let mut terminal = Terminal::new(4, 10);
    terminal.process(escape.as_bytes()).unwrap();
    let colour = format!(
        "\x1b[38;2;{};{};{}m",
        id >> 16 & 255,
        id >> 8 & 255,
        id & 255
    );
    for row in 0..2 {
        let cells = placeholder_row(row, 4).unwrap();
        let line = format!("\x1b[{};1H{colour}{cells}\x1b[0m", row + 1);
        terminal.process(line.as_bytes()).unwrap();
    }
    let snapshot = terminal.snapshot(&Theme::default());
    // A tile for each row of placeholder cells, of the one image.
    assert_eq!(snapshot.images.len(), 2);
    assert_eq!(snapshot.images[0].image, snapshot.images[1].image);
    let image = &snapshot.images[0].image;
    assert_eq!((image.width(), image.height()), (40, 30));
    image.pixels().expect("decoded")
}

#[test]
fn raw_frames_decode_to_the_same_pixels() {
    let rgba: Vec<u8> = (0..40 * 30 * 4)
        .map(|i| if i % 4 == 3 { 255 } else { (i * 7 % 256) as u8 })
        .collect();
    let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| p[..3].to_vec()).collect();
    for compress in [false, true] {
        let how = Transmit::new(5, 4, 2).compress(compress);
        let out = transmit_raw(&RawImage::new(RawFormat::Rgba, &rgba, 40, 30), &how);
        assert!(out.matches("\x1b_G").count() > 1 || compress, "chunked");
        assert_eq!(shown(&out, 5), rgba, "rgba, compressed {compress}");
        let out = transmit_raw(&RawImage::new(RawFormat::Rgb, &rgb, 40, 30), &how);
        assert_eq!(shown(&out, 5), rgba, "rgb, compressed {compress}");
    }
}
