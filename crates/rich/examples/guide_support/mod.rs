//! Shared plumbing for the `guide_*` examples: argument parsing and screenshot
//! capture. Not an example itself (Cargo only builds `examples/<name>/main.rs`
//! directories), and not part of any snippet shown in the guide.
//!
//! Every guide example accepts an optional `--svg DIR`. Without it, each shot
//! prints to the real terminal through `Console::new()`. With it, each shot is
//! rendered on a pinned console (fixed width, truecolor, forced terminal) and
//! exported with `Console::export_svg` to `DIR/<topic>-<shot>.svg`, using the
//! file stem as the SVG's unique id so the output is byte-stable.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use rich::console::ConsoleBuilder;
use rich::{ColorSystem, Console, Rule, Style};

/// Where (and whether) to write screenshots for one example program.
pub struct Shots {
    topic: &'static str,
    dir: Option<PathBuf>,
}

impl Shots {
    /// Read `--svg DIR` from the command line.
    pub fn from_args(topic: &'static str) -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let dir =
            args.iter()
                .position(|arg| arg == "--svg")
                .map(|index| match args.get(index + 1) {
                    Some(dir) => PathBuf::from(dir),
                    None => {
                        eprintln!("usage: {topic} [--svg DIR]");
                        std::process::exit(2);
                    }
                });
        if let Some(dir) = &dir {
            std::fs::create_dir_all(dir).expect("create the screenshot directory");
        }
        Shots { topic, dir }
    }

    /// True when writing screenshots rather than printing.
    pub fn is_svg(&self) -> bool {
        self.dir.is_some()
    }

    /// The deterministic console every screenshot is rendered on.
    pub fn pinned_console(width: usize) -> Console {
        pinned_builder(width).build()
    }

    /// The builder behind [`pinned_console`](Self::pinned_console), for shots
    /// that need one more option set.
    pub fn pinned_builder(width: usize) -> ConsoleBuilder {
        pinned_builder(width)
    }

    /// Run one shot: print it, or export it as `<topic>-<name>.svg`.
    pub fn shot(&self, name: &str, width: usize, f: impl FnOnce(&Console)) {
        self.shot_on(name, Self::pinned_console(width), Console::new(), f);
    }

    /// As [`shot`](Self::shot), with a caller-configured console for each mode
    /// (for shots that need a theme or highlighter on the console).
    pub fn shot_on(
        &self,
        name: &str,
        pinned: Console,
        terminal: Console,
        f: impl FnOnce(&Console),
    ) {
        match &self.dir {
            None => {
                terminal.print(
                    &Rule::new(format!("{}-{name}", self.topic))
                        .style(Style::parse("dim").expect("valid style")),
                );
                f(&terminal);
            }
            Some(dir) => {
                let stem = format!("{}-{name}", self.topic);
                export_shot(dir, &stem, &pinned, f);
            }
        }
    }

    /// Write an already-exported SVG document (for themed exports).
    pub fn write(&self, stem: &str, svg: String) {
        if let Some(dir) = &self.dir {
            let path = dir.join(format!("{stem}.svg"));
            std::fs::write(&path, svg).expect("write the screenshot");
            eprintln!("wrote {}", path.display());
        }
    }

    /// The file stem for a shot of this topic.
    pub fn stem(&self, name: &str) -> String {
        format!("{}-{name}", self.topic)
    }
}

// --8<-- [start:pinned]
/// A console that renders the same bytes on every machine.
fn pinned_builder(width: usize) -> ConsoleBuilder {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false) // ignore NO_COLOR in the environment
}
// --8<-- [end:pinned]

// --8<-- [start:export]
/// Render `f` on `console` and write it to `dir/<stem>.svg`.
fn export_shot(dir: &Path, stem: &str, console: &Console, f: impl FnOnce(&Console)) {
    // The stem doubles as the SVG's unique id: same input, same bytes.
    let svg = console.export_svg(stem, stem, f);
    let path = dir.join(format!("{stem}.svg"));
    std::fs::write(&path, svg).expect("write the screenshot");
    eprintln!("wrote {}", path.display());
}
// --8<-- [end:export]
