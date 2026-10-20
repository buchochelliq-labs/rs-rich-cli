//! Snapshots of ER diagrams drawn as text. Set `UPDATE_SNAPSHOTS=1` to
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

/// Mermaid's own example: attributes with keys and a comment, identifying
/// and non-identifying relationships, an entity without attributes.
const SHOP: &str = r#"erDiagram
    CUSTOMER ||--o{ ORDER : places
    ORDER ||--|{ LINE-ITEM : contains
    CUSTOMER }|..|{ DELIVERY-ADDRESS : uses
    CUSTOMER {
        string name
        string custNumber PK
        string sector
    }
    ORDER {
        int orderNumber PK
        string deliveryAddress FK "where it goes"
    }
    LINE-ITEM {
        string productCode FK
        int quantity
        float pricePerUnit
    }"#;

/// A title, `RL`, aliases, self-relationships, an empty label and word forms.
const STAFF: &str = r#"---
title: Staff
---
erDiagram
    direction RL
    EMPLOYEE |o--o{ EMPLOYEE : manages
    EMPLOYEE["Employee"] {
        int id PK
        int manager_id FK
    }
    EMPLOYEE }o..|| DEPARTMENT : "works in"
    DEPARTMENT ||--|| DEPARTMENT : ""
    PROJECT one or more to zero or many EMPLOYEE : staffs"#;

#[test]
fn shop_top_down() {
    check("er_shop", SHOP, 100, false);
    check("er_shop_ascii", SHOP, 100, true);
}

#[test]
fn shop_left_right() {
    let source = SHOP.replacen("erDiagram", "erDiagram\n    direction LR", 1);
    check("er_shop_lr", &source, 160, false);
}

#[test]
fn staff_right_left_with_self_relationships() {
    check("er_staff", STAFF, 100, false);
    check("er_staff_ascii", STAFF, 100, true);
}

#[test]
fn a_wide_diagram_is_cropped_with_a_note() {
    let source = SHOP.replacen("erDiagram", "erDiagram\n    direction LR", 1);
    let out = render(&source, 80, false);
    assert!(out.contains("Mermaid: cropped to 80 of "), "{out}");
}

#[test]
fn unsupported_syntax_falls_back_to_the_source_naming_it() {
    let out = render("erDiagram\n  A ||--o{ B : has\n  A <|-- C", 60, false);
    assert!(
        out.starts_with("Mermaid: line 3: `A <|-- C` is not drawn as text"),
        "{out}"
    );
    assert!(out.contains("A ||--o{ B : has"), "{out}");
    let out = render("erDiagram", 60, false);
    assert!(
        out.starts_with("Mermaid: the ER diagram has no entities"),
        "{out}"
    );
}
