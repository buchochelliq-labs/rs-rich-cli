//! The termusic rebuild (examples/music.rs): its parsers, and the app on a
//! library made for each test (WAV files written here, a cover and an
//! `.lrc` file), playback driven on the app's clock.

#![cfg(all(feature = "wav", feature = "art"))]

#[path = "../examples/music.rs"]
#[allow(dead_code)]
mod music;

use std::path::{Path, PathBuf};
use std::time::Duration;

use intuituive::{App, Driver};
use music::*;

const RATE: u32 = 8000;

/// A 16-bit mono WAV of `secs` seconds of a quiet tone, with a
/// `LIST`/`INFO` chunk when `info` has fields (`INAM`, `IART`, `IPRD`).
fn wav_bytes(secs: u32, info: &[(&str, &str)]) -> Vec<u8> {
    let mut list = Vec::new();
    if !info.is_empty() {
        list.extend_from_slice(b"INFO");
        for (id, value) in info {
            let mut v = value.as_bytes().to_vec();
            v.push(0);
            list.extend_from_slice(id.as_bytes());
            list.extend_from_slice(&(v.len() as u32).to_le_bytes());
            if v.len() % 2 == 1 {
                v.push(0);
            }
            list.extend_from_slice(&v);
        }
    }
    let samples: Vec<u8> = (0..RATE * secs)
        .flat_map(|i| (((i % 40) as i16 - 20) * 100).to_le_bytes())
        .collect();
    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes()); // PCM
    body.extend_from_slice(&1u16.to_le_bytes()); // mono
    body.extend_from_slice(&RATE.to_le_bytes());
    body.extend_from_slice(&(RATE * 2).to_le_bytes());
    body.extend_from_slice(&2u16.to_le_bytes());
    body.extend_from_slice(&16u16.to_le_bytes());
    if !list.is_empty() {
        body.extend_from_slice(b"LIST");
        body.extend_from_slice(&(list.len() as u32).to_le_bytes());
        body.extend_from_slice(&list);
    }
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    body.extend_from_slice(&samples);
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

const LRC: &str = "[ar:Artist A]\n[00:01.00]line one\n[00:03.00]line two\n[00:05.00]line three\n";

