//! Each protocol through a real pseudo-terminal (#573–#576): the bytes a
//! view writes are played through a PTY (`cat`, so the line discipline
//! turns `\n` into `\r\n` as it would for a program) into a terminal
//! emulator. Wherever a protocol draws an asset, the screen outside the
//! asset's cells and the cursor must be exactly where the text fallback
//! puts them: through wrapping, inside a table, while the screen scrolls,
//! and across a live redraw.
#![cfg(unix)]

mod common;

use std::io::Read;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Renderable, Segment, Table, Text, Theme};
use rich_art::graphics::CellPixels;
use rich_ext::live::LiveCoordinator;
use rich_ext::target::{RenderTarget, TargetKind};
use rich_micro::package::load_package;
use rich_micro::{
    render_markup, FallbackPreference, Layer, Limits, MicroGraphics, MicroMode, MicroRegistry,
    Selection,
};

const COLS: u16 = 24;
const ROWS: u16 = 8;

fn registry() -> Arc<MicroRegistry> {
    let mut registry = MicroRegistry::new();
    for name in ["check", "spark"] {
        let path = common::fixtures().join(format!("builtin/{name}.richmicro"));
        let asset = load_package(&path, Layer::Inline, &Limits::default()).expect("fixture");
        registry.add(Layer::Inline, asset).expect("adds");
    }
    Arc::new(registry)
}

fn console() -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(COLS as usize)
        .build()
}

fn markup(registry: &MicroRegistry, source: &str) -> Text {
    let (text, diagnostics) =
        render_markup(&console(), source, registry, FallbackPreference::Emoji);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    text
}

/// `bytes` played through a PTY into an emulator of `COLS × ROWS`.
///
/// The slave stays open here until everything `cat` wrote has been read:
/// on macOS, output still queued when the last slave descriptor closes is
/// discarded and the master reads EOF, which left the screen empty when
/// `cat` finished before the first read. The line discipline turns each
/// `\n` into `\r\n`, so the expected length is known exactly.
fn through_pty(bytes: &[u8]) -> vt100::Parser {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out");
    std::fs::write(&path, bytes).unwrap();
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new("cat");
    command.arg(&path);
    let mut child = pty.slave.spawn_command(command).unwrap();
    let expected = bytes.len() + bytes.iter().filter(|&&b| b == b'\n').count();
    let mut reader = pty.master.try_clone_reader().unwrap();
    let (sender, chunks) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 65536];
        while let Ok(read @ 1..) = reader.read(&mut buffer) {
            if sender.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut out = Vec::new();
    while out.len() < expected {
        let left = deadline.saturating_duration_since(Instant::now());
        match chunks.recv_timeout(left) {
            Ok(chunk) => out.extend_from_slice(&chunk),
            Err(_) => panic!(
                "read {} of {expected} bytes from the PTY: {:?}",
                out.len(),
                String::from_utf8_lossy(&out)
            ),
        }
    }
    child.wait().unwrap();
    drop(pty.slave);
    let mut parser = vt100::Parser::new(ROWS, COLS, 0);
    parser.process(&out);
    parser
}

