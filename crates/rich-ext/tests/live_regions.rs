use rich::protocol::{RenderEnvironment, Support, TargetCapabilities};
use rich::{Segment, Theme};
use rich_ext::{
    live::{LiveCoordinator, LiveError},
    target::{RenderTarget, TargetKind},
};
fn target(w: usize, h: usize, interactive: bool) -> RenderTarget {
    RenderTarget::new(
        TargetKind::Custom,
        TargetCapabilities {
            width: w,
            height: h,
            color_system: None,
            interactive,
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
fn regions_reject_stale_foreign_and_control_content() {
    let mut bytes = Vec::new();
    let mut live = LiveCoordinator::new(&mut bytes, target(80, 24, true));
    let a = live.add(row("first")).unwrap();
    let b = live.add(row("second")).unwrap();
    live.refresh().unwrap();
    live.update(b.clone(), row("changed")).unwrap();
    live.refresh().unwrap();
    live.remove(b.clone()).unwrap();
    assert!(matches!(
        live.update(b, row("no")),
        Err(LiveError::InvalidRegion)
    ));
    let mut other = LiveCoordinator::new(Vec::new(), target(80, 24, true));
    assert!(matches!(other.remove(a), Err(LiveError::InvalidRegion)));
    assert!(matches!(
        live.add(vec![Segment::control("\x1bPbad")]),
        Err(LiveError::UnsupportedControl)
    ));
    live.finish().unwrap();
    drop(live);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("first"));
    assert!(text.contains("changed"));
    assert!(!text.contains("\x1bP"));
}
#[test]
fn empty_viewports_emit_no_dynamic_controls_and_pipes_emit_one_final_snapshot() {
    for (w, h) in [(0, 24), (1, 24), (80, 0), (80, 1)] {
        let mut b = Vec::new();
        let mut live = LiveCoordinator::new(&mut b, target(w, h, true));
        live.add(row("hidden")).unwrap();
        live.refresh().unwrap();
        live.finish().unwrap();
        drop(live);
        assert!(b.is_empty());
    }
    let mut b = Vec::new();
    let mut live = LiveCoordinator::new(&mut b, target(80, 24, false));
    let id = live.add(row("old")).unwrap();
    live.refresh().unwrap();
    live.update(id, row("final")).unwrap();
    live.refresh().unwrap();
    live.finish().unwrap();
    drop(live);
    assert_eq!(String::from_utf8(b).unwrap(), "final\n");
}
#[test]
fn tall_content_is_clipped_before_cursor_geometry_and_refresh_is_quiet() {
    let mut b = Vec::new();
    let mut live = LiveCoordinator::new(&mut b, target(5, 4, true));
    live.add(row(&"abcdefgh\n".repeat(200))).unwrap();
    live.refresh().unwrap();
    live.refresh().unwrap();
    live.finish().unwrap();
    drop(live);
    let text = String::from_utf8(b).unwrap();
    assert!(!text.contains("abcde"));
    assert_eq!(text.matches("abcd").count(), 3);
    assert!(!text.contains("200A"));
}

#[test]
fn ordinary_pipe_print_respects_link_policy_and_full_width() {
    let mut caps = target(1, 10, false).capabilities();
    caps.color_system = Some(rich::ColorSystem::Truecolor);
    let target = RenderTarget::new(TargetKind::Capture, caps, Theme::default_theme());
    let mut bytes = Vec::new();
    let mut live = LiveCoordinator::new(&mut bytes, target);
    live.print(&[Segment::new(
        "abc",
        Some(rich::Style::parse("red link https://example.com").unwrap()),
    )])
    .unwrap();
    live.finish().unwrap();
    drop(live);
    let out = String::from_utf8(bytes).unwrap();
    assert!(!out.contains("\x1b]8"));
    assert!(!out.contains('\r'));
    assert!(
        out.contains('a') && out.contains('b') && out.contains('c'),
        "{out:?}"
    );
}

#[test]
fn narrow_plain_print_preserves_every_character_and_normal_newlines() {
    let mut bytes = Vec::new();
    let mut live = LiveCoordinator::new(&mut bytes, target(1, 10, false));
    live.print(&row("abc")).unwrap();
    live.finish().unwrap();
    drop(live);
    assert_eq!(bytes, b"a\nb\nc\n");
}
