//! A self-contained tour of the public CLI and renderable APIs.
use super::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type WatchChild = Arc<Mutex<Option<std::process::Child>>>;

/// Recognize a flag only in option position, never as an option's value.
pub(super) fn requested(args: &[String]) -> bool {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            iter.next();
            continue;
        }
        if arg == "--demo" {
            return true;
        }
    }
    false
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let options = options(args);
    let (no_color, delay) = match options {
        Ok(options) => options,
        Err(message) => return emit_error(false, ExitClass::Usage, &message),
    };
    match tour(no_color, delay) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => emit_error(false, ExitClass::Input, &format!("demo: {error}")),
    }
}

fn options(args: &[String]) -> Result<(bool, Duration), String> {
    let mut no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    let mut seconds = 3.0;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--demo" | "--no-config" => {}
            "--no-color" => no_color = true,
            "--demo-delay" => {
                seconds = iter.next().and_then(|value| value.parse::<f64>().ok())
                    .filter(|value| value.is_finite() && (0.0..=60.0).contains(value))
                    .ok_or("--demo-delay expects seconds between 0 and 60")?;
            }
            _ => return Err(format!("--demo accepts only --demo-delay SECONDS, --no-color and --no-config; unexpected {arg}")),
        }
    }
    // A pipe gets a finite transcript, without terminal pauses or controls.
    Ok((
        no_color,
        if std::io::stdout().is_terminal() {
            Duration::from_secs_f64(seconds)
        } else {
            Duration::ZERO
        },
    ))
}

pub(super) fn section(console: &Console, delay: Duration, title: &str) {
    if !delay.is_zero() {
        let _ = std::io::stdout().flush();
        std::thread::sleep(delay);
    }
    console.print(&Rule::new(title));
}

