#![cfg(feature = "art")]

use intuituive::interact::{Backend, Event};
use rich_art::picture::{ImageView, PictureTag, Pixels};
use rich_art::{ImageAnchor, ImageMode};
use rich_ext::frame::{Frame, Placement, PlacementSource};
use std::io;
use std::time::Duration;

struct OldBackend;
impl Backend for OldBackend {
    fn size(&self) -> (u16, u16) {
        (4, 2)
    }
    fn read(&mut self, _: Option<Duration>) -> io::Result<Option<Event>> {
        Ok(None)
    }
    fn write(&mut self, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
    fn handoff(&mut self, _: &mut std::process::Command) -> io::Result<Option<i32>> {
        Ok(None)
    }
    fn alternate_screen(&self) -> bool {
        false
    }
}

struct OldSource;
impl PlacementSource for OldSource {
    fn placements(&self, _: &Frame) -> Vec<Placement> {
        Vec::new()
    }
}

#[test]
fn existing_literals_and_trait_implementors_remain_usable() {
    let view = ImageView {
        mode: ImageMode::Blocks,
        fit: None,
        anchor: ImageAnchor::Center,
        background: None,
        pixels: Pixels::Cells,
    };
    assert_eq!(view, ImageView::default());
    let tag = PictureTag {
        id: 1,
        version: 0,
        cols: 4,
        rows: 2,
        row: 0,
        pixels: Pixels::Kitty,
        fit: None,
        anchor: ImageAnchor::Center,
    };
    assert_eq!(tag.cols, 4);
    use intuituive::interact::headless::{Script, Step};
    let script = Script(std::collections::VecDeque::from([Step::Wait(
        Duration::ZERO,
    )]));
    for step in script.0 {
        match step {
            Step::Event(_) => (),
            Step::Wait(_) => (),
        }
    }
    assert_eq!(OldBackend.cell_pixels(), None);
    let frame = Frame::from_segments(&[]);
    assert!(
        OldSource
            .placements_with_metrics(&frame, Some((10, 20)))
            .is_empty()
    );
}
