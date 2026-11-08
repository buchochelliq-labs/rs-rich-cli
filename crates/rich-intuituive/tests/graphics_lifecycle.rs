#![cfg(feature = "art")]

use intuituive::{ImageView, Picture, Pixels, image_graphics};
use rich::Segment;
use rich::color::ColorSystem;
use rich_ext::frame::{
    Frame,
    graphics::{Graphic, Placement, plan},
};
use std::sync::Arc;

#[derive(Debug)]
struct Overlay(u64);
impl Graphic for Overlay {
    fn key(&self) -> u64 {
        self.0
    }
    fn draw(&self, _: usize) -> String {
        self.0.to_string()
    }
}
fn overlay(id: u64, column: usize) -> Placement {
    Placement {
        row: 0,
        column,
        cols: 2,
        rows: 1,
        graphic: Arc::new(Overlay(id)),
        frame: 0,
    }
}

#[test]
fn public_planner_keeps_higher_overlay_on_top() {
    let upper = overlay(2, 1);
    let lower = overlay(1, 0);
    let next = [lower, upper.clone()];
    let transition = plan(std::slice::from_ref(&upper), &next, &[]);
    let clean = plan(&[], &next, &[]);
    assert_eq!(
        transition
            .draw
            .iter()
            .map(|p| p.graphic.key())
            .collect::<Vec<_>>(),
        clean
            .draw
            .iter()
            .map(|p| p.graphic.key())
            .collect::<Vec<_>>()
    );
}

#[test]
fn public_image_source_retains_kitty_and_clamps_crops() {
    let picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
    for pixels in [Pixels::Iterm, Pixels::Sixel] {
        assert!(picture.encode_rows(pixels, 4, 3, usize::MAX, 3).is_none());
        let part = picture.encode_rows(pixels, 4, 3, 2, 3).unwrap();
        assert!(if pixels == Pixels::Iterm {
            part.contains("height=1")
        } else {
            part.contains("\"1;1;32;16")
        });
    }
    let source = image_graphics();
    let console = rich::Console::builder()
        .width(2)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let mut segments = Vec::new();
    for line in picture.lines(&console, ImageView::new().pixels(Pixels::Kitty), 2, 2) {
        segments.extend(line);
        segments.push(Segment::line());
    }
    let placements = source.placements(&Frame::from_segments(&segments));
    assert_eq!(placements.len(), 1);
    assert!(source.release(&placements).is_empty());
    assert!(source.release(&[]).contains("a=d"));
    let again = source.placements(&Frame::from_segments(&segments));
    assert_eq!(again.len(), 1);
    assert!(source.release(&[]).contains("a=d"));
}

#[test]
fn unknown_cell_metrics_can_be_read_while_another_signal_is_borrowed() {
    let app = intuituive::App::new(|| {
        let content = intuituive::signal("ready".to_string());
        intuituive::text(move || {
            content.with(|value| format!("{value} {:?}", intuituive::cell_pixels().get()))
        })
    });
    let rows = app.render_with(&[], 20, 1).unwrap();
    assert!(rows[0].contains("ready None"));
}
