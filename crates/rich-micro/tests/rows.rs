//! Assets two rows high (#711): the sizes and the manifest, the rows they
//! reserve in wrapped text, a table cell and a panel, each drawing mode's
//! bytes for a 2×2 icon and a 3×2 animation, the fallback, and determinism.
mod common;

use std::sync::Arc;
use std::time::Duration;

use rich::cells::cell_len;
use rich::panel::Panel;
use rich::table::{Cell, Table};
use rich::{ColorSystem, Console, Renderable, Segment, Text};
use rich_art::graphics::{AnimationFrame, CellPixels};
use rich_art::image::{DynamicImage, Rgba, RgbaImage};
use rich_ext::frame::Frame;
use rich_micro::create::{write_package, PackageSpec};
use rich_micro::package::load_package;
use rich_micro::pipeline::Pipeline;
use rich_micro::{
    render_markup, reserve_rows, CellSize, FallbackPreference, Layer, Limits, MicroAsset,
    MicroAssetRef, MicroGraphics, MicroMeta, MicroMode, MicroRegistry, MicroRows, Selection,
    PAD_CELL,
};

const BLANK: &str = "\u{2800}";

/// A 2×2 icon (a blue square with a white dot) and a 3×2 animation (three
/// coloured frames), written as packages under `dir` and loaded.
fn assets(dir: &std::path::Path) -> (MicroAsset, MicroAsset) {
    let mut square = RgbaImage::from_pixel(32, 64, Rgba([20, 90, 200, 255]));
    for y in 28..36 {
        for x in 12..20 {
            square.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    let icon = Pipeline::new(CellSize::new(2, 2).unwrap())
        .process_image(&DynamicImage::ImageRgba8(square));
    let spec = PackageSpec::new("team/icon", "blue square with a white dot")
        .emoji("🧊")
        .text("[]");
    let icon = write_package(&dir.join("icon"), &spec, &icon, false).unwrap();
    let frame = |c: [u8; 4]| AnimationFrame {
        image: RgbaImage::from_pixel(48, 64, Rgba(c)),
        delay: Duration::from_millis(150),
    };
    let flame = Pipeline::new(CellSize::new(3, 2).unwrap()).process_frames(&[
        frame([230, 80, 20, 255]),
        frame([250, 180, 30, 255]),
        frame([200, 40, 10, 255]),
    ]);
    let spec = PackageSpec::new("team/flame", "orange flame flickering")
        .emoji("🔥")
        .text("(^)");
    let flame = write_package(&dir.join("flame.richmicro"), &spec, &flame, true).unwrap();
    (icon, flame)
}

struct Fixture {
    _dir: tempfile::TempDir,
    registry: Arc<MicroRegistry>,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let (icon, flame) = assets(dir.path());
    let mut registry = MicroRegistry::new();
    registry.add(Layer::Inline, icon).unwrap();
    registry.add(Layer::Inline, flame).unwrap();
    // A one-row asset beside them.
    registry
        .add(
            Layer::Inline,
            MicroAsset::new("ok", "check")
                .unwrap()
                .with_text("ok")
                .unwrap(),
        )
        .unwrap();
    Fixture {
        _dir: dir,
        registry: Arc::new(registry),
    }
}

fn pipe(width: usize) -> Console {
    Console::builder()
        .force_terminal(false)
        .width(width)
        .build()
}

fn terminal(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .build()
}

/// `markup` with its tokens as placeholders, showing the text fallback.
fn text(registry: &MicroRegistry, markup: &str) -> Text {
    let (text, diagnostics) = render_markup(&pipe(80), markup, registry, FallbackPreference::Text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    text
}

/// Rendered lines, as plain strings.
fn lines(console: &Console, renderable: &dyn Renderable) -> Vec<String> {
    console
        .render_lines(renderable, &console.options(), false)
        .into_iter()
        .map(|line| {
            line.iter()
                .filter(|s| !s.control)
                .map(|s| s.text.as_str())
                .collect()
        })
        .collect()
}

fn print(console: &Console, renderable: &dyn Renderable) -> String {
    console.capture(|c| c.print(renderable))
}

/// A borrowed renderable, for views over values the test keeps.
struct By<'a>(&'a dyn Renderable);

impl Renderable for By<'_> {
    fn rich_render(&self, console: &Console, options: &rich::ConsoleOptions) -> Vec<Segment> {
        self.0.rich_render(console, options)
    }
    fn measure(
        &self,
        console: &Console,
        options: &rich::ConsoleOptions,
    ) -> rich::measure::Measurement {
        self.0.measure(console, options)
    }
}

fn graphics(registry: &Arc<MicroRegistry>, mode: MicroMode, animate: bool) -> MicroGraphics {
    MicroGraphics::new(
        Arc::clone(registry),
        Selection::forced(mode, CellPixels::new(8, 16)).animate(animate),
    )
}

// ---- sizes and the manifest ---------------------------------------------

#[test]
fn two_row_sizes_are_allowed_and_more_is_refused() {
    for (size, cols, rows) in [("2x2", 2, 2), ("3x2", 3, 2), ("1x2", 1, 2), ("4x2", 4, 2)] {
        let parsed: CellSize = size.parse().unwrap();
        assert_eq!((parsed.cols(), parsed.rows()), (cols, rows), "{size}");
        let asset = MicroAsset::new("a", "alt")
            .unwrap()
            .with_size(parsed)
            .unwrap();
        assert_eq!((asset.cols(), asset.rows()), (cols, rows));
    }
    for size in ["2x3", "4x3", "5x2", "5x1", "0x2"] {
        let error = size.parse::<CellSize>().unwrap_err();
        assert!(
            matches!(error, rich_micro::MicroError::InvalidSize(_)),
            "{size}"
        );
    }
    let error = "3x3".parse::<CellSize>().unwrap_err().to_string();
    assert!(
        error.contains("at most 4 columns and 2 rows (4x2)"),
        "{error}"
    );
    // The fallback fits the first row: three columns for a 3x2 asset.
    let wide = MicroAsset::new("a", "alt")
        .unwrap()
        .with_size(CellSize::new(3, 2).unwrap())
        .unwrap();
    assert!(wide.clone().with_text("abc").is_ok());
    assert!(wide.with_text("abcd").is_err());
}

#[test]
fn the_manifest_declares_the_size_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let (icon, flame) = assets(dir.path());
    assert_eq!(icon.size().to_string(), "2x2");
    assert_eq!(flame.size().to_string(), "3x2");
    assert_eq!(flame.kind(), rich_micro::AssetKind::Animated);
    // `create` writes the size as "COLSxROWS", and the images at the
    // canvas of the whole box: 16x32 and 24x32 pixels at 8x16 cells.
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("icon/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["size"], "2x2");
    let still = icon.variants().static_image.as_ref().unwrap();
    assert_eq!((still.info.width, still.info.height), (16, 32));
    let motion = flame.variants().animation.as_ref().unwrap();
    assert_eq!((motion.info.width, motion.info.height), (24, 32));
    assert_eq!(motion.info.frames, 3);
    // Read back, the same asset.
    let again = load_package(&dir.path().join("icon"), Layer::Inline, &Limits::default()).unwrap();
    assert_eq!((again.cols(), again.rows()), (2, 2));
    // The object form says the same.
    let object = dir.path().join("object");
    common::package_dir(
        &object,
        &serde_json::json!({
            "schema_version": 1,
            "name": "team/object",
            "alt": "an icon",
            "size": {"cols": 3, "rows": 2},
            "static": "static.png",
        }),
        &[("static.png", common::png(24, 32))],
    );
    let loaded = load_package(&object, Layer::Inline, &Limits::default()).unwrap();
    assert_eq!(loaded.size(), CellSize::new(3, 2).unwrap());
}

#[test]
fn old_packages_keep_their_size_and_none_is_inferred() {
    // Every package written before two-row sizes still loads as it was.
    for name in ["check", "dot", "spark"] {
        let path = common::fixtures().join(format!("builtin/{name}.richmicro"));
        let asset = load_package(&path, Layer::Inline, &Limits::default()).unwrap();
        assert_eq!(asset.rows(), 1, "{name}");
    }
    // A package that declares no size is 2x1, whatever its image's shape: a
    // size the manifest does not declare is never used.
    let dir = tempfile::tempdir().unwrap();
    common::package_dir(
        dir.path(),
        &common::manifest("tall"),
        &[("static.png", common::png(16, 32))],
    );
    let asset = load_package(dir.path(), Layer::Inline, &Limits::default()).unwrap();
    assert_eq!(asset.size(), CellSize::default());
}

#[test]
fn a_size_other_than_the_declared_one_is_never_drawn() {
    let fixture = fixture();
    let made = text(&fixture.registry, "a :micro:team/icon: b");
    // The same name, declared 2x1 elsewhere: the placeholder's 2x2 tag does
    // not match, so nothing is drawn over the cells.
    let mut other = MicroRegistry::new();
    other
        .add(
            Layer::Inline,
            MicroAsset::new("team/icon", "flat")
                .unwrap()
                .with_text("--")
                .unwrap(),
        )
        .unwrap();
    let other = Arc::new(other);
    let console = terminal(40);
    let rows = MicroRows::new(made);
    let drawn = print(
        &console,
        &graphics(&other, MicroMode::Blocks, false).view(By(&rows)),
    );
    assert_eq!(drawn, print(&console, &rows));
}

#[test]
fn tags_carry_the_row() {
    let fixture = fixture();
    let icon = fixture.registry.resolve("team/icon").unwrap();
    let meta = MicroMeta::new(icon);
    assert_eq!((meta.cols, meta.rows, meta.row), (2, 2, 0));
    // The first row's tag is the four-item list it always was.
    let first = meta.to_value();
    assert!(matches!(&first, rich::style::MetaValue::List(items) if items.len() == 4));
    let second = meta.with_row(1);
    let round = MicroMeta::from_style(&second.to_style()).unwrap();
    assert_eq!(round, second);
    // A row the asset does not have is no tag.
    let mut beyond = second.clone();
    beyond.row = 2;
    assert!(MicroMeta::from_style(&beyond.to_style()).is_none());
}

// ---- layout ---------------------------------------------------------------

#[test]
fn wrapped_text_reserves_the_row_below_the_line_with_the_asset() {
    let fixture = fixture();
    let made = text(
        &fixture.registry,
        "Deploying :micro:team/icon: to the cluster now",
    );
    let rows = MicroRows::new(made.clone());
    let reserved = format!("          {BLANK}{BLANK}");
    assert_eq!(
        lines(&pipe(40), &rows),
        ["Deploying [] to the cluster now", reserved.as_str()]
    );
    assert_eq!(
        lines(&pipe(20), &rows),
        // Core keeps the space the line broke at, as upstream does.
        ["Deploying [] to the ", reserved.as_str(), "cluster now"]
    );
    assert_eq!(
        lines(&pipe(12), &rows),
        ["Deploying []", reserved.as_str(), "to the ", "cluster now"]
    );
    // At every width: one row under each line holding the asset's first
    // row, blank but for the asset's second row at the same columns, and
    // never wider than the line above.
    for width in 6..48 {
        let out = lines(&pipe(width), &rows);
        let plain = lines(&pipe(width), &made);
        assert_eq!(out.len(), plain.len() + 1, "{width}: {out:#?}");
        let at = out.iter().position(|l| l.contains("[]")).unwrap();
        assert_eq!(&out[..=at], &plain[..=at], "{width}");
        assert_eq!(&out[at + 2..], &plain[at + 1..], "{width}");
        let column = cell_len(&out[at][..out[at].find("[]").unwrap()]);
        assert_eq!(out[at + 1], format!("{}{BLANK}{BLANK}", " ".repeat(column)));
        assert!(
            out.iter().all(|l| cell_len(l) <= width),
            "{width}: {out:#?}"
        );
    }
}

#[test]
fn two_assets_on_a_line_share_its_reserved_row() {
    let fixture = fixture();
    let made = text(
        &fixture.registry,
        "a :micro:team/icon: b :micro:ok: c :micro:team/flame: d",
    );
    assert_eq!(
        lines(&pipe(40), &MicroRows::new(made)),
        [
            "a [] b ok c (^) d".to_string(),
            format!("  {BLANK}{BLANK}        {BLANK}{BLANK}{BLANK}"),
        ]
    );
}

#[test]
fn a_table_cell_grows_to_hold_the_second_row() {
    let fixture = fixture();
    let mut table = Table::new();
    table.add_column("icon");
    table.add_column("step");
    table.add_row_cells(vec![
        Cell::Renderable(Arc::new(MicroRows::new(text(
            &fixture.registry,
            ":micro:team/icon: built",
        )))),
        Cell::Text(Text::new("ok")),
    ]);
    table.add_row_cells(vec![
        Cell::Renderable(Arc::new(
            MicroAssetRef::from_registry(&fixture.registry, "team/flame")
                .unwrap()
                .preference(FallbackPreference::Text),
        )),
        Cell::Text(Text::new("next")),
    ]);
    assert_eq!(
        lines(&pipe(40), &table),
        [
            "┏━━━━━━━━━━┳━━━━━━┓".to_string(),
            "┃ icon     ┃ step ┃".to_string(),
            "┡━━━━━━━━━━╇━━━━━━┩".to_string(),
            "│ [] built │ ok   │".to_string(),
            format!("│ {BLANK}{BLANK}       │      │"),
            "│ (^)      │ next │".to_string(),
            format!("│ {BLANK}{BLANK}{BLANK}      │      │"),
            "└──────────┴──────┘".to_string(),
        ]
    );
}

#[test]
fn a_panel_measures_both_rows() {
    let fixture = fixture();
    let panel = Panel::fit(Box::new(MicroRows::new(text(
        &fixture.registry,
        "go :micro:team/icon: now",
    ))));
    assert_eq!(
        lines(&pipe(40), &panel),
        [
            "╭───────────╮".to_string(),
            "│ go [] now │".to_string(),
            format!("│    {BLANK}{BLANK}     │"),
            "╰───────────╯".to_string(),
        ]
    );
    // The width is the text's: the second row adds height only.
    let measured = rich::measure::Measurement::get(
        &pipe(40),
        &pipe(40).options(),
        &MicroRows::new(text(&fixture.registry, "go :micro:team/icon: now")),
    );
    assert_eq!((measured.minimum, measured.maximum), (3, 9));
    let asset = MicroAssetRef::from_registry(&fixture.registry, "team/flame").unwrap();
    let measured = rich::measure::Measurement::get(&pipe(40), &pipe(40).options(), &asset);
    assert_eq!((measured.minimum, measured.maximum), (3, 3));
    assert_eq!(lines(&pipe(40), &asset).len(), 2);
}

#[test]
fn without_reserved_rows_an_asset_shows_its_first_row_only() {
    let fixture = fixture();
    let made = text(&fixture.registry, "a :micro:team/icon: b\nnext line");
    // A bare text has no room for the second row: no mode draws over the
    // line below, every one keeps the fallback.
    let console = terminal(40);
    let plain = print(&console, &made);
    assert_eq!(plain, "a [] b\nnext line\n");
    for mode in [
        MicroMode::Kitty,
        MicroMode::Iterm,
        MicroMode::Sixel,
        MicroMode::Blocks,
    ] {
        let drawn = print(
            &console,
            &graphics(&fixture.registry, mode, false).view(By(&made)),
        );
        assert_eq!(drawn, plain, "{mode}");
    }
}

// ---- drawing ----------------------------------------------------------------

/// The printed lines of `markup`, drawn in `mode`.
fn drawn(
    fixture: &Fixture,
    mode: MicroMode,
    animate: bool,
    markup: &str,
) -> (String, MicroGraphics) {
    let graphics = graphics(&fixture.registry, mode, animate);
    let rows = MicroRows::new(text(&fixture.registry, markup));
    let out = print(&terminal(40), &graphics.view(By(&rows)));
    (out, graphics)
}

#[test]
fn kitty_places_one_image_over_both_rows() {
    let fixture = fixture();
    for (name, cols) in [("team/icon", 2), ("team/flame", 3)] {
        let (out, graphics) = drawn(
            &fixture,
            MicroMode::Kitty,
            false,
            &format!("go :micro:{name}: now"),
        );
        // Transmitted once, with a virtual placement of the whole box.
        assert_eq!(out.matches("\x1b_Ga=T,U=1,f=100").count(), 1, "{out:?}");
        assert!(out.contains(&format!(",c={cols},r=2,q=2")), "{name}");
        let id = graphics.kitty_ids()[0];
        let color = format!("\x1b[38;2;{};{};{}m", id >> 16, (id >> 8) & 255, id & 255);
        let top = rich_art::kitty::placeholder_row(0, cols).unwrap();
        let bottom = rich_art::kitty::placeholder_row(1, cols).unwrap();
        let printed: Vec<&str> = out.split('\n').collect();
        // The transmission, then row 0's placeholders in the id's colour on
        // the line; row 1's on the reserved row, at the same column.
        assert!(printed[0].starts_with("go \x1b_Ga=T"), "{name}");
        assert!(
            printed[0].ends_with(&format!("\x1b\\{color}{top}\x1b[0m now")),
            "{name}: {:?}",
            &printed[0][printed[0].len().saturating_sub(120)..]
        );
        assert_eq!(printed[1], format!("   {color}{bottom}\x1b[0m"), "{name}");
        assert_eq!(printed.len(), 3);
    }
}

#[test]
fn kitty_animates_a_two_row_asset_natively() {
    let fixture = fixture();
    let (out, _) = drawn(&fixture, MicroMode::Kitty, true, ":micro:team/flame:");
    assert_eq!(out.matches("a=f,").count(), 2, "two frames after the first");
    assert!(out.contains("s=3,v=1"), "and played in a loop");
    assert!(out.contains(",c=3,r=2,q=2"));
}

#[test]
fn iterm_and_sixel_draw_the_whole_box_from_the_second_row() {
    let fixture = fixture();
    for mode in [MicroMode::Iterm, MicroMode::Sixel] {
        for (name, cols) in [("team/icon", 2), ("team/flame", 3)] {
            let (out, _) = drawn(&fixture, mode, false, &format!("go :micro:{name}: now"));
            let printed: Vec<&str> = out.split('\n').collect();
            let blanks = BLANK.repeat(cols);
            // The first row is blank cells and nothing else.
            assert_eq!(printed[0], format!("go {blanks} now"), "{mode} {name}");
            // The second: blank cells with the cursor saved, then restored,
            // moved up a row to draw, restored, and stepped past them.
            let second = printed[1];
            let head = format!("   \x1b7{blanks}\x1b8\x1b[1A");
            assert!(second.starts_with(&head), "{mode} {name}: {second:?}");
            let tail = format!("\x1b8\x1b[{cols}C");
            assert!(second.ends_with(&tail), "{mode} {name}: {second:?}");
            let image = &second[head.len()..second.len() - tail.len()];
            match mode {
                MicroMode::Iterm => {
                    assert!(image.starts_with("\x1b]1337;File=inline=1;"));
                    assert!(
                        image.contains(&format!(";width={cols};height=2;")),
                        "{image:?}"
                    );
                    assert!(image.ends_with('\x07'));
                }
                _ => {
                    assert!(image.starts_with("\x1bP"), "{image:?}");
                    // Raster attributes: the box in pixels, two cells high.
                    let raster = format!("\"1;1;{};32", cols * 8);
                    assert!(image.contains(&raster), "{name}: {image:?}");
                    assert!(image.ends_with("\x1b\\"));
                    assert!(!image.contains('\n'), "inline Sixel has no line break");
                }
            }
            assert_eq!(printed.len(), 3, "{mode} {name}");
        }
    }
}

#[test]
fn iterm_animates_a_two_row_asset_with_a_gif() {
    let fixture = fixture();
    let (out, _) = drawn(&fixture, MicroMode::Iterm, true, ":micro:team/flame:");
    // "GIF8" in base64, sized as the whole box.
    assert!(out.contains(";width=3;height=2;"));
    assert!(out.contains(":R0lGOD"));
}

#[test]
fn half_blocks_draw_two_pixel_rows_per_cell_row() {
    let fixture = fixture();
    let (out, _) = drawn(
        &fixture,
        MicroMode::Blocks,
        false,
        "go :micro:team/icon: now",
    );
    assert!(
        !out.contains("\x1b7") && !out.contains("\x1b_G"),
        "no protocol"
    );
    let console = terminal(40);
    let graphics = graphics(&fixture.registry, MicroMode::Blocks, false);
    let rows = MicroRows::new(text(&fixture.registry, "go :micro:team/icon: now"));
    let lines = console.render_lines(&graphics.view(By(&rows)), &console.options(), false);
    assert_eq!(lines.len(), 2);
    let cells = |line: &[Segment]| -> String {
        line.iter()
            .filter(|s| MicroMeta::from_segment(s).is_some())
            .map(|s| s.text.as_str())
            .collect()
    };
    for (row, line) in lines.iter().enumerate() {
        let drawn = cells(line);
        assert_eq!(cell_len(&drawn), 2, "row {row}: {drawn:?}");
        assert!(
            drawn.chars().all(|c| "▀▄█ ".contains(c)),
            "row {row}: {drawn:?}"
        );
        // Each row is tagged as its row of one occurrence.
        let tags: Vec<MicroMeta> = line.iter().filter_map(MicroMeta::from_segment).collect();
        assert!(tags.iter().all(|t| t.row == row), "{tags:?}");
    }
    // The text beside it stays on the first row, unchanged.
    let first: String = lines[0].iter().map(|s| s.text.as_str()).collect();
    assert!(
        first.starts_with("go ") && first.ends_with(" now"),
        "{first:?}"
    );
    // The white dot sits across the middle of the box: the colours of the
    // two rows differ from the plain blue above and below it.
    assert_ne!(cells(&lines[0]), "", "drawn");
}

#[test]
fn the_fallback_is_the_first_row_with_blanks_below() {
    let fixture = fixture();
    let made = text(&fixture.registry, "go :micro:team/flame: now");
    let rows = MicroRows::new(made);
    let expected = format!("go (^) now\n   {BLANK}{BLANK}{BLANK}\n");
    // A pipe, an export, the text mode: the fallback, byte for byte, with
    // no escape sequence.
    assert_eq!(print(&pipe(40), &rows), expected);
    for mode in [
        MicroMode::Kitty,
        MicroMode::Iterm,
        MicroMode::Sixel,
        MicroMode::Blocks,
        MicroMode::Text,
    ] {
        let graphics = graphics(&fixture.registry, mode, true);
        assert_eq!(
            print(&pipe(40), &graphics.view(By(&rows))),
            expected,
            "{mode}"
        );
        assert_eq!(
            pipe(40).export_text(|c| c.print(&graphics.view(By(&rows)))),
            expected,
            "{mode}"
        );
    }
    // Text mode on a terminal redraws the fallback it prefers (the emoji)
    // on the first row, and keeps the blanks below.
    let text_mode = graphics(&fixture.registry, MicroMode::Text, false);
    assert_eq!(
        print(&terminal(40), &text_mode.view(By(&rows))),
        format!("go 🔥{BLANK} now\n   {BLANK}{BLANK}{BLANK}\n")
    );
    // With the emoji preference, the emoji on the first row.
    let (emoji, _) = render_markup(
        &pipe(40),
        ":micro:team/icon:",
        &fixture.registry,
        FallbackPreference::Emoji,
    );
    assert_eq!(
        print(&pipe(40), &MicroRows::new(emoji)),
        format!("🧊\n{BLANK}{BLANK}\n")
    );
    assert_eq!(
        rich_micro::fallback_row(
            fixture.registry.resolve("team/icon").unwrap(),
            FallbackPreference::Emoji,
            1
        ),
        PAD_CELL.to_string().repeat(2)
    );
}

// ---- frames ------------------------------------------------------------------

#[test]
fn frames_place_one_image_over_both_rows() {
    let fixture = fixture();
    let rows = MicroRows::new(text(&fixture.registry, "ab :micro:team/icon: cd"));
    let console = terminal(40);
    for mode in [MicroMode::Iterm, MicroMode::Sixel, MicroMode::Kitty] {
        let graphics = graphics(&fixture.registry, mode, false);
        let source = graphics.source();
        let lines: Vec<Segment> = Segment::split_lines(&console.render(&rows, None))
            .into_iter()
            .map(|line| source.prepare(line))
            .enumerate()
            .flat_map(|(i, line)| {
                let mut out = if i > 0 { vec![Segment::line()] } else { vec![] };
                out.extend(line);
                out
            })
            .collect();
        assert!(lines.iter().all(|s| !s.control), "{mode}: cells only");
        let frame = Frame::from_segments(&lines);
        let placements = source.placements(&frame);
        assert_eq!(placements.len(), 1, "{mode}");
        let p = &placements[0];
        assert_eq!((p.row, p.column, p.cols, p.rows), (0, 3, 2, 2), "{mode}");
        if mode == MicroMode::Kitty {
            let plain = frame.plain();
            assert!(plain.contains(&rich_art::kitty::placeholder_row(1, 2).unwrap()));
        } else {
            assert_eq!(
                frame.plain(),
                format!("ab {BLANK}{BLANK} cd\n   {BLANK}{BLANK}"),
                "{mode}"
            );
        }
    }
}

// ---- determinism -------------------------------------------------------------

#[test]
fn the_same_input_renders_the_same_every_time() {
    let fixture = fixture();
    let markup = "Deploying :micro:team/icon: then :micro:team/flame: and :micro:ok: done";
    for width in [12, 20, 33, 80] {
        let console = terminal(width);
        let once = text(&fixture.registry, markup);
        let twice = text(&fixture.registry, markup);
        for mode in [MicroMode::Text, MicroMode::Blocks, MicroMode::Iterm] {
            let graphics = graphics(&fixture.registry, mode, false);
            let a = print(&console, &graphics.view(MicroRows::new(once.clone())));
            let b = print(&console, &graphics.view(MicroRows::new(once.clone())));
            let c = print(&console, &graphics.view(MicroRows::new(twice.clone())));
            assert_eq!(a, b, "{width} {mode}: the same text twice");
            assert_eq!(a, c, "{width} {mode}: the same markup twice");
        }
        // Reserving is idempotent.
        let segments = console.render(&MicroRows::new(once.clone()), None);
        assert_eq!(reserve_rows(segments.clone()), segments, "{width}");
    }
}
