//! Snapshots of the native text renderer. Set `UPDATE_SNAPSHOTS=1` to rewrite
//! them after a deliberate change, then review the diff.

use rich::Console;
use rich_mermaid::Mermaid;

fn render(source: &str, width: usize, ascii: bool) -> String {
    let console = Console::builder().width(width).color_system(None).build();
    console.render_to_string(&Mermaid::new(source).ascii(ascii))
}

fn check(name: &str, source: &str, width: usize, ascii: bool) {
    let actual = render(source, width, ascii);
    for line in actual.lines() {
        assert!(
            rich::cells::cell_len(line) <= width,
            "{name}: a line is wider than {width}: {line:?}"
        );
    }
    let path = format!("{}/tests/snapshots/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {path}; run with UPDATE_SNAPSHOTS=1"));
    assert_eq!(actual, expected, "{name} changed:\n{actual}");
}

const SHAPES: &str = "flowchart TD
  a[rect] --> b(round) --> c([stadium]) --> d[[subroutine]]
  e[(cylinder)] --> f((circle)) --> g(((double))) --> h>flag]
  i{decision} --> j{{hexagon}} --> k[/in/] --> l[\\out\\]
  m[/top\\] --> n[\\bottom/]";

#[test]
fn every_shape() {
    check("shapes", SHAPES, 100, false);
    check("shapes_ascii", SHAPES, 100, true);
}

const LABELS: &str = "graph TD
  A[Christmas] -->|Get money| B(Go shopping)
  B --> C{Let me think}
  C -->|One| D[Laptop]
  C -->|Two| E[iPhone]
  C -->|Three| F[Car]";

#[test]
fn edge_labels_top_down() {
    check("labels_td", LABELS, 80, false);
}

#[test]
fn every_direction() {
    let body = "  A[Start] --> B(Middle) --> C([End])\n  A -- skip --> C\n";
    for (name, direction) in [
        ("dir_td", "TD"),
        ("dir_bt", "BT"),
        ("dir_lr", "LR"),
        ("dir_rl", "RL"),
    ] {
        check(name, &format!("graph {direction}\n{body}"), 80, false);
    }
    // TB is TD.
    assert_eq!(
        render(&format!("graph TB\n{body}"), 80, false),
        render(&format!("graph TD\n{body}"), 80, false)
    );
}

#[test]
fn strokes_and_heads() {
    check(
        "strokes",
        "graph LR\n  A --> B\n  A ==> C\n  A -.-> D\n  A --- E\n  A <--> F\n  A --o G\n  A --x H\n  A ~~~ I\n  A -.->|dotted| J",
        80,
        false,
    );
}

#[test]
fn cycles_and_self_loops() {
    check(
        "cycle",
        "graph TD\n  A[Draft] --> B[Review]\n  B -->|changes| A\n  B --> C[Merged]\n  C --> C",
        80,
        false,
    );
}

#[test]
fn disconnected_parts() {
    check(
        "disconnected",
        "graph LR\n  A --> B\n  C --> D --> E\n  lonely[On its own]",
        80,
        false,
    );
}

#[test]
fn a_large_graph_is_cropped_with_a_note() {
    let mut source = String::from("graph TD\n");
    for i in 0..12 {
        source.push_str(&format!("  root --> n{i}[node number {i}]\n"));
    }
    let out = render(&source, 60, false);
    assert!(out.contains("Mermaid: cropped to 60 of "), "{out}");
    check("cropped", &source, 60, false);
}

#[test]
fn unsupported_diagrams_show_their_source_under_a_note() {
    let out = render("sequenceDiagram\n  Alice->>Bob: Hi", 60, false);
    assert!(
        out.starts_with("Mermaid: sequence diagrams are not drawn as text"),
        "{out}"
    );
    assert!(out.contains("Alice->>Bob: Hi"), "{out}");
    let out = render("graph TD\n  A -->", 60, false);
    assert!(
        out.starts_with("Mermaid: line 2: an arrow needs a node after it"),
        "{out}"
    );
    let out = render("graph TD", 60, false);
    assert!(
        out.starts_with("Mermaid: the flowchart has no nodes"),
        "{out}"
    );
}

#[test]
fn subgraphs_are_drawn_flat_with_a_note() {
    let out = render("graph TD\n  subgraph one\n    A --> B\n  end", 60, false);
    assert!(out.contains("│ A │") && out.contains("│ B │"), "{out}");
    assert!(
        out.contains("Mermaid: subgraphs are drawn without their frames"),
        "{out}"
    );
}

#[test]
fn labels_cannot_carry_escape_sequences() {
    let out = render(
        "graph LR\n  A[\"\u{1b}[31mred\u{1b}[0m\"] -->|\u{1b}[1mbold\u{7}| B",
        60,
        false,
    );
    assert!(out.contains("red") && out.contains("bold"), "{out}");
    assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
    // Nor can source shown in a fallback code block.
    let out = render("sequenceDiagram\n  A->>B: \u{1b}]0;title\u{7}", 60, false);
    assert!(out.contains("A->>B"), "{out}");
    assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
}

/// Render `source` on another thread; fail if it takes longer than `limit`
/// (generous for a debug build; these took minutes before the limits).
fn render_within(source: String, limit: std::time::Duration) -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(render(&source, 80, false));
    });
    rx.recv_timeout(limit)
        .unwrap_or_else(|_| panic!("rendering took longer than {limit:?}"))
}

/// `a0 & … & a44 ---…---> b0 & … & b43`: under 1 KiB, 1980 edges, each
/// crossing many ranks. Refused as too large right after ranking, before the
/// layout builds its per-rank points.
#[test]
fn long_fanout_links_are_refused_quickly() {
    let left: Vec<String> = (0..45).map(|i| format!("a{i}")).collect();
    let right: Vec<String> = (0..44).map(|i| format!("b{i}")).collect();
    let source = format!(
        "graph TD\n{} {}> {}\n",
        left.join("&"),
        "-".repeat(400),
        right.join("&")
    );
    assert!(source.len() < 1024, "{}", source.len());
    let out = render_within(source, std::time::Duration::from_secs(10));
    assert!(out.contains("Mermaid: too large to draw: "), "{out}");
}

/// Links longer than `MAX_LINK_LENGTH` ranks are drawn at that length.
#[test]
fn very_long_links_are_capped() {
    let capped = render_within(
        format!("graph TD\nA {}> B\n", "-".repeat(60_000)),
        std::time::Duration::from_secs(10),
    );
    let longest = render(
        &format!(
            "graph TD\nA {}> B\n",
            "-".repeat(rich_mermaid::flowchart::MAX_LINK_LENGTH + 1)
        ),
        80,
        false,
    );
    assert_eq!(capped, longest);
    assert!(capped.contains('▼'), "{capped}");
}
