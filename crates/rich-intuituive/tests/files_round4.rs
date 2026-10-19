//! Round 4's `files` plugin pieces: the first frame's preview, the
//! hovered row's caps, the `file` previewer, tabs, the selection, yank and
//! paste, find next, the cursor kept by name, the context handler, tasks'
//! summary and failures, the frecency store, the finder, the spotter, bulk
//! rename, the shell prompt and the search listing.

#![cfg(feature = "files")]

use std::path::{Path, PathBuf};

use intuituive::files::*;
use intuituive::interact::headless::Script;
use intuituive::prelude::*;
use intuituive::Driver;

/// root/
///   alpha/a.txt  beta/  .hidden  notes.md  data.bin  z.txt
fn tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "intuituive-files-round4-{name}-{}",
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

fn styled(driver: &Driver) -> String {
    intuituive::testing::styled(driver)
}

/// Miller columns over `root`, with `q` to quit.
fn columns_app(root: &Path) -> App {
    let start = root.to_path_buf();
    App::new(move || {
        let m = miller(start.clone());
        m.navigation(m.view()).on_key("q", |cx| cx.quit())
    })
    .wait_for_tasks(true)
}

// 1. The preview on the first frame.

#[test]
fn the_first_frame_previews_the_hovered_entry_with_no_wait() {
    let root = tree("first");
    // No wait before the first key: the first frame's layout gives the
    // preview its width, and its load is settled before `q` comes.
    let mut backend = intuituive::interact::headless::Headless::new(Script::new().keys("q"), 60, 4);
    let record = backend.record();
    columns_app(&root).run_on(&mut backend).unwrap();
    let frames = &record.borrow().frames;
    let first = frames.first().expect("a frame");
    assert!(first.contains("a.txt"), "{first}");
    // The same through the driver: the first render shows it.
    let mut driver = columns_app(&root).driver(60, 4);
    driver.settle();
    assert!(rows(&driver)[0].ends_with("a.txt"), "{:?}", rows(&driver));
}

// 2. The hovered row's caps.

#[test]
fn the_hovered_row_has_rounded_ends_in_its_colour() {
    let root = tree("caps");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        m.navigation(m.view_with(MillerOptions::new().hovered_caps("\u{e0b6}", "\u{e0b4}")))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 6);
    driver.settle();
    let row = &rows(&driver)[0];
    // alpha, hovered: an end before it and one at the column's right.
    assert!(row.contains("\u{e0b6}alpha"), "{row}");
    assert!(row.contains("\u{e0b4}"), "{row}");
    // beta keeps a cell at the left, in line.
    assert!(rows(&driver)[1].contains(" beta"), "{:?}", rows(&driver));
    // The ends are not reversed; the name is.
    let styled = styled(&driver);
    assert!(styled.contains("[reverse"), "{styled}");
    assert!(!styled.contains("reverse]\u{e0b6}"), "{styled}");
}

// 3. The `file` previewer.

#[test]
fn a_binary_file_previews_as_what_it_is() {
    let root = tree("file-type");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        m.navigation(m.view_with(MillerOptions::new().binary(BinaryPreview::FileType)))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 6);
    driver.press("j j");
    driver.settle();
    let screen = rows(&driver).join("\n");
    // data.bin: `file` says `data` (or the MIME type without `file`).
    assert!(!screen.contains("00000000"), "{screen}");
    assert!(
        screen.contains("data") || screen.contains("octet-stream"),
        "{screen}"
    );
    // Text still previews as text.
    driver.press("j");
    driver.settle();
    assert!(rows(&driver).join("\n").contains("# Title"));
}

// 4. Tabs.

