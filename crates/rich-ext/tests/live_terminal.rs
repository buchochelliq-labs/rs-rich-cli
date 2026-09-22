#[path = "support/screen.rs"]
mod screen;
use rich::protocol::{Support, TargetCapabilities};
use rich::{Segment, Theme};
use rich_ext::{
    live::LiveCoordinator,
    target::{RenderTarget, TargetKind},
};
use screen::{Screen, Writer};
use std::{cell::RefCell, io::Write, rc::Rc};
fn target() -> RenderTarget {
    RenderTarget::new(
        TargetKind::Terminal,
        TargetCapabilities {
            width: 20,
            height: 6,
            color_system: None,
            interactive: true,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}
fn row(s: &str) -> Vec<Segment> {
    vec![Segment::new(s, None)]
}
#[test]
fn visible_screen_keeps_logs_and_regions_without_wrap() {
    let screen = Rc::new(RefCell::new(Screen::new(20, 6)));
    let mut live = LiveCoordinator::new(Writer(screen.clone()), target());
    let a = live.add(row("Alpha")).unwrap();
    live.add(row("Beta")).unwrap();
    live.refresh().unwrap();
    live.print(&row("Log")).unwrap();
    live.update(a, row("A")).unwrap();
    live.refresh().unwrap();
    assert_eq!(screen.borrow().lines(), ["", "", "Log", "A", "Beta", ""]);
    assert_eq!(screen.borrow().wraps, 0);
    live.finish().unwrap();
    assert!(screen.borrow().visible);
    assert_eq!(screen.borrow().lines(), ["", "", "Log", "", "", ""]);
}
#[test]
fn active_to_empty_restores_cursor_then_growing_starts_fresh() {
    let screen = Rc::new(RefCell::new(Screen::new(20, 6)));
    let mut live = LiveCoordinator::new(Writer(screen.clone()), target());
    live.add(row("active")).unwrap();
    live.refresh().unwrap();
    assert!(!screen.borrow().visible);
    screen.borrow_mut().resize(1, 1);
    live.resize(0, 1).unwrap();
    assert!(
        screen.borrow().visible,
        "empty viewport must restore owned cursor"
    );
    screen.borrow_mut().resize(20, 6);
    live.resize(20, 6).unwrap();
    assert!(!screen.borrow().visible);
    assert!(screen.borrow().lines().contains(&"active".into()));
    live.finish().unwrap();
    assert!(screen.borrow().visible);
}
#[test]
fn unwind_restores_cursor_and_repeated_finish_is_quiet() {
    let screen = Rc::new(RefCell::new(Screen::new(20, 6)));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut live = LiveCoordinator::new(Writer(screen.clone()), target());
        live.add(row("busy")).unwrap();
        live.refresh().unwrap();
        panic!("application panic");
    }));
    assert!(result.is_err());
    assert!(screen.borrow().visible);
    let mut bytes = Vec::new();
    {
        let mut live = LiveCoordinator::new(&mut bytes, target());
        live.add(row("x")).unwrap();
        live.refresh().unwrap();
        live.finish().unwrap();
        live.finish().unwrap();
    }
    assert_eq!(
        String::from_utf8(bytes)
            .unwrap()
            .matches("\x1b[?25h")
            .count(),
        1
    );
}
#[derive(Default)]
struct Failure {
    accepted: usize,
    restores: usize,
}
struct Broken(Rc<RefCell<Failure>>);
impl Write for Broken {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let mut s = self.0.borrow_mut();
        if b.starts_with(b"\x1b[?25h") {
            s.restores += 1;
        }
        if s.accepted >= 8 {
            return Err(std::io::Error::other("lost writer"));
        }
        let n = b.len().min(8 - s.accepted);
        s.accepted += n;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[test]
fn partial_writer_failure_closes_session_and_attempts_restore_once() {
    let state = Rc::new(RefCell::new(Failure::default()));
    let mut live = LiveCoordinator::new(Broken(state.clone()), target());
    live.add(row("long content")).unwrap();
    assert!(live.refresh().is_err());
    assert!(live.resize(10, 4).is_err());
    live.finish().unwrap();
    assert_eq!(state.borrow().restores, 1);
}

#[test]
fn removing_last_region_restores_cursor() {
    let screen = Rc::new(RefCell::new(Screen::new(20, 6)));
    let mut live = LiveCoordinator::new(Writer(screen.clone()), target());
    let id = live.add(row("work")).unwrap();
    live.refresh().unwrap();
    live.remove(id).unwrap();
    live.refresh().unwrap();
    assert!(screen.borrow().visible);
    assert!(screen.borrow().lines().iter().all(|s| s.is_empty()));
}
