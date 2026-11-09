use intuituive::prelude::*;
use intuituive::screen::{PremeasuredCell, PremeasuredError, Rect};
use intuituive::widget::{Canvas, DrawCx, Widget, widget};

struct Grid;
impl Widget for Grid {
    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let text = format!("a{}", "\u{903}".repeat(300));
        let style = rich::Style::parse("bold red")
            .unwrap()
            .with_link("https://example.test/");
        let mut cells = vec![
            PremeasuredCell {
                text: "",
                width: 0,
                style: Some(&style)
            };
            301
        ];
        cells[0] = PremeasuredCell {
            text: &text,
            width: 301,
            style: Some(&style),
        };
        cells.push(PremeasuredCell {
            text: "X",
            width: 1,
            style: None,
        });
        canvas.write_premeasured(&cells).unwrap();
        assert_eq!(canvas.cell(0, 0).unwrap().symbol(), text);
        assert_eq!(canvas.cell(0, 0).unwrap().width(), 301);
        assert!(canvas.cell(300, 0).unwrap().is_continuation());
        assert_eq!(canvas.cell(301, 0).unwrap().symbol(), "X");
        let before = canvas.cell(0, 0).unwrap().symbol().to_owned();
        assert_eq!(
            canvas.write_premeasured(&[]),
            Err(PremeasuredError::Shape {
                expected: 302,
                actual: 0
            })
        );
        assert_eq!(canvas.cell(0, 0).unwrap().symbol(), before);
    }
}

#[test]
fn premeasured_widget_driver_preserves_cells_after_inputs_drop() {
    let mut driver = App::new(|| widget(Grid)).driver(302, 1);
    let out = driver.try_render().unwrap().unwrap();
    assert!(out.contains("\x1b[1;302H"));
    assert_eq!(driver.screen().cell(0, 0).width, 301);
    assert_eq!(driver.screen().cell(300, 0).text(), "");
    assert_eq!(driver.screen().cell(301, 0).text(), "X");
    driver.try_settle().unwrap();
    assert!(driver.try_render().unwrap().is_none());
    assert!(driver.try_finish().unwrap().contains("\x1b[?7h"));
}

#[test]
fn premeasured_inline_driver_returns_an_error() {
    let mut driver = App::new(|| widget(Grid)).inline(1).driver(302, 4);
    let error = driver.try_render().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn premeasured_blit_resize_keeps_no_orphans() {
    let mut driver = App::new(|| widget(Grid)).driver(302, 1);
    driver.try_settle().unwrap();
    let mut clipped = intuituive::screen::Screen::new(299, 1);
    clipped.blit(driver.screen(), Rect::new(0, 0, 302, 1), 0, 0);
    for x in 0..299 {
        assert_eq!(
            (clipped.cell(x, 0).text(), clipped.cell(x, 0).width),
            (" ", 1)
        );
    }
}

#[test]
fn premeasured_existing_layout_caps_canvas_at_500_by_200() {
    use std::{cell::Cell, rc::Rc};
    struct Bounded(Rc<Cell<(u16, u16)>>);
    impl Widget for Bounded {
        fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
            self.0.set((canvas.width(), canvas.height()));
            let mut cells = vec![
                PremeasuredCell {
                    text: " ",
                    width: 1,
                    style: None
                };
                usize::from(canvas.width()) * usize::from(canvas.height())
            ];
            cells[0].text = "X";
            canvas.write_premeasured(&cells).unwrap();
        }
    }
    let dimensions = Rc::new(Cell::new((0, 0)));
    let captured = dimensions.clone();
    let mut driver = App::new(move || {
        column([row([widget(Bounded(captured.clone())).max_size(500)]).max_size(200)])
    })
    .driver(501, 201);
    driver.try_settle().unwrap();
    assert_eq!(dimensions.get(), (500, 200));
    assert_eq!(driver.screen().cell(0, 0).text(), "X");
    for y in 0..201 {
        assert_eq!(driver.screen().cell(500, y).text(), " ");
    }
    for x in 0..501 {
        assert_eq!(driver.screen().cell(x, 200).text(), " ");
    }
}

#[test]
fn premeasured_inline_owned_loop_writes_no_bytes() {
    use intuituive::interact::headless::{Headless, Script};
    let mut backend = Headless::new(Script::new().keys("q"), 302, 4);
    let record = backend.record();
    let error = App::new(|| widget(Grid))
        .inline(1)
        .run_on(&mut backend)
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        record.borrow().writes.is_empty(),
        "{:?}",
        record.borrow().writes
    );
}