#[test]
fn each_tab_keeps_its_directory_and_history() {
    let root = tree("tabs");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let t = tabs(m);
        let cwd = move || {
            m.cwd
                .get()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        column([
            t.strip().fixed(1),
            m.navigation(m.view()).flex(1),
            text(cwd).fixed(1),
        ])
        .on_key("t", move |_| {
            t.open();
        })
        .on_key("1", move |_| t.switch(0))
        .on_key("2", move |_| t.switch(1))
        .on_key("x", move |_| {
            t.close_active();
        })
        .on_key("H", move |_| {
            t.back();
        })
        .on_key("L", move |_| {
            t.forward();
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 6);
    let here = root.file_name().unwrap().to_string_lossy().into_owned();
    driver.press("t l");
    assert_eq!(rows(&driver)[5], "alpha");
    assert!(rows(&driver)[0].contains(&here) && rows(&driver)[0].contains("alpha"));
    driver.press("1");
    assert_eq!(rows(&driver)[5], here);
    driver.press("2 H");
    assert_eq!(rows(&driver)[5], here, "back in the second tab");
    driver.press("L");
    assert_eq!(rows(&driver)[5], "alpha");
    driver.press("x");
    assert_eq!(rows(&driver)[5], here, "the first tab again");
}

// 5 and 6. The selection, targets, yank and paste.

#[test]
fn selected_and_yanked_files_are_marked_and_pasted() {
    let root = tree("select");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let picked = selection(m);
        let yanked = signal(None::<Yank>);
        let jobs = tasks();
        let options = MillerOptions::new().markers(Yank::markers(yanked));
        let names = move || {
            m.targets()
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        };
        column([
            m.navigation(m.view_with(options)).flex(1),
            text(names).fixed(1),
        ])
        .on_key("space", move |_| {
            if let Some(path) = m.hovered_path() {
                picked.toggle(path);
            }
            m.arrow(1);
        })
        .on_key("y", move |_| {
            yanked.set(Some(Yank::copy(m.targets())));
            picked.clear();
        })
        .on_key("p", move |_| {
            let Some(yank) = yanked.get_untracked() else {
                return;
            };
            let into = m.cwd.get_untracked().join("beta");
            jobs.run(
                yank.paste(into, rich_ext::file_ops::Conflict::Rename),
                move |_, _| m.refresh(),
            );
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 7);
    // Select alpha and data.bin.
    driver.press("space j space");
    assert_eq!(rows(&driver)[6], "alpha data.bin");
    assert!(
        styled(&driver).contains("[on yellow] [/]"),
        "selected marks"
    );
    driver.press("y");
    // Yanked: green marks; the hovered entry is the target now.
    assert!(
        styled(&driver).contains("on bright_green"),
        "{}",
        styled(&driver)
    );
    assert_eq!(rows(&driver)[6], "notes.md");
    driver.press("p");
    assert!(root.join("beta/alpha/a.txt").exists() && root.join("beta/data.bin").exists());
}

#[test]
fn yank_pastes_as_a_copy_or_a_move() {
    use rich_ext::file_ops::{Conflict, Op};
    let paths = vec![PathBuf::from("/a/x"), PathBuf::from("/a/y")];
    let copy = Yank::copy(paths.clone());
    assert_eq!(copy.marker(Path::new("/a/x")), Marker::Copied);
    assert_eq!(copy.marker(Path::new("/a/z")), Marker::None);
    assert!(matches!(
        copy.paste("/b", Conflict::Rename),
        Op::Copy { .. }
    ));
    let cut = Yank::cut(paths);
    assert_eq!(cut.marker(Path::new("/a/y")), Marker::Cut);
    match cut.paste("/b", Conflict::Overwrite) {
        Op::Move { into, conflict, .. } => {
            assert_eq!(into, PathBuf::from("/b"));
            assert_eq!(conflict, Conflict::Overwrite);
        }
        other => panic!("{other:?}"),
    }
}

// 7 and 8. Find next; the cursor kept by name.

#[test]
fn find_next_wraps_and_the_cursor_follows_its_entry() {
    let root = tree("find");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let hovered = move || m.hovered.get().map(|e| e.name).unwrap_or_default();
        column([m.navigation(m.view()).flex(1), text(hovered).fixed(1)])
            .on_key("f", move |_| m.find.set(Some("t".into())))
            .on_key("n", move |_| {
                m.find_next(false);
            })
            .on_key("N", move |_| {
                m.find_next(true);
            })
            .on_key("r", move |_| m.sort.update(|s| *s = s.reverse(!s.reverse)))
            .on_key(".", move |_| m.hidden.update(|h| *h = !*h))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 8);
    // `t` matches beta, notes.md and z.txt (and data.bin): after alpha.
    driver.press("f n");
    assert_eq!(rows(&driver)[7], "beta");
    driver.press("N");
    assert_eq!(rows(&driver)[7], "z.txt", "wrapped back to the last");
    // Reversed, hidden files shown: z.txt stays hovered.
    driver.press("r");
    assert_eq!(rows(&driver)[7], "z.txt");
    driver.press(".");
    assert_eq!(rows(&driver)[7], "z.txt");
    assert!(styled(&driver).contains("reverse") && rows(&driver).join("\n").contains(".hidden"));
}

// 9. The context handler and the current column's hook.

#[test]
fn a_right_press_on_an_entry_runs_the_context_handler() {
    use intuituive::interact::{Button, Event, Mouse, MouseKind};
    use intuituive::widgets::HighlightLayer;
    let root = tree("context");
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let said = signal(String::new());
        let options = MillerOptions::new()
            .on_context(move |entry, cx| said.set(format!("{} {:?}", entry.name, cx.pointer())))
            .current(|list| list.highlight_layer(HighlightLayer::Under));
        column([
            m.navigation(m.view_with(options)).flex(1),
            text!("{said}").fixed(1),
        ])
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 7);
    driver.settle();
    // The current column starts at 11 at 80 wide; row 2 is data.bin.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Right),
        20,
        2,
    )));
    driver.settle();
    assert_eq!(rows(&driver)[6], "data.bin Some((20, 2))");
    driver.press("k");
    driver.press("menu");
    assert_eq!(rows(&driver)[6], "beta None");
}

