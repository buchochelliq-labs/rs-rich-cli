//! Framework-only picture demonstration. Run with `--features art --example
//! metric_pictures -- <cells|kitty|iterm|sixel> <8x16|10x20|12x24|12x12>
//! <contain|cover|stretch> <left|right|center> [clip|change|overlay|restore]`.
//! `change` writes a second frame at 12×24 with the same picture and cell grid;
//! `clip` shows the middle rows in a smaller viewport. `overlay` covers the
//! left half; `restore` paints that overlay and then dismisses it.
use intuituive::{
    App, ImageAnchor, ImageFit, ImageView, Picture, Pixels, image_graphics, picture_of,
};
use rich_art::image::{DynamicImage, Rgb, RgbImage};
use std::io::{self, Write};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let arg = |index: usize, default: &str| {
        args.get(index)
            .map(String::as_str)
            .unwrap_or(default)
            .to_string()
    };
    let pixels = match arg(0, "cells").as_str() {
        "kitty" => Pixels::Kitty,
        "iterm" => Pixels::Iterm,
        "sixel" => Pixels::Sixel,
        _ => Pixels::Cells,
    };
    let metrics = match arg(1, "8x16").as_str() {
        "10x20" => (10, 20),
        "12x24" => (12, 24),
        "12x12" => (12, 12),
        _ => (8, 16),
    };
    let fit = match arg(2, "cover").as_str() {
        "contain" => ImageFit::Contain,
        "stretch" => ImageFit::Stretch,
        _ => ImageFit::Cover,
    };
    let anchor = match arg(3, "center").as_str() {
        "left" => ImageAnchor::Left,
        "right" => ImageAnchor::Right,
        _ => ImageAnchor::Center,
    };
    let action = arg(4, "");
    let image = RgbImage::from_fn(120, 60, |x, y| {
        Rgb([(x * 2) as u8, (y * 4) as u8, (255 - x * 2) as u8])
    });
    let picture = Picture::new(DynamicImage::ImageRgb8(image));
    let view = ImageView::new().fit(fit).anchor(anchor).pixels(pixels);
    let overlay = std::rc::Rc::new(std::cell::Cell::new(None));
    let overlay_handle = overlay.clone();
    let app = App::new(move || {
        let covered = intuituive::signal(action == "overlay" || action == "restore");
        overlay_handle.set(Some(covered));
        let image = picture_of(
            {
                let picture = picture.clone();
                move || Some(picture.clone())
            },
            view,
        )
        .fixed(8);
        if action == "clip" {
            intuituive::scroll_with(image, intuituive::signal(2u16))
        } else {
            intuituive::layers([
                image,
                intuituive::label("Dialog")
                    .bg("black")
                    .inset(2, 8, 2, 0)
                    .shown_when(move || covered.get()),
            ])
        }
    })
    .graphics(image_graphics());
    let mut driver = app.driver(16, if arg(4, "") == "clip" { 4 } else { 8 });
    driver.set_cell_pixels(Some(metrics));
    driver.update(std::time::Duration::ZERO);
    let mut out = driver.render().unwrap_or_default();
    if arg(4, "") == "change" {
        driver.set_cell_pixels(Some((12, 24)));
        driver.update(std::time::Duration::ZERO);
        out.push_str(&driver.render().unwrap_or_default());
    }
    if arg(4, "") == "restore" {
        overlay.get().unwrap().set(false);
        driver.update(std::time::Duration::ZERO);
        out.push_str(&driver.render().unwrap_or_default());
    }
    io::stdout().write_all(out.as_bytes()).unwrap();
}
