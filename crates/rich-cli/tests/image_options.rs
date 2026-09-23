//! Public image fitting and batch help contracts.
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .arg("--no-config")
        .args(args)
        .env_remove("NO_COLOR")
        .env("COLUMNS", "80")
        .output()
        .unwrap()
}

#[test]
fn image_fit_requires_a_target_height() {
    let out = run(&["image", "missing.png", "--image-fit", "contain"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--image-fit requires --height"));
}

#[test]
fn image_options_reject_invalid_values_and_other_modes() {
    for (args, message) in [
        (
            vec!["image", "missing.png", "--image-fit", "squash"],
            "contain, cover or stretch",
        ),
        (
            vec!["image", "missing.png", "--image-background", "purple"],
            "#RRGGBB",
        ),
        (
            vec!["image", "missing.png", "--image-background", "#12345g"],
            "#RRGGBB",
        ),
        (
            vec!["--print", "hello", "--image-background", "#000000"],
            "only has an effect with --image",
        ),
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn jobs_help_states_parallel_export_requirement() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let jobs = text.lines().find(|line| line.contains("--jobs N")).unwrap();
    assert!(jobs.contains("Parallel file-export workers"), "{jobs}");
}

#[cfg(feature = "art")]
#[test]
fn image_fit_cli_routes_to_library_and_background_is_applied() {
    use rich_art::{image, ImageArt, ImageFit, ImageMode};
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(8, 2, |x, _| {
        if x < 3 {
            image::Rgba([0, 0, 0, 255])
        } else {
            image::Rgba([255, 0, 0, 0])
        }
    }));
    let path = std::env::temp_dir().join(format!("rich-fit-{}.png", std::process::id()));
    image.save(&path).unwrap();
    let console = rich::Console::builder()
        .width(80)
        .force_terminal(false)
        .color_system(None)
        .build();
    let mut outputs = Vec::new();
    for (name, fit) in [("contain", ImageFit::Contain), ("cover", ImageFit::Cover)] {
        let out = run(&[
            "image",
            path.to_str().unwrap(),
            "--image-mode",
            "ascii",
            "--width",
            "8",
            "--height",
            "4",
            "--image-fit",
            name,
            "--image-background",
            "#FFFFFF",
            "--no-color",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let expected = console.render_to_string(
            &ImageArt::new(image.clone())
                .mode(ImageMode::Ascii)
                .width(8)
                .height(4)
                .fit(fit)
                .background([255, 255, 255]),
        );
        let actual = String::from_utf8(out.stdout).unwrap();
        assert_eq!(
            actual.trim_end_matches('\n'),
            expected.trim_end_matches('\n')
        );
        outputs.push(actual);
    }
    assert_ne!(
        outputs[0], outputs[1],
        "contain and cover must visibly differ"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn crop_anchor_requires_cover_and_rejects_invalid_names() {
    for (args, expected) in [
        (
            vec!["image", "missing.png", "--image-anchor", "top"],
            "requires --image-fit cover",
        ),
        (
            vec![
                "image",
                "missing.png",
                "--image-fit",
                "contain",
                "--height",
                "8",
                "--image-anchor",
                "top",
            ],
            "requires --image-fit cover",
        ),
        (
            vec!["image", "missing.png", "--image-anchor", "banana"],
            "--image-anchor requires center",
        ),
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
}

#[cfg(feature = "art")]
#[test]
fn cli_crop_anchors_keep_different_edges_of_the_image() {
    use rich_art::image;
    let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(24, 4, |x, _| {
        let value = if x < 12 { 0 } else { 255 };
        image::Rgba([value, value, value, 255])
    }));
    let root = std::env::temp_dir().join(format!("rich-anchor-cli-{}.png", std::process::id()));
    source.save(&root).unwrap();
    let mut results = Vec::new();
    for anchor in ["left", "right"] {
        let output = run(&[
            "image",
            root.to_str().unwrap(),
            "--image-mode",
            "ascii",
            "--width",
            "4",
            "--height",
            "2",
            "--image-fit",
            "cover",
            "--image-anchor",
            anchor,
            "--no-color",
        ]);
        assert!(output.status.success(), "{output:?}");
        results.push(output.stdout);
    }
    std::fs::remove_file(root).unwrap();
    assert_ne!(results[0], results[1]);
}

#[cfg(feature = "art")]
#[test]
fn still_transforms_and_bayer_export_and_worker_options_are_effective() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    rich_art::image::RgbImage::from_pixel(4, 2, rich_art::image::Rgb([128, 64, 192]))
        .save(&source)
        .unwrap();
    let export = temp.path().join("transformed.html");
    let output = run(&[
        "image",
        source.to_str().unwrap(),
        "--image-mode",
        "blocks",
        "--image-rotate",
        "90",
        "--image-flip-horizontal",
        "--image-grayscale",
        "--image-color",
        "ansi256",
        "--image-dither",
        "bayer4x4",
        "--width",
        "4",
        "--export-html",
        export.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let html = std::fs::read_to_string(export).unwrap();
    assert!(html.contains("▀"));
    assert!(!html.contains("\x1bP"));
    let invalid = run(&["image", source.to_str().unwrap(), "--image-rotate", "45"]);
    assert_eq!(invalid.status.code(), Some(2));
    let invalid = run(&["gif", "missing.gif", "--image-grayscale"]);
    assert_eq!(invalid.status.code(), Some(2));
}

#[cfg(feature = "art")]
#[test]
fn transform_config_and_cli_override_match_explicit_options() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    rich_art::image::RgbImage::from_fn(6, 2, |x, y| {
        rich_art::image::Rgb([x as u8 * 40, y as u8 * 200, 40])
    })
    .save(&source)
    .unwrap();
    let config = temp.path().join("config.toml");
    std::fs::write(&config, "[defaults]\nimage_rotate = 90\nimage_flip_horizontal = true\nimage_grayscale = true\nimage_color = 'ansi256'\nimage_dither = 'bayer4x4'\n").unwrap();
    let configured = std::process::Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "image",
            source.to_str().unwrap(),
            "--image-mode",
            "blocks",
            "--width",
            "6",
            "--no-image-grayscale",
            "--image-rotate",
            "270",
        ])
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    let explicit = run(&[
        "image",
        source.to_str().unwrap(),
        "--image-mode",
        "blocks",
        "--width",
        "6",
        "--image-rotate",
        "270",
        "--image-flip-horizontal",
        "--image-color",
        "ansi256",
        "--image-dither",
        "bayer4x4",
    ]);
    assert!(
        configured.status.success(),
        "{}",
        String::from_utf8_lossy(&configured.stderr)
    );
    assert!(explicit.status.success());
    assert_eq!(configured.stdout, explicit.stdout);
}

#[test]
fn new_image_options_reject_bad_values_combinations_and_other_modes() {
    for (args, message) in [
        (
            vec!["image", "missing.png", "--image-color", "ansi8"],
            "truecolor, ansi256, ansi16 or grayscale",
        ),
        (
            vec!["image", "missing.png", "--image-brightness", "-1"],
            "--image-brightness requires a finite number of at least 0",
        ),
        (
            vec!["image", "missing.png", "--image-contrast", "NaN"],
            "--image-contrast requires a finite number",
        ),
        (
            vec!["image", "missing.png", "--image-gamma", "0"],
            "--image-gamma requires a finite number greater than 0",
        ),
        (
            vec!["image", "missing.png", "--image-gamma", "bright"],
            "--image-gamma requires",
        ),
        (
            vec!["image", "missing.png", "--image-max-width", "0"],
            "--image-max-width requires a positive integer",
        ),
        (
            vec!["image", "missing.png", "--image-max-height", "-3"],
            "--image-max-height requires a positive integer",
        ),
        (
            vec!["image", "missing.png", "--image-mode", "tiles"],
            "quadrants",
        ),
        (
            vec![
                "image",
                "missing.png",
                "--image-mode",
                "braille",
                "--image-color",
                "ansi16",
            ],
            "ascii, blocks or quadrants",
        ),
        (
            vec![
                "image",
                "missing.png",
                "--image-color",
                "truecolor",
                "--image-dither",
                "bayer4x4",
            ],
            "requires --image-color ansi256, ansi16 or grayscale",
        ),
        (
            vec!["--print", "hi", "--image-gamma", "2"],
            "--image-gamma only has an effect with --image",
        ),
        (
            vec!["--print", "hi", "--image-max-width", "4"],
            "--image-max-width only has an effect with --image",
        ),
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(message),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[cfg(feature = "art")]
#[test]
fn new_image_options_route_to_the_library_exactly() {
    use rich_art::{image, Dither, ImageArt, ImageColorMode, ImageFit, ImageMode, ImageTransforms};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.png");
    let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(9, 7, |x, y| {
        image::Rgba([
            (x * 29) as u8,
            (y * 37) as u8,
            90,
            if x == 4 { 0 } else { 255 },
        ])
    }));
    source.save(&path).unwrap();
    let console = rich::Console::builder()
        .width(80)
        .force_terminal(false)
        .color_system(None)
        .build();
    let base = [
        "image",
        path.to_str().unwrap(),
        "--width",
        "8",
        "--image-brightness",
        "1.3",
        "--image-contrast",
        "0.8",
        "--image-gamma",
        "1.5",
    ];
    let transforms = ImageTransforms {
        brightness: 1.3,
        contrast: 0.8,
        gamma: 1.5,
        ..Default::default()
    };
    let cases: Vec<(Vec<&str>, ImageArt)> = vec![
        (
            vec![
                "--image-mode",
                "quadrants",
                "--image-color",
                "ansi16",
                "--image-dither",
                "floyd-steinberg",
            ],
            ImageArt::new(source.clone())
                .mode(ImageMode::Quadrants)
                .color_mode(ImageColorMode::Ansi16)
                .dither(Dither::FloydSteinberg),
        ),
        (
            vec![
                "--image-mode",
                "ascii",
                "--image-color",
                "grayscale",
                "--image-max-height",
                "2",
            ],
            ImageArt::new(source.clone())
                .mode(ImageMode::Ascii)
                .color_mode(ImageColorMode::Grayscale)
                .max_height(2),
        ),
        (
            vec![
                "--image-mode",
                "ascii",
                "--height",
                "3",
                "--image-fit",
                "stretch",
                "--image-max-width",
                "5",
            ],
            ImageArt::new(source.clone())
                .mode(ImageMode::Ascii)
                .height(3)
                .fit(ImageFit::Stretch)
                .max_width(5),
        ),
    ];
    for (extra, art) in cases {
        let mut args = base.to_vec();
        args.extend(&extra);
        args.push("--no-color");
        let out = run(&args);
        assert!(
            out.status.success(),
            "{extra:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let expected = console.render_to_string(&art.width(8).color(false).transforms(transforms));
        assert_eq!(
            String::from_utf8(out.stdout)
                .unwrap()
                .trim_end_matches('\n'),
            expected.trim_end_matches('\n'),
            "{extra:?}"
        );
    }
}

#[cfg(feature = "art")]
#[test]
fn new_image_options_load_from_config_and_validate_there() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    rich_art::image::RgbImage::from_fn(8, 6, |x, y| {
        rich_art::image::Rgb([x as u8 * 30, y as u8 * 40, 60])
    })
    .save(&source)
    .unwrap();
    let config = temp.path().join("config.toml");
    std::fs::write(
        &config,
        "[defaults]\nimage_color = 'grayscale'\nimage_dither = 'bayer4x4'\nimage_brightness = 2\nimage_gamma = 0.8\nimage_max_width = 5\n",
    )
    .unwrap();
    let configured = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "image",
            source.to_str().unwrap(),
        ])
        .args(["--image-mode", "quadrants", "--width", "8"])
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    let explicit = run(&[
        "image",
        source.to_str().unwrap(),
        "--image-mode",
        "quadrants",
        "--width",
        "8",
        "--image-color",
        "grayscale",
        "--image-dither",
        "bayer4x4",
        "--image-brightness",
        "2",
        "--image-gamma",
        "0.8",
        "--image-max-width",
        "5",
    ]);
    assert!(
        configured.status.success(),
        "{}",
        String::from_utf8_lossy(&configured.stderr)
    );
    assert!(explicit.status.success());
    assert_eq!(configured.stdout, explicit.stdout);
    let first = String::from_utf8(explicit.stdout).unwrap();
    assert_eq!(
        first.lines().next().unwrap().chars().count(),
        5,
        "{first:?}"
    );

    for bad in [
        "image_gamma = 0",
        "image_contrast = -1",
        "image_max_height = 0",
        "image_color = 'sepia'",
        "image_fit = 'squash'",
    ] {
        std::fs::write(&config, format!("[defaults]\n{bad}\n")).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_rich"))
            .args([
                "--config",
                config.to_str().unwrap(),
                "image",
                source.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{bad}");
    }
}

#[cfg(feature = "art")]
#[test]
fn quadrant_diff_heatmaps_render_in_exports() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("rich-art/tests/fixtures");
    let temp = tempfile::tempdir().unwrap();
    let html = temp.path().join("diff.html");
    let out = run(&[
        "--diff",
        dir.join("halo-before.png").to_str().unwrap(),
        dir.join("halo-after.png").to_str().unwrap(),
        "--width",
        "40",
        "--image-mode",
        "quadrants",
        "--threshold",
        "100",
        "--export-html",
        html.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let document = std::fs::read_to_string(html).unwrap();
    assert!(
        document
            .chars()
            .any(|c| matches!(c, '▘' | '▀' | '▌' | '▛' | '▚' | '▜' | '▙' | '█')),
        "no quadrant glyphs in the export"
    );
}
