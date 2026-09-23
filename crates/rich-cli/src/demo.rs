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
        if matches!(arg.as_str(), "--demo" | "--demo-list") {
            return true;
        }
    }
    false
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let options = match options(args) {
        Ok(options) => options,
        Err(message) => return emit_error(false, ExitClass::Usage, &message),
    };
    if options.list {
        println!("core       Core renderables and extensions");
        println!("workflows  Configuration, batch, exports and watch");
        println!(
            "art        {}",
            if cfg!(feature = "art") {
                "Banners, images, image diff and GIF"
            } else {
                "unavailable (build without art feature)"
            }
        );
        return ExitCode::SUCCESS;
    }
    match tour(options.no_color, options.delay, options.group.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => emit_error(false, ExitClass::Input, &format!("demo: {error}")),
    }
}

struct Options {
    no_color: bool,
    delay: Duration,
    group: Option<String>,
    list: bool,
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    let mut seconds = 3.0;
    let mut group = None;
    let mut list = false;
    let mut play = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--demo" => play = true,
            "--demo-list" => list = true,
            "--no-config" => {}
            "--no-color" => no_color = true,
            "--demo-section" => {
                let value = iter.next().ok_or("--demo-section requires core, workflows, art")?;
                if !matches!(value.as_str(), "core" | "workflows" | "art") {
                    return Err(format!("unknown demo section {value:?}; choose core, workflows, art"));
                }
                group = Some(value.clone());
            }
            "--demo-delay" => {
                seconds = iter.next().and_then(|value| value.parse::<f64>().ok())
                    .filter(|value| value.is_finite() && (0.0..=60.0).contains(value))
                    .ok_or("--demo-delay expects seconds between 0 and 60")?;
            }
            _ => return Err(format!("demo accepts only --demo, --demo-list, --demo-section NAME, --demo-delay SECONDS, --no-color and --no-config; unexpected {arg}")),
        }
    }
    if list && (play || group.is_some()) {
        return Err("--demo-list cannot be combined with --demo or --demo-section".into());
    }
    if !list && !play {
        return Err("--demo-section requires --demo".into());
    }
    if group.as_deref() == Some("art") && !cfg!(feature = "art") {
        return Err("art demo is unavailable in this build; rebuild with the art feature".into());
    }
    // A pipe gets a finite transcript, without terminal pauses or controls.
    Ok(Options {
        no_color,
        delay: if std::io::stdout().is_terminal() {
            Duration::from_secs_f64(seconds)
        } else {
            Duration::ZERO
        },
        group,
        list,
    })
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

fn tour(no_color: bool, delay: Duration, group: Option<&str>) -> std::io::Result<()> {
    println!("rich --demo: guided suite tour (3 seconds per section by default). Ctrl+C to stop.");
    println!("Examples are offline; exports use a temporary directory. Configuration is ignored.");
    let root = tempfile::tempdir()?;
    let watch_child: WatchChild = Arc::new(Mutex::new(None));
    // GIF playback owns Rust's stdout lock. Duplicate the OS handle so
    // terminal restoration never waits for that lock; pipes need no controls.
    let mut terminal = if std::io::stdout().is_terminal() {
        #[cfg(unix)]
        let terminal = {
            use std::os::fd::AsFd;
            std::fs::File::from(std::io::stdout().as_fd().try_clone_to_owned()?)
        };
        #[cfg(windows)]
        let terminal = {
            use std::os::windows::io::AsHandle;
            std::fs::File::from(std::io::stdout().as_handle().try_clone_to_owned()?)
        };
        Some(terminal)
    } else {
        None
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
        if let Some(terminal) = terminal.as_mut() {
            let _ = terminal.write_all(b"\x1b[?25h\x1b[0m\n");
            let _ = terminal.flush();
        }
        std::process::exit(130);
    })
    .map_err(std::io::Error::other)?;
    let console = if group.is_none() || group == Some("core") {
        run_demo(no_color, delay)
    } else {
        build_demo_console(no_color)
    };
    if group.is_none() || group == Some("workflows") {
        let file = |name: &str| root.path().join(name).to_string_lossy().into_owned();
        let release_json = format!(
            "{{\"release\":\"{}\",\"ready\":true}}",
            env!("CARGO_PKG_VERSION")
        );
        for (name, contents) in [
        ("first.json", release_json.as_str()),
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
        section(&console, delay, "Typed event presentation");
        command(
            &console,
            no_color,
            "--log logs.jsonl --log-presentation rich",
            vec![
                "--log".into(),
                file("logs.jsonl"),
                "--log-presentation".into(),
                "rich".into(),
            ],
        )?;
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
        watch(
            &console,
            no_color,
            [&file("first.json"), &file("second.json")],
            &watch_child,
        )?;
        section(&console, delay, "Pager, input and confidence controls");
        console.print(&Text::new("--auto-pager opens a pager for tall TTY output; --no-pager opts out.\nURL fetch, encoding, sanitization and JSON reports support scripts and CI.\nThe tour stays offline and does not open an external pager."));
        command(
            &console,
            no_color,
            "--print --sanitize '<ESC>[31m visible controls'",
            strings(&["--print", "--sanitize", "\x1b[31m visible controls"]),
        )?;
    }

    if group.is_none() || group == Some("art") {
        #[cfg(feature = "art")]
        art(&console, no_color, delay, root.path())?;
        #[cfg(not(feature = "art"))]
        {
            section(&console, delay, "Rich art");
            console.print(&Text::new("The art feature is disabled in this build; enable it for FIGlet, images, GIF and image diff."));
        }
    }
    section(&console, delay, "Tour complete");
    console.print(&Text::new(
        "Try rich --help for every option. Run rich --demo again to replay the tour.",
    ));
    Ok(())
}

