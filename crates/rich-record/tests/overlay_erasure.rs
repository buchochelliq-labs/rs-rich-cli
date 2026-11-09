//! Repainting a cell erases cell-bound graphics even when its value is unchanged.
use rich_record::{Theme, terminal::Terminal};

fn image(iterm: bool) -> String {
    if iterm {
        let pixels =
            rich_art::image::RgbaImage::from_pixel(1, 1, rich_art::image::Rgba([255, 0, 0, 255]));
        let png = rich_art::graphics::encode_png(&pixels).unwrap();
        format!(
            "\x1b]1337;File=inline=1;width=2;height=2:{}\x07",
            rich_art::graphics::base64(&png)
        )
    } else {
        "\x1bPq\"1;1;16;32#1;2;100;0;0!16~-!16~-!16~-!16~-!16~-!16B\x1b\\".into()
    }
}
fn area(terminal: &Terminal) -> usize {
    terminal
        .snapshot(&Theme::default())
        .images
        .iter()
        .map(|t| usize::from(t.columns) * usize::from(t.rows))
        .sum()
}
#[test]
fn same_value_ech_and_space_writes_erase_only_touched_overlay_cells() {
    for iterm in [false, true] {
        for erase in [b"\x1b[H\x1b[1X".as_slice(), b"\x1b[H "] {
            let mut terminal = Terminal::new(4, 8);
            terminal.process(image(iterm).as_bytes()).unwrap();
            assert_eq!(area(&terminal), 4, "decoded overlay iterm={iterm}");
            terminal.process(erase).unwrap();
            assert_eq!(area(&terminal), 3, "iterm={iterm} erase={erase:?}");
            terminal.process(b"\x1b[H ").unwrap();
            assert_eq!(
                area(&terminal),
                3,
                "same blank never resurrects erased tile"
            );
        }
    }
}
#[test]
fn partial_line_and_display_erase_preserve_uncovered_fragments() {
    for iterm in [false, true] {
        let mut terminal = Terminal::new(4, 8);
        terminal.process(image(iterm).as_bytes()).unwrap();
        terminal.process(b"\x1b[1;2H\x1b[K").unwrap();
        assert_eq!(area(&terminal), 3);
        terminal.process(b"\x1b[2;1H\x1b[J").unwrap();
        assert_eq!(area(&terminal), 1);
        terminal.process(b"\x1b[H\x1b[1K").unwrap();
        assert_eq!(area(&terminal), 0);
    }
}
