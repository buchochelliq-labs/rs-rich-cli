//! `LiveCoordinator` repaints changed cells only (#226), and the screen ends
//! up as a full repaint would leave it.
#[path = "support/screen.rs"]
#[allow(dead_code)]
mod screen;
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Segment, Style, Table, Theme};
use rich_ext::{
    live::LiveCoordinator,
    target::{RenderTarget, TargetKind},
};
use screen::{Screen, Writer};
use std::{cell::RefCell, io::Write, rc::Rc};

const WIDTH: usize = 30;
const HEIGHT: usize = 12;

fn target() -> RenderTarget {
    RenderTarget::new(
        TargetKind::Terminal,
        TargetCapabilities {
            width: WIDTH,
            height: HEIGHT,
            color_system: Some(ColorSystem::Truecolor),
            interactive: true,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}

/// Paint `content` once on a fresh screen: what the screen should show.
fn fresh(content: &[Vec<Segment>]) -> Vec<String> {
    let screen = Rc::new(RefCell::new(Screen::new(WIDTH, HEIGHT)));
    let mut live = LiveCoordinator::new(Writer(screen.clone()), target());
    for region in content {
        live.add(region.clone()).unwrap();
    }
    live.refresh().unwrap();
    let lines = screen.borrow().lines();
    lines
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize
    }
}

#[test]
fn cell_repaints_leave_the_screen_as_a_full_repaint_would() {
    let words = [
        "ok",
        "busy",
        "漢字",
        "done ✔",
        "",
        "a longer status line",
        "x",
    ];
    let styles = [None, Some(Style::parse("bold red").unwrap())];
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let screen = Rc::new(RefCell::new(Screen::new(WIDTH, HEIGHT)));
    let mut live = LiveCoordinator::new(Writer(screen.clone()), target());
    let mut content: Vec<Vec<Segment>> = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let region = vec![Segment::new("start", None)];
        ids.push(live.add(region.clone()).unwrap());
        content.push(region);
    }
    live.refresh().unwrap();
    for step in 0..300 {
        let index = rng.next() % ids.len();
        let region: Vec<Segment> = (0..1 + rng.next() % 3)
            .map(|_| {
                Segment::new(
                    words[rng.next() % words.len()],
                    styles[rng.next() % styles.len()].clone(),
                )
            })
            .collect();
        live.update(ids[index].clone(), region.clone()).unwrap();
        content[index] = region;
        live.refresh().unwrap();
        let lines = screen.borrow().lines();
        assert_eq!(lines, fresh(&content), "step {step}");
        assert_eq!(screen.borrow().wraps, 0, "step {step}");
    }
    live.finish().unwrap();
}

#[test]
fn one_cell_change_in_a_table_writes_fewer_bytes_than_its_row() {
    fn build(highlight: usize) -> Table {
        let mut table = Table::new();
        table.add_column("Job");
        table.add_column("State");
        for row in 0..8 {
            let state = if row == highlight {
                "[bold]done[/]"
            } else {
                "wait"
            };
            table.add_row(&[&format!("job {row}"), state]);
        }
        table
    }
    fn table(highlight: usize) -> Vec<Segment> {
        target().segments(&build(highlight))
    }
    #[derive(Clone, Default)]
    struct Count(Rc<RefCell<Vec<u8>>>);
    impl Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let out = Count::default();
    let mut live = LiveCoordinator::new(out.clone(), target());
    let id = live.add(table(usize::MAX)).unwrap();
    live.refresh().unwrap();
    let before = out.0.borrow().len();
    live.update(id, table(3)).unwrap();
    live.refresh().unwrap();
    let written = out.0.borrow().len() - before;
    // What the row diff wrote before frames: move up, erase, the whole row,
    // move back down.
    let text = target().text(&build(3));
    let lines: Vec<&str> = text.split('\n').collect();
    let index = lines
        .iter()
        .position(|line| line.contains("job 3"))
        .unwrap();
    let distance = lines.len() - index;
    let row_diff = format!(
        "\r\x1b[{distance}A\x1b[2K{}\r\x1b[{distance}B",
        lines[index]
    )
    .len();
    assert!(
        written < row_diff,
        "cell diff wrote {written} bytes, the row diff {row_diff}"
    );
}
