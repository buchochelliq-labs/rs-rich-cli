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
            vec!["image", "missing.png", "--image-fit", "stretch"],
            "contain or cover",
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
