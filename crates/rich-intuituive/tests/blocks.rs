//! The rata-tetris rebuild (examples/blocks.rs): the game's rules on the
//! model, and the app driven by keys and the app's clock, on a seeded bag.

#[path = "../examples/blocks.rs"]
#[allow(dead_code)]
mod blocks;

use std::path::PathBuf;
use std::time::Duration;

use blocks::*;
use intuituive::Driver;

const W: u16 = SCREEN_WIDTH;
const H: u16 = SCREEN_HEIGHT;

/// A seed whose first piece is `kind`.
fn seed_for(kind: Kind) -> u64 {
    (1..1000)
        .find(|&seed| Game::new(seed, None).piece().kind == kind)
        .expect("some seed starts with each piece")
}

fn driver(game: Game) -> Driver {
    let mut driver = game_app(game).driver(W, H);
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

fn dump(driver: &Driver) -> String {
    rows(driver).join("\n")
}

/// The playfield's twenty rows, two columns a cell (the board's inside).
fn field(driver: &Driver) -> Vec<String> {
    let rows = rows(driver);
    rows[5..25]
        .iter()
        .map(|r| r.chars().skip(25).take(20).collect())
        .collect()
}

/// The rows of the playfield holding a block.
fn rows_with(field: &[String], glyph: &str) -> Vec<usize> {
    (0..field.len())
        .filter(|&i| field[i].contains(glyph))
        .collect()
}

fn frames(n: u32) -> Duration {
    FRAME * n
}

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("blocks-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

// The model.

#[test]
fn a_seed_deals_the_same_bag() {
    let deal = |seed| {
        let mut game = Game::new(seed, None);
        game.space();
        let mut kinds = vec![game.piece().kind];
        for _ in 0..13 {
            game.hard_drop();
            // An empty board again, so the stack never tops out.
            game.fill_rows(&[".........."; HEIGHT]);
            kinds.push(game.piece().kind);
        }
        kinds
    };
    assert_eq!(deal(42), deal(42));
    // Each bag of seven holds every piece once.
    let kinds = deal(42);
    for kind in KINDS {
        assert_eq!(kinds[..7].iter().filter(|k| **k == kind).count(), 1);
        assert_eq!(kinds[7..].iter().filter(|k| **k == kind).count(), 1);
    }
}

#[test]
fn walls_stop_moves_and_rotations_without_kicks() {
    let mut game = Game::new(seed_for(Kind::I), None);
    game.space();
    for _ in 0..10 {
        game.shift(-1);
    }
    assert_eq!(game.piece().col, 0);
    // Upright, the I sits in its matrix's third column: it can go one
    // further left, and then cannot turn back flat (no wall kick).
    game.rotate(1);
    game.shift(-1);
    game.shift(-1);
    game.shift(-1);
    assert_eq!(game.piece().col, -2);
    game.rotate(1);
    assert_eq!(
        game.piece().rotation,
        1,
        "a rotation into the wall does nothing"
    );
}

#[test]
fn clearing_scores_by_level_and_ten_lines_level_up() {
    let mut game = Game::new(seed_for(Kind::I), None);
    game.space();
    game.fill_rows(&["###.######"; 4]);
    // The I upright in column 3 (its matrix's third column) and a slam:
    // four rows, a tetris.
    game.rotate(1);
    game.shift(-2);
    assert_eq!(game.piece().col + 2, 3);
    game.hard_drop();
    assert_eq!((game.lines, game.score), (4, 1200));
    // The rows flash for 300 ms + 100 ms a row, then go.
    for _ in 0..(700 / 17 + 1) {
        game.frame();
    }
    assert!((0..HEIGHT).all(|y| (0..WIDTH).all(|x| game.at(x, y).is_none())));
    assert_eq!(game.level, 0);

    // The tenth line: level 1, scored at level 0, and gravity of 43 frames.
    let mut game = Game::new(seed_for(Kind::I), None);
    game.space();
    game.lines = 9;
    game.fill_rows(&["###.######"]);
    game.rotate(1);
    game.shift(-2);
    game.hard_drop();
    assert_eq!((game.lines, game.level, game.score), (10, 1, 40));
    for _ in 0..(400 / 17 + 1) {
        game.frame();
    }
    let row = game.piece().row;
    for _ in 0..42 {
        game.frame();
    }
    assert_eq!(game.piece().row, row);
    game.frame();
    assert_eq!(game.piece().row, row + 1, "level 1 waits 43 frames");
}

#[test]
fn gravity_follows_the_nes_table() {
    let mut game = Game::new(1, None);
    game.space();
    let row = game.piece().row;
    for _ in 0..47 {
        game.frame();
    }
    assert_eq!(game.piece().row, row, "level 0 waits 48 frames");
    game.frame();
    assert_eq!(game.piece().row, row + 1);
    // Paused, nothing falls.
    game.space();
    for _ in 0..100 {
        game.frame();
    }
    assert_eq!(game.piece().row, row + 1);
}

#[test]
fn hold_swaps_once_a_piece() {
    let mut game = Game::new(3, None);
    game.space();
    let (first, next) = (game.piece().kind, game.next());
    game.hold();
    assert_eq!(game.held(), Some(first));
    assert_eq!(game.piece().kind, next);
    game.hold();
    assert_eq!(game.piece().kind, next, "one hold a piece");
    game.hard_drop();
    game.hold();
    assert_eq!(game.piece().kind, first, "the held piece comes back");
}

// The app.

#[test]
fn start_screen_then_the_board() {
    let mut driver = driver(Game::new(5, None));
    assert!(has(&driver, "██████  ████  ██████"), "{}", dump(&driver));
    assert!(has(&driver, "start: space quit: q"));
    assert!(!has(&driver, "STATISTICS"));
    driver.press("space");
    for needle in [
        "STATISTICS",
        "LINES - 000",
        "TOP",
        "SCORE",
        "NEXT",
        "LEVEL 00",
        "HOLD",
    ] {
        assert!(has(&driver, needle), "{needle}\n{}", dump(&driver));
    }
    // The piece is in the top rows, its ghost on the floor.
    let field = field(&driver);
    assert!(
        rows_with(&field, "██").iter().all(|&r| r < 2),
        "{}",
        field.join("\n")
    );
    assert_eq!(rows_with(&field, "┌┐").last(), Some(&19));
}

#[test]
fn the_clock_drops_the_piece_and_keys_move_it() {
    let mut driver = driver(Game::new(seed_for(Kind::O), None));
    driver.press("space");
    let before = field(&driver);
    assert_eq!(rows_with(&before, "██"), vec![0, 1]);
    // 47 frames: still; the 48th: a row down.
    driver.advance(frames(47));
    assert_eq!(rows_with(&field(&driver), "██"), vec![0, 1]);
    driver.advance(frames(1));
    assert_eq!(rows_with(&field(&driver), "██"), vec![1, 2]);
    // Left twice, two cells (four columns) left.
    let col = |f: &[String]| f[1].chars().position(|c| c == '█').unwrap();
    let at = col(&field(&driver));
    driver.press("left left");
    assert_eq!(col(&field(&driver)) + 4, at);
    // Down: a row; up: slammed to the floor, and the next piece comes in.
    driver.press("down");
    assert_eq!(rows_with(&field(&driver), "██"), vec![2, 3]);
    driver.press("up");
    let landed = field(&driver);
    assert!(
        landed[18].contains("████") && landed[19].contains("████"),
        "{}",
        landed.join("\n")
    );
}

#[test]
fn cleared_rows_flash_then_go() {
    let mut game = Game::new(seed_for(Kind::I), None);
    game.fill_rows(&["###....###"]);
    let mut driver = driver(game);
    driver.press("space up");
    assert!(has(&driver, "LINES - 001"));
    assert!(has(&driver, "000040"), "{}", dump(&driver));
    // Lit first, then dark, then lit again: four frames each.
    assert!(field(&driver)[19].starts_with("████████"));
    driver.advance(frames(4));
    assert!(
        field(&driver)[19].starts_with("░░░░"),
        "{}",
        field(&driver).join("\n")
    );
    driver.advance(frames(4));
    assert!(field(&driver)[19].starts_with("████████"));
    // 400 ms in all, then the row is gone and the next piece is in play.
    driver.advance(frames(16));
    let field = field(&driver);
    assert!(!field[19].contains('░'), "{}", field.join("\n"));
    assert!(!rows_with(&field, "██").contains(&19));
}

#[test]
fn pause_hides_the_board() {
    let mut driver = driver(Game::new(5, None));
    driver.press("space space");
    assert!(has(&driver, "██████  ██████  ██  ██"), "{}", dump(&driver));
    assert!(!has(&driver, "STATISTICS"));
    // Paused, the clock does nothing; Space resumes where it was.
    driver.advance(Duration::from_secs(5));
    driver.press("space");
    assert!(rows_with(&field(&driver), "██").iter().all(|&r| r < 2));
}

#[test]
fn game_over_keeps_the_top_score_and_restarts() {
    let file = temp_file("top_score");
    std::fs::write(&file, "100").unwrap();
    let mut game = Game::new(9, Some(file.clone()));
    assert_eq!(game.top, 100);
    // A stack to the second row, no row full: the next piece cannot fall.
    game.fill_rows(&[".#########"; 19]);
    game.score = 500;
    let mut driver = driver(game);
    driver.press("space up");
    assert!(
        has(&driver, "New top score set at 500!"),
        "{}",
        dump(&driver)
    );
    assert!(has(&driver, "Reach level 0, and cleared 0 lines."));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "500");
    // Space plays again, the top score now 500.
    driver.press("space");
    assert!(has(&driver, "000500"), "{}", dump(&driver));
    assert!(has(&driver, "LINES - 000"));
    let _ = std::fs::remove_file(file);
}

#[test]
fn a_lower_score_is_reported_not_kept() {
    let file = temp_file("lower");
    std::fs::write(&file, "900").unwrap();
    let mut game = Game::new(9, Some(file.clone()));
    game.fill_rows(&[".#########"; 19]);
    game.score = 20;
    let mut driver = driver(game);
    driver.press("space up");
    assert!(has(&driver, "You scored 20 points."), "{}", dump(&driver));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "900");
    let _ = std::fs::remove_file(file);
}

