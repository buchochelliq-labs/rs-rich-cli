//! The `files` feature's components: the directory model, file lists,
//! previews, Miller columns, openers, tasks, the finder and the hex view.

#![cfg(feature = "files")]

use std::path::PathBuf;

use intuituive::interact::{Button, Event, Mouse, MouseKind};

use intuituive::files::*;
use intuituive::prelude::*;
use intuituive::Driver;

/// root/
///   alpha/a.txt  beta/  .hidden  notes.md  data.bin  z.txt
fn tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "intuituive-files-components-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("alpha")).unwrap();
    std::fs::create_dir_all(root.join("beta")).unwrap();
    std::fs::write(root.join("alpha/a.txt"), "in alpha").unwrap();
    std::fs::write(root.join(".hidden"), "secret").unwrap();
    std::fs::write(root.join("notes.md"), "# Title\n\nhello notes\n").unwrap();
    std::fs::write(root.join("data.bin"), [0u8, 1, 2, 3, 0, 9]).unwrap();
    std::fs::write(root.join("z.txt"), "zzz ".repeat(300)).unwrap();
    root.canonicalize().unwrap()
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

/// Settle until `done` holds of the screen (tasks and threads finish on
/// their own time), or panic with the screen after a few seconds.
fn until(driver: &mut Driver, done: impl Fn(&[String]) -> bool) -> Vec<String> {
    let start = std::time::Instant::now();
    loop {
        driver.settle();
        let screen = rows(driver);
        if done(&screen) {
            return screen;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "{screen:#?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn names(model: DirModel) -> String {
    model.entries().with(|e| {
        e.iter()
            .map(|e| e.name.clone())
            .collect::<Vec<_>>()
            .join(" ")
    })
}

#[test]
fn the_model_sorts_hides_filters_and_reads_again() {
    let root = tree("model");
    let dir = root.clone();
    let app = App::new(move || {
        let dir = dir.clone();
        let model = dir_model(move || Some(dir.clone()));
        text(move || names(model))
            .on_key(".", move |_| model.hidden.update(|h| *h = !*h))
            .on_key("s", move |_| {
                model.sort.set(Sort::by(SortBy::Size).reverse(true))
            })
            .on_key("f", move |_| model.filter.set("Z".into()))
            .on_key("F", move |_| model.filter.set("z".into()))
            .on_key("r", move |_| model.refresh())
    });
    let mut driver = app.driver(60, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "alpha beta data.bin notes.md z.txt");
    driver.press(".");
    assert_eq!(
        rows(&driver)[0],
        "alpha beta .hidden data.bin notes.md z.txt"
    );
    driver.press("s");
    // Reversed, ties too: directories stay first.
    assert_eq!(
        rows(&driver)[0],
        "beta alpha z.txt notes.md data.bin .hidden"
    );
    // Smart case: a capital must match a capital.
    driver.press("f");
    assert_eq!(rows(&driver)[0], "");
    driver.press("F");
    assert_eq!(rows(&driver)[0], "z.txt");
    std::fs::write(root.join("zz.txt"), "").unwrap();
    driver.press("r");
    assert_eq!(rows(&driver)[0], "z.txt zz.txt");
}

#[test]
fn sniffing_types_files_by_their_first_bytes() {
    let root = tree("sniff");
    std::fs::write(root.join("photo.txt"), b"\x89PNG\r\n\x1a\n0000").unwrap();
    let dir = root.clone();
    let app = App::new(move || {
        let dir = dir.clone();
        let model = dir_model(move || Some(dir.clone())).sniff();
        text(move || {
            let entry = model
                .entries()
                .with(|e| e.iter().find(|e| e.name == "photo.txt").cloned());
            entry.map_or(String::new(), |e| model.mime(&e).to_string())
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(30, 1);
    let screen = until(&mut driver, |s| s[0] == "image/png");
    assert_eq!(screen[0], "image/png");
}

#[test]
fn a_file_list_shows_line_modes_finds_and_empty_states() {
    let root = tree("list");
    let dir = root.clone();
    let app = App::new(move || {
        let dir = dir.clone();
        let model = dir_model(move || Some(dir.clone()));
        let mode = signal(Linemode::Size);
        let find = signal(Some("t".to_string()));
        let cursor = signal(3usize);
        let options = FileListOptions::new().linemode(mode).find(find);
        file_list(model, cursor, options)
            .on_key("p", move |_| mode.set(Linemode::Permissions))
            .on_key("f", move |_| model.filter.set("nothing-matches".into()))
    });
    let mut driver = app.driver(40, 6);
    driver.settle();
    let screen = rows(&driver);
    // A directory's size is its number of entries; a file's is readable.
    assert_eq!(screen[0], format!("alpha{}1", " ".repeat(40 - 5 - 2)));
    assert!(
        screen[2].starts_with("data.bin") && screen[2].ends_with("6B"),
        "{screen:?}"
    );
    // The hovered entry's place among the find's matches: notes.md is the
    // third of the names with a `t`.
    assert!(screen[3].contains("notes.md  [3/4]"), "{screen:?}");
    #[cfg(unix)]
    {
        driver.press("p");
        assert!(rows(&driver)[0].ends_with("drwxr-xr-x") || rows(&driver)[0].contains("drwx"));
    }
    driver.press("f");
    assert_eq!(rows(&driver)[0].trim(), "No filter results");
}

#[test]
fn a_directory_that_cannot_be_read_says_why() {
    let app = App::new(|| {
        let model = dir_model(|| Some(PathBuf::from("/no/such/directory/here")));
        file_list(model, signal(0), FileListOptions::new())
    });
    let mut driver = app.driver(60, 1);
    driver.settle();
    assert!(rows(&driver)[0].contains("Error:"), "{:?}", rows(&driver));
}

#[test]
fn previews_pick_their_renderer_by_type() {
    let root = tree("preview");
    let path_for = |name: &str| root.join(name);
    for (name, expect) in [
        ("notes.md", "# Title"),
        ("data.bin", "00000000  00 01 02 03 00 09"),
        ("alpha", "a.txt"),
    ] {
        let path = path_for(name);
        let app = App::new(move || {
            let path = path.clone();
            preview(move || Some(path.clone()), PreviewOptions::new())
        })
        .wait_for_tasks(true);
        let mut driver = app.driver(60, 4);
        let screen = until(&mut driver, |s| s.iter().any(|r| r.contains(expect)));
        assert!(
            screen.iter().any(|r| r.contains(expect)),
            "{name}: {screen:?}"
        );
    }
}

#[cfg(not(feature = "art"))]
#[test]
fn an_image_is_described_without_the_art_feature() {
    let root = tree("image");
    let path = root.join("pixel.png");
    std::fs::write(&path, b"\x89PNG\r\n\x1a\n0000").unwrap();
    let app = App::new(move || {
        let path = path.clone();
        preview(move || Some(path.clone()), PreviewOptions::new())
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(60, 3);
    let screen = until(&mut driver, |s| s[0].contains("image/png"));
    assert!(screen[1].contains("`art` feature"), "{screen:?}");
}

#[cfg(feature = "art")]
#[test]
fn an_image_is_drawn_with_the_art_feature() {
    let root = tree("image-art");
    let path = root.join("red.png");
    rich_art::image::RgbImage::from_pixel(4, 4, rich_art::image::Rgb([255, 0, 0]))
        .save(&path)
        .unwrap();
    let app = App::new(move || {
        let path = path.clone();
        preview(move || Some(path.clone()), PreviewOptions::new())
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(8, 2);
    // The decode is the image node's own worker, not a task: poll.
    until(&mut driver, |s| s[0].starts_with('▀'));
}

#[cfg(feature = "archive")]
#[test]
fn an_archive_is_listed_and_the_extract_opener_extracts_it() {
    let root = tree("archive");
    let path = root.join("docs.tar");
    std::fs::write(
        &path,
        tar(&[("guide/", b'5', b""), ("guide/intro.md", b'0', b"hello")]),
    )
    .unwrap();
    let shown = path.clone();
    let app = App::new(move || {
        let shown = shown.clone();
        let extract =
            rich_ext::open::Rules::yazi().matching(&shown, "application/x-tar")[0].clone();
        let done = signal(false);
        let target = shown.clone();
        column([
            preview(move || Some(shown.clone()), PreviewOptions::new()).flex(1),
            text(move || {
                if done.get() {
                    "extracted".into()
                } else {
                    String::new()
                }
            })
            .fixed(1),
        ])
        .on_key("x", move |cx| {
            open_with(cx, &extract, vec![target.clone()], move |_| done.set(true))
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(60, 4);
    let screen = until(&mut driver, |s| {
        s.iter().any(|r| r.contains("guide/intro.md"))
    });
    assert!(screen[0].contains("guide/"), "{screen:?}");
    assert!(screen[1].contains("5B guide/intro.md"), "{screen:?}");
    driver.press("x");
    until(&mut driver, |s| s[3] == "extracted");
    assert_eq!(
        std::fs::read_to_string(root.join("docs/guide/intro.md")).unwrap(),
        "hello"
    );
}

/// A ustar archive of `(path, type, data)` entries.
#[cfg(feature = "archive")]
fn tar(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (path, kind, data) in entries {
        let mut h = vec![0u8; 512];
        h[..path.len()].copy_from_slice(path.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        h[136..147].copy_from_slice(b"14707072000");
        h[156] = *kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| *b as u32).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        out.extend(h);
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.extend([0u8; 1024]);
    out
}

#[test]
fn a_late_preview_never_replaces_the_current_one() {
    let root = tree("stale");
    let path = root.clone();
    let app = App::new(move || {
        let path = path.clone();
        let which = signal("slow");
        let options = PreviewOptions::new().previewer(|path, _, _| {
            // The first file takes a while.
            if path.ends_with("notes.md") {
                std::thread::sleep(std::time::Duration::from_millis(300));
                return Some(Preview::Note("slow".into()));
            }
            Some(Preview::Note("fast".into()))
        });
        preview(
            move || {
                Some(path.join(if which.get() == "slow" {
                    "notes.md"
                } else {
                    "z.txt"
                }))
            },
            options,
        )
        .on_key("n", move |_| which.set("fast"))
    });
    let mut driver = app.driver(20, 1);
    driver.settle();
    driver.press("n");
    let screen = until(&mut driver, |s| s[0] == "fast");
    std::thread::sleep(std::time::Duration::from_millis(400));
    driver.settle();
    assert_eq!(rows(&driver)[0], "fast", "{screen:?}");
}

fn miller_app(root: PathBuf) -> App {
    App::new(move || {
        let columns = miller(root.clone());
        let jobs = tasks();
        columns
            .navigation(column([
                columns.view().flex(1),
                text(move || {
                    let hovered = columns.hovered.get().map(|e| e.name).unwrap_or_default();
                    format!("{} | {hovered}", columns.cwd.get().display())
                })
                .fixed(1),
            ]))
            .on_key("m", move |_| columns.linemode.set(Linemode::Size))
            .on_key("D", move |_| {
                if let Some(path) = columns.hovered_path() {
                    jobs.run(rich_ext::file_ops::Op::Delete(vec![path]), move |_, _| {
                        columns.refresh()
                    });
                }
            })
    })
    .wait_for_tasks(true)
}

fn status(driver: &Driver) -> String {
    rows(driver).last().cloned().unwrap_or_default()
}

#[test]
fn miller_columns_move_enter_leave_and_remember() {
    let root = tree("miller");
    let mut driver = miller_app(root.clone()).driver(80, 8);
    driver.settle();
    let screen = until(&mut driver, |s| s[0].contains("a.txt"));
    driver.assert_focused("current");
    // Parent | current | preview of the hovered `alpha`.
    assert!(screen[0].contains("│alpha"), "{screen:?}");
    assert!(screen[0].ends_with("a.txt"), "{screen:?}");
    assert!(status(&driver).ends_with("| alpha"));
    driver.press("j j j");
    assert!(status(&driver).ends_with("| notes.md"));
    let screen = until(&mut driver, |s| s.iter().any(|r| r.contains("# Title")));
    assert!(screen[0].contains("# Title"), "{screen:?}");
    // Into alpha and back out: alpha is hovered again, then notes.md
    // again when coming back to it.
    driver.press("g g l");
    assert!(
        status(&driver).ends_with("alpha | a.txt"),
        "{}",
        status(&driver)
    );
    driver.press("h");
    assert!(status(&driver).ends_with("| alpha"), "{}", status(&driver));
    driver.press("G");
    assert!(status(&driver).ends_with("| z.txt"));
    driver.press("l");
    // A file: `l` stays.
    assert!(status(&driver).ends_with("| z.txt"));
}

#[test]
fn miller_clicks_navigate_and_the_wheel_moves() {
    let root = tree("clicks");
    let mut driver = miller_app(root.join("alpha")).driver(80, 8);
    until(&mut driver, |s| s[0].contains("a.txt"));
    // The parent column (10 wide at 80) lists root: alpha, beta, …; a click
    // on `beta` (row 1) goes there.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        3,
        1,
    )));
    driver.settle();
    assert!(status(&driver).contains("beta |"), "{}", status(&driver));
    // Below the parent's entries: up a level, hovering where we were.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        3,
        6,
    )));
    driver.settle();
    assert!(status(&driver).ends_with("| beta"), "{}", status(&driver));
    // The wheel over the current column moves the cursor down; a click
    // hovers the row.
    driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 30, 2)));
    driver.settle();
    assert!(!status(&driver).ends_with("| beta"), "{}", status(&driver));
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        30,
        3,
    )));
    driver.settle();
    assert!(
        status(&driver).ends_with("| notes.md"),
        "{}",
        status(&driver)
    );
}

#[test]
fn miller_reads_again_after_a_job() {
    let root = tree("job");
    let mut driver = miller_app(root.clone()).driver(80, 8);
    until(&mut driver, |s| s[0].contains("a.txt"));
    driver.press("G D");
    let screen = until(&mut driver, |s| !s.iter().any(|r| r.contains("z.txt")));
    assert!(!root.join("z.txt").exists(), "{screen:?}");
    assert!(
        status(&driver).ends_with("| notes.md"),
        "{}",
        status(&driver)
    );
}

#[test]
fn a_blocking_opener_takes_the_terminal() {
    let app = App::new(|| {
        label("files").on_key("e", |cx| {
            let rules = opener_rules();
            let edit = rules.matching(std::path::Path::new("a.md"), "text/markdown")[0].clone();
            open_with(cx, &edit, vec!["a.md".into()], |_| {});
        })
    });
    let mut driver = app.driver(10, 1);
    driver.press("e");
    let handoff = driver.take_handoff().expect("the editor took the terminal");
    let args: Vec<_> = handoff.command.get_args().collect();
    assert_eq!(
        args.last().map(|a| a.to_string_lossy()),
        Some("a.md".into())
    );
}

#[test]
fn tasks_report_and_failures_stay_listed() {
    use rich_ext::file_ops::{Conflict, Op};
    let root = tree("tasks");
    let base = root.clone();
    let app = App::new(move || {
        let jobs = tasks();
        let (base, other) = (base.clone(), base.clone());
        let report = signal(String::new());
        column([tasks_panel(jobs).flex(1), text!("{report}").fixed(1)])
            .on_key("c", move |_| {
                let op = Op::Copy {
                    from: vec![base.join("alpha")],
                    into: base.join("beta"),
                    conflict: Conflict::Rename,
                };
                jobs.run(op, move |r, _| report.set(format!("made {}", r.made.len())));
            })
            .on_key("m", move |_| {
                let op = Op::Move {
                    from: vec![other.join("missing")],
                    into: other.join("beta"),
                    conflict: Conflict::Rename,
                };
                jobs.run(op, move |r, _| {
                    report.set(format!("{} errors", r.errors.len()))
                });
            })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(60, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0].trim(), "No tasks");
    driver.press("c");
    let screen = until(&mut driver, |s| s[2] == "made 1");
    // Done jobs leave the list.
    assert_eq!(screen[0].trim(), "No tasks");
    assert!(root.join("beta/alpha/a.txt").exists());
    driver.press("m");
    let screen = until(&mut driver, |s| s[2] == "1 errors");
    assert!(
        screen[0].contains("failed  Move missing to beta: missing:"),
        "{screen:?}"
    );
    driver.press("c");
    let _ = screen;
}

#[test]
fn the_finder_streams_a_walk_and_picks() {
    let root = tree("finder");
    for i in 0..300 {
        let dir = root.join(format!("d{}", i % 10));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("file{i}.txt")), "").unwrap();
    }
    let base = root.clone();
    let app = App::new(move || {
        let picked = signal(String::new());
        let base = base.clone();
        column([
            finder(rich_ext::listing::walk(base.clone()), move |path, _| {
                picked.set(path.strip_prefix(&base).unwrap().display().to_string())
            })
            .flex(1),
            text!("{picked}").fixed(1),
        ])
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(40, 6);
    // 300 files, 10 directories and the tree's own 6 (not `.hidden`).
    until(&mut driver, |s| s[1].trim() == "316/316");
    driver.type_text("file299");
    let screen = rows(&driver);
    assert_eq!(screen[1].trim(), "1/316", "{screen:?}");
    assert!(screen[2].ends_with("file299.txt"), "{screen:?}");
    driver.press("enter");
    let shown = rows(&driver)[5].replace('\\', "/");
    assert_eq!(shown, "d9/file299.txt");
}

#[test]
fn the_hex_view_scrolls() {
    let app = App::new(|| {
        let top = signal(0u16);
        hex_view(|| (0u8..=255).collect(), top).on_key("n", move |_| top.set(15))
    });
    let mut driver = app.driver(80, 2);
    driver.settle();
    assert!(
        rows(&driver)[0].starts_with("00000000  00 01 02"),
        "{:?}",
        rows(&driver)
    );
    driver.press("n");
    assert!(
        rows(&driver)[0].starts_with("000000f0  f0 f1"),
        "{:?}",
        rows(&driver)
    );
}

#[cfg(feature = "watch")]
#[test]
fn a_watched_model_follows_the_disk() {
    let root = tree("watch");
    let dir = root.clone();
    let app = App::new(move || {
        let dir = dir.clone();
        let model = dir_model(move || Some(dir.clone())).watch();
        text(move || names(model))
    });
    let mut driver = app.driver(80, 1);
    driver.settle();
    std::fs::write(root.join("new.txt"), "").unwrap();
    until(&mut driver, |s| s[0].contains("new.txt"));
}

#[test]
fn git_signs_beside_the_line_mode() {
    let root = tree("git");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
    };
    // No git here: nothing to show.
    if !git(&["init", "-q"]).is_ok_and(|o| o.status.success()) {
        return;
    }
    git(&["add", "notes.md"]).unwrap();
    git(&["commit", "-qm", "first"]).unwrap();
    std::fs::write(root.join("notes.md"), "changed").unwrap();
    let dir = root.clone();
    let app = App::new(move || {
        let dir = dir.clone();
        let model = dir_model(move || Some(dir.clone()));
        let options = FileListOptions::new().git(git_status(model));
        file_list(model, signal(0), options)
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(30, 5);
    let screen = until(&mut driver, |s| s[3].ends_with('M'));
    assert!(screen[3].starts_with("notes.md"), "{screen:?}");
    assert!(screen[4].ends_with('?'), "{screen:?}");
}