/// root/
///   Artist A/Album One/01 - Intro.wav (4 s, tagged)
///                      02 - Song.wav  (6 s, tagged) + 02 - Song.lrc
///                      cover.png
///   Beta - Tune.wav (3 s, untagged: named Artist - Title)
///   track.mp3 (no decoder)  notes.txt  .hidden.wav
fn library(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("intuituive-music-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let album = root.join("Artist A/Album One");
    std::fs::create_dir_all(&album).unwrap();
    let tags = |title| [("INAM", title), ("IART", "Artist A"), ("IPRD", "Album One")];
    std::fs::write(album.join("01 - Intro.wav"), wav_bytes(4, &tags("Intro"))).unwrap();
    std::fs::write(album.join("02 - Song.wav"), wav_bytes(6, &tags("Song"))).unwrap();
    std::fs::write(album.join("02 - Song.lrc"), LRC).unwrap();
    rich_art::image::RgbImage::from_pixel(8, 8, rich_art::image::Rgb([200, 30, 30]))
        .save(album.join("cover.png"))
        .unwrap();
    std::fs::write(root.join("Beta - Tune.wav"), wav_bytes(3, &[])).unwrap();
    std::fs::write(root.join("track.mp3"), b"ID3 not really").unwrap();
    std::fs::write(root.join("notes.txt"), "not music").unwrap();
    std::fs::write(root.join(".hidden.wav"), wav_bytes(1, &[])).unwrap();
    root.canonicalize().unwrap()
}

fn app(root: &Path) -> App {
    music_app(root.to_path_buf()).wait_for_tasks(true)
}

fn start(root: &Path) -> Driver {
    let mut driver = app(root).driver(110, 30);
    driver.settle();
    driver
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn has(driver: &Driver, needle: &str) -> bool {
    rows(driver).iter().any(|r| r.contains(needle))
}

#[track_caller]
fn assert_has(driver: &Driver, needle: &str) {
    assert!(
        has(driver, needle),
        "{needle:?} not on\n{}",
        rows(driver).join("\n")
    );
}

#[track_caller]
fn assert_hasnt(driver: &Driver, needle: &str) {
    assert!(
        !has(driver, needle),
        "{needle:?} on\n{}",
        rows(driver).join("\n")
    );
}

/// Open the album and add both its tracks from the library (l on each),
/// then go to the playlist (Tab).
fn queue_album(driver: &mut Driver) {
    // root ▾, Artist A, Beta - Tune.wav, track.mp3
    driver.press("j l j l j l j l tab");
}

// -- the parsers -------------------------------------------------------------

#[test]
fn info_chunks_names_and_lengths_become_tags() {
    let root = library("tags");
    let intro = Track::read(&root.join("Artist A/Album One/01 - Intro.wav"));
    assert_eq!(intro.title(), "Intro");
    assert_eq!(intro.artist(), "Artist A");
    assert_eq!(intro.album(), "Album One");
    assert_eq!(intro.duration, Some(Duration::from_secs(4)));
    assert_eq!(intro.duration_cell(), "[ 00:04 ]");

    let tune = Track::read(&root.join("Beta - Tune.wav"));
    assert_eq!((tune.artist(), tune.title()), ("Beta", "Tune"));
    assert_eq!(tune.album(), UNKNOWN_ALBUM);

    let mp3 = Track::read(&root.join("track.mp3"));
    assert_eq!(mp3.duration, None);
    assert_eq!(mp3.duration_cell(), "[--:--]");
    assert_eq!(mp3.artist(), UNKNOWN_ARTIST);
}

#[test]
fn lrc_files_parse_and_follow_the_clock_two_seconds_ahead() {
    let lyrics = parse_lrc("[offset:+500]\n[00:01.00][00:07.5]again\n[00:03.250]two\nplain\n");
    assert_eq!(lyrics.offset, 500);
    assert_eq!(
        lyrics.captions,
        vec![
            (1000, "again".to_string()),
            (3250, "two".to_string()),
            (7500, "again".to_string())
        ]
    );
    // termusic shows a caption two seconds early (plus the offset).
    let at = |ms| lyrics.index_at(Duration::from_millis(ms));
    assert_eq!(at(0), Some(0));
    assert_eq!(at(800), Some(1));
    assert_eq!(at(5000), Some(2));
    assert_eq!(parse_lrc("no stamps").index_at(Duration::ZERO), None);
}

#[test]
fn small_helpers() {
    assert_eq!(fmt_short(Duration::from_secs(205)), "03:25");
    assert_eq!(fmt_short(Duration::from_secs(3723)), "1:02:03");
    assert!(wildcard("song", "02 - Song.wav"));
    assert!(wildcard("0? -*wav", "02 - Song.wav"));
    assert!(!wildcard("tune", "02 - Song.wav"));
    assert_eq!(natural_cmp("track 2", "Track 10"), std::cmp::Ordering::Less);
    let mut mode = LoopMode::Playlist;
    let modes: Vec<&str> = (0..4)
        .map(|_| {
            mode = mode.cycle();
            mode.symbol()
        })
        .collect();
    assert_eq!(modes, ["⮕", "🔂", "🔀", "🔁"]);
}

#[test]
fn the_library_lists_folders_and_tracks_only() {
    let root = library("scan");
    let entry = scan(&root);
    let names: Vec<&str> = entry.children.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Artist A", "Beta - Tune.wav", "track.mp3"]);
    assert_eq!(entry.tracks().len(), 4);
    assert_eq!(entry.albums().len(), 2);
    assert_eq!(
        cover_for(&root.join("Artist A/Album One/02 - Song.wav")),
        Some(root.join("Artist A/Album One/cover.png"))
    );
    assert_eq!(cover_for(&root.join("Beta - Tune.wav")), None);
}

// -- the app -----------------------------------------------------------------

#[test]
fn the_tree_view_layout() {
    let root = library("layout");
    let driver = start(&root);
    let screen = rows(&driver);
    assert!(screen[0].starts_with("╭ Library ─"), "{}", screen[0]);
    assert!(
        screen[0].contains("╭─ Playlist ──┤ Total 0 tracks | 00:00 | Mode: 🔁 ├─"),
        "{}",
        screen[0]
    );
    assert_has(&driver, "Duration  Title");
    assert_has(&driver, "Artist A");
    assert_has(&driver, "Beta - Tune.wav");
    assert_has(&driver, "track.mp3");
    assert_hasnt(&driver, "notes.txt");
    assert_hasnt(&driver, ".hidden");
    assert_has(
        &driver,
        " Status: Stopped | Volume: 30 | Speed: 1.0 | Gapless: True ",
    );
    assert_has(&driver, "--:--");
    assert_has(&driver, " No track is playing ");
    assert_has(&driver, "Stopped.");
    assert!(screen[29].contains("Help: <control+h>"), "{}", screen[29]);
}

#[test]
fn tracks_are_added_from_the_library_and_played_on_the_clock() {
    let root = library("play");
    let mut driver = start(&root);
    queue_album(&mut driver);
    assert_has(&driver, "Total 2 tracks | 00:10 |");
    assert_has(&driver, "🚀 [ 00:04 ] Intro");
    assert_has(&driver, "Artist A");
    assert_has(&driver, "Album One");

    driver.press("l");
    assert_has(&driver, " Status: Running | Volume: 30");
    assert_has(&driver, "00:00    -    00:04");
    assert_has(&driver, " Lyrics of Artist A - Intro ");
    assert_has(&driver, "No lyrics available.");
    driver.advance(Duration::from_secs(2));
    assert_has(&driver, "00:02    -    00:04");

    // The end of the first plays the second, its lyrics following.
    driver.advance(Duration::from_millis(2100));
    assert_has(&driver, " Lyrics of Artist A - Song ");
    // The cursor's 🚀 over Intro, the playing track's ► on Song.
    assert_has(&driver, "🚀 [ 00:04 ] Intro");
    assert_has(&driver, "►  [ 00:06 ] Song");
    assert_has(&driver, "line one");
    driver.advance(Duration::from_secs(2));
    assert_has(&driver, "00:02    -    00:06");
    // The current line is lit; the one before it is not.
    let style_of = |needle: &str| {
        let screen = rows(&driver);
        let y = screen.iter().position(|r| r.contains(needle)).unwrap();
        let x = screen[y][..screen[y].find(needle).unwrap()].chars().count() as u16;
        let cell = driver.screen().cell(x, y as u16).style;
        driver.screen().style(cell).cloned()
    };
    assert_ne!(style_of("line two"), style_of("line one"));
}

#[test]
fn pause_seek_volume_speed_and_gapless() {
    let root = library("keys");
    let mut driver = start(&root);
    queue_album(&mut driver);
    driver.press("space");
    assert_has(&driver, "Status: Running");
    driver.advance(Duration::from_secs(1));
    driver.press("space");
    assert_has(&driver, "Status: Paused");
    driver.advance(Duration::from_secs(2));
    assert_has(&driver, "00:01    -    00:04");
    driver.press("f");
    assert_has(&driver, "00:04    -    00:04");
    driver.press("b b");
    assert_has(&driver, "00:00    -    00:04");
    driver.press("plus plus minus");
    assert_has(&driver, "Volume: 35");
    driver.press("ctrl+f ctrl+f ctrl+g");
    assert_has(&driver, "Speed: 1.2 | Gapless: False");
    driver.press("space");
    driver.advance(Duration::from_secs(1));
    assert_has(&driver, "00:01    -    00:04");
    driver.press("0");
    assert_has(&driver, "00:00    -    00:04");
}

#[test]
fn loop_modes_decide_what_follows() {
    let root = library("modes");
    let mut driver = start(&root);
    queue_album(&mut driver);
    // Playlist once: the last track ends the playback.
    driver.press("m");
    assert_has(&driver, "Mode: ⮕");
    driver.press("j l");
    assert_has(&driver, "Lyrics of Artist A - Song");
    driver.advance(Duration::from_millis(6200));
    assert_has(&driver, "Status: Stopped");
    assert_has(&driver, "Stopped.");

    // Track: the same track again.
    driver.press("m");
    assert_has(&driver, "Mode: 🔂");
    driver.press("k l");
    driver.advance(Duration::from_millis(4100));
    assert_has(&driver, "Lyrics of Artist A - Intro");
    assert_has(&driver, "Status: Running");

    // Playlist: on to the next, and round.
    driver.press("m m");
    assert_has(&driver, "Mode: 🔁");
    driver.press("n");
    assert_has(&driver, "Lyrics of Artist A - Song");
    driver.press("n");
    assert_has(&driver, "Lyrics of Artist A - Intro");
    driver.press("N");
    assert_has(&driver, "Lyrics of Artist A - Song");
}

#[test]
fn the_playlist_is_edited_with_termusics_keys() {
    let root = library("edit");
    let mut driver = start(&root);
    queue_album(&mut driver);
    driver.press("J");
    let screen = rows(&driver).join("\n");
    assert!(
        screen.find("Song").unwrap() < screen.find("Intro").unwrap(),
        "{screen}"
    );
    driver.press("K");
    let screen = rows(&driver).join("\n");
    assert!(
        screen.find("Intro").unwrap() < screen.find("Song").unwrap(),
        "{screen}"
    );
    driver.press("r");
    assert_has(&driver, "Total 2 tracks");
    driver.press("d");
    assert_has(&driver, "Total 1 tracks");
    driver.press("S");
    assert!(has(&driver, "Total 2 tracks") || has(&driver, "Total 3 tracks"));
    driver.press("D");
    assert_has(&driver, "Total 0 tracks");
    driver.press("s");
    assert_has(&driver, "Total 4 tracks");
}

#[test]
fn search_locates_and_loads_tracks() {
    let root = library("search");
    let mut driver = start(&root);
    driver.press("/");
    assert_has(&driver, "─ Search for: (support * and ?) ─");
    assert_has(&driver, "─ Results: (Enter: locate/l: load to playlist) ─");
    assert_has(&driver, "3     track.mp3");
    driver.type_text("song");
    driver.settle();
    assert_has(&driver, "0     02 - Song.wav");
    assert_hasnt(&driver, "3     track.mp3");
    driver.press("enter l");
    assert_has(&driver, "Total 1 tracks");
    driver.press("enter");
    assert_hasnt(&driver, "Search for");
    // Located: the album is open in the tree, the track selected.
    assert_has(&driver, "Album One");
    assert_has(&driver, "02 - Song.wav");
    driver.press("l");
    assert_has(&driver, "Total 2 tracks");

    // In the playlist, `l` plays the result.
    driver.press("tab /");
    assert_has(&driver, "─ Results: (Enter: locate/l: play selected) ─");
    driver.press("enter l esc");
    assert_has(&driver, "Status: Running");
}

#[test]
fn the_cover_is_drawn_while_a_track_with_one_plays() {
    let root = library("cover");
    let mut driver = start(&root);
    queue_album(&mut driver);
    assert_hasnt(&driver, "▀");
    driver.press("l");
    driver.advance(Duration::from_millis(200));
    assert_has(&driver, "▀");
    driver.press("ctrl+shift+end");
    assert_hasnt(&driver, "▀");
    driver.press("ctrl+shift+end");
    driver.advance(Duration::from_millis(200));
    assert_has(&driver, "▀");
    // A folder with no picture: none.
    driver.press("D");
    driver.press("shift+tab j l tab l");
    driver.advance(Duration::from_millis(200));
    assert_has(&driver, "Lyrics of Beta - Tune");
    assert_hasnt(&driver, "▀");
}

#[test]
fn a_format_without_a_decoder_is_an_error() {
    let root = library("mp3");
    let mut driver = start(&root);
    driver.press("j j j l tab l");
    assert_has(&driver, "[--:--]");
    assert_has(&driver, "No decoder for .mp3 files: track.mp3");
    assert_has(&driver, "Status: Stopped");
}

#[test]
fn deleting_from_the_library_asks_first() {
    let root = library("delete");
    let mut driver = start(&root);
    driver.press("j j l");
    assert_has(&driver, "Total 1 tracks");
    driver.press("d");
    assert_has(&driver, "Are you sure you want to delete?");
    driver.press("y");
    assert!(!root.join("Beta - Tune.wav").exists());
    assert_hasnt(&driver, "Beta - Tune.wav");
    assert_has(&driver, "Total 0 tracks");
}

#[test]
fn yank_and_paste_move_a_file() {
    let root = library("paste");
    let mut driver = start(&root);
    // Yank Beta - Tune.wav, paste into Artist A.
    driver.press("j j y k p");
    assert!(root.join("Artist A/Beta - Tune.wav").exists());
    assert!(!root.join("Beta - Tune.wav").exists());
}

#[test]
fn folders_open_as_the_root_and_back() {
    let root = library("roots");
    let mut driver = start(&root);
    driver.press("j enter");
    assert!(
        rows(&driver)[1].contains("▾ Artist A"),
        "{}",
        rows(&driver)[1]
    );
    assert_hasnt(&driver, "track.mp3");
    driver.press("backspace");
    assert_has(&driver, "track.mp3");
    // Back up, Artist A is selected: a second root, and back to the first.
    driver.press("a o");
    assert!(
        rows(&driver)[1].contains("▾ Artist A"),
        "{}",
        rows(&driver)[1]
    );
    driver.press("o");
    assert_has(&driver, "track.mp3");
}

#[test]
fn the_playlist_saves_as_m3u() {
    let root = library("save");
    let mut driver = start(&root);
    queue_album(&mut driver);
    driver.press("ctrl+s");
    driver.type_text("mix");
    driver.press("enter");
    let m3u = std::fs::read_to_string(root.join("mix.m3u")).unwrap();
    assert_eq!(m3u.lines().count(), 2);
    assert!(m3u.contains("01 - Intro.wav"), "{m3u}");
}

#[test]
fn help_lists_the_keys_by_section_and_q_asks_before_quitting() {
    let root = library("help");
    let mut driver = start(&root);
    driver.press("ctrl+h");
    assert_has(&driver, "Global");
    assert_has(&driver, "Add one track to playlist");
    driver.type_text("pause");
    driver.settle();
    assert_has(&driver, "Pause current track");
    driver.press("esc esc");
    driver.press("q");
    assert_has(&driver, "Are sure you want to quit?");
    driver.press("n");
    assert!(!driver.is_done());
    driver.press("q y");
    assert!(driver.is_done());
}