#[test]
fn hold_shows_in_its_panel() {
    let game = Game::new(seed_for(Kind::T), None);
    let mut driver = driver(game);
    driver.press("space c");
    let rows = rows(&driver);
    let hold = rows.iter().position(|r| r.contains("HOLD")).unwrap();
    assert!(
        rows[hold + 1].contains("  ██") && rows[hold + 2].contains("██████"),
        "{}",
        dump(&driver)
    );
}

#[test]
fn statistics_count_each_piece() {
    let game = Game::new(seed_for(Kind::O), None);
    let mut driver = driver(game);
    driver.press("space");
    // The O is the third piece of the statistics: ████ then its count,
    // the only one dealt so far.
    let rows = rows(&driver);
    let counts: Vec<String> = rows[3..23]
        .iter()
        .map(|r| r.chars().take(24).collect::<String>())
        .filter_map(|r| r.split_whitespace().nth(2).map(String::from))
        .filter(|c| c.len() == 3 && c.chars().all(|d| d.is_ascii_digit()))
        .collect();
    assert_eq!(
        counts,
        ["000", "000", "001", "000", "000", "000", "000"],
        "{}",
        dump(&driver)
    );
}

#[test]
fn a_small_terminal_asks_for_more_room() {
    let mut driver = game_app(Game::new(1, None)).driver(50, 20);
    driver.settle();
    assert!(
        has(&driver, "Terminal must be at least 64 x 28!"),
        "{}",
        dump(&driver)
    );
    driver.resize(80, 30);
    driver.settle();
    assert!(has(&driver, "start: space quit: q"));
}

#[test]
fn level_colours_change_and_the_board_is_drawn_in_them() {
    // Two games at levels 0 and 1 draw the same piece in other colours.
    let styled = |level: u8| {
        let mut game = Game::new(seed_for(Kind::O), None);
        game.level = level;
        let mut driver = driver(game);
        driver.press("space");
        intuituive::testing::styled(&driver)
    };
    assert_ne!(styled(0), styled(1));
}
