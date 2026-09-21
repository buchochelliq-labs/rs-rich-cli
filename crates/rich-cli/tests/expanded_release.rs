use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Segment, Theme};
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
            hyperlinks: true,
            sixel: Support::Confirmed,
        },
        Theme::default_theme(),
    )
}
#[cfg(feature = "art")]
#[test]
fn nested_transformed_image_respects_capture_target_and_layout_bounds() {
    use rich_art::{image, ImageArt, ImageTransforms, Rotation};
    use rich_ext::layout::{Axis, Constraint, LayoutNode};
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        3,
        5,
        image::Rgba([255, 0, 0, 128]),
    ));
    let node = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(ImageArt::new(image).transforms(ImageTransforms {
                rotation: Rotation::Clockwise90,
                grayscale: true,
                ..Default::default()
            })))
            .width(Constraint::fixed(6)),
            LayoutNode::leaf(Box::new(rich::Text::new("image"))),
        ],
    );
    let segments = target(TargetKind::Capture, 12, 5).segments(&node);
    assert!(!segments.is_empty());
    assert!(segments
        .iter()
        .all(|s| !s.control && !s.text.contains('\x1b')));
    let rows = Segment::split_lines(&segments);
    assert!(rows.len() <= 5);
    assert!(rows
        .iter()
        .all(|r| r.iter().map(Segment::cell_length).sum::<usize>() <= 12));
}
#[test]
fn typed_diagnostic_event_exports_with_order_and_no_terminal_controls() {
    use rich_ext::diagnostic::{Diagnostic, SourceSnippet};
    use rich_ext::event::{EventView, Message, StructuredEvent, Value};
    let event = StructuredEvent::new(Message::Literal("invalid value".into()))
        .field("second", Value::Integer(2))
        .field("first", Value::Bool(true))
        .field_order(vec!["first".into()])
        .diagnostic(
            Diagnostic::new("expected integer")
                .view(EventView::Expanded)
                .snippet(
                    SourceSnippet::new("input.rs".into(), "count = x".into(), 8..9, 1).unwrap(),
                ),
        )
        .view(EventView::Expanded);
    let segments = target(TargetKind::Html, 80, 20).segments(&event);
    let html =
        rich::export::export_html_inline(&segments, &rich::terminal_theme::DEFAULT_TERMINAL_THEME);
    let svg = rich::svg::export_svg(
        &segments,
        &rich::terminal_theme::DEFAULT_TERMINAL_THEME,
        "diagnostic",
        "fixed",
        80,
    );
    for output in [&html, &svg] {
        assert!(output.replace("&#160;", " ").contains("expected integer"));
        assert!(output.contains("input.rs"));
        assert!(output.find("first").unwrap() < output.find("second").unwrap());
        assert!(!output.contains('\x1b'));
    }
}
#[cfg(feature = "art")]
#[test]
fn bayer_transforms_and_template_exports_match_across_workers() {
    use std::process::Command;
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    for directory in ["a", "b"] {
        std::fs::create_dir_all(input.join(directory)).unwrap();
        rich_art::image::RgbImage::from_fn(8, 4, |x, y| {
            rich_art::image::Rgb([x as u8 * 30, y as u8 * 60, 128])
        })
        .save(input.join(directory).join("report.png"))
        .unwrap();
    }
    let run = |name: &str, jobs: &str| {
        let out = temp.path().join(name);
        let result = Command::new(env!("CARGO_BIN_EXE_rich"))
            .env_remove("NO_COLOR")
            .env("TERM", "xterm-sixel")
            .args([
                "--no-config",
                "image",
                "--batch",
                "--batch-preserve-dirs",
                "--batch-input-root",
                input.to_str().unwrap(),
                "--batch-name-template",
                "{index}-{stem}.{output_ext}",
                "--image-mode",
                "blocks",
                "--image-rotate",
                "90",
                "--image-grayscale",
                "--image-color",
                "ansi256",
                "--image-dither",
                "bayer4x4",
                "--width",
                "8",
                "--jobs",
                jobs,
                "--export-html",
                out.to_str().unwrap(),
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        ["a/1-report.html", "b/2-report.html"].map(|p| std::fs::read(out.join(p)).unwrap())
    };
    assert_eq!(run("serial", "1"), run("parallel", "4"));
}
