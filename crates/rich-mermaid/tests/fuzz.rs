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

const SEQUENCE_TOKENS: &[&str] = &[
    "A",
    "B",
    "Carol",
    " ",
    " ",
    "\n",
    "\n",
    ";",
    ":",
    ",",
    "->>",
    "-->>",
    "->",
    "-->",
    "-x",
    "--x",
    "-)",
    "--)",
    "<<->>",
    "+",
    "-",
    "participant ",
    "actor ",
    " as ",
    "Note left of ",
    "Note right of ",
    "Note over ",
    "activate ",
    "deactivate ",
    "loop x\n",
    "alt y\n",
    "else\n",
    "par\n",
    "and\n",
    "critical\n",
    "option\n",
    "break\n",
    "rect rgb(1,2,3)\n",
    "opt\n",
    "end\n",
    "autonumber\n",
    "title T\n",
    "box\n",
    "create ",
    "destroy ",
    "<br>",
    "#59;",
    "日本",
    "\u{1b}",
    "\t",
    "a long message text",
];

const ER_TOKENS: &[&str] = &[
    "A",
    "B",
    "C-D",
    "\"Two words\"",
    " ",
    " ",
    "\n",
    "\n",
    ":",
    "||--o{",
    "}|..|{",
    "|o--||",
    "}o..o|",
    "only one",
    " to ",
    " optionally to ",
    "zero or more",
    "{",
    "}",
    "string",
    "int",
    "PK",
    "FK",
    "UK",
    ",",
    "\"comment\"",
    "[\"alias\"]",
    "direction LR\n",
    "direction BT\n",
    "style A x\n",
    "日本",
    "\u{1b}",
    "\t",
];

/// Sequence and ER diagrams: never a panic, never wider than asked.
#[test]
fn random_sequence_and_er_sources_never_panic() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let console = Console::builder().width(50).color_system(None).build();
    for case in 0..3000 {
        let (header, tokens) = if case % 2 == 0 {
            ("sequenceDiagram\n", SEQUENCE_TOKENS)
        } else {
            ("erDiagram\n", ER_TOKENS)
        };
        let length = rng.next() % 50;
        let mut source = header.to_string();
        for _ in 0..length {
            source.push_str(rng.pick(tokens));
        }
        let _ = rich_mermaid::sequence::parse(&source);
        let _ = rich_mermaid::er::parse(&source);
        let ascii = case % 4 < 2;
        let out = console.render_to_string(&Mermaid::new(source.clone()).ascii(ascii));
        for line in out.lines() {
            assert!(
                rich::cells::cell_len(line) <= 50,
                "case {case}: too wide for {source:?}: {line:?}"
            );
        }
    }
}

/// Well-formed random sequence diagrams always draw.
#[test]
fn random_valid_sequences_draw() {
    let mut rng = Rng(7);
    for case in 0..300 {
        let people = 1 + rng.next() % 6;
        let mut source = String::from("sequenceDiagram\n");
        let mut depth = 0;
        for _ in 0..1 + rng.next() % 25 {
            let a = rng.next() % people;
            let b = rng.next() % people;
            match rng.next() % 6 {
                0 if depth < 3 => {
                    source.push_str(rng.pick(&["loop l\n", "alt a\n", "par p\n", "opt\n"]));
                    depth += 1;
                }
                1 if depth > 0 => {
                    source.push_str("end\n");
                    depth -= 1;
                }
                2 => source.push_str(&format!(
                    "Note {} p{a}: note {case}\n",
                    rng.pick(&["left of", "right of", "over"])
                )),
                _ => source.push_str(&format!(
                    "p{a}{}p{b}: message {a} to {b}\n",
                    rng.pick(&["->>", "-->>", "-x", "--)", "<<->>"])
                )),
            }
        }
        source.push_str(&"end\n".repeat(depth));
        let diagram = rich_mermaid::sequence::parse(&source)
            .unwrap_or_else(|e| panic!("case {case}: {e}\n{source}"));
        rich_mermaid::sequence::draw(&diagram, false).unwrap();
    }
}