// 11. Tasks: the summary, failures told once.

#[test]
fn failed_tasks_are_told_once_and_counted() {
    use rich_ext::file_ops::Op;
    let app = App::new(|| {
        let told = signal(Vec::<String>::new());
        let jobs = tasks().on_fail(move |job, _| {
            told.update(|t| t.push(job.title.clone()));
        });
        let summary = move || {
            let (share, running, failed) = jobs.summary();
            format!("{share:.1} {running} {failed}")
        };
        column([
            tasks_panel_with(jobs, JobsPanelOptions::new().failed_title(false)).fixed(1),
            text(summary).fixed(1),
            text(move || told.get().join(",")).fixed(1),
        ])
        .on_key("d", move |_| {
            jobs.run(
                Op::Delete(vec!["/no/such/intuituive/file".into()]),
                |_, _| {},
            );
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(60, 3);
    driver.press("d");
    let screen = rows(&driver);
    // The panel's row has the error, not the title the handler told.
    assert!(screen[0].starts_with("failed  file:"), "{screen:?}");
    assert!(!screen[0].contains("Delete"), "{screen:?}");
    assert_eq!(screen[1], "0.0 0 1");
    assert!(screen[2].starts_with("Delete"), "{screen:?}");
}

// 10. Extraction as a task.

#[cfg(feature = "archive")]
#[test]
fn extracting_with_tasks_shows_a_job_and_extracts() {
    let root = tree("extract");
    let archive = root.join("pack.tar");
    std::fs::write(&archive, tar(&[("inside.txt", b'0', b"packed")])).unwrap();
    let path = archive.clone();
    let app = App::new(move || {
        let jobs = tasks();
        let said = signal(String::new());
        let path = path.clone();
        column([tasks_panel(jobs).fixed(1), text!("{said}").fixed(1)]).on_key("x", move |cx| {
            let extract = opener_rules().get("extract").unwrap().clone();
            open_with_tasks(cx, &extract, vec![path.clone()], jobs, move |_| {
                said.set("extracted".into())
            });
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(40, 2);
    driver.press("x");
    assert_eq!(rows(&driver)[1], "extracted");
    assert_eq!(
        std::fs::read_to_string(root.join("pack/inside.txt")).unwrap(),
        "packed"
    );
}

// 12. Visited directories and the frecency store.

#[test]
fn visits_are_ranked_across_sessions() {
    let root = tree("frecency");
    let file = root.join("state/frecency.tsv");
    let (start, store) = (root.clone(), file.clone());
    let app = App::new(move || {
        let m = miller(start.clone());
        let ranked = frecency_at(store.clone()).follow(m);
        let shown = move || {
            ranked
                .ranked()
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let visited = move || m.visited().with(Vec::len).to_string();
        column([
            m.navigation(m.view()).flex(1),
            text(shown).fixed(1),
            text(visited).fixed(1),
        ])
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 6);
    // alpha twice, beta once.
    driver.press("l h l h j l h");
    let here = root.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(rows(&driver)[4], format!("{here} alpha beta"));
    assert_eq!(rows(&driver)[5], "3");
    // The file is there for the next session.
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.lines().count(), 3, "{text}");
    let again = frecency_file_ranked(file);
    assert_eq!(again.first().unwrap(), &root);
}

fn frecency_file_ranked(file: PathBuf) -> Vec<PathBuf> {
    let out = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = out.clone();
    let app = App::new(move || {
        *seen.borrow_mut() = frecency_at(file.clone()).ranked();
        label("")
    });
    app.render_with(&[], 4, 1).unwrap();
    let ranked = out.borrow().clone();
    ranked
}

// 13. The finder, titled, Esc closing its modal.

#[test]
fn the_finder_in_a_modal_has_a_title_and_closes_on_esc() {
    use intuituive::finder::FuzzyOptions;
    let root = tree("finder");
    let start = root.clone();
    let app = App::new(move || {
        let start = start.clone();
        let picked = signal(String::new());
        text!("{picked}").on_key("z", move |cx| {
            let start = start.clone();
            cx.modal(Size::Fixed(40), Size::Fixed(8), move || {
                let base = start.clone();
                finder_with(
                    rich_ext::listing::walk(&start),
                    move |path, cx| {
                        let rel = path.strip_prefix(&base).unwrap().display().to_string();
                        picked.set(rel.replace('\\', "/"));
                        cx.pop();
                    },
                    FuzzyOptions::new().title("Find").border(Border::Rounded),
                )
            });
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(60, 10);
    driver.press("z");
    assert!(rows(&driver).join("\n").contains("╭─ Find "));
    driver.press("esc");
    assert!(!rows(&driver).join("\n").contains("Find"));
    driver.press("z");
    driver.type_text("a/a");
    driver.press("enter");
    assert_eq!(rows(&driver)[0], "alpha/a.txt");
}

// 15. The spotter.

#[test]
fn the_spotter_lists_a_files_details() {
    let root = tree("spot");
    let facts = spot_facts(&root.join("notes.md"));
    let keys: Vec<&str> = facts.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        &keys[..6],
        ["Name", "Path", "MIME", "Size", "Mode", "Owner"]
    );
    assert!(keys.contains(&"Modified"));
    assert_eq!(facts[0].1, "notes.md");
    assert!(facts[3].1.contains("bytes"), "{facts:?}");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("gone"), root.join("link")).unwrap();
        let link = spot_facts(&root.join("link"));
        let target = link.iter().find(|(k, _)| k == "Target").expect("a target");
        assert!(
            target.1.contains("gone") && target.1.contains("missing"),
            "{link:?}"
        );
    }
}

// 16. Bulk rename.

#[test]
fn bulk_rename_renames_what_the_editor_changed_and_reports_conflicts() {
    let root = tree("bulk");
    let base = root.clone();
    let app = App::new(move || {
        let base = base.clone();
        let renamed = signal(String::new());
        text!("{renamed}").on_key("r", move |cx| {
            let paths = vec![
                base.join("notes.md"),
                base.join("z.txt"),
                base.join("data.bin"),
            ];
            bulk_rename(cx, paths, move |done, _| {
                renamed.set(format!("{} renamed", done.len()))
            });
        })
    });
    let mut driver = app.driver(60, 10);
    driver.press("r");
    let handoff = driver.take_handoff().expect("the editor");
    let names = PathBuf::from(handoff.command.get_args().last().unwrap());
    assert_eq!(
        std::fs::read_to_string(&names).unwrap(),
        "notes.md\nz.txt\ndata.bin\n"
    );
    // notes.md and z.txt swap; data.bin takes a name that is in use.
    std::fs::write(&names, "z.txt\nnotes.md\nalpha\n").unwrap();
    driver.handed_back(handoff, Ok(Some(0)));
    driver.settle();
    let screen = rows(&driver).join("\n");
    assert!(screen.contains("2 renamed"), "{screen}");
    assert!(screen.contains("data.bin: alpha is taken"), "{screen}");
    assert!(std::fs::read_to_string(root.join("z.txt"))
        .unwrap()
        .starts_with("# Title"));
    assert!(std::fs::read_to_string(root.join("notes.md"))
        .unwrap()
        .starts_with("zzz"));
    assert!(!names.exists(), "the names file is removed");
}

// 17. The shell prompt.

#[cfg(unix)]
#[test]
fn the_shell_runs_in_the_background_as_a_task_or_blocks() {
    let root = tree("shell");
    let base = root.clone();
    let app = App::new(move || {
        let jobs = tasks();
        let base = base.clone();
        let other = base.clone();
        column([tasks_panel(jobs).fixed(2), label("files")])
            .on_key("semicolon", move |cx| {
                let options = ShellOptions::new()
                    .hovered(Some(base.join("notes.md")))
                    .tasks(jobs);
                shell(cx, vec![base.join("z.txt")], options);
            })
            .on_key("colon", move |cx| {
                shell(
                    cx,
                    vec![other.join("z.txt")],
                    ShellOptions::new().block(true),
                );
            })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(60, 10);
    driver.press("semicolon");
    assert!(rows(&driver).join("\n").contains("Shell:"));
    driver.type_text("cp \"$0\" \"$1.copy\"; false");
    driver.press("enter");
    driver.settle();
    assert!(root.join("z.txt.copy").exists());
    // `false` failed: the job stays, failed.
    assert!(
        rows(&driver)[0].starts_with("failed"),
        "{:?}",
        rows(&driver)
    );
    driver.press("colon");
    assert!(rows(&driver).join("\n").contains("Shell (block):"));
    driver.type_text("less \"$@\"");
    driver.press("enter");
    let handoff = driver
        .take_handoff()
        .expect("the command takes the terminal");
    assert_eq!(handoff.command.get_program(), "sh");
}

// 18. A search's results in Miller.

#[test]
fn a_search_lists_its_results_in_the_columns_until_left() {
    let root = tree("search");
    std::fs::write(root.join("alpha/hello.md"), "hello there").unwrap();
    let start = root.clone();
    let app = App::new(move || {
        let m = miller(start.clone());
        let root = start.clone();
        let header = move || match m.searching() {
            Some(s) => format!(
                "search: {} ({})",
                s.query(),
                if s.running() { "…" } else { "done" }
            ),
            None => "browsing".into(),
        };
        column([text(header).fixed(1), m.navigation(m.view()).flex(1)])
            .on_key("S", {
                let root = root.clone();
                move |_| m.show_search(search(&root, "hello", SearchKind::Grep))
            })
            .on_key("s", move |_| {
                m.show_search(search(&root, "a.t", SearchKind::Name))
            })
            .on_key("escape", move |_| m.end_search())
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(80, 6);
    driver.press("S");
    driver.settle();
    let screen = rows(&driver).join("\n").replace('\\', "/");
    assert!(screen.contains("search: hello (done)"), "{screen}");
    // notes.md says `hello notes`; alpha/hello.md `hello there`.
    assert!(
        screen.contains("alpha/hello.md") && screen.contains("notes.md"),
        "{screen}"
    );
    assert!(!screen.contains("z.txt"), "{screen}");
    // The hovered result is previewed.
    assert!(
        screen.contains("hello there") || screen.contains("hello notes"),
        "{screen}"
    );
    driver.press("escape");
    assert!(rows(&driver)[0].starts_with("browsing"));
    assert!(rows(&driver).join("\n").contains("z.txt"));
    // A name search (fd, else the walk's substring).
    driver.press("s");
    driver.settle();
    let screen = rows(&driver).join("\n").replace('\\', "/");
    assert!(screen.contains("alpha/a.txt"), "{screen}");
    // Going up ends it.
    driver.press("h");
    assert!(rows(&driver)[0].starts_with("browsing"));
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
