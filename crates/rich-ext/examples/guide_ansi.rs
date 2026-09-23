//! Guide: Explaining ANSI output — run: cargo run -p rs-rich-ext --example guide_ansi [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/ansi.md` comes from this file.
use std::path::PathBuf;

use rich::{ColorSystem, Console};
use rich_ext::ansi_explain::{
    escape_visible, explain, explain_bytes, sgr_effects, ExplanationView, Token, ViewMode,
};
use rich_ext::ConsoleExt;

/// Output as a program might write it: a title, colours, a link, a cursor
/// move and a truncated escape at the end.
const CAPTURE: &str =
    "\x1b]0;build\x07\x1b[1;32m✔\x1b[0m compiled \x1b[38;5;208m3 crates\x1b[39m\n\
\x1b]8;;https://ci.example/42\x1b\\log\x1b]8;;\x1b\\ \x1b[2K\x1b[1A\x1b[";

// --8<-- [start:explain]
fn show_table(console: &Console) {
    let explanation = explain(CAPTURE);
    // What a terminal would show, with every escape removed.
    assert_eq!(explanation.visible_text, "✔ compiled 3 crates\nlog ");
    // One token per escape, text run or control, with its byte offset.
    for spanned in explanation.escapes() {
        let meaning = spanned.token.meaning();
        eprintln!(
            "{:>3} {:<8} {meaning}",
            spanned.offset,
            spanned.token.kind()
        );
    }
    assert_eq!(explanation.invalid().count(), 1); // the trailing `ESC[`
    console.print(&ExplanationView::new(&explanation));
}
// --8<-- [end:explain]

// --8<-- [start:options]
fn show_options(console: &Console) {
    let explanation = explain(CAPTURE);
    // Only escapes, no text rows, no visible text, raw input cut at 24 chars.
    let view = ExplanationView::new(&explanation)
        .escapes_only(true)
        .show_visible(false)
        .raw_width(24);
    console.print(&view);
}
// --8<-- [end:options]

// --8<-- [start:inline]
fn show_inline(console: &Console) {
    let explanation = explain(CAPTURE);
    console.print(&ExplanationView::new(&explanation).mode(ViewMode::Inline));
}
// --8<-- [end:inline]

// --8<-- [start:tokens]
fn inspect_tokens() {
    let explanation = explain("\x1b[4:3;58;2;255;0;0mwavy\x1b[0m");
    for spanned in &explanation.tokens {
        if let Token::Sgr {
            params, effects, ..
        } = &spanned.token
        {
            let described: Vec<String> = effects.iter().map(ToString::to_string).collect();
            println!("{params}: {}", described.join(", "));
        }
    }
    // The building blocks are public too.
    let effects = sgr_effects("1;38;5;208");
    assert_eq!(effects[1].code, "38;5;208");
    assert_eq!(escape_visible("\x1b[0m\x07"), "ESC[0m<BEL>");
    // Raw bytes: C1 controls (0x9B is CSI) in non-UTF-8 input are decoded.
    let from_bytes = explain_bytes(b"\x9b31mred\x9b0m");
    assert_eq!(from_bytes.visible_text, "red");
}
// --8<-- [end:tokens]

fn main() {
    let shots = Shots::from_args();
    shots.shot("table", 88, "ExplanationView", show_table);
    shots.shot("escapes-only", 88, "escapes_only", show_options);
    shots.shot("inline", 88, "ViewMode::Inline", show_inline);
    if !shots.svg() {
        inspect_tokens();
    }
}

/// `--svg DIR` writes each shot as `DIR/guide_ansi-<shot>.svg`; without it,
/// shots print to the terminal.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { dir }
    }

    fn svg(&self) -> bool {
        self.dir.is_some()
    }

    fn shot(&self, name: &str, width: usize, title: &str, f: impl FnOnce(&Console)) {
        let Some(dir) = &self.dir else {
            let mut console = Console::new();
            console.install_extensions();
            return f(&console);
        };
        let mut console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        console.install_extensions();
        let id = format!("guide_ansi-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}
