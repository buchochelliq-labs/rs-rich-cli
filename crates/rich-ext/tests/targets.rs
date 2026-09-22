use rich::protocol::{ConsoleEnvironment, RenderEnvironment, Support, TargetCapabilities};
use rich::{ColorSystem, Console, ConsoleOptions, Renderable, Segment, Style, Text, Theme};
use rich_ext::target::{RenderTarget, TargetKind};

fn target(kind: TargetKind, width: usize, height: usize) -> RenderTarget {
    RenderTarget::new(
        kind,
        TargetCapabilities {
            width,
            height,
            color_system: Some(ColorSystem::Truecolor),
            interactive: true,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Confirmed,
        },
        Theme::default_theme(),
    )
}
struct PanicRenderable;
impl Renderable for PanicRenderable {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        panic!("empty viewport rendered child")
    }
}
#[test]
fn empty_viewports_do_not_invoke_children() {
    for (w, h) in [(0, 8), (8, 0), (0, 0)] {
        let t = target(TargetKind::Capture, w, h);
        assert!(t.segments(&PanicRenderable).is_empty());
        assert_eq!(t.text(&PanicRenderable), "");
    }
}
struct StyledControls;
impl Renderable for StyledControls {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        vec![
            Segment::new(
                "hello",
                Some(Style::parse("red link https://example.com").unwrap()),
            ),
            Segment {
                text: "\x1b[2J".into(),
                style: None,
                control: true,
            },
        ]
    }
}
#[test]
fn capture_preserves_colour_but_removes_links_and_controls() {
    let t = target(TargetKind::Capture, 80, 25);
    let out = t.text(&StyledControls);
    assert!(out.contains("\x1b[31mhello"), "{out:?}");
    assert!(!out.contains("https://"));
    assert!(!out.contains("\x1b[2J"));
    assert_eq!(t.capabilities().sixel, Support::Unsupported);
}
#[test]
fn plain_stream_normalises_requested_terminal_capabilities() {
    let t = target(TargetKind::PlainStream, 80, 25);
    assert_eq!(t.text(&StyledControls), "hello");
    assert!(!t.capabilities().interactive);
    assert_eq!(t.capabilities().color_system, None);
}
struct NestedContext;
impl Renderable for NestedContext {
    fn rich_render(&self, c: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        assert_eq!(c.render_environment().unwrap().capabilities().width, 80);
        Text::new("nested").rich_render(c, o)
    }
}
#[test]
fn target_context_is_attached_without_changing_legacy_console() {
    assert!(Console::new().render_environment().is_none());
    assert!(target(TargetKind::Capture, 80, 25)
        .text(&NestedContext)
        .contains("nested"));
    fn assert_send<T: Send>() {}
    assert_send::<Console>();
}
#[test]
fn explicit_target_ignores_ambient_terminal_settings() {
    if std::env::var_os("RICH_TARGET_CHILD").is_some() {
        let out = target(TargetKind::Capture, 80, 25).text(&StyledControls);
        assert!(out.contains("\x1b[31mhello"));
        return;
    }
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "explicit_target_ignores_ambient_terminal_settings",
        ])
        .env("RICH_TARGET_CHILD", "1")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .env("COLUMNS", "1")
        .env("LINES", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
}

#[test]
fn capability_overrides_keep_configuration_and_inference_distinct() {
    use rich_ext::target::{
        resolve_capabilities, CapabilityOrigin, TargetObservations, TargetOverrides,
    };
    let result = resolve_capabilities(
        TargetObservations {
            width: None,
            height: None,
            is_terminal: true,
            color_system: Some(ColorSystem::Truecolor),
            unicode: true,
            hyperlinks: true,
            sixel_hint: Support::Inferred,
        },
        TargetOverrides {
            width: Some(42),
            color_system: Some(None),
            ..Default::default()
        },
    );
    assert_eq!(result.capabilities.width, 42);
    assert_eq!(result.capabilities.height, 25);
    assert_eq!(result.capabilities.color_system, None);
    assert!(result
        .origins
        .contains(&("width".into(), CapabilityOrigin::Configured)));
    assert!(result
        .origins
        .contains(&("height".into(), CapabilityOrigin::Default)));
    assert!(result
        .origins
        .contains(&("sixel".into(), CapabilityOrigin::Inferred)));
}
