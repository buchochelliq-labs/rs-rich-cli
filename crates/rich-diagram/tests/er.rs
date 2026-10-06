//! ER diagrams (0.0.16, #247). Snapshots live in `tests/snapshots`; set
//! `UPDATE_SNAPSHOTS=1` to rewrite them after a deliberate change, then
//! review the diff.

use rich::cells::cell_len;
use rich::Console;
use rich_diagram::er::{Cardinality, Column, Entity, ErDiagram, ErModel, Group, Relationship};
use rich_diagram::{Direction, Shape};

/// A small shop: customers, orders and their lines, and a product catalogue.
fn shop() -> ErModel {
    ErModel::new()
        .entity(
            Entity::new("customers")
                .column(Column::new("id").data_type("int").primary_key())
                .column(Column::new("email").data_type("text").unique())
                .column(Column::new("name").data_type("text").nullable()),
        )
        .entity(
            Entity::new("orders")
                .column(Column::new("id").data_type("int").primary_key())
                .column(Column::new("customer_id").data_type("int").foreign_key())
                .column(Column::new("placed_at").data_type("timestamp")),
        )
        .entity(
            Entity::new("order_lines")
                .column(
                    Column::new("order_id")
                        .data_type("int")
                        .primary_key()
                        .foreign_key(),
                )
                .column(
                    Column::new("product_id")
                        .data_type("int")
                        .primary_key()
                        .foreign_key(),
                )
                .column(Column::new("quantity").data_type("int")),
        )
        .entity(
            Entity::new("products")
                .column(Column::new("id").data_type("int").primary_key())
                .column(Column::new("sku").data_type("varchar(32)").unique()),
        )
        .relationship(
            Relationship::new("orders", "customers")
                .columns(["customer_id"], ["id"])
                .cardinality(Cardinality::ManyToOne),
        )
        .relationship(Relationship::new("order_lines", "orders").columns(["order_id"], ["id"]))
        .relationship(Relationship::new("order_lines", "products").columns(["product_id"], ["id"]))
        .group(Group::new("Sales", ["orders", "order_lines"]))
}

fn render(diagram: ErDiagram, width: usize) -> String {
    let console = Console::builder().width(width).color_system(None).build();
    console.render_export(&diagram)
}

fn check(name: &str, actual: &str, width: usize) {
    for line in actual.lines() {
        assert!(
            cell_len(line) <= width,
            "{name}: wider than {width}: {line:?}"
        );
    }
    let path = format!("{}/tests/snapshots/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {path}; run with UPDATE_SNAPSHOTS=1"));
    assert_eq!(actual, expected, "{name} changed:\n{actual}");
}

#[test]
fn the_shop_renders() {
    check("er_shop", &render(ErDiagram::new(shop()), 140), 140);
    check(
        "er_shop_ascii",
        &render(ErDiagram::new(shop()).ascii(true), 140),
        140,
    );
    check(
        "er_shop_td",
        &render(ErDiagram::new(shop()).direction(Direction::TopDown), 120),
        120,
    );
}

#[test]
fn entities_are_tables_with_aligned_rows() {
    let (graph, notes) = shop().to_graph(Direction::LeftRight, false);
    assert!(notes.is_empty(), "{notes:?}");
    let lines = &graph.nodes()[2];
    assert_eq!(lines.shape, Shape::Table);
    assert_eq!(
        lines.label,
        "order_lines\n\
         order_id    int  PK FK\n\
         product_id  int  PK FK\n\
         quantity    int       "
    );
    let edges: Vec<Option<&str>> = graph.edges().iter().map(|e| e.label.as_deref()).collect();
    assert_eq!(
        edges,
        [
            Some("customer_id → id (N:1)"),
            Some("order_id → id"),
            Some("product_id → id"),
        ]
    );
    assert_eq!(graph.clusters().len(), 1);
    assert_eq!(graph.clusters()[0].nodes, [1, 2]);
    let (graph, _) = shop().to_graph(Direction::LeftRight, true);
    assert_eq!(graph.edges()[1].label.as_deref(), Some("order_id -> id"));
}

#[test]
fn the_group_is_framed() {
    let out = render(ErDiagram::new(shop()), 120);
    assert!(out.contains(" Sales "), "{out}");
    assert!(out.contains('╎'), "{out}");
}

#[test]
fn what_cannot_be_drawn_is_noted() {
    let model = ErModel::new()
        .entity(Entity::new("a").column(Column::new("id")))
        .entity(Entity::new("a"))
        .relationship(Relationship::new("a", "missing"))
        .relationship(Relationship::new("a", "a").columns(["nope"], ["id"]))
        .group(Group::new("G", ["a", "ghost"]));
    let out = render(ErDiagram::new(model), 80);
    for note in [
        "ER: entity `a` is defined more than once; the first is drawn",
        "ER: the relationship a → missing is not drawn: there is no entity `missing`",
        "ER: entity `a` has no column `nope`",
        "ER: group `G` names entity `ghost`, which does not exist",
    ] {
        assert!(out.contains(note), "{note}\n{out}");
    }
}

#[test]
fn labels_and_cardinality_alone() {
    let model = ErModel::new()
        .entity(Entity::new("a"))
        .entity(Entity::new("b"))
        .relationship(Relationship::new("a", "b").cardinality(Cardinality::OneToOne))
        .relationship(
            Relationship::new("b", "a")
                .label("owns")
                .cardinality(Cardinality::OneToMany),
        )
        .relationship(Relationship::new("a", "b").columns(Vec::<String>::new(), ["x"]));
    let (graph, _) = model.to_graph(Direction::TopDown, false);
    let labels: Vec<Option<&str>> = graph.edges().iter().map(|e| e.label.as_deref()).collect();
    assert_eq!(labels, [Some("1:1"), Some("owns (1:N)"), Some("→ x")]);
    // An entity without columns is a plain one-line box, no rule.
    assert_eq!(graph.nodes()[0].label, "a");
}

#[test]
fn it_crops_and_says_so() {
    let out = render(ErDiagram::new(shop()), 40);
    assert!(out.lines().all(|line| cell_len(line) <= 40), "{out}");
    assert!(out.contains("ER: cropped to 40 of"), "{out}");
}

#[test]
fn names_cannot_break_rows_or_carry_escapes() {
    let model = ErModel::new()
        .entity(Entity::new("t\x1b[2J").column(Column::new("a\nb").data_type("int\x07")));
    let out = render(ErDiagram::new(model), 80);
    assert!(!out.contains('\x1b') && !out.contains('\x07'), "{out:?}");
    assert!(out.contains("a b"), "{out}");
}