#[test]
fn premeasured_owned_loop_restores_wrap_after_backend_failure_and_panic() {
    use intuituive::interact::{Backend, Event};
    use std::{io, process::Command, time::Duration};
    struct FaultBackend {
        bytes: String,
        panic: bool,
        failed: bool,
    }
    impl Backend for FaultBackend {
        fn size(&self) -> (u16, u16) {
            (302, 1)
        }
        fn read(&mut self, _: Option<Duration>) -> io::Result<Option<Event>> {
            Err(io::Error::other("unexpected read"))
        }
        fn write(&mut self, text: &str) -> io::Result<()> {
            if !self.failed {
                self.failed = true;
                self.bytes.push_str("\x1b[?7l");
                if self.panic {
                    panic!("backend writer panic");
                }
                return Err(io::Error::other("original backend error"));
            }
            self.bytes.push_str(text);
            Ok(())
        }
        fn elapsed(&self) -> Duration {
            Duration::ZERO
        }
        fn handoff(&mut self, _: &mut Command) -> io::Result<Option<i32>> {
            Ok(None)
        }
        fn alternate_screen(&self) -> bool {
            true
        }
    }
    for panic in [false, true] {
        let mut backend = FaultBackend {
            bytes: String::new(),
            panic,
            failed: false,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            App::new(|| widget(Grid)).run_on(&mut backend)
        }));
        if panic {
            assert!(result.is_err());
        } else {
            assert_eq!(
                result.unwrap().unwrap_err().to_string(),
                "original backend error"
            );
        }
        assert!(backend.bytes.ends_with("\x1b[?7h"), "{:?}", backend.bytes);
    }
}

#[test]
fn premeasured_inline_later_round_rolls_back_unsent_painter_state() {
    struct Later(intuituive::Signal<bool>);
    impl Widget for Later {
        fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
            if self.0.get() {
                canvas
                    .write_premeasured(&[PremeasuredCell {
                        text: "字",
                        width: 1,
                        style: None,
                    }])
                    .unwrap();
            } else {
                canvas.print(0, 0, "A", None);
                self.0.set(true);
            }
        }
    }
    use intuituive::interact::headless::{Headless, Script};
    let mut backend = Headless::new(Script::new().keys("q"), 1, 3);
    let record = backend.record();
    let error = App::new(|| widget(Later(signal(false))))
        .inline(1)
        .run_on(&mut backend)
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        record.borrow().writes.is_empty(),
        "{:?}",
        record.borrow().writes
    );
}

#[test]
fn premeasured_inline_rollback_keeps_style_ids_valid_until_frame_success() {
    use std::{cell::Cell, rc::Rc};
    struct Churn(intuituive::Signal<u8>);
    impl Widget for Churn {
        fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
            match self.0.get() {
                0 => canvas.print(0, 0, "A", Some(&rich::Style::parse("red").unwrap())),
                1 => {
                    for color in 0..1100 {
                        let style = rich::Style::parse(&format!("#{color:06x}")).unwrap();
                        canvas.print(0, 0, "B", Some(&style));
                    }
                    self.0.set(2);
                }
                _ => canvas
                    .write_premeasured(&[PremeasuredCell {
                        text: "字",
                        width: 1,
                        style: None,
                    }])
                    .unwrap(),
            }
        }
    }
    let handle = Rc::new(Cell::new(None));
    let captured = handle.clone();
    let mut driver = App::new(move || {
        let state = signal(0u8);
        captured.set(Some(state));
        widget(Churn(state))
    })
    .inline(1)
    .driver(1, 3);
    driver.try_render().unwrap();
    let old_id = driver.screen().cell(0, 0).style;
    let old_style = driver.screen().style(old_id).unwrap().clone();
    handle.get().unwrap().set(1);
    assert_eq!(
        driver.try_render().unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    // The restored painter still refers to the previously sent style IDs.
    assert_eq!(driver.screen().style(old_id), Some(&old_style));
    assert!(driver.screen().style_count() > 1024);
    handle.get().unwrap().set(0);
    driver.try_render().unwrap();
    assert!(driver.screen().style_count() < 1024);
}
