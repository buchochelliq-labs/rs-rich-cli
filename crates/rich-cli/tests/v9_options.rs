use std::process::Command;

#[test]
fn explicit_theme_binding_renders_and_exports_without_config() {
    let root = tempfile::tempdir().unwrap();
    let html = root.path().join("theme.html");
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "--no-config",
            "--theme-style",
            "accent=bold #f01234",
            "--print",
            "[accent]THEMED[/]",
            "--export-html",
        ])
        .arg(&html)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let document = std::fs::read_to_string(html).unwrap();
    assert!(document.contains("THEMED"));
    assert!(document.contains("#f01234"), "{document}");
}

#[test]
fn invalid_theme_binding_fails_before_rendering() {
    for binding in ["invalid", "=red", "accent=not-a-style"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rich"))
            .args(["--no-config", "--theme-style", binding, "--print", "hello"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn image_processing_rejects_unsupported_combinations_before_io() {
    for args in [
        vec![
            "image",
            "missing.png",
            "--image-color",
            "ansi256",
            "--image-mode",
            "braille",
        ],
        vec!["image", "missing.png", "--image-dither", "floyd-steinberg"],
        vec!["--print", "hello", "--image-color", "truecolor"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rich"))
            .arg("--no-config")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[cfg(feature = "art")]
#[test]
fn image_processing_emits_indexed_color_and_default_output_is_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("gradient.png");
    rich_art::image::RgbImage::from_fn(16, 8, |x, y| {
        rich_art::image::Rgb([(x * 15) as u8, (y * 30) as u8, 117])
    })
    .save(&path)
    .unwrap();
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rich"))
            .env_remove("NO_COLOR")
            .env("RICH_SIXEL", "0")
            .env("RS_RICH_BATCH_TERMINAL", "1")
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .args(["--no-config", "image"])
            .arg(&path)
            .args(["--image-mode", "blocks", "--width", "16", "--height", "4"])
            .args(extra)
            .output()
            .unwrap()
    };
    let default = run(&[]);
    let explicit = run(&["--image-color", "truecolor", "--image-dither", "none"]);
    assert!(default.status.success());
    assert_eq!(default.stdout, explicit.stdout);
    assert_eq!(default.status.code(), explicit.status.code());
    let quantized = run(&[
        "--image-color",
        "ansi256",
        "--image-dither",
        "floyd-steinberg",
    ]);
    assert!(quantized.status.success(), "{quantized:?}");
    let output = String::from_utf8(quantized.stdout).unwrap();
    assert!(
        output.contains("38;5;") || output.contains("48;5;"),
        "expected indexed ANSI palette: {output:?}"
    );
    assert!(!output.contains("38;2;"));
    assert!(!output.contains("48;2;"));
}
