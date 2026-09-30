//! Drawing micro assets (#572–#577, #584): selection, each protocol's bytes,
//! the fallback in pipes and exports, animation and reduced motion, the
//! graphics source, and Kitty ids released when a view closes.
mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rich::{ColorSystem, Console, Renderable, Segment, Table};
use rich_ext::capabilities::MapEnvironment;
use rich_ext::frame::Frame;
use rich_ext::graphics::GraphicsEnvironment;
use rich_micro::package::load_package;
use rich_micro::{
    render_markup, select, FallbackPreference, Layer, Limits, MicroGraphics, MicroMode,
    MicroRegistry, Selection,
};

use rich_art::graphics::CellPixels;

fn registry() -> Arc<MicroRegistry> {
    let mut registry = MicroRegistry::new();
    for name in ["check", "dot", "spark"] {
        let path = common::fixtures().join(format!("builtin/{name}.richmicro"));
        let asset = load_package(&path, Layer::Inline, &Limits::default()).expect("fixture");
        registry.add(Layer::Inline, asset).expect("adds");
    }
    Arc::new(registry)
}

fn terminal(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .build()
}

fn pipe(width: usize) -> Console {
    Console::builder()
        .force_terminal(false)
        .width(width)
        .build()
}

fn graphics(mode: MicroMode) -> MicroGraphics {
    MicroGraphics::new(registry(), Selection::forced(mode, CellPixels::new(8, 16)))
}

