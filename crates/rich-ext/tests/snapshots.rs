#![cfg(feature = "testing")]
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Text, Theme};
use rich_ext::{
    target::{RenderTarget, TargetKind},
    testing::RenderSnapshot,
};
fn target() -> RenderTarget {
    RenderTarget::new(
        TargetKind::Capture,
        TargetCapabilities {
            width: 20,
            height: 8,
            color_system: Some(ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}
#[test]
fn style_only_differences_are_visible_and_serialization_is_stable() {
    let t = target();
    let a = RenderSnapshot::capture(&t, &Text::styled("hello", "red"));
    let b = RenderSnapshot::capture(&t, &Text::styled("hello", "blue"));
    assert_eq!(a.plain, "hello");
    assert_eq!(a.plain, b.plain);
    assert!(a.diff(&b).unwrap().contains("foreground"));
    assert_eq!(
        a.to_json().unwrap(),
        RenderSnapshot::capture(&t, &Text::styled("hello", "red"))
            .to_json()
            .unwrap()
    );
    assert_eq!((a.width, a.height, a.schema_version), (20, 8, 1));
    assert!(a.ansi.contains("\x1b[31m"));
}
#[test]
fn visible_diff_identifies_changed_line_and_metadata_does_not_hide_links() {
    let t = target();
    let a = RenderSnapshot::capture(&t, &Text::new("one\ntwo"));
    let b = RenderSnapshot::capture(&t, &Text::new("one\nthree"));
    let diff = a.diff(&b).unwrap();
    assert!(diff.contains("-two"));
    assert!(diff.contains("+three"));
    assert_eq!(a.diff(&a), None);
}