/// Each row of the screen, with an asset's cells (whatever drew them)
/// shown as `#`.
fn masked(screen: &vt100::Screen) -> Vec<String> {
    (0..ROWS)
        .map(|row| {
            (0..COLS)
                .map(|col| {
                    let cell = screen.cell(row, col).unwrap();
                    let text = cell.contents();
                    let asset = cell.is_wide_continuation()
                        || text.starts_with(['✅', '✨', '\u{2800}', '\u{10EEEE}']);
                    if asset {
                        "#".to_string()
                    } else if text.is_empty() {
                        " ".to_string()
                    } else {
                        text.to_string()
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn graphics(mode: MicroMode) -> MicroGraphics {
    MicroGraphics::new(registry(), Selection::forced(mode, CellPixels::new(8, 16)))
}

/// What printing `renderable` writes to the terminal.
fn printed(renderable: &dyn Renderable) -> Vec<u8> {
    console().capture(|c| c.print(renderable)).into_bytes()
}

struct By<'a>(&'a dyn Renderable);

impl Renderable for By<'_> {
    fn rich_render(&self, console: &Console, options: &rich::ConsoleOptions) -> Vec<Segment> {
        self.0.rich_render(console, options)
    }
}

/// Play `renderable` through the PTY with `mode` and with the text
/// fallback; the screens match outside the assets, and so do the cursors.
fn same_layout(mode: MicroMode, renderable: &dyn Renderable) -> (Vec<u8>, vt100::Parser) {
    let fallback = through_pty(&printed(renderable));
    let bytes = printed(&graphics(mode).view(By(renderable)));
    let drawn = through_pty(&bytes);
    assert_eq!(
        masked(drawn.screen()),
        masked(fallback.screen()),
        "{mode}: the screen outside the assets"
    );
    assert_eq!(
        drawn.screen().cursor_position(),
        fallback.screen().cursor_position(),
        "{mode}: the cursor"
    );
    (bytes, drawn)
}

const PROTOCOLS: [MicroMode; 3] = [MicroMode::Kitty, MicroMode::Iterm, MicroMode::Sixel];

fn check_protocol_bytes(mode: MicroMode, bytes: &[u8], parser: &vt100::Parser) {
    let text = String::from_utf8_lossy(bytes);
    match mode {
        MicroMode::Kitty => {
            assert!(text.contains("\x1b_Ga=T,U=1,"), "kitty transmits");
            // The placeholder cells are in the image id's colour.
            let screen = parser.screen();
            let placeholder = (0..ROWS)
                .flat_map(|r| (0..COLS).map(move |c| (r, c)))
                .find(|(r, c)| {
                    screen
                        .cell(*r, *c)
                        .unwrap()
                        .contents()
                        .starts_with('\u{10EEEE}')
                })
                .expect("placeholder cells on screen");
            let cell = screen.cell(placeholder.0, placeholder.1).unwrap();
            assert!(matches!(cell.fgcolor(), vt100::Color::Rgb(..)));
        }
        MicroMode::Iterm => assert!(text.contains("\x1b]1337;File=inline=1;")),
        MicroMode::Sixel => {
            assert!(text.contains("\x1bP"));
            assert!(
                text.contains("\x1b\\\x1b8\x1b[2C"),
                "restored after the image"
            );
        }
        _ => {}
    }
}

#[test]
fn wrapping() {
    let registry = registry();
    let text = markup(
        &registry,
        "one two three four :micro:check: five six seven :micro:check: eight nine",
    );
    for mode in PROTOCOLS {
        let (bytes, parser) = same_layout(mode, &text);
        check_protocol_bytes(mode, &bytes, &parser);
        assert_eq!(
            &masked(parser.screen())[..3],
            ["one two three four ##", "five six seven ## eight", "nine"],
            "{mode}"
        );
    }
}

#[test]
fn a_table() {
    let registry = registry();
    let mut table = Table::new();
    table.add_column("state");
    table.add_column("name");
    table.add_row_text(vec![markup(&registry, ":micro:check:"), Text::new("build")]);
    table.add_row_text(vec![
        markup(&registry, ":micro:check: ok"),
        Text::new("tests"),
    ]);
    for mode in PROTOCOLS {
        let (bytes, parser) = same_layout(mode, &table);
        check_protocol_bytes(mode, &bytes, &parser);
    }
}

#[test]
fn a_scroll() {
    let registry = registry();
    // Three times the screen: every line scrolls through the bottom row.
    let lines: Vec<String> = (0..ROWS * 3)
        .map(|i| format!("line {i} :micro:check: end"))
        .collect();
    let text = markup(&registry, &lines.join("\n"));
    for mode in PROTOCOLS {
        let (bytes, parser) = same_layout(mode, &text);
        check_protocol_bytes(mode, &bytes, &parser);
        assert!(masked(parser.screen())[ROWS as usize - 2].starts_with("line 23 ## end"));
    }
}

#[derive(Clone, Default)]
struct Bytes(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Bytes {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn target() -> RenderTarget {
    RenderTarget::new(
        TargetKind::Terminal,
        TargetCapabilities {
            width: COLS as usize,
            height: ROWS as usize,
            color_system: Some(ColorSystem::Truecolor),
            interactive: true,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}

/// A live region redrawn three times, then printed above and finished.
fn live(graphics: Option<&MicroGraphics>) -> Vec<u8> {
    let registry = registry();
    let out = Bytes::default();
    let mut live = LiveCoordinator::new(out.clone(), target());
    if let Some(graphics) = graphics {
        live = live.with_graphics(graphics.source());
    }
    let render = |source: &str| {
        let console = console();
        console.render(&markup(&registry, source), None)
    };
    let id = live.add(render("start :micro:check: a")).unwrap();
    live.refresh().unwrap();
    live.update(id.clone(), render("step :micro:check: bb"))
        .unwrap();
    live.refresh().unwrap();
    live.update(id.clone(), render("x :micro:check: :micro:check: c"))
        .unwrap();
    live.refresh().unwrap();
    live.print(&render("printed :micro:check: above")).unwrap();
    live.update(id, render("last :micro:check: row")).unwrap();
    live.refresh().unwrap();
    drop(live);
    let bytes = out.0.lock().unwrap().clone();
    bytes
}

#[test]
fn a_live_redraw() {
    let fallback = through_pty(&live(None));
    for mode in PROTOCOLS {
        let graphics = graphics(mode);
        let bytes = live(Some(&graphics));
        let drawn = through_pty(&bytes);
        assert_eq!(masked(drawn.screen()), masked(fallback.screen()), "{mode}");
        assert_eq!(
            drawn.screen().cursor_position(),
            fallback.screen().cursor_position(),
            "{mode}"
        );
        let text = String::from_utf8_lossy(&bytes);
        match mode {
            MicroMode::Kitty => {
                // Uploaded once, however often it was redrawn, and the
                // printed copy keeps its image: nothing else was held.
                assert_eq!(text.matches("\x1b_Ga=T,").count(), 1);
                assert_eq!(graphics.kitty_ids().len(), 1);
            }
            _ => assert!(text.matches("\x1b7").count() >= 4, "{mode}: redrawn"),
        }
    }
}

#[test]
fn kitty_ids_do_not_leak_from_a_closed_live_view() {
    let graphics = graphics(MicroMode::Kitty);
    let registry = registry();
    let out = Bytes::default();
    let mut live = LiveCoordinator::new(out.clone(), target()).with_graphics(graphics.source());
    let segments = console().render(&markup(&registry, "busy :micro:check:"), None);
    live.add(segments).unwrap();
    live.refresh().unwrap();
    assert_eq!(graphics.kitty_ids().len(), 1);
    live.finish().unwrap();
    assert!(graphics.kitty_ids().is_empty());
    let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("\x1b_Ga=d,d=I,"));
}