/// Watch two files, each in its own Live region. The tour cannot deliver a
/// portable Ctrl+C to the child, so the last edit writes invalid JSON and
/// `--watch-exit-on-error` ends the watch, restoring the terminal itself.
fn watch(
    console: &Console,
    no_color: bool,
    [first, second]: [&str; 2],
    slot: &WatchChild,
) -> std::io::Result<()> {
    if !std::io::stdout().is_terminal() {
        for value in [1, 2] {
            std::fs::write(first, format!("{{\"live_update\":{value}}}"))?;
            command(
                console,
                no_color,
                "--watch first.json second.json (one snapshot each when redirected)",
                vec!["--watch".into(), first.into(), second.into()],
            )?;
        }
        return Ok(());
    }
    console.print(&Text::new(
        "$ rich --watch --watch-exit-on-error first.json second.json\n  \
         (two edits to first.json repaint only its region; invalid JSON then ends the watch)",
    ));
    // Run beside the files and pass bare names, so the region headers read
    // `first.json` rather than a temporary path.
    let (first_path, second_path) = (Path::new(first), Path::new(second));
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    if let Some(directory) = first_path.parent() {
        cmd.current_dir(directory);
    }
    cmd.args([
        "--no-config",
        "--no-pager",
        "--watch",
        "--watch-debounce",
        "0.1",
    ]);
    cmd.arg("--watch-exit-on-error");
    cmd.args(
        [first_path.file_name(), second_path.file_name()]
            .into_iter()
            .flatten(),
    );
    if no_color {
        cmd.arg("--no-color");
    }
    *slot.lock().unwrap() = Some(cmd.spawn()?);
    let result = (|| {
        for value in [1, 2] {
            std::thread::sleep(Duration::from_millis(700));
            std::fs::write(first, format!("{{\"live_update\":{value}}}"))?;
        }
        std::thread::sleep(Duration::from_millis(700));
        if let Some(status) = slot.lock().unwrap().as_mut().unwrap().try_wait()? {
            return Err(std::io::Error::other(format!(
                "watch stopped early: {status}"
            )));
        }
        std::fs::write(first, "{\"live_update\": ")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = slot.lock().unwrap().as_mut().unwrap().try_wait()? {
                return if status.success() {
                    Err(std::io::Error::other("watch ignored the invalid edit"))
                } else {
                    Ok(())
                };
            }
            if std::time::Instant::now() >= deadline {
                return Err(std::io::Error::other(
                    "watch did not stop on the invalid edit",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
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
    section(console, delay, "Rotation, grayscale and Bayer dithering");
    command(console, no_color, "--image art.png --image-rotate 90 --image-grayscale --image-color ansi256 --image-dither bayer4x4", vec!["--image".into(), source.clone(), "--image-mode".into(), "blocks".into(), "--width".into(), width.clone(), "--image-rotate".into(), "90".into(), "--image-grayscale".into(), "--image-color".into(), "ansi256".into(), "--image-dither".into(), "bayer4x4".into()])?;
    section(
        console,
        delay,
        "Quadrant blocks, ANSI16 and tone adjustments",
    );
    for (shown, extra) in [
        ("--image-mode quadrants", vec![]),
        (
            "--image-mode quadrants --image-color ansi16 --image-dither floyd-steinberg",
            vec![
                "--image-color",
                "ansi16",
                "--image-dither",
                "floyd-steinberg",
            ],
        ),
        (
            "--image-mode quadrants --image-brightness 1.3 --image-contrast 1.6 --image-gamma 0.7",
            vec![
                "--image-brightness",
                "1.3",
                "--image-contrast",
                "1.6",
                "--image-gamma",
                "0.7",
            ],
        ),
    ] {
        let mut args: Vec<String> = [
            "--image",
            &source,
            "--image-mode",
            "quadrants",
            "--width",
            &width,
            "--height",
            "12",
            "--image-background",
            "#142032",
        ]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
        args.extend(extra.iter().map(|arg| arg.to_string()));
        command(
            console,
            no_color,
            &format!("--image art.png {shown} --width {width} --height 12"),
            args,
        )?;
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
        "--image art.png --image-mode blocks --image-fit contain --width 40 --height 10",
        vec![
            "--image".into(),
            source.clone(),
            "--image-mode".into(),
            "blocks".into(),
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
