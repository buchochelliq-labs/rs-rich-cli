//! WebP images through the CLI: `rich image`, `rich view` and `rich gif` read
//! rs-rich-art's WebP fixtures (made from `ball.gif` by
//! `crates/rich-art/tests/fixtures/make_webp.py`), and Markdown takes a WebP
//! image like any other.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn art(name: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("rich-art");
    let path = match name {
        "ball.gif" => dir.join("examples/assets").join(name),
        _ => dir.join("tests/fixtures").join(name),
    };
    path.display().to_string()
}

fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .arg("--no-config")
        .args(args)
        .env_remove("NO_COLOR")
        .env("COLUMNS", "80")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(feature = "art")]
#[test]
fn a_webp_renders_as_the_picture_it_was_made_from() {
    let ascii = ["--image-mode", "ascii", "--width", "24", "--no-color"];
    let webp = run(&[&["image", &art("ball.webp")][..], &ascii].concat(), b"");
    assert!(webp.status.success(), "{}", text(&webp.stderr));
    assert!(!text(&webp.stdout).trim().is_empty());
    // ball.webp is ball.gif's first frame, losslessly.
    let gif = run(&[&["image", &art("ball.gif")][..], &ascii].concat(), b"");
    assert_eq!(text(&webp.stdout), text(&gif.stdout));
    // By content alone: stdin has no name.
    let bytes = std::fs::read(art("ball.webp")).unwrap();
    let piped = run(&[&["image", "-"][..], &ascii].concat(), &bytes);
    assert!(piped.status.success(), "{}", text(&piped.stderr));
    assert_eq!(text(&piped.stdout), text(&webp.stdout));
    // `rich view` routes a `.webp` to the image renderer.
    let plain = ["--width", "24", "--no-color"];
    let view = run(&[&["view", &art("ball.webp")][..], &plain].concat(), b"");
    assert!(view.status.success(), "{}", text(&view.stderr));
    let image = run(&[&["image", &art("ball.webp")][..], &plain].concat(), b"");
    assert_eq!(text(&view.stdout), text(&image.stdout));
    assert!(!text(&view.stdout).trim().is_empty());
}

#[cfg(feature = "art")]
#[test]
fn an_animated_webp_plays_like_a_gif() {
    // Off a terminal, playback draws the first frame and says so.
    let gif = run(&["gif", &art("ball-animated.webp"), "--width", "24"], b"");
    assert!(gif.status.success(), "{}", text(&gif.stderr));
    assert!(!text(&gif.stdout).trim().is_empty());
    assert!(
        text(&gif.stderr).contains("first frame"),
        "{}",
        text(&gif.stderr)
    );
    // `rich view` plays an animated WebP, as it does a GIF.
    let view = run(&["view", &art("ball-animated.webp"), "--width", "24"], b"");
    assert!(view.status.success(), "{}", text(&view.stderr));
    assert!(
        text(&view.stderr).contains("first frame"),
        "{}",
        text(&view.stderr)
    );
    // `rich image` draws its first frame.
    let image = run(&["image", &art("ball-animated.webp"), "--width", "24"], b"");
    assert!(image.status.success(), "{}", text(&image.stderr));
    assert!(!text(&image.stdout).trim().is_empty());
}

#[test]
fn a_bare_webp_path_points_at_the_image_commands() {
    // As for a PNG: `rich photo.webp` names the commands that draw it.
    let out = run(&[&art("ball.webp")], b"");
    assert!(
        text(&out.stderr).contains("rich --image"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn markdown_with_a_webp_image_renders() {
    let markdown = format!("# Ball\n\n![a bouncing ball]({})\n", art("ball.webp"));
    let out = run(&["--markdown", "-", "--no-color"], markdown.as_bytes());
    assert!(out.status.success(), "{}", text(&out.stderr));
    let shown = text(&out.stdout);
    assert!(shown.contains("Ball"), "{shown}");
    assert!(shown.contains("a bouncing ball"), "{shown}");
    // A `data:image/webp` image is allowed (markdown-it's validateLink), not
    // left as source text.
    let data = "![dot](data:image/webp;base64,UklGRg==)\n";
    let out = run(&["--markdown", "-", "--no-color"], data.as_bytes());
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        !text(&out.stdout).contains("data:image/webp"),
        "{}",
        text(&out.stdout)
    );
}
