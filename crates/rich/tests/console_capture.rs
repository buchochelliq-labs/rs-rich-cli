//! `Console::capture` is per thread, as upstream's (`ConsoleThreadLocals`
//! holds the buffer): a thread printing while another captures is not
//! swallowed, concurrent captures never mix, clones are independent, and a
//! panicking capture still ends.

use std::panic::{catch_unwind, AssertUnwindSafe};

use rich::{ColorSystem, Console, Control};

fn console() -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(20)
        .height(25)
        .highlight(false)
        .no_color(false)
        .legacy_windows(false)
        .build()
}

#[test]
fn concurrent_captures_do_not_mix() {
    let console = console();
    for _round in 0..50 {
        let (a, b) = std::thread::scope(|s| {
            let ha = s.spawn(|| console.capture(|c| (0..200).for_each(|_| c.print_str("A"))));
            let hb = s.spawn(|| console.capture(|c| (0..200).for_each(|_| c.print_str("B"))));
            (ha.join().unwrap(), hb.join().unwrap())
        });
        assert_eq!(a, "A\n".repeat(200));
        assert_eq!(b, "B\n".repeat(200));
    }
}

/// A plain print from another thread goes to that thread's output, not into
/// the capture running elsewhere.
#[test]
fn print_on_other_thread_is_not_captured() {
    let console = console();
    let barrier = std::sync::Barrier::new(2);
    let captured = std::thread::scope(|s| {
        let h = s.spawn(|| {
            console.capture(|c| {
                barrier.wait();
                std::thread::sleep(std::time::Duration::from_millis(50));
                c.print_str("mine");
            })
        });
        s.spawn(|| {
            barrier.wait();
            console.print_str("other-thread");
        });
        h.join().unwrap()
    });
    assert_eq!(captured, "mine\n");
}

#[test]
fn clone_captures_are_independent() {
    let a = console();
    let b = a.clone();
    let (x, y) = std::thread::scope(|s| {
        let h1 = s.spawn(|| a.capture(|c| (0..100).for_each(|_| c.print_str("x"))));
        let h2 = s.spawn(|| b.capture(|c| (0..100).for_each(|_| c.print_str("y"))));
        (h1.join().unwrap(), h2.join().unwrap())
    });
    assert_eq!(x, "x\n".repeat(100));
    assert_eq!(y, "y\n".repeat(100));
}

#[test]
fn nested_captures_on_one_thread() {
    let console = console();
    let mut inner = String::new();
    let outer = console.capture(|c| {
        c.print_str("before");
        inner = c.capture(|c| {
            c.print_str("inner");
            c.control(&Control::bell());
        });
        c.print_str("after");
    });
    assert_eq!(inner, "inner\n\x07");
    assert_eq!(outer, "before\nafter\n");
}

/// A capture that panics still ends: an enclosing capture sees what follows,
/// and output after it is not recorded any more.
#[test]
fn panicking_capture_ends() {
    let console = console();
    let outer = console.capture(|c| {
        let result = catch_unwind(AssertUnwindSafe(|| {
            c.capture(|c| {
                c.print_str("lost");
                panic!("renderable failed");
            })
        }));
        assert!(result.is_err());
        c.print_str("kept");
    });
    assert_eq!(outer, "kept\n");
}

/// After a panicking top-level capture, prints reach stdout again. Runs the
/// scenario in a child process and checks its stdout.
#[test]
fn panicking_capture_restores_output() {
    if std::env::var("RICH_CAPTURE_CHILD").is_ok() {
        let c = console();
        c.print_str("before-panic");
        let _ = catch_unwind(AssertUnwindSafe(|| {
            c.capture(|c| {
                c.print_str("inside");
                panic!("renderable failed");
            })
        }));
        c.print_str("after-panic");
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "panicking_capture_restores_output",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("RICH_CAPTURE_CHILD", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("before-panic"), "sanity: {stdout:?}");
    assert!(!stdout.contains("inside"), "{stdout:?}");
    assert!(
        stdout.contains("after-panic"),
        "print after a panicking capture was swallowed; child stdout: {stdout:?}"
    );
}
