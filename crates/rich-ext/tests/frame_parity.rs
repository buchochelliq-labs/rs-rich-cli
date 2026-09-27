//! Frame encoding parity: `Frame::to_ansi` writes the same bytes as
//! `Console::segments_to_string` for any control-free stream (#226).

use rich::markdown::Markdown;
use rich::panel::Panel;
use rich::{
    ColorSystem, Columns, Console, Json, Pretty, Renderable, Rule, Segment, Style, Syntax, Table,
    Text, Tree,
};
use rich_ext::frame::Frame;

const SYSTEMS: [Option<ColorSystem>; 5] = [
    None,
    Some(ColorSystem::Standard),
    Some(ColorSystem::EightBit),
    Some(ColorSystem::Truecolor),
    Some(ColorSystem::Windows),
];

fn consoles() -> Vec<Console> {
    let mut consoles = Vec::new();
    for system in SYSTEMS {
        for no_color in [false, true] {
            consoles.push(
                Console::builder()
                    .width(60)
                    .force_terminal(true)
                    .color_system(system)
                    .no_color(no_color)
                    .highlight(false)
                    .build(),
            );
        }
    }
    consoles
}

fn assert_parity(label: &str, segments: &[Segment], consoles: &[Console]) {
    let segments: Vec<Segment> = segments.iter().filter(|s| !s.control).cloned().collect();
    let frame = Frame::from_segments(&segments);
    let plain: String = segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(frame.plain(), plain, "{label}: plain");
    assert_eq!(frame.merged().plain(), plain, "{label}: merged plain");
    for console in consoles {
        assert_eq!(
            frame.to_ansi(console),
            console.segments_to_string(&segments),
            "{label}: {:?} no_color={}",
            console.color_system(),
            console.no_color(),
        );
    }
}

/// A small xorshift generator, so the test needs no extra dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() % items.len() as u64) as usize]
    }
}

#[test]
fn random_streams_encode_exactly() {
    let pieces = [
        "", "a", "word", " ", "\n", "\n\n", "x\ny", "漢字", "👍🏽", "e\u{301}", "\t", "end\n",
    ];
    let styles: Vec<Option<Style>> = [
        None,
        Some("bold"),
        Some("red on white"),
        Some("italic #ff8800"),
        Some("underline link https://example.com"),
        Some("not bold dim"),
        Some("color(200) on color(17) strike"),
        Some(""),
    ]
    .iter()
    .map(|s| s.map(|s| Style::parse(s).unwrap()))
    .collect();
    let consoles = consoles();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..2_000 {
        let length = (rng.next() % 12) as usize;
        let segments: Vec<Segment> = (0..length)
            .map(|_| {
                let text: String = (0..1 + rng.next() % 3)
                    .map(|_| *rng.pick(&pieces))
                    .collect();
                if rng.next().is_multiple_of(16) {
                    Segment::control("\x1b[2K")
                } else {
                    Segment::new(text, rng.pick(&styles).clone())
                }
            })
            .collect();
        assert_parity(&format!("random case {case}"), &segments, &consoles);
    }
}

fn renderables() -> Vec<(&'static str, Box<dyn Renderable>)> {
    let mut table = Table::new().title("People").caption("two rows");
    table.add_column("Name");
    table.add_column("Notes");
    table.add_row(&["Alice", "[bold]lead[/] [link=https://a.test]site[/link]"]);
    table.add_row(&["漢字", "multi\nline"]);
    let mut tree = Tree::new("root");
    tree.add("src").add("main.rs");
    tree.add("[red]Cargo.toml[/]");
    vec![
        ("table", Box::new(table)),
        ("tree", Box::new(tree)),
        (
            "panel",
            Box::new(Panel::new(Box::new(
                Text::from_markup("[b]hello[/b]\n[on blue]world[/]").unwrap(),
            ))),
        ),
        (
            "markdown",
            Box::new(Markdown::new(
                "# Title\n\nSome *emphasis*, `code` and a [link](https://x.test).\n\n\
                 - one\n- two\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
            )),
        ),
        (
            "syntax",
            Box::new(Syntax::new(
                "fn main() {\n    println!(\"hi\");\n}\n",
                "rust",
            )),
        ),
        (
            "json",
            Box::new(Json::new(r#"{"a": [1, 2.5, null], "b": {"c": "d"}}"#).unwrap()),
        ),
        ("pretty", Box::new(Pretty::new(&vec![Some(1), None]))),
        ("rule", Box::new(Rule::new("[bold]Section[/]"))),
        (
            "columns",
            Box::new(Columns::new(
                ["alpha", "beta", "gamma", "delta", "漢字"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            )),
        ),
        (
            "wrapped text",
            Box::new(Text::from_markup(&"[u]wrapped[/] words and 👍🏽 ".repeat(12)).unwrap()),
        ),
    ]
}

#[test]
fn renderables_encode_exactly() {
    let consoles = consoles();
    for (name, renderable) in renderables() {
        for console in &consoles {
            let segments = console.render(renderable.as_ref(), None);
            assert_parity(name, &segments, std::slice::from_ref(console));
        }
    }
}

/// Every golden fixture's expected output, decoded back into a segment
/// stream, encodes exactly. The fixtures live in the core crate, so this
/// reads them at run time and skips when they are absent (a packaged crate).
#[test]
fn golden_fixtures_encode_exactly() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rich/tests/golden");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let consoles = consoles();
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "tsv") {
            continue;
        }
        let data = std::fs::read_to_string(&path).unwrap();
        for line in data
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let expected = line.rsplit('\t').next().unwrap();
            let expected = expected.replace("\\x1b", "\x1b").replace("\\n", "\n");
            let text = Text::from_ansi(&expected, Style::new());
            for console in &consoles {
                let segments = console.render(&text, None);
                let label = format!("{}: {}", path.display(), line.split('\t').next().unwrap());
                assert_parity(&label, &segments, std::slice::from_ref(console));
            }
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} golden rows found");
}

#[test]
fn render_target_frame_matches_text() {
    use rich::protocol::{Support, TargetCapabilities};
    use rich_ext::target::{RenderTarget, TargetKind};
    for kind in [
        TargetKind::Terminal,
        TargetKind::PlainStream,
        TargetKind::Capture,
        TargetKind::Html,
    ] {
        let capabilities = TargetCapabilities {
            width: 50,
            height: 20,
            color_system: Some(ColorSystem::Truecolor),
            interactive: true,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        };
        let target = RenderTarget::new(kind, capabilities, rich::Theme::default());
        for (name, renderable) in renderables() {
            let frame = target.frame(renderable.as_ref());
            assert_eq!(
                frame.to_ansi(&target.console()),
                target.text(renderable.as_ref()),
                "{kind:?} {name}"
            );
        }
    }
}
