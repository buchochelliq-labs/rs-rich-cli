//! The graphics side channel in `LiveCoordinator`: placements are drawn
//! after the cells, redrawn when the cells under them are repainted, never
//! drawn without a terminal, and released when the coordinator finishes.
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Segment, Theme};
use rich_ext::{
    frame::{Frame, Graphic, Placement, PlacementSource},
    live::LiveCoordinator,
    target::{RenderTarget, TargetKind},
};
use std::sync::{Arc, Mutex};

fn target(interactive: bool) -> RenderTarget {
    RenderTarget::new(
        TargetKind::Terminal,
        TargetCapabilities {
            width: 20,
            height: 6,
            color_system: Some(ColorSystem::Truecolor),
            interactive,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}

#[derive(Debug)]
struct Star;

impl Graphic for Star {
    fn key(&self) -> u64 {
        1
    }
    fn draw(&self, frame: usize) -> String {
        format!("\x1b_Gstar{frame}\x1b\\")
    }
}

/// Places a star on every `*` cell.
struct Stars(Mutex<usize>);

impl PlacementSource for Stars {
    fn placements(&self, frame: &Frame) -> Vec<Placement> {
        let mut out = Vec::new();
        for row in 0..frame.height() {
            for (column, cell) in frame.cells(row).iter().enumerate() {
                if cell.text == "*" {
                    out.push(Placement {
                        row,
                        column,
                        cols: 1,
                        rows: 1,
                        graphic: Arc::new(Star),
                        frame: *self.0.lock().unwrap(),
                    });
                }
            }
        }
        out
    }
    fn release(&self, retained: &[Placement]) -> String {
        format!("<release {}>", retained.len())
    }
}

#[derive(Clone, Default)]
struct Bytes(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Bytes {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Bytes {
    fn take(&self) -> String {
        String::from_utf8(std::mem::take(&mut *self.0.lock().unwrap())).unwrap()
    }
}

fn line(text: &str) -> Vec<Segment> {
    vec![Segment::new(text, None)]
}

#[test]
fn placements_are_drawn_after_cells_and_only_when_needed() {
    let out = Bytes::default();
    let stars = Arc::new(Stars(Mutex::new(0)));
    let mut live = LiveCoordinator::new(out.clone(), target(true)).with_graphics(stars.clone());
    let id = live.add(line("a * b")).unwrap();
    live.refresh().unwrap();
    let first = out.take();
    // The cells, then a move to column 2 and the drawing.
    let at = first.find("a * b").expect("cells");
    let drawn = first.find("\x1b[3G\x1b_Gstar0\x1b\\").expect("drawn");
    assert!(at < drawn, "{first:?}");

    // Nothing changed: nothing written.
    live.refresh().unwrap();
    assert_eq!(out.take(), "");

    // A change beside the star leaves it; one under it redraws it.
    live.update(id.clone(), line("a * c")).unwrap();
    live.refresh().unwrap();
    let beside = out.take();
    assert!(!beside.contains("star"), "{beside:?}");
    // Shorter: the row is written whole (it is fewer bytes), which wipes
    // the star, so it is drawn again after the cells.
    live.update(id.clone(), line("a *")).unwrap();
    live.refresh().unwrap();
    let whole = out.take();
    assert!(whole.contains("\x1b[2Ka *\x1b[3G\x1b_Gstar0"), "{whole:?}");

    // Another animation frame redraws it with no cell change.
    *stars.0.lock().unwrap() = 1;
    live.refresh().unwrap();
    let next = out.take();
    assert!(next.contains("\x1b_Gstar1\x1b\\"), "{next:?}");
    assert!(!next.contains("a *"));

    // Gone: its cell is repainted by the diff, and nothing is drawn.
    live.update(id, line("a . c")).unwrap();
    live.refresh().unwrap();
    let gone = out.take();
    assert!(gone.contains('.') && !gone.contains("star"), "{gone:?}");

    live.print(&line("kept *")).unwrap();
    assert!(out.take().contains("star1"));
    live.finish().unwrap();
    assert!(out.take().ends_with("<release 1>\x1b[?25h"));
}

#[test]
fn a_pipe_never_gets_graphics() {
    let out = Bytes::default();
    let stars = Arc::new(Stars(Mutex::new(0)));
    let mut live = LiveCoordinator::new(out.clone(), target(false)).with_graphics(stars);
    live.add(line("a * b")).unwrap();
    live.print(&line("printed *")).unwrap();
    live.refresh().unwrap();
    live.finish().unwrap();
    assert_eq!(out.take(), "printed *\na * b\n");
}
