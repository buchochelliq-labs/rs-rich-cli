//! Snapshots of sequence diagrams drawn as text. Set `UPDATE_SNAPSHOTS=1` to
//! rewrite them after a deliberate change, then review the diff.

use rich::Console;
use rich_mermaid::Mermaid;

fn render(source: &str, width: usize, ascii: bool) -> String {
    let console = Console::builder().width(width).color_system(None).build();
    console.render_export(&Mermaid::new(source).ascii(ascii))
}

/// Compare with `tests/snapshots/{name}.txt`; no line may be wider than
/// `width`.
fn check(name: &str, source: &str, width: usize, ascii: bool) {
    let actual = render(source, width, ascii);
    for line in actual.lines() {
        assert!(
            rich::cells::cell_len(line) <= width,
            "{name}: a line is wider than {width}: {line:?}"
        );
    }
    if ascii {
        // Labels may hold any text; the drawing itself is ASCII.
        let drawn = |c: char| ('\u{2500}'..='\u{25ff}').contains(&c) || c == '×' || c == '↻';
        assert!(!actual.chars().any(drawn), "{name}: not ASCII:\n{actual}");
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

/// Actors, activations (`+`/`-` and statements), a self-message in a loop,
/// `alt`/`else`, a note over two participants, `autonumber` and a title.
const CHECKOUT: &str = r#"sequenceDiagram
    title: Checkout
    autonumber
    actor C as Customer
    participant W as Web shop
    participant P as Payments
    C->>+W: Place order
    W->>+P: Charge card
    loop Every second
        P->>P: Poll bank
    end
    alt card accepted
        P-->>W: Paid
    else declined
        P--xW: Declined
    end
    deactivate P
    Note over W,P: Both log the result
    W-->>-C: Confirmation
    Note right of C: Reads email
    C-)W: Feedback"#;

/// Every other arrow, notes left of and over one participant, multi-line
/// and wide text, nested `par`/`and` and `critical`/`option`, an empty
/// `break`, `rect` and an unlabelled `opt`.
const BLOCKS: &str = r#"sequenceDiagram
    participant A
    participant B as Bob<br>the builder
    participant C as 中文
    Note left of A: left
    Note over A: a very long note over A
    A->C: no head
    C-->A: dotted no head
    A<<->>B: both
    B<<-->>C: both dotted
    par A to B
        A->>B: hello
    and B to C
        B->>C: hi
        critical inner
            C->>C: self<br>two lines
        option fallback
            C-xA: lost
        end
    end
    break stop
    end
    rect rgb(1,2,3)
        A->>A: rect
    end
    opt
        B->>A: plain opt
    end"#;

#[test]
fn checkout() {
    check("sequence_checkout", CHECKOUT, 100, false);
    check("sequence_checkout_ascii", CHECKOUT, 100, true);
}

#[test]
fn arrows_notes_and_blocks() {
    check("sequence_blocks", BLOCKS, 100, false);
    check("sequence_blocks_ascii", BLOCKS, 100, true);
}

#[test]
fn a_wide_diagram_is_cropped_with_a_note() {
    let out = render(CHECKOUT, 40, false);
    assert!(
        out.contains("Mermaid: cropped to 40 of 64 columns"),
        "{out}"
    );
    check("sequence_cropped", CHECKOUT, 40, false);
}

#[test]
fn simplified_constructs_are_drawn_with_a_note() {
    let out = render(
        "sequenceDiagram\n  box Purple Team\n    participant A\n  end\n  A->>B: hi",
        60,
        false,
    );
    assert!(out.contains("│ A │   │ B │"), "{out}");
    assert!(
        out.ends_with("Mermaid: boxes are drawn without their frames\n"),
        "{out}"
    );
}

#[test]
fn unsupported_syntax_falls_back_to_the_source_naming_it() {
    for (source, note) in [
        (
            "sequenceDiagram\n  A->>B: hi\n  wobble A",
            "Mermaid: line 3: `wobble` is not drawn as text",
        ),
        (
            "sequenceDiagram\n  A->>B: hi\n  end",
            "Mermaid: line 3: `end` without a block to close",
        ),
        (
            "sequenceDiagram\n  loop forever\n  A->>B: hi",
            "Mermaid: line 2: `loop` is never closed with `end`",
        ),
        (
            "sequenceDiagram\n  A->>B: hi\n  deactivate B",
            "Mermaid: line 3: `B` is deactivated but is not active",
        ),
    ] {
        let out = render(source, 60, false);
        assert!(out.starts_with(note), "{source}: {out}");
        assert!(out.contains("A->>B: hi"), "{out}");
    }
    let out = render("sequenceDiagram\n  %% nothing yet", 60, false);
    assert!(
        out.starts_with("Mermaid: the sequence diagram has no participants"),
        "{out}"
    );
}

#[test]
fn fences_and_the_source_renderer_draw_them() {
    use rich::markdown::Markdown;
    use rich_mermaid::MermaidFences;
    let console = Console::builder().width(60).color_system(None).build();
    let markdown = Markdown::new("```mermaid\nsequenceDiagram\n  A->>B: hi\n```")
        .fence_renderer(std::sync::Arc::new(MermaidFences::default()));
    let out = console.render_export(&markdown);
    assert!(
        out.contains("│  hi   │") && out.contains("├──────►│"),
        "{out}"
    );
}

/// The largest diagrams the parser accepts draw (or are refused) quickly.
#[test]
fn large_diagrams_are_bounded() {
    let render_within = |source: String| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(render(&source, 80, false));
        });
        rx.recv_timeout(std::time::Duration::from_secs(20))
            .expect("rendering took longer than 20 s")
    };
    // Many participants, each message spanning them all.
    let mut wide = String::from("sequenceDiagram\n");
    for i in 0..1990 {
        wide.push_str(&format!(
            "p{} ->> p{}: message {i}\n",
            i % 400,
            399 - i % 400
        ));
    }
    let out = render_within(wide);
    assert!(
        out.contains("Mermaid: too large to draw: "),
        "{}",
        &out[..200]
    );
    // Blocks nested as deep as allowed, each around every message.
    let depth = rich_mermaid::sequence::MAX_NESTING;
    let mut deep = String::from("sequenceDiagram\n");
    for i in 0..depth {
        deep.push_str(&format!("loop level {i} with a long label\n"));
    }
    for i in 0..1900 {
        deep.push_str(&format!("A->>B: {i}\n"));
    }
    deep.push_str(&"end\n".repeat(depth));
    let out = render_within(deep);
    assert!(out.contains("Mermaid: "), "{}", &out[..200]);
    let too_deep = format!("sequenceDiagram\n{}", "loop\n".repeat(depth + 1));
    let out = render(&too_deep, 80, false);
    assert!(
        out.starts_with("Mermaid: the diagram is too large: blocks nested more than 32 deep"),
        "{out}"
    );
}
