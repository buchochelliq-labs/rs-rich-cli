#[cfg(feature = "art")]
mod images {
    use rich::protocol::{RenderEnvironment, Support, TargetCapabilities};
    use rich::{ColorSystem, Panel, Theme};
    use rich_art::{ImageArt, ImageMode};
    use rich_ext::target::{RenderTarget, TargetKind};
    fn target(kind: TargetKind) -> RenderTarget {
        RenderTarget::new(
            kind,
            TargetCapabilities {
                width: 10,
                height: 8,
                color_system: Some(ColorSystem::Truecolor),
                interactive: true,
                unicode: true,
                hyperlinks: false,
                sixel: Support::Unsupported,
            },
            Theme::default_theme(),
        )
    }
    fn image(mode: ImageMode) -> ImageArt {
        ImageArt::new(rich_art::image::DynamicImage::ImageRgb8(
            rich_art::image::RgbImage::from_pixel(2, 2, rich_art::image::Rgb([255, 0, 0])),
        ))
        .mode(mode)
        .width(2)
    }
    #[test]
    fn ascii_only_explicit_target_selects_ascii_but_explicit_blocks_remain_opt_in() {
        let mut caps = target(TargetKind::Capture).capabilities();
        caps.unicode = false;
        let target = RenderTarget::new(TargetKind::Capture, caps, Theme::default_theme());
        let auto = target.text(&image(ImageMode::Auto));
        assert!(auto.is_ascii(), "{auto:?}");
        assert_eq!(auto, target.text(&image(ImageMode::Ascii)));
        assert!(target.text(&image(ImageMode::Blocks)).contains('▀'));
    }
    #[test]
    fn nested_images_use_injected_capabilities_even_on_a_sixel_terminal() {
        if std::env::var_os("RICH_ART_TARGET_CHILD").is_none() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "images::nested_images_use_injected_capabilities_even_on_a_sixel_terminal",
                ])
                .env("RICH_ART_TARGET_CHILD", "1")
                .env("TERM", "xterm-sixel")
                .env("NO_COLOR", "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
            return;
        }
        let t = target(TargetKind::Terminal);
        let actual = t.text(&Panel::new(Box::new(image(ImageMode::Auto))));
        let expected = t.text(&Panel::new(Box::new(image(ImageMode::Blocks))));
        assert_eq!(actual, expected);
        assert!(!actual.contains("\x1bP"));
    }
    #[test]
    fn strict_environment_entry_rejects_sixel_for_exports() {
        for kind in [TargetKind::Capture, TargetKind::Html, TargetKind::Svg] {
            let t = target(kind);
            let c = t.console();
            assert!(image(ImageMode::Sixel)
                .render_with_environment(&c, &c.options(), &t)
                .is_err());
            assert_eq!(t.capabilities().sixel, Support::Unsupported);
            let s = t.segments(&Panel::new(Box::new(image(ImageMode::Auto))));
            assert!(s.iter().all(|s| !s.control));
            assert!(s.iter().any(|s| s.style.is_some()));
        }
    }
}

#[test]
fn doctor_reports_configured_capability_provenance() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["doctor", "--no-config", "--width", "42", "--report", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["terminal"]["provenance"]["width"], "configured");
}

#[test]
fn doctor_color_override_and_fallback_dimensions_have_honest_origins() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["doctor", "--no-config", "--no-color", "--report", "json"])
        .env_remove("COLUMNS")
        .env_remove("LINES")
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value["terminal"]["provenance"]["color_system"],
        "configured"
    );
    assert_eq!(value["terminal"]["provenance"]["width"], "default");
    assert_eq!(value["terminal"]["provenance"]["height"], "default");
}
