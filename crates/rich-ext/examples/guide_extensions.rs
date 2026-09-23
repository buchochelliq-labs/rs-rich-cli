//! Guide: Extensions — run: cargo run -p rs-rich-ext --example guide_extensions [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/extensions.md` come from this file. With
//! `--svg DIR` every shot is written as `DIR/guide_extensions-<shot>.svg`.

use std::path::PathBuf;

use rich::console::ConsoleBuilder;
use rich::{ColorSystem, Console, Highlighter, Table, Text};
use rich_ext::ConsoleExt;

/// Where shots go: the terminal, or one SVG per shot.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let dir = args
            .iter()
            .position(|arg| arg == "--svg")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        Shots { dir }
    }

    /// Build a console with `build`, then run `body` on it: printed to the
    /// terminal, or recorded into an SVG `width` cells wide.
    fn shot(
        &self,
        name: &str,
        width: usize,
        build: impl FnOnce(ConsoleBuilder) -> Console,
        body: impl FnOnce(&Console),
    ) {
        match &self.dir {
            None => {
                let console = build(Console::builder());
                console.print_str(&format!("[dim]── {name} ──[/]"));
                body(&console);
            }
            Some(dir) => {
                let console = build(
                    Console::builder()
                        .width(width)
                        .force_terminal(true)
                        .color_system(Some(ColorSystem::Truecolor)),
                );
                let id = format!("guide_extensions-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

/// A console with this crate's default extensions installed.
fn extended(builder: ConsoleBuilder) -> Console {
    let mut console = builder.build();
    console.install_extensions();
    console
}

// --8<-- [start:highlighter]
/// Highlights `TODO` and `FIXME` markers, wherever they appear.
struct TodoHighlighter;

impl Highlighter for TodoHighlighter {
    fn highlight(&self, text: &mut Text) {
        // `highlight_words` styles every occurrence; the count is not needed.
        let _ = text.highlight_words(&["TODO", "FIXME"], "bold black on yellow", true);
    }
}
// --8<-- [end:highlighter]

// --8<-- [start:registry]
use rich_ext::{ExtensionRegistry, NumberHighlighter};

/// Our extensions, as a registry that can be installed on any console.
fn my_extensions() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::new();
    registry
        .register_highlighter(|| Box::new(NumberHighlighter::new()))
        .register_highlighter(|| Box::new(TodoHighlighter));
    registry
}

fn my_console(builder: ConsoleBuilder) -> Console {
    let mut console = builder.build();
    // `install` calls every factory, so one registry serves many consoles.
    my_extensions().install(&mut console);
    console
}
// --8<-- [end:registry]

// --8<-- [start:theme]
use rich_ext::theme::extended_theme;

fn themed_console(builder: ConsoleBuilder) -> Console {
    builder.theme(extended_theme()).build()
}
// --8<-- [end:theme]

fn main() {
    let shots = Shots::from_args();

    shots.shot(
        "install",
        64,
        |b| b.build(),
        |console| {
            // --8<-- [start:install]
            let line = "build42 finished: 3 crates, x86_64, 12 warnings";

            let core = Console::new();
            let mut with_ext = Console::new();
            with_ext.install_extensions(); // registers NumberHighlighter

            // `build_text` applies a console's markup and highlighters.
            console.print(&core.build_text(line));
            console.print(&with_ext.build_text(line));
            // --8<-- [end:install]
        },
    );

    shots.shot("registry", 64, my_console, |console| {
        // --8<-- [start:registry-use]
        console.print_str("TODO: retry step 3 of 7 (FIXME: flaky on runner2)");
        // --8<-- [end:registry-use]
    });

    shots.shot("theme", 64, themed_console, |console| {
        // --8<-- [start:theme-use]
        console.print_str("[error]error[/]  [warning]warning[/]  [info]info[/]  [success]ok[/]");
        console.print_str("[help.option]--width[/] [help.metavar]<SIZE>[/]  [diff.added]+ added[/]  [diff.removed]- removed[/]");
        // --8<-- [end:theme-use]
    });

    shots.shot("hyperlinks", 80, extended, |console| {
        // --8<-- [start:hyperlinks]
        use rich_ext::hyperlink::Hyperlinker;

        let linker = Hyperlinker::new()
            .base_dir("/work/app")
            .repository("https://github.com/acme/app");
        let message = "see src/main.rs:12:5, fixed in #42 and acme/lib#7 (https://acme.dev/faq)";

        // Add OSC 8 link spans to a Text in place...
        let mut text = Text::new(message);
        linker.link(&mut text);
        console.print(&text);

        // ...or just ask what would be linked.
        let mut table = Table::new();
        table.add_column("Span");
        table.add_column("URL");
        for link in linker.find(message) {
            table.add_row(&[&message[link.start..link.end], &link.url]);
        }
        console.print(&table);
        // --8<-- [end:hyperlinks]
    });

    shots.shot("editor", 80, extended, |console| {
        // --8<-- [start:editor]
        use rich_ext::hyperlink::Hyperlinker;

        let vscode = Hyperlinker::new().editor("vscode://file{path}:{line}:{column}");
        let url = vscode.file_url("/work/app/src/main.rs", Some(12), Some(5));
        console.print(&Text::new(format!("{url:?}")));

        // A ready-made, linked `path:line:col` label in a style of your choice.
        console.print(&vscode.location("src/lib.rs", Some(3), None, "magenta"));

        // `disabled()` is the explicit plain fallback: same text, no links.
        let mut text = Text::new("src/lib.rs:3");
        Hyperlinker::disabled().link(&mut text);
        assert!(text.spans().is_empty());
        // --8<-- [end:editor]
    });

    shots.shot("sanitize", 64, extended, |console| {
        // --8<-- [start:sanitize]
        use rich_ext::sanitize_terminal_controls;

        // A file name from an untrusted source that tries to clear the screen.
        let untrusted = "report\x1b[2J\x1b[H.txt\tsize\u{7}";
        let safe = sanitize_terminal_controls(untrusted);
        console.print(&Text::new(safe));
        // --8<-- [end:sanitize]
    });

    shots.shot(
        "encoding",
        64,
        |b| b.highlight(false).build(),
        |console| {
            // --8<-- [start:encoding]
            use rich_ext::encoding::{has_utf16_bom, Encoding};

            let bytes = [0xff, 0xfe, b'h', 0, b'i', 0]; // UTF-16LE with a BOM
            assert!(has_utf16_bom(&bytes));
            let text = Encoding::Utf16.decode(&bytes).expect("valid UTF-16");
            console.print(&Text::new(format!("decoded: {text:?}")));

            // Nothing is guessed: headerless UTF-16 needs an explicit byte order.
            let err = Encoding::Utf16.decode(b"h\0i\0").unwrap_err();
            console.print(&Text::new(format!("error:   {err}")));
            let le: Encoding = "utf-16le".parse().expect("a known name");
            console.print(&Text::new(format!(
                "utf-16le: {:?}",
                le.decode(b"h\0i\0").unwrap()
            )));
            // --8<-- [end:encoding]
        },
    );
}
