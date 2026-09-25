//! The parser and layout never panic, and never draw wider than asked, on
//! random Mermaid-like input.

use rich::Console;
use rich_mermaid::{parse, Mermaid};

/// A small deterministic generator (xorshift), so failures reproduce.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

const TOKENS: &[&str] = &[
    "A",
    "B",
    "c1",
    "node",
    "x",
    "o",
    " ",
    " ",
    "\n",
    ";",
    "&",
    "-->",
    "---",
    "-.->",
    "==>",
    "<-->",
    "--o",
    "--x",
    "~~~",
    "--->",
    "-- t -->",
    "|lbl|",
    "[",
    "]",
    "(",
    ")",
    "{",
    "}",
    "((",
    "))",
    "[[",
    "]]",
    "[(",
    ")]",
    "[/",
    "/]",
    "[\\",
    "\\]",
    ">",
    "\"",
    "#quot;",
    "<br>",
    "%%",
    ":::c",
    "@{",
    "subgraph s",
    "end",
    "style A fill:#f00",
    "classDef c",
    "日本",
    "é",
    "\u{1b}",
    "\t",
    "-",
    "=",
    ".",
    "|",
    "graph",
    "TD",
    "LR",
];

#[test]
fn random_sources_never_panic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let console = Console::builder().width(50).color_system(None).build();
    for case in 0..3000 {
        let header = rng.pick(&[
            "graph TD\n",
            "graph LR\n",
            "flowchart BT\n",
            "graph RL\n",
            "",
            "graph\n",
        ]);
        let length = rng.next() % 40;
        let mut source = header.to_string();
        for _ in 0..length {
            source.push_str(rng.pick(TOKENS));
        }
        let _ = parse(&source);
        let ascii = case % 2 == 0;
        let out = console.render_to_string(&Mermaid::new(source.clone()).ascii(ascii));
        for line in out.lines() {
            assert!(
                rich::cells::cell_len(line) <= 50,
                "case {case}: too wide for {source:?}: {line:?}"
            );
        }
    }
}

#[test]
fn random_valid_graphs_draw() {
    let mut rng = Rng(42);
    let console = Console::builder().width(120).color_system(None).build();
    for case in 0..300 {
        let direction = rng.pick(&["TD", "BT", "LR", "RL"]);
        let nodes = 1 + rng.next() % 12;
        let edges = 1 + rng.next() % 20;
        let mut source = format!("graph {direction}\n");
        for _ in 0..edges {
            let a = rng.next() % nodes;
            let b = rng.next() % nodes;
            let link = rng.pick(&["-->", "---", "-.->", "==>", "-->|yes|", "--->", "<-->"]);
            source.push_str(&format!("  n{a}[Node {a}] {link} n{b}[Node {b}]\n"));
        }
        let chart = parse(&source).unwrap_or_else(|e| panic!("case {case}: {e}\n{source}"));
        rich_mermaid::draw(&chart, false).unwrap();
        let out = console.render_to_string(&Mermaid::new(source.clone()));
        assert!(
            !out.starts_with("Mermaid:"),
            "case {case} fell back:\n{source}\n{out}"
        );
    }
}