fn command(
    console: &Console,
    no_color: bool,
    label: &str,
    args: Vec<String>,
) -> std::io::Result<()> {
    console.print(&Text::new(format!("$ rich {label}")));
    let mut full = vec!["--no-pager".to_owned()];
    if !args.iter().any(|arg| arg == "--config") {
        full.push("--no-config".into());
    }
    if no_color {
        full.push("--no-color".into());
    }
    full.extend(args);
    let cli = parse(&full).map_err(std::io::Error::other)?;
    if let Some(cli) = cli {
        if run(cli) != ExitCode::SUCCESS {
            return Err(std::io::Error::other(format!("example failed: {label}")));
        }
    }
    Ok(())
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn tour(no_color: bool, delay: Duration) -> std::io::Result<()> {
    println!("rich --demo: guided suite tour (3 seconds per section by default). Ctrl+C to stop.");
    println!("Examples are offline; exports use a temporary directory. Configuration is ignored.");
    let root = tempfile::tempdir()?;
    let watch_child: WatchChild = Arc::new(Mutex::new(None));
    if std::io::stdout().is_terminal() {
        // Write through a duplicated OS handle: GIF playback owns the Rust
        // stdout lock for its entire animation, so taking that lock here can
        // delay interruption until playback has already returned.
        #[cfg(unix)]
        let mut terminal = {
            use std::os::fd::AsFd;
            std::fs::File::from(std::io::stdout().as_fd().try_clone_to_owned()?)
        };
        #[cfg(windows)]
        let mut terminal = {
            use std::os::windows::io::AsHandle;
            std::fs::File::from(std::io::stdout().as_handle().try_clone_to_owned()?)
        };
        let cleanup = root.path().to_owned();
        let child = Arc::clone(&watch_child);
        ctrlc::set_handler(move || {
            if let Ok(mut slot) = child.lock() {
                if let Some(mut child) = slot.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            let _ = std::fs::remove_dir_all(&cleanup);
            // GIF playback hides the cursor. Restore it even if interrupted
            // mid-frame, before exiting without Rust destructors.
            let _ = terminal.write_all(b"\x1b[?25h\x1b[0m\n");
            let _ = terminal.flush();
            std::process::exit(130);
        })
        .map_err(std::io::Error::other)?;
    }
    let console = run_demo(no_color, delay);
    let file = |name: &str| root.path().join(name).to_string_lossy().into_owned();
    for (name, contents) in [
        ("first.json", "{\"release\":\"0.0.8\",\"ready\":true}"),
        ("second.json", "{\"features\":[\"config\",\"batch\",\"art\"]}"),
        ("events.jsonl", "{\"event\":\"start\",\"count\":1}\n{\"event\":\"finish\",\"count\":2}\n"),
        ("logs.jsonl", "{\"level\":\"info\",\"message\":\"Tour started\"}\n{\"level\":\"warn\",\"message\":\"Example warning\"}\n"),
        ("notes.ipynb", r##"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"markdown","metadata":{},"source":["# Notebook\n","Rich renders notebook cells."]},{"cell_type":"code","metadata":{},"execution_count":1,"source":["print('vroom vroom')"],"outputs":[{"output_type":"stream","name":"stdout","text":["vroom vroom\n"]}]}]}"##),
        ("demo.toml", "[defaults]\nwidth = 60\n[profiles.preview]\npanel = 'rounded'\n"),
    ] { std::fs::write(file(name), contents)?; }

    for (title, mode, name) in [
        ("JSON Lines", "--jsonl", "events.jsonl"),
        ("Structured logs", "--log", "logs.jsonl"),
        ("Notebook", "--ipynb", "notes.ipynb"),
    ] {
        section(&console, delay, title);
        command(
            &console,
            no_color,
            &format!("{mode} {name}"),
            vec![mode.into(), file(name)],
        )?;
    }
    section(&console, delay, "Configuration profiles");
    console.print(&Text::new(
        "$ rich --config demo.toml --profile preview config show",
    ));
    let args = vec![
        "--config".into(),
        file("demo.toml"),
        "--profile".into(),
        "preview".into(),
        "config".into(),
        "show".into(),
    ];
    let output = config::inspect(&args, &ConfigRoots::default())
        .map_err(std::io::Error::other)?
        .ok_or_else(|| std::io::Error::other("config example produced no result"))?;
    console.print(&Json::new(&output).map_err(std::io::Error::other)?);
    command(
        &console,
        no_color,
        "--config demo.toml --profile preview first.json",
        vec![
            "--config".into(),
            file("demo.toml"),
            "--profile".into(),
            "preview".into(),
            file("first.json"),
        ],
    )?;

    section(&console, delay, "Batch planning");
    let exports = root.path().join("exports");
    std::fs::create_dir(&exports)?;
    let export = exports.join("out.html").to_string_lossy().into_owned();
    command(
        &console,
        no_color,
        "--batch --dry-run --export-html exports/out.html first.json second.json",
        vec![
            "--batch".into(),
            "--dry-run".into(),
            "--export-html".into(),
            export.clone(),
            file("first.json"),
            file("second.json"),
        ],
    )?;
    section(&console, delay, "Parallel exports");
    command(
        &console,
        no_color,
        "--batch --jobs 2 --export-html exports/out.html first.json second.json",
        vec![
            "--batch".into(),
            "--jobs".into(),
            "2".into(),
            "--export-html".into(),
            export,
            file("first.json"),
            file("second.json"),
        ],
    )?;
    console.print(&Text::new(
        "Created first.html and second.html in the temporary demo directory.",
    ));
    command(
        &console,
        no_color,
        "--json first.json --export-svg preview.svg",
        vec![
            "--json".into(),
            file("first.json"),
            "--export-svg".into(),
            file("preview.svg"),
        ],
    )?;
    console.print(&Text::new(
        "Created preview.svg; all demo exports are removed when the tour finishes.",
    ));

    section(&console, delay, "Watch updates");
    watch(&console, no_color, &file("first.json"), &watch_child)?;
    section(&console, delay, "Pager, input and confidence controls");
    console.print(&Text::new("--auto-pager opens a pager for tall TTY output; --no-pager opts out.\nURL fetch, encoding, sanitization and JSON reports support scripts and CI.\nThe tour stays offline and does not open an external pager."));
    command(
        &console,
        no_color,
        "--print --sanitize '<ESC>[31m visible controls'",
        strings(&["--print", "--sanitize", "\x1b[31m visible controls"]),
    )?;

    #[cfg(feature = "art")]
    art(&console, no_color, delay, root.path())?;
    #[cfg(not(feature = "art"))]
    {
        section(&console, delay, "Rich art");
        console.print(&Text::new("The art feature is disabled in this build; enable it for FIGlet, images, GIF and image diff."));
    }
    section(&console, delay, "Tour complete");
    console.print(&Text::new(
        "Try rich --help for every option. Run rich --demo again to replay the tour.",
    ));
    Ok(())
}

fn watch(console: &Console, no_color: bool, path: &str, slot: &WatchChild) -> std::io::Result<()> {
    if !std::io::stdout().is_terminal() {
        for value in [1, 2] {
            std::fs::write(path, format!("{{\"live_update\":{value}}}"))?;
            command(
                console,
                no_color,
                "--watch first.json (snapshot when redirected)",
                vec!["--watch".into(), path.into()],
            )?;
        }
        return Ok(());
    }
    console.print(&Text::new(
        "$ rich --watch --watch-interval 0.1 first.json (two edits, then stop)",
    ));
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.args([
        "--no-config",
        "--no-pager",
        "--watch",
        "--watch-interval",
        "0.1",
        path,
    ]);
    if no_color {
        cmd.arg("--no-color");
    }
    *slot.lock().unwrap() = Some(cmd.spawn()?);
    let result = (|| {
        for value in [1, 2] {
            std::thread::sleep(Duration::from_millis(700));
            std::fs::write(path, format!("{{\"live_update\":{value}}}"))?;
        }
        std::thread::sleep(Duration::from_millis(700));
        if let Some(status) = slot.lock().unwrap().as_mut().unwrap().try_wait()? {
            return Err(std::io::Error::other(format!(
                "watch stopped early: {status}"
            )));
        }
        Ok(())
    })();
    if let Some(mut child) = slot.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(feature = "art")]
fn art(console: &Console, no_color: bool, delay: Duration, root: &Path) -> std::io::Result<()> {
    use rich_art::image::{Rgba, RgbaImage};
    section(console, delay, "FIGlet banners");
    console.print(&rich_art::Figlet::new("RICH"));
    let image = RgbaImage::from_fn(120, 60, |x, y| {
        let inside = (x as i32 - 38).pow(2) + (y as i32 - 30).pow(2) < 24 * 24;
        if inside {
            Rgba([255, 110, 190, 255])
        } else if x > 78 && y > 12 && y < 48 {
            Rgba([70, 220, 255, 210])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let source = root.join("art.png");
    image.save(&source).map_err(std::io::Error::other)?;
    let source = source.to_string_lossy().into_owned();
    let width = console.width().clamp(1, 48).to_string();
    for (label, mode) in [
        ("Braille", "braille"),
        ("Half-block", "blocks"),
        ("ASCII", "ascii"),
    ] {
        section(console, delay, label);
        command(console, no_color, &format!("--image art.png --image-mode {mode} --width {width} --height 12 --image-background '#142032'"), vec!["--image".into(), source.clone(), "--image-mode".into(), mode.into(), "--width".into(), width.clone(), "--height".into(), "12".into(), "--image-background".into(), "#142032".into()])?;
    }
    section(console, delay, "Crop anchors and transparent backgrounds");
    for anchor in ["left", "center", "right"] {
        command(
            console,
            no_color,
            &format!(
                "--image art.png --image-fit cover --image-anchor {anchor} --width 20 --height 12"
            ),
            vec![
                "--image".into(),
                source.clone(),
                "--image-mode".into(),
                "ascii".into(),
                "--image-fit".into(),
                "cover".into(),
                "--image-anchor".into(),
                anchor.into(),
                "--width".into(),
                "20".into(),
                "--height".into(),
                "12".into(),
                "--image-background".into(),
                "#142032".into(),
            ],
        )?;
    }
    command(
        console,
        no_color,
        "--image art.png --image-fit contain --width 40 --height 10",
        vec![
            "--image".into(),
            source.clone(),
            "--image-fit".into(),
            "contain".into(),
            "--width".into(),
            "40".into(),
            "--height".into(),
            "10".into(),
        ],
    )?;
    section(console, delay, "Image diff");
    let mut changed = image.clone();
    for y in 20..30 {
        for x in 30..40 {
            changed.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    let after = root.join("after.png");
    changed.save(&after).map_err(std::io::Error::other)?;
    command(
        console,
        no_color,
        "--diff art.png after.png --image-mode ascii --width 40",
        vec![
            "--diff".into(),
            source,
            after.to_string_lossy().into_owned(),
            "--image-mode".into(),
            "ascii".into(),
            "--width".into(),
            "40".into(),
        ],
    )?;
    section(console, delay, "GIF animation");
    let mut bytes = Vec::new();
    {
        let mut encoder = rich_art::image::codecs::gif::GifEncoder::new(&mut bytes);
        for frame in [image, changed] {
            encoder
                .encode_frame(rich_art::image::Frame::from_parts(
                    frame,
                    0,
                    0,
                    rich_art::image::Delay::from_numer_denom_ms(500, 1),
                ))
                .map_err(std::io::Error::other)?;
        }
    }
    let animation = rich_art::AnimatedArt::from_bytes(&bytes)
        .map_err(std::io::Error::other)?
        .width(40)
        .height(10);
    if std::io::stdout().is_terminal() {
        let path = root.join("demo.gif");
        std::fs::write(&path, bytes)?;
        command(
            console,
            no_color,
            "--gif demo.gif --loop 1 --width 40",
            vec![
                "--gif".into(),
                path.to_string_lossy().into_owned(),
                "--loop".into(),
                "1".into(),
                "--width".into(),
                "40".into(),
            ],
        )?;
    } else {
        console.print(&Text::new("GIF frames (animation plays in a terminal):"));
        for index in 0..animation.frame_count() {
            if let Some(frame) = animation.frame(index) {
                console.print(&frame);
            }
        }
    }
    console.print(&Text::new(
        "Sixel is available on compatible terminals; this tour uses portable image modes.",
    ));
    Ok(())
}
