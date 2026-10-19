//! The sprite plugins: an image cut into sprites (`art`) and micro assets
//! as sprites (`micro`).

#![cfg(feature = "art")]

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::sprite::{stage, Placed};
use intuituive::sprite_sheet::{sprite_sheet, sprite_sheet_bytes};
use rich_art::image::{ImageFormat, Rgba, RgbaImage};

const COLORS: [[u8; 4]; 6] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 0, 255],
    [0, 255, 255, 255],
    [0, 0, 0, 0],
];

/// A 7x5 sheet of six 2x2 tiles in three columns and two rows, each one
/// colour, with a column and a row left over at the edges.
fn sheet_png() -> Vec<u8> {
    let image = RgbaImage::from_fn(7, 5, |x, y| {
        if x >= 6 || y >= 4 {
            return Rgba([9, 9, 9, 255]);
        }
        Rgba(COLORS[(y / 2 * 3 + x / 2) as usize])
    });
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
        .unwrap();
    png
}

#[test]
fn a_sheet_is_cut_into_whole_tiles_row_by_row() {
    let sheet = sprite_sheet_bytes(&sheet_png(), 2, 2).unwrap();
    assert_eq!((sheet.columns(), sheet.rows(), sheet.len()), (3, 2, 6));
    for (i, color) in COLORS.iter().enumerate() {
        let tile = sheet.tile(i);
        assert_eq!((tile.width(), tile.height()), (2, 2));
        assert!(tile.frame(0).iter().all(|p| p == color), "tile {i}");
    }
    assert_eq!(sheet.tile_at(1, 1), Some(sheet.tile(4)));
    assert_eq!(sheet.tile_at(3, 0), None);
    let walk = sheet
        .animation(&[0, 2, 0])
        .period(Duration::from_millis(50));
    assert_eq!(walk.frame_count(), 3);
    assert_eq!(
        walk.frame(walk.frame_at(Duration::from_millis(60)))[0],
        COLORS[2]
    );
}

#[test]
fn a_sheet_from_a_file_and_a_bad_one() {
    let path = std::env::temp_dir().join(format!("intuituive-sheet-{}.png", std::process::id()));
    std::fs::write(&path, sheet_png()).unwrap();
    let sheet = sprite_sheet(&path, 3, 4).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!((sheet.columns(), sheet.rows()), (2, 1));
    let bad = sprite_sheet_bytes(b"not an image", 2, 2).unwrap_err();
    assert_eq!(bad.kind(), std::io::ErrorKind::InvalidData);
    assert!(sprite_sheet("/no/such/sheet.png", 2, 2).is_err());
}

#[test]
fn sheet_tiles_draw_on_a_stage_in_half_blocks_and_animate() {
    let png = sheet_png();
    let app = App::new(move || {
        let sheet = sprite_sheet_bytes(&png, 2, 2).unwrap();
        let still = sheet.sprite(5);
        let blink = sheet
            .sprite_animation(&[0, 1])
            .period(Duration::from_millis(100));
        stage(move || {
            vec![
                Placed::new(0, blink.clone()),
                Placed::new(1, still.clone()).at(3.0, 0.0),
            ]
        })
    });
    let mut driver = app.driver(5, 1);
    driver.settle();
    // The transparent tile draws nothing.
    assert_eq!(driver.screen().plain(), ["▀▀   "]);
    let fg = |driver: &intuituive::Driver| {
        let screen = driver.screen();
        let style = screen.style(screen.cell(0, 0).style).unwrap();
        let t = style.color().unwrap().get_truecolor().unwrap();
        (t.red, t.green, t.blue)
    };
    assert_eq!(fg(&driver), (255, 0, 0));
    driver.advance(Duration::from_millis(100));
    assert_eq!(fg(&driver), (0, 255, 0));
}

#[cfg(feature = "micro")]
mod micro {
    use intuituive::micro_sprite::{micro_pixels, micro_sprite};
    use intuituive::prelude::*;
    use intuituive::sprite::{stage, Placed};
    use rich_micro::{Limits, MicroAsset, MicroRegistry};

    #[test]
    fn a_built_in_micro_asset_is_a_sprite_of_its_cells() {
        let registry = MicroRegistry::builtin();
        let loading = registry.require("status/loading").unwrap().clone();
        let pixels = micro_pixels(&loading, &Limits::default())
            .unwrap()
            .expect("the built-in asset has images");
        assert_eq!(
            (pixels.width() as usize, pixels.height() as usize),
            (loading.cols(), loading.rows() * 2)
        );
        let sprite = micro_sprite(&loading).unwrap();
        assert_eq!(sprite.frame_count(), pixels.frame_count());
        assert_eq!(
            sprite.size(),
            (loading.cols() as u16, loading.rows() as u16)
        );
        let app = App::new(move || {
            let sprite = sprite.clone();
            stage(move || vec![Placed::new(0, sprite.clone())])
        });
        let mut driver = app.driver(4, 2);
        driver.settle();
        let drawn = driver.screen().plain().concat();
        assert!(drawn.contains(['▀', '▄']), "{drawn:?}");
    }

    #[test]
    fn an_asset_without_images_is_its_fallback() {
        let star = MicroAsset::new("star", "a star")
            .unwrap()
            .with_text("*!")
            .unwrap();
        let sprite = micro_sprite(&star).unwrap();
        let app = App::new(move || {
            let sprite = sprite.clone();
            stage(move || vec![Placed::new(0, sprite.clone()).at(1.0, 0.0)])
        });
        let mut driver = app.driver(4, 1);
        driver.settle();
        assert_eq!(driver.screen().plain(), [" *! "]);
    }
}