fn text(console: &Console, registry: &MicroRegistry, markup: &str) -> rich::Text {
    let (text, diagnostics) = render_markup(console, markup, registry, FallbackPreference::Emoji);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    text
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

fn render_line(console: &Console, renderable: &dyn Renderable) -> Vec<Segment> {
    Segment::split_lines(&console.render(renderable, None)).remove(0)
}

// ---- selection (#573) -------------------------------------------------------

fn selected(env: MapEnvironment) -> Selection {
    select(&GraphicsEnvironment::detect(&env), &env)
}

#[test]
fn selection_prefers_kitty_then_iterm_then_sixel_then_blocks_then_text() {
    let tty = || MapEnvironment::tty().var("COLORTERM", "truecolor");
    assert_eq!(
        selected(tty().var("TERM", "xterm-kitty")).mode,
        MicroMode::Kitty
    );
    assert_eq!(
        selected(tty().var("TERM_PROGRAM", "iTerm.app")).mode,
        MicroMode::Iterm
    );
    // Sixel needs the cell size in pixels: images are sized from it.
    let foot = selected(tty().var("TERM", "foot").cell_pixels(9, 18));
    assert_eq!(foot.mode, MicroMode::Sixel);
    assert_eq!(foot.cell, CellPixels::new(9, 18));
    let unknown = selected(tty().var("TERM", "foot"));
    assert_eq!(unknown.mode, MicroMode::Blocks);
    assert!(unknown.reason.contains("cell size"), "{}", unknown.reason);
    assert_eq!(selected(tty()).mode, MicroMode::Blocks);
    assert_eq!(selected(tty().var("NO_COLOR", "1")).mode, MicroMode::Text);
}

#[test]
fn a_pipe_gets_text_whatever_the_override() {
    let pipe = MapEnvironment::new()
        .var("TERM", "xterm-kitty")
        .var("RICH_MICRO", "kitty");
    let selection = selected(pipe);
    assert_eq!(selection.mode, MicroMode::Text);
    assert!(!selection.animate);
}

#[test]
fn rich_micro_overrides_on_a_terminal() {
    let tty = || MapEnvironment::tty().var("COLORTERM", "truecolor");
    let forced = selected(tty().var("RICH_MICRO", "iterm"));
    assert_eq!(forced.mode, MicroMode::Iterm);
    assert_eq!(forced.reason, "RICH_MICRO=iterm");
    assert_eq!(
        selected(tty().var("TERM", "xterm-kitty").var("RICH_MICRO", "text")).mode,
        MicroMode::Text
    );
    let blocks = selected(tty().var("RICH_MICRO", "blocks"));
    assert!(blocks.blocks_first);
    let bad = selected(tty().var("TERM", "xterm-kitty").var("RICH_MICRO", "png"));
    assert_eq!(bad.mode, MicroMode::Kitty);
    assert!(bad.warning.is_some());
}

#[test]
fn reduced_motion_and_rich_animation_stop_animation() {
    let tty = || MapEnvironment::tty().var("TERM", "xterm-kitty");
    assert!(selected(tty()).animate);
    assert!(!selected(tty().var("RICH_A11Y", "reduced-motion")).animate);
    assert!(!selected(tty().var("RICH_ANIMATION", "0")).animate);
}

// ---- fallback (#577) -------------------------------------------------------

#[test]
fn pipes_and_exports_are_byte_identical_to_the_fallback() {
    let registry = registry();
    for mode in [
        MicroMode::Kitty,
        MicroMode::Iterm,
        MicroMode::Sixel,
        MicroMode::Blocks,
        MicroMode::Text,
    ] {
        let graphics = MicroGraphics::new(
            Arc::clone(&registry),
            Selection::forced(mode, CellPixels::new(8, 16)).animate(true),
        );
        let console = pipe(40);
        let text = text(
            &console,
            &registry,
            "a :micro:check: b :micro:dot: c :micro:fun/spark:",
        );
        let mut table = Table::new();
        table.add_column("icon");
        table.add_row_text(vec![text.clone()]);
        for renderable in [&text as &dyn Renderable, &table] {
            let plain = print(&console, renderable);
            let drawn = print(&console, &graphics.view(By(renderable)));
            assert_eq!(plain, drawn, "{mode}");
            assert!(!drawn.contains('\x1b'));
            let exported = console.export_text(|c| c.print(&graphics.view(By(renderable))));
            assert_eq!(exported, console.export_text(|c| c.print(renderable)));
            let svg = |r: &dyn Renderable| console.export_svg("t", "id", |c| c.print(r));
            assert_eq!(svg(&graphics.view(By(renderable))), svg(renderable));
        }
        assert!(graphics.kitty_ids().is_empty());
    }
}

#[test]
fn text_mode_on_a_terminal_is_the_fallback() {
    let registry = registry();
    let graphics = graphics(MicroMode::Text);
    let console = terminal(40);
    let text = text(&console, &registry, "a :micro:check: b");
    assert_eq!(
        print(&console, &graphics.view(By(&text))),
        print(&console, &text)
    );
}

#[test]
fn blocks_draw_an_asset_without_emoji_or_text_and_keep_its_width() {
    let mut registry = (*registry()).clone();
    let path = common::fixtures().join("builtin/check.richmicro");
    let bare = load_package(&path, Layer::Inline, &Limits::default()).unwrap();
    // The same image under another name, with only alt text to fall back on.
    let manifest = std::fs::read_to_string(path.join("manifest.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("bare.richmicro");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::copy(path.join("static.png"), copy.join("static.png")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    manifest["name"] = "bare".into();
    manifest.as_object_mut().unwrap().remove("fallback");
    manifest.as_object_mut().unwrap().remove("aliases");
    std::fs::write(copy.join("manifest.json"), manifest.to_string()).unwrap();
    registry
        .add(
            Layer::Inline,
            load_package(&copy, Layer::Inline, &Limits::default()).unwrap(),
        )
        .unwrap();
    drop(bare);
    let registry = Arc::new(registry);
    let graphics = MicroGraphics::new(
        Arc::clone(&registry),
        Selection::forced(MicroMode::Blocks, CellPixels::new(8, 16)),
    );
    let console = terminal(20);
    let marked = text(&console, &registry, "a :micro:bare: b");
    let line = render_line(&console, &graphics.view(By(&marked)));
    assert_eq!(line.iter().map(Segment::cell_length).sum::<usize>(), 6);
    let drawn: String = line.iter().map(|s| s.text.as_str()).collect();
    assert!(
        !drawn.contains("green"),
        "the image, not the alt text: {drawn:?}"
    );
    assert!(line.iter().all(|s| !s.control));

    // Selected automatically, blocks mode keeps an emoji where there is one
    // (#577); `RICH_MICRO=blocks` puts the image first.
    let mut automatic = Selection::forced(MicroMode::Blocks, CellPixels::new(8, 16));
    automatic.blocks_first = false;
    let automatic = MicroGraphics::new(Arc::clone(&registry), automatic);
    let check = text(&console, &registry, ":micro:check:");
    assert_eq!(
        print(&console, &automatic.view(By(&check))),
        print(&console, &check)
    );
    assert_ne!(
        print(&console, &graphics.view(By(&check))),
        print(&console, &check)
    );
    // Without a terminal, the alt text stays.
    let console = pipe(20);
    let bare = text(&console, &registry, ":micro:bare:");
    assert_eq!(
        print(&console, &graphics.view(By(&bare))),
        "gr\n",
        "the alt text, cut to fit"
    );
}

// ---- Kitty (#574) ------------------------------------------------------------

#[test]
fn kitty_transmits_once_and_prints_placeholders() {
    let registry = registry();
    let graphics = graphics(MicroMode::Kitty);
    let console = terminal(40);
    let text = text(&console, &registry, "a :micro:check: b :micro:check:");
    let out = print(&console, &graphics.view(By(&text)));
    // One transmission, quiet, with a virtual placement of 2x1 cells.
    assert_eq!(out.matches("\x1b_Ga=T,U=1,f=100").count(), 1, "{out:?}");
    assert!(out.contains(",c=2,r=1,q=2"));
    // Two occurrences of the placeholder row, in the id's colour.
    let id = graphics.kitty_ids()[0];
    let row = rich_art::kitty::placeholder_row(0, 2).unwrap();
    assert_eq!(out.matches(row.as_str()).count(), 2);
    let color = format!("38;2;{};{};{}", id >> 16, (id >> 8) & 255, id & 255);
    assert!(out.contains(&color), "{out:?}");
    // Printed again: no second transmission.
    let again = print(&console, &graphics.view(By(&text)));
    assert!(!again.contains("\x1b_G"));
    // Closing deletes the image by id.
    assert_eq!(graphics.close(), format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\"));
    assert!(graphics.kitty_ids().is_empty());
}

#[test]
fn kitty_placeholders_come_after_markup_substitution_and_are_never_reparsed() {
    let registry = registry();
    let graphics = graphics(MicroMode::Kitty);
    let console = terminal(40);
    // The pre-parse path uses U+100000.. stand-ins; placeholders (U+10EEEE)
    // only appear after it, when the view draws.
    let text = text(&console, &registry, "[bold]:micro:check:[/bold] :fire:");
    assert!(!text.plain().contains(rich_art::kitty::PLACEHOLDER));
    let out = print(&console, &graphics.view(By(&text)));
    assert!(out.contains(rich_art::kitty::PLACEHOLDER));
    // Drawn output fed back through the pre-parse path is left as written
    // and reported, not mistaken for stand-ins.
    let (again, diagnostics) = render_markup(
        &console,
        &format!("{out} :micro:check:"),
        &registry,
        FallbackPreference::Emoji,
    );
    assert!(diagnostics
        .iter()
        .any(|d| d.kind == rich_micro::DiagnosticKind::Reserved));
    assert!(again.plain().contains(rich_art::kitty::PLACEHOLDER));
}

#[test]
fn kitty_needs_the_colour_system_it_was_selected_for() {
    let registry = registry();
    let graphics = graphics(MicroMode::Kitty);
    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .width(40)
        .build();
    let text = text(&console, &registry, "a :micro:check:");
    assert_eq!(
        print(&console, &graphics.view(By(&text))),
        print(&console, &text)
    );
}

#[test]
fn kitty_ids_are_released_when_a_view_closes() {
    let registry = registry();
    let graphics = graphics(MicroMode::Kitty);
    let source = graphics.source();
    let console = terminal(40);
    let text = text(&console, &registry, "a :micro:check: b :micro:dot:");
    let line = render_line(&console, &text);
    let prepared = source.prepare(line);
    // Placeholder cells, and no escapes in the cells.
    assert!(prepared.iter().all(|s| !s.control));
    let frame = Frame::from_segments(&prepared);
    let placements = source.placements(&frame);
    assert_eq!(placements.len(), 2);
    assert!(placements.iter().all(|p| p.graphic.in_cells()));
    assert_eq!((placements[0].column, placements[1].column), (2, 7));
    // Drawing uploads each image once.
    assert!(placements[0].graphic.draw(0).starts_with("\x1b_Ga=T"));
    assert_eq!(placements[0].graphic.draw(0), "");
    placements[1].graphic.draw(0);
    assert_eq!(graphics.kitty_ids().len(), 2);
    // Closing with one still on screen keeps that one.
    let released = source.release(&placements[1..]);
    assert_eq!(released.matches("a=d,d=I").count(), 1);
    assert_eq!(graphics.kitty_ids().len(), 1);
    source.release(&[]);
    assert!(graphics.kitty_ids().is_empty(), "no leaked Kitty ids");
}

// ---- iTerm2 (#575) and inline Sixel (#576) ----------------------------------

#[test]
fn iterm_and_sixel_draw_over_blank_cells_and_step_past_them() {
    let registry = registry();
    for (mode, image) in [
        (MicroMode::Iterm, "\x1b]1337;File=inline=1;"),
        (MicroMode::Sixel, "\x1bP"),
    ] {
        let graphics = graphics(mode);
        let console = terminal(40);
        let text = text(&console, &registry, "a :micro:check: b");
        let out = print(&console, &graphics.view(By(&text)));
        let save = out.find("\x1b7").expect("saves the cursor");
        let blanks = out.find("\u{2800}\u{2800}").expect("blank cells");
        let drawn = out.find(image).expect("image");
        let after = out.find("\x1b8\x1b[2C").expect("restores, then steps past");
        assert!(
            save < blanks && blanks < drawn && drawn < after,
            "{mode}: {out:?}"
        );
        assert!(out[after..].contains(" b"));
        if mode == MicroMode::Iterm {
            assert!(out.contains(";width=2;height=1;"));
        } else {
            assert!(
                !out[drawn..after].contains('\n'),
                "inline Sixel has no line break"
            );
        }
    }
}

#[test]
fn overlays_are_placements_in_frames() {
    let registry = registry();
    let graphics = graphics(MicroMode::Iterm);
    let source = graphics.source();
    let console = terminal(40);
    let text = text(&console, &registry, "ab :micro:check:");
    let line = render_line(&console, &text);
    let prepared = source.prepare(line);
    let frame = Frame::from_segments(&prepared);
    assert_eq!(frame.plain(), "ab \u{2800}\u{2800}");
    let placements = source.placements(&frame);
    assert_eq!(placements.len(), 1);
    assert_eq!(
        (placements[0].row, placements[0].column, placements[0].cols),
        (0, 3, 2)
    );
    let draw = placements[0].graphic.draw(0);
    assert!(draw.starts_with("\x1b7\x1b]1337;") && draw.ends_with("\x07\x1b8"));
    assert_eq!(source.release(&[]), "");
}

// ---- animation (#572) ------------------------------------------------------

#[test]
fn animation_is_time_indexed_and_stops_with_reduced_motion() {
    let registry = registry();
    let clock = Arc::new(AtomicU64::new(0));
    let at = Arc::clone(&clock);
    let graphics = MicroGraphics::new(
        Arc::clone(&registry),
        Selection::forced(MicroMode::Sixel, CellPixels::new(8, 16)).animate(true),
    )
    .with_clock(move || Duration::from_millis(at.load(Ordering::SeqCst)));
    let source = graphics.source();
    let console = terminal(40);
    let text = text(&console, &registry, ":micro:fun/spark:");
    let frame_now = || {
        let line = render_line(&console, &text);
        let frame = Frame::from_segments(&source.prepare(line));
        source.placements(&frame)[0].frame
    };
    assert_eq!(frame_now(), 0);
    let wait = source.next_change().expect("animated");
    clock.fetch_add(wait.as_millis() as u64, Ordering::SeqCst);
    assert_eq!(frame_now(), 1);

    let still = MicroGraphics::new(
        Arc::clone(&registry),
        Selection::forced(MicroMode::Sixel, CellPixels::new(8, 16)).animate(false),
    );
    let source = still.source();
    let line = render_line(&console, &text);
    let frame = Frame::from_segments(&source.prepare(line));
    assert_eq!(source.placements(&frame)[0].frame, 0);
    assert_eq!(source.next_change(), None, "reduced motion: nothing moves");
}

#[test]
fn kitty_animates_natively() {
    let registry = registry();
    let graphics = MicroGraphics::new(
        Arc::clone(&registry),
        Selection::forced(MicroMode::Kitty, CellPixels::new(8, 16)).animate(true),
    );
    let console = terminal(40);
    let out = print(
        &console,
        &graphics.view(By(&text(&console, &registry, ":micro:fun/spark:"))),
    );
    assert!(out.contains("a=f,"), "frames are transmitted");
    assert!(out.contains("s=3,v=1"), "and played in a loop");
    let still = graphics_still(&registry);
    let out = print(
        &console,
        &still.view(By(&text(&console, &registry, ":micro:fun/spark:"))),
    );
    assert!(!out.contains("a=f,"));
}

fn graphics_still(registry: &Arc<MicroRegistry>) -> MicroGraphics {
    MicroGraphics::new(
        Arc::clone(registry),
        Selection::forced(MicroMode::Kitty, CellPixels::new(8, 16)).animate(false),
    )
}

#[test]
fn iterm_animates_with_a_gif() {
    let registry = registry();
    let graphics = MicroGraphics::new(
        Arc::clone(&registry),
        Selection::forced(MicroMode::Iterm, CellPixels::new(8, 16)).animate(true),
    );
    let console = terminal(40);
    let out = print(
        &console,
        &graphics.view(By(&text(&console, &registry, ":micro:fun/spark:"))),
    );
    // "GIF8" in base64.
    assert!(out.contains(":R0lGOD"), "{}", &out[..out.len().min(120)]);
}

// ---- cache (#584) ----------------------------------------------------------

#[test]
fn images_are_cached_in_memory_and_on_disk() {
    let registry = registry();
    let dir = tempfile::tempdir().unwrap();
    let graphics = graphics(MicroMode::Iterm).with_disk_cache(Some(dir.path().to_path_buf()));
    let console = terminal(40);
    let text = text(&console, &registry, ":micro:check: :micro:dot:");
    let first = print(&console, &graphics.view(By(&text)));
    assert!(graphics.cache_bytes() > 0);
    let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(files.len(), 2, "one optimised variant per asset and size");
    // A new session reads the variants back and draws the same bytes.
    let again = MicroGraphics::new(
        Arc::clone(&registry),
        Selection::forced(MicroMode::Iterm, CellPixels::new(8, 16)),
    )
    .with_disk_cache(Some(dir.path().to_path_buf()));
    assert_eq!(print(&console, &again.view(By(&text))), first);
    // A tiny budget keeps the cache bounded.
    let tight = graphics_with_budget(&registry, 1);
    print(&console, &tight.view(By(&text)));
    assert!(
        tight.cache_bytes() <= 16 * 16 * 4,
        "{}",
        tight.cache_bytes()
    );
}

fn graphics_with_budget(registry: &Arc<MicroRegistry>, bytes: usize) -> MicroGraphics {
    MicroGraphics::new(
        Arc::clone(registry),
        Selection::forced(MicroMode::Iterm, CellPixels::new(8, 16)),
    )
    .with_cache_budget(bytes)
}
