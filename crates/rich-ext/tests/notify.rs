//! Transient notifications and status messages (#392).
use std::time::Duration;

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Renderable, Segment, Theme};
use rich_ext::a11y::{Status, SymbolSet};
use rich_ext::live::LiveCoordinator;
use rich_ext::notify::{Notification, Notifications, ToastStyle};
use rich_ext::target::{RenderTarget, TargetKind};

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn plain(width: usize, renderable: &dyn Renderable) -> String {
    Console::builder()
        .width(width)
        .build()
        .render_to_string(renderable)
}

fn target(kind: TargetKind, interactive: bool) -> RenderTarget {
    RenderTarget::new(
        kind,
        TargetCapabilities {
            width: 40,
            height: 10,
            color_system: None,
            interactive,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}

#[test]
fn one_line_toasts_carry_the_level_without_colour() {
    let saved = Notification::ok("3 files written").title("Saved");
    assert_eq!(plain(40, &saved), "✔ ok Saved: 3 files written");
    assert_eq!(
        plain(40, &Notification::error("disk full")),
        "✖ error disk full"
    );
    assert_eq!(
        plain(40, &Notification::info("x").symbols(SymbolSet::Ascii)),
        "[INFO] x"
    );
    assert_eq!(
        plain(40, &Notification::warning("x").symbols(SymbolSet::Words)),
        "warning: x"
    );
    let ascii = Console::builder().width(40).ascii_only(true).build();
    assert_eq!(
        ascii.render_to_string(&saved),
        "[OK] Saved: 3 files written"
    );
    // Long messages are cropped to one line.
    assert_eq!(
        plain(20, &Notification::info("a very long message indeed")),
        "ℹ info a very long m"
    );
    assert_eq!(saved.status(), Status::Ok);
    assert_eq!(saved.get_title(), Some("Saved"));
    assert_eq!(saved.message(), "3 files written");
}

#[test]
fn colour_toast_uses_notify_styles() {
    let console = Console::builder()
        .width(40)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    assert_eq!(
        console.render_to_string(&Notification::warning("92% full").title("Disk")),
        "\u{1b}[33m⚠ warning\u{1b}[0m \u{1b}[1mDisk\u{1b}[0m: 92% full"
    );
}

#[test]
fn panel_toast() {
    let toast = Notification::error("upload failed")
        .title("Sync")
        .toast_style(ToastStyle::Panel);
    assert_eq!(
        plain(40, &toast),
        concat!(
            "╭─ ✖ error Sync ─╮\n",
            "│ upload failed  │\n",
            "╰────────────────╯",
        )
    );
}

#[test]
fn stack_expires_by_ttl_and_limits_what_it_shows() {
    let mut stack = Notifications::new()
        .default_ttl(Some(secs(5)))
        .max_visible(2);
    stack.push(Notification::info("one"), secs(0));
    stack.push(Notification::info("two").ttl(secs(1)), secs(1));
    let three = stack.push(Notification::info("three"), secs(2));
    assert_eq!(stack.len(), 3);
    assert_eq!(stack.next_expiry(), Some(secs(2)));
    assert_eq!(plain(40, &stack), "+1 more\nℹ info two\nℹ info three");

    assert_eq!(stack.expire(secs(2)), 1, "two lived its own 1s TTL");
    assert_eq!(plain(40, &stack), "ℹ info one\nℹ info three");
    assert_eq!(stack.expire(secs(5)), 1);
    assert!(stack.dismiss(three));
    assert!(stack.is_empty());
    assert_eq!(plain(40, &stack), "");
}

#[test]
fn stack_overrides_symbols_and_style() {
    let mut stack = Notifications::new()
        .symbols(SymbolSet::Words)
        .toast_style(ToastStyle::Line);
    stack.push(
        Notification::ok("done").toast_style(ToastStyle::Panel),
        secs(0),
    );
    assert_eq!(plain(40, &stack), "ok: done");
    assert_eq!(stack.iter().count(), 1);
}

fn region_text(segments: &[Segment]) -> String {
    segments.iter().map(|s| s.text.as_str()).collect()
}

#[test]
fn non_interactive_targets_print_each_notification_once() {
    let stream = target(TargetKind::PlainStream, false);
    let mut stack = Notifications::for_target(&stream);
    let mut out = Vec::new();
    {
        let mut live = LiveCoordinator::new(&mut out, stream.clone());
        let mut region = None;
        let progress = live.add(vec![Segment::new("working", None)]).unwrap();
        stack.push(Notification::ok("cache warmed"), secs(0));
        stack.push(Notification::warning("slow mirror").title("Net"), secs(0));
        assert!(stack.is_empty(), "nothing is stacked");
        stack
            .present(&mut live, &stream, &mut region, secs(0))
            .unwrap();
        stack
            .present(&mut live, &stream, &mut region, secs(9))
            .unwrap();
        assert!(region.is_none());
        live.update(progress, vec![Segment::new("finished", None)])
            .unwrap();
        live.finish().unwrap();
    }
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "✔ ok cache warmed\n⚠ warning Net: slow mirror\nfinished\n"
    );
}

#[test]
fn interactive_toasts_appear_and_vanish() {
    let terminal = target(TargetKind::Terminal, true);
    let mut stack = Notifications::for_target(&terminal).default_ttl(Some(secs(3)));
    let mut out = Vec::new();
    {
        let mut live = LiveCoordinator::new(&mut out, terminal.clone());
        let _body = live.add(vec![Segment::new("downloading", None)]).unwrap();
        // Added after the body when needed, so toasts sit below it.
        let mut region = None;
        stack.push(Notification::info("resumed").title("Net"), secs(0));
        assert_eq!(
            region_text(&terminal.segments(&stack)),
            "ℹ info Net: resumed"
        );
        stack
            .present(&mut live, &terminal, &mut region, secs(1))
            .unwrap();
        assert!(region.is_some());
        stack
            .present(&mut live, &terminal, &mut region, secs(4))
            .unwrap();
        assert!(stack.is_empty() && region.is_none());
        live.finish().unwrap();
    }
    let out = String::from_utf8(out).unwrap();
    assert_eq!(out.matches("ℹ info Net: resumed").count(), 1, "{out:?}");
    let shown = out.find("resumed").unwrap();
    let body_after = out[shown..].find("downloading");
    assert!(body_after.is_some(), "redrawn without the toast: {out:?}");
}
