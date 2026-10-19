//! The diskonaut rebuild (examples/disk.rs): the treemap's layout on the
//! model, and the app on a folder made for each test, scanned with
//! apparent sizes so every tile is the size written.

#[path = "../examples/disk.rs"]
#[allow(dead_code)]
mod disk;

use std::path::{Path, PathBuf};

use disk::*;
use intuituive::focus::Direction;
use intuituive::Driver;

const W: u16 = 80;
const H: u16 = 24;

/// Where the test folders go: a short path, as the top line shows the
/// folder's path and, like diskonaut's, drops the totals before it when the
/// path is too long (macOS's and Windows's temporary folders are).
fn base() -> PathBuf {
    let short = if cfg!(windows) {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        PathBuf::from(format!("{drive}\\tmp"))
    } else {
        PathBuf::from("/tmp")
    };
    if std::fs::create_dir_all(&short).is_ok() {
        short
    } else {
        std::env::temp_dir()
    }
}

/// root/
///   big/        a.bin 60000, b.bin 20000
///   docs/       notes.txt 30000
///   movie.mp4   50000
///   song.mp3    15000
///   tiny0..tiny5  100 each
fn tree(name: &str) -> PathBuf {
    let root = base().join(format!("intuituive-disk-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("big")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let file = |path: &str, size: usize| std::fs::write(root.join(path), vec![b'x'; size]).unwrap();
    file("big/a.bin", 60_000);
    file("big/b.bin", 20_000);
    file("docs/notes.txt", 30_000);
    file("movie.mp4", 50_000);
    file("song.mp3", 15_000);
    for i in 0..6 {
        file(&format!("tiny{i}"), 100);
    }
    root
}

fn options(root: &Path) -> Options {
    let mut options = Options::new(root);
    options.apparent = true;
    options.root_user = false;
    options
}

fn driver_with(options: Options, w: u16, h: u16) -> Driver {
    let mut driver = disk_app(options).wait_for_tasks(true).driver(w, h);
    driver.settle();
    driver
}

fn driver(root: &Path) -> Driver {
    driver_with(options(root), W, H)
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn dump(driver: &Driver) -> String {
    rows(driver).join("\n")
}

fn has(driver: &Driver, needle: &str) -> bool {
    rows(driver).iter().any(|r| r.contains(needle))
}

fn selected(driver: &Driver) -> String {
    rows(driver)[H as usize - 2].trim().to_string()
}

/// The item's tile on the map: its name's row and column.
fn tile_at(driver: &Driver, name: &str) -> Option<(usize, usize)> {
    rows(driver)[1..H as usize - 2]
        .iter()
        .enumerate()
        .find_map(|(y, r)| r.find(name).map(|x| (y + 1, x)))
}

// The model.

fn folder(sizes: &[(&str, u64)]) -> Folder {
    let mut tree = Tree::default();
    for (name, size) in sizes {
        tree.add(&Found::Entry {
            path: PathBuf::from(name),
            dir: false,
            size: *size,
        });
    }
    tree.root
}

/// The tiles cover the map and the small items' block, line to line,
/// once each.
fn assert_covers(map: &Map, w: u16, h: u16) {
    let mut cells = vec![0u8; usize::from(w) * usize::from(h)];
    let mut mark = |x: u16, y: u16, tw: u16, th: u16| {
        for yy in y..y + th {
            for xx in x..x + tw {
                cells[usize::from(yy) * usize::from(w) + usize::from(xx)] += 1;
            }
        }
    };
    for t in &map.tiles {
        mark(t.x, t.y, t.w, t.h);
    }
    if let Some(s) = &map.small {
        mark(s.x, s.y, s.w, s.h);
    }
    assert!(cells.iter().all(|&c| c == 1), "{map:#?}");
}

#[test]
fn the_treemap_tiles_the_folder_by_size() {
    let root = folder(&[("a", 600), ("b", 300), ("c", 100)]);
    let items = children(&root, 0);
    assert_eq!(
        items.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    let map = treemap(&items, 79, 20, 0);
    assert_covers(&map, 79, 20);
    assert!(map.small.is_none());
    let area = |t: &Tile| u32::from(t.w) * u32::from(t.h);
    let total: u32 = map.tiles.iter().map(area).sum();
    for (tile, share) in map.tiles.iter().zip([600, 300, 100]) {
        let got = area(tile) * 1000 / total;
        assert!(
            got.abs_diff(share) <= 60,
            "{} has {got}‰, not {share}‰",
            tile.name
        );
        assert!(tile.w >= MIN_WIDTH && tile.h >= MIN_HEIGHT, "{tile:?}");
        assert_eq!(tile.permille, share);
    }
}

#[test]
fn items_too_small_for_a_tile_share_one_block() {
    let mut sizes = vec![("big".to_string(), 100_000u64)];
    sizes.extend((0..40).map(|i| (format!("f{i:02}"), 10)));
    let named: Vec<(&str, u64)> = sizes.iter().map(|(n, s)| (n.as_str(), *s)).collect();
    let root = folder(&named);
    let map = treemap(&children(&root, 0), 79, 20, 0);
    assert_covers(&map, 79, 20);
    assert_eq!(map.tiles.len(), 1);
    assert_eq!(map.small.as_ref().map(|s| s.count), Some(40));
    // Zoomed past the largest, the small ones get tiles of their own.
    let zoomed = treemap(&children(&root, 1), 79, 20, 1);
    assert_covers(&zoomed, 79, 20);
    assert!(zoomed.tiles.len() > 10, "{zoomed:#?}");
    assert!(zoomed
        .tiles
        .iter()
        .all(|t| t.w >= MIN_WIDTH && t.h >= MIN_HEIGHT));
}

#[test]
fn empty_files_share_the_map_equally() {
    let root = folder(&[("x", 0), ("y", 0)]);
    let map = treemap(&children(&root, 0), 40, 12, 0);
    assert_covers(&map, 40, 12);
    assert_eq!(map.tiles.len(), 2);
    assert_eq!(map.tiles[0].permille, 500);
    assert!(
        map.tiles[0].w.abs_diff(map.tiles[1].w) <= 1
            || map.tiles[0].h.abs_diff(map.tiles[1].h) <= 1
    );
}

#[test]
fn moving_goes_to_the_tile_sharing_the_most_edge() {
    let tile = |name: &str, x, y, w, h| Tile {
        name: name.into(),
        size: 1,
        descendants: None,
        permille: 0,
        x,
        y,
        w,
        h,
    };
    // a | b
    //   |---
    //   | c (taller)
    let tiles = [
        tile("a", 0, 0, 30, 20),
        tile("b", 30, 0, 40, 8),
        tile("c", 30, 8, 40, 12),
    ];
    assert_eq!(
        step(&tiles, None, Direction::Left),
        Some(0),
        "nothing selected: the first"
    );
    assert_eq!(step(&tiles, Some(0), Direction::Right), Some(2));
    assert_eq!(step(&tiles, Some(2), Direction::Up), Some(1));
    assert_eq!(step(&tiles, Some(1), Direction::Left), Some(0));
    assert_eq!(step(&tiles, Some(0), Direction::Left), None, "off the edge");
    assert_eq!(step(&tiles, Some(1), Direction::Down), Some(2));
}

#[test]
fn sizes_are_shown_as_diskonaut_shows_them() {
    assert_eq!(display_size(999), "999");
    assert_eq!(display_size(60_000), "58.6K");
    assert_eq!(display_size(5 * 1_048_576), "5.0M");
    assert_eq!(display_size(3 * 1_073_741_824), "3.0G");
}

#[test]
fn the_tree_counts_and_deletes_up_the_path() {
    let mut tree = Tree::default();
    for (path, dir, size) in [
        ("d", true, 0),
        ("d/e", true, 0),
        ("d/e/f", false, 7),
        ("g", false, 3),
    ] {
        tree.add(&Found::Entry {
            path: PathBuf::from(path),
            dir,
            size,
        });
    }
    tree.add(&Found::Failed);
    assert_eq!(
        (tree.root.size, tree.root.descendants, tree.failed),
        (10, 4, 1)
    );
    let d = tree.root.folder(&["d".into()]).unwrap();
    assert_eq!((d.size, d.descendants), (7, 2));
}

// The app.

#[test]
fn the_map_shows_the_folder_sized_by_usage() {
    let root = tree("map");
    let driver = driver(&root);
    let screen = dump(&driver);
    let top = &rows(&driver)[0];
    assert!(
        top.contains("Total: 171.5K (13 files), freed: 0"),
        "{screen}"
    );
    assert!(top.contains(&format!(" | {}", root.display())), "{screen}");
    for label in [
        "big/ (+2 descendants)",
        "78.1K (46%)",
        "movie.mp4",
        "48.8K (29%)",
        "docs/ (+1 descendants)",
        "29.3K (17%)",
        "song.mp3",
        "14.6K (9%)",
    ] {
        assert!(has(&driver, label), "{label} missing:\n{screen}");
    }
    // The six tiny files are the block of x, and the legend says so.
    assert!(has(&driver, "│xxxxxxxx"), "{screen}");
    assert!(selected(&driver).ends_with("(x = Small files)"), "{screen}");
    assert_eq!(
        rows(&driver)[H as usize - 1],
        " ←↓↑→/<ENTER>/<ESC>: navigate, <BACKSPACE>: del"
    );
    // One frame, the lines joined.
    assert!(
        rows(&driver)[1].starts_with('┌') && rows(&driver)[1].ends_with('┐'),
        "{screen}"
    );
    assert!(
        screen.contains('┬') && screen.contains('┴') && screen.contains('├'),
        "{screen}"
    );
    // The largest is the biggest tile, at the left.
    let (by, bx) = tile_at(&driver, "big/").unwrap();
    let (_, mx) = tile_at(&driver, "movie.mp4").unwrap();
    assert!(bx < mx && by > 1, "{screen}");
}

#[test]
fn arrows_and_hjkl_move_between_tiles() {
    let root = tree("move");
    let mut driver = driver(&root);
    assert!(!selected(&driver).contains("SELECTED"));
    driver.press("right");
    assert!(
        selected(&driver).starts_with("SELECTED: big (78.1K, 2 files)"),
        "{}",
        dump(&driver)
    );
    driver.press("l");
    assert!(
        selected(&driver).starts_with("SELECTED: docs"),
        "{}",
        dump(&driver)
    );
    driver.press("k");
    assert!(
        selected(&driver).starts_with("SELECTED: movie.mp4 (48.8K)"),
        "{}",
        dump(&driver)
    );
    driver.press("j");
    assert!(
        selected(&driver).starts_with("SELECTED: docs"),
        "{}",
        dump(&driver)
    );
    driver.press("ctrl+f");
    assert!(
        selected(&driver).starts_with("SELECTED: song.mp3"),
        "{}",
        dump(&driver)
    );
    driver.press("h left");
    assert!(
        selected(&driver).starts_with("SELECTED: big"),
        "{}",
        dump(&driver)
    );
    // Off the edge: nothing selected.
    driver.press("left");
    assert!(!selected(&driver).contains("SELECTED"), "{}", dump(&driver));
}

#[test]
fn the_selected_tile_is_filled() {
    let root = tree("fill");
    let mut driver = driver(&root);
    driver.press("right");
    let styled = intuituive::testing::styled(&driver);
    assert!(
        styled.contains("[bold white on blue]big/ (+2 descendants)[/]"),
        "{styled}"
    );
    driver.press("up");
    let styled = intuituive::testing::styled(&driver);
    assert!(!styled.contains("on blue]big/"), "{styled}");
}

#[test]
fn enter_goes_into_a_folder_and_esc_comes_back() {
    let root = tree("enter");
    let mut driver = driver(&root);
    // Nothing selected: Enter takes the largest folder.
    driver.press("enter");
    let top = rows(&driver)[0].clone();
    assert!(
        top.contains("/big (78.1K, 2 files)") || top.contains("/big (78.1K)"),
        "{}",
        dump(&driver)
    );
    assert!(
        has(&driver, "a.bin") && has(&driver, "58.6K (75%)"),
        "{}",
        dump(&driver)
    );
    assert!(
        has(&driver, "b.bin") && !has(&driver, "movie.mp4"),
        "{}",
        dump(&driver)
    );
    // A file is not entered.
    driver.press("right enter");
    assert!(
        selected(&driver).starts_with("SELECTED: a.bin"),
        "{}",
        dump(&driver)
    );
    assert!(has(&driver, "b.bin"));
    driver.press("esc");
    assert!(has(&driver, "movie.mp4"), "{}", dump(&driver));
    assert!(!rows(&driver)[0].contains("/big"));
    // The folder left is selected again.
    assert!(
        selected(&driver).starts_with("SELECTED: big"),
        "{}",
        dump(&driver)
    );
}

#[test]
fn esc_at_the_top_turns_the_path_red_for_a_moment() {
    let root = tree("top");
    let mut driver = driver(&root);
    driver.press("esc");
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("on red"), "{styled}");
    driver.advance(std::time::Duration::from_millis(300));
    assert!(!intuituive::testing::styled(&driver).contains("on red"));
}

#[test]
fn zooming_hides_the_largest_items() {
    let root = tree("zoom");
    let mut driver = driver(&root);
    driver.press("+");
    assert!(
        rows(&driver)[0].contains("(+1 larger file(s)"),
        "{}",
        dump(&driver)
    );
    assert!(!has(&driver, "big/"), "{}", dump(&driver));
    assert!(
        has(&driver, "48.8K (52%)"),
        "the shares are of what is left:\n{}",
        dump(&driver)
    );
    driver.press("+ +");
    assert!(
        rows(&driver)[0].contains("(+3 larger file(s)"),
        "{}",
        dump(&driver)
    );
    assert!(
        has(&driver, "14.6K (96%)") && has(&driver, "│xx"),
        "{}",
        dump(&driver)
    );
    // Past the large ones, the tiny files get tiles.
    driver.press("+");
    assert!(
        has(&driver, "tiny0") && has(&driver, "tiny5"),
        "{}",
        dump(&driver)
    );
    assert!(has(&driver, "100 (17%)"), "{}", dump(&driver));
    driver.press("-");
    assert!(
        rows(&driver)[0].contains("(+3 larger file(s)"),
        "{}",
        dump(&driver)
    );
    driver.press("0");
    assert!(!rows(&driver)[0].contains("larger"), "{}", dump(&driver));
    assert!(has(&driver, "big/"));
}

/// Select `name`, on the folder's map: big at the left, movie.mp4 top
/// right, docs under it and song.mp3 beside docs.
fn select(driver: &mut Driver, name: &str) {
    let keys = match name {
        "big" => "right",
        "docs" => "right right",
        "movie.mp4" => "right right up",
        "song.mp3" => "right right right",
        _ => unreachable!(),
    };
    driver.press(keys);
    assert!(
        selected(driver).starts_with(&format!("SELECTED: {name}")),
        "{}",
        dump(driver)
    );
}

#[test]
fn backspace_deletes_after_asking() {
    let root = tree("delete");
    let mut driver = driver(&root);
    select(&mut driver, "song.mp3");
    driver.press("backspace");
    assert!(has(&driver, "Delete this file?"), "{}", dump(&driver));
    assert!(has(&driver, "(y/n)"));
    assert!(has(&driver, &root.join("song.mp3").display().to_string()) || has(&driver, "song.mp3"));
    driver.press("n");
    assert!(!has(&driver, "Delete this file?"));
    assert!(root.join("song.mp3").exists());
    driver.press("backspace y");
    assert!(!root.join("song.mp3").exists());
    assert!(!has(&driver, "Delete this file?"), "{}", dump(&driver));
    assert!(!has(&driver, "song.mp3"), "{}", dump(&driver));
    assert!(
        rows(&driver)[0].contains("Total: 156.8K (12 files), freed: 14.6K"),
        "{}",
        dump(&driver)
    );
    // The total is lit for a moment.
    assert!(intuituive::testing::styled(&driver).contains("on yellow"));
    driver.advance(std::time::Duration::from_millis(300));
    assert!(!intuituive::testing::styled(&driver).contains("on yellow"));
}

#[test]
fn ctrl_d_deletes_a_folder_with_its_count() {
    let root = tree("folder");
    let mut driver = driver(&root);
    select(&mut driver, "docs");
    driver.press("ctrl+d");
    assert!(
        has(&driver, "Delete folder with 1 children?"),
        "{}",
        dump(&driver)
    );
    driver.press("y");
    assert!(!root.join("docs").exists());
    assert!(
        rows(&driver)[0].contains("Total: 142.2K (11 files), freed: 29.3K"),
        "{}",
        dump(&driver)
    );
}

#[test]
fn a_failed_delete_says_why() {
    let root = tree("gone");
    let mut driver = driver(&root);
    select(&mut driver, "song.mp3");
    driver.press("backspace");
    std::fs::remove_file(root.join("song.mp3")).unwrap();
    driver.press("y");
    assert!(
        has(&driver, "Error: No such file or directory"),
        "{}",
        dump(&driver)
    );
    assert!(has(&driver, "(Press <ESC> to dismiss)"));
    driver.press("esc");
    assert!(!has(&driver, "Error:"));
    assert!(
        has(&driver, "song.mp3"),
        "the map is as it was:\n{}",
        dump(&driver)
    );
}

#[test]
fn without_confirmation_delete_is_at_once() {
    let root = tree("noconfirm");
    let mut options = options(&root);
    options.no_confirm = true;
    let mut driver = driver_with(options, W, H);
    select(&mut driver, "movie.mp4");
    driver.press("backspace");
    assert!(!root.join("movie.mp4").exists());
    assert!(
        rows(&driver)[0].contains("freed: 48.8K"),
        "{}",
        dump(&driver)
    );
}

#[test]
fn quitting_asks_first() {
    let root = tree("quit");
    let mut driver = driver(&root);
    driver.press("q");
    assert!(
        has(&driver, "Are you sure you want to quit?"),
        "{}",
        dump(&driver)
    );
    driver.press("n");
    assert!(!has(&driver, "Are you sure"));
    assert!(!driver.is_done());
    driver.press("ctrl+c y");
    assert!(driver.is_done());
}

#[test]
fn a_small_window_says_so() {
    let root = tree("small");
    let mut driver = driver_with(options(&root), 45, 12);
    assert!(
        has(&driver, "Terminal window is too small ¯\\_(ツ)_/¯"),
        "{}",
        dump(&driver)
    );
    assert!(!has(&driver, "Total:"));
    driver.resize(W, H);
    driver.settle();
    assert!(
        has(&driver, "Total:") && has(&driver, "movie.mp4"),
        "{}",
        dump(&driver)
    );
}

#[test]
fn an_empty_folder_says_so() {
    let root = base().join(format!("intuituive-disk-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("void")).unwrap();
    let mut driver = driver(&root);
    driver.press("enter");
    assert!(has(&driver, "Folder is empty"), "{}", dump(&driver));
}

/// A walker fed by the test: one entry at a time, each after a pause, so
/// the scan sends each on its own.
fn fed() -> (std::sync::mpsc::Sender<Found>, Walker) {
    let (tx, rx) = std::sync::mpsc::channel::<Found>();
    let walker: Walker = Box::new(move |emit| {
        for found in rx {
            std::thread::sleep(std::time::Duration::from_millis(60));
            if !emit(found) {
                break;
            }
        }
    });
    (tx, walker)
}

fn entry(path: &str, dir: bool, size: u64) -> Found {
    Found::Entry {
        path: PathBuf::from(path),
        dir,
        size,
    }
}

/// Step the app until `ok` holds (the scan's thread is the test's).
fn until(driver: &mut Driver, ok: impl Fn(&Driver) -> bool) {
    for _ in 0..300 {
        if ok(driver) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        driver.advance(std::time::Duration::from_millis(10));
    }
    panic!("never happened:\n{}", dump(driver));
}

#[test]
fn the_map_grows_while_the_scan_runs() {
    let (tx, walker) = fed();
    let mut options = Options::new("/scanned");
    options.walker = Some(walker);
    options.root_user = false;
    let mut driver = disk_app(options).driver(W, H);
    driver.settle();
    assert!(
        rows(&driver)[0].contains("Scanning: 0 (0 files)"),
        "{}",
        dump(&driver)
    );
    tx.send(entry("big", true, 0)).unwrap();
    tx.send(entry("big/a.bin", false, 60_000)).unwrap();
    until(&mut driver, |d| {
        rows(d)[0].contains("Scanning: 58.6K (2 files)")
    });
    assert!(has(&driver, "big/ (+1 descendants)"), "{}", dump(&driver));
    // The path read last, and the keys without delete.
    assert!(has(&driver, " big/a.bin"), "{}", dump(&driver));
    assert!(rows(&driver)[H as usize - 1].contains("<ENTER>/<ESC>: navigate"));
    assert!(!rows(&driver)[H as usize - 1].contains("BACKSPACE"));
    // Deleting waits for the scan.
    driver.press("right backspace");
    assert!(
        has(&driver, "Deletion is not allowed while scanning"),
        "{}",
        dump(&driver)
    );
    driver.press("x");
    assert!(!has(&driver, "delete while"));
    tx.send(entry("clip.mov", false, 120_000)).unwrap();
    until(&mut driver, |d| has(d, "clip.mov"));
    assert!(
        rows(&driver)[0].contains("Scanning: 175.8K (3 files)"),
        "{}",
        dump(&driver)
    );
    tx.send(Found::Failed).unwrap();
    drop(tx);
    until(&mut driver, |d| {
        rows(d)[0].contains("Total: 175.8K (3 files), freed: 0")
    });
    assert!(
        rows(&driver)[0].contains("(failed to read 1 files)")
            || rows(&driver)[0].contains("(1 errors)"),
        "{}",
        dump(&driver)
    );
    assert!(rows(&driver)[H as usize - 1].contains("BACKSPACE"));
}

#[test]
fn the_title_runs_a_bold_marker_while_scanning() {
    let (tx, walker) = fed();
    let mut options = Options::new("/scanned");
    options.walker = Some(walker);
    options.root_user = false;
    let mut driver = disk_app(options).driver(W, H);
    driver.settle();
    let first = intuituive::testing::styled(&driver)
        .lines()
        .next()
        .unwrap()
        .to_string();
    driver.advance(std::time::Duration::from_millis(200));
    let later = intuituive::testing::styled(&driver)
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert_ne!(first, later, "the marker moved");
    assert!(later.contains("bold"), "{later}");
    drop(tx);
}
