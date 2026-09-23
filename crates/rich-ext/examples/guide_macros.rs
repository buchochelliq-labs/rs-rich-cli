//! Guide: Macros — run: cargo run -p rs-rich-ext --example guide_macros --features macros [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/macros.md` come from this file. With
//! `--svg DIR` every shot is written as `DIR/guide_macros-<shot>.svg`.

use std::path::PathBuf;

use rich::{ColorSystem, Console};
use rich_ext::{markup, rich_panel, rich_table, rich_tree, richf, style, theme_key, Rich};

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

    fn shot(&self, name: &str, width: usize, body: impl FnOnce(&Console)) {
        match &self.dir {
            None => {
                let console = Console::new();
                console.print_str(&format!("[dim]── {name} ──[/]"));
                body(&console);
            }
            Some(dir) => {
                let console = Console::builder()
                    .width(width)
                    .force_terminal(true)
                    .color_system(Some(ColorSystem::Truecolor))
                    .build();
                let id = format!("guide_macros-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

// --8<-- [start:derive]
#[derive(Rich)]
#[rich(title = "Server")]
struct Server {
    #[rich(label = "Host", style = "bold cyan")]
    host: String,
    #[rich(order = -1)] // before the others
    port: u16,
    #[rich(skip)]
    #[allow(dead_code)]
    token: String,
    #[rich(format = "{:.1}%", justify = "right")]
    load: f64,
    tags: Vec<&'static str>, // Debug, highlighted
}
// --8<-- [end:derive]

fn server(host: &str, port: u16, load: f64) -> Server {
    Server {
        host: host.into(),
        port,
        token: "hunter2".into(),
        load,
        tags: vec!["eu", "primary"],
    }
}

// --8<-- [start:derive-presentations]
#[derive(Rich)]
#[rich(panel, title = "Release")]
struct Release {
    #[rich(display)]
    version: semver_like::Version,
    crates: u32,
}

#[derive(Rich)]
#[rich(table)]
struct Download {
    #[rich(label = "Crate")]
    name: &'static str,
    #[rich(justify = "right")]
    downloads: u64,
}

#[derive(Rich)]
enum Job {
    Queued,
    Running { pid: u32 },
    Failed(#[rich(label = "code")] i32),
}
// --8<-- [end:derive-presentations]

/// A stand-in type with a `Display` impl, for `#[rich(display)]`.
mod semver_like {
    pub struct Version(pub u32, pub u32, pub u32);
    impl std::fmt::Display for Version {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}.{}.{}", self.0, self.1, self.2)
        }
    }
}

fn main() {
    let shots = Shots::from_args();

    shots.shot("richf", 64, |console| {
        // --8<-- [start:richf]
        let name = "[red]mallory[/red]"; // user data: printed, never parsed
        let count = 7;
        let text = richf!("[bold]{name}[/] has {count:>3} items, {} left", 2);
        console.print(&text);

        // Named arguments, positions and width references, as in `format!`.
        let width = 6;
        console.print(&richf!("[green]{0}|{v:>width$}|{0}[/]", "x", v = 1.5));

        // Keys your theme defines, declared up front so they are accepted.
        console.print(&richf!(
            keys["app.title"],
            "[app.title]{}[/] {{literal braces}}",
            "Title"
        ));

        // A placeholder inside a tag is inserted as markup (checked at run time).
        let colour = "magenta";
        console.print(&richf!("[{colour}]hue[/]"));
        // --8<-- [end:richf]
    });

    shots.shot("literals", 64, |console| {
        // --8<-- [start:literals]
        let warn: rich::Style = style!("bold yellow on grey23"); // parsed at compile time
        let key: &'static str = theme_key!("repr.number"); // must exist in the default theme
        let banner: &'static str = markup!("[green]ok[/] [dim]all checks passed[/]");

        console.print(&rich::Text::styled("careful", warn));
        console.print(&rich::Text::styled("42", key));
        console.print_str(banner);
        // --8<-- [end:literals]
    });

    shots.shot("derive", 64, |console| {
        // --8<-- [start:derive-use]
        console.print(&server("example.com", 8080, 12.25));
        // --8<-- [end:derive-use]
    });

    shots.shot("derive-presentations", 64, |console| {
        // --8<-- [start:derive-presentations-use]
        console.print(&Release {
            version: semver_like::Version(0, 0, 11),
            crates: 5,
        });
        console.print(&Download {
            name: "rs-rich",
            downloads: 1204,
        });
        console.print(&Job::Running { pid: 4242 });
        console.print(&Job::Queued);
        // --8<-- [end:derive-presentations-use]
    });

    shots.shot("derive-table", 64, |console| {
        // --8<-- [start:derive-table]
        use rich_ext::derive;

        let servers = [
            server("a.example.com", 8080, 1.0),
            server("b.example.com", 8443, 99.5),
        ];
        console.print(&derive::table(&servers));

        // Enum variants with different fields leave the missing cells empty.
        let jobs = [Job::Running { pid: 7 }, Job::Failed(2), Job::Queued];
        console.print(&derive::table(&jobs));
        // --8<-- [end:derive-table]
    });

    shots.shot("builders", 64, |console| {
        // --8<-- [start:builders]
        let table = rich_table!(["Name", "Age"], ["Alice", 30], ["Bob", 4]);
        let panel = rich_panel!("[bold]ready[/]", title = "status", subtitle = "api");
        let tree = rich_tree!("src" => ["main.rs", "lib" => ["mod.rs"], "build.rs"]);

        // They return the ordinary core types, so keep configuring them.
        let table = table.title("People");
        console.print(&table);
        console.print(&panel);
        console.print(&tree);
        // --8<-- [end:builders]
    });

    shots.shot("dbg", 64, |console| {
        // `rich_dbg!` writes to standard error; this shows the same line.
        let line =
            rich_ext::macros::dbg_line("src/main.rs", 12, 5, "config.retries * 2", &vec![3, 6]);
        console.print(&line);
    });

    if shots.dir.is_none() {
        // --8<-- [start:print]
        use rich_ext::{rich_dbg, rich_eprintln, rich_println, rich_progress, rich_trace};

        let retries = rich_dbg!(3 * 2); // like dbg!: prints to stderr, returns the value
                                        // Pass values as arguments: inside these print macros, `{retries}`
                                        // cannot capture a local variable (it can in `richf!`).
        rich_println!("[bold green]done[/] after {} retries", retries);
        rich_eprintln!("[yellow]warning:[/] {} files skipped", 2);
        rich_trace!("[dim]cache[/] warmed"); // dim `file:line` prefix, to stderr

        let mut total = 0;
        for n in rich_progress!(0..50u64, "Summing") {
            total += n;
        }
        rich_println!("total = {t}", t = total);
        // --8<-- [end:print]
    }
}
