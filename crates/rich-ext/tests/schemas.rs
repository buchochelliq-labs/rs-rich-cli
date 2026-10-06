//! The format-neutral schema model (0.0.16 workstream 4): JSON Schema and
//! SQL DDL read into it, drawn as trees, compared, and laid on a timeline.
#![cfg(feature = "data")]

use rich::{Console, Renderable};
use rich_ext::chart::Charset;
use rich_ext::schema::{
    self, json, sql, Constraint, DataType, FieldKind, SchemaDiff, SchemaTimeline, SchemaTree,
};

fn plain(width: usize, r: &dyn Renderable) -> String {
    Console::builder()
        .width(width)
        .color_system(None)
        .build()
        .render_to_string(r)
}

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/sources/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

const SHOP_V1: &str = "
-- The shop, version 1.
CREATE TABLE customers (
    id      BIGINT PRIMARY KEY,
    email   VARCHAR(255) NOT NULL UNIQUE,
    name    TEXT
);

CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers (id),
    status      VARCHAR(16) DEFAULT 'new',
    total       NUMERIC(10, 2) CHECK (total >= 0)
);
";

const SHOP_V2: &str = "
CREATE TABLE customers (
    id      BIGINT PRIMARY KEY,
    email   VARCHAR(320) NOT NULL UNIQUE,
    name    TEXT NOT NULL
);

CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers (id),
    status      VARCHAR(16) DEFAULT 'pending',
    total       NUMERIC(12, 2),
    placed_at   TIMESTAMP WITH TIME ZONE
);

CREATE TABLE order_lines (
    order_id BIGINT REFERENCES orders (id),
    line     INT,
    sku      TEXT NOT NULL,
    PRIMARY KEY (order_id, line)
);
";

#[test]
fn json_schema_maps_into_the_model() {
    let value = schema::parse(&fixture("order-v1.schema.json")).unwrap();
    let model = json::to_model(&value);
    assert_eq!(model.name(), Some("Order"));
    assert_eq!(model.description(), Some("A customer's order."));
    assert_eq!(
        model.constraints(),
        [Constraint::Additional(schema::Literal::new("false"))]
    );
    let names: Vec<(&str, FieldKind)> = model
        .fields()
        .iter()
        .map(|f| (f.name(), f.kind()))
        .collect();
    assert_eq!(
        names,
        [
            ("id", FieldKind::Field),
            ("status", FieldKind::Field),
            ("customer", FieldKind::Field),
            ("items", FieldKind::Field),
            ("payment", FieldKind::Field),
            ("notes", FieldKind::Field),
        ]
    );
    let id = model.field("id").unwrap();
    assert!(id.is_required());
    assert_eq!(
        id.constraints(),
        [Constraint::Pattern("^ord_[a-z0-9]+$".into())]
    );
    let status = model.field("status").unwrap();
    assert_eq!(status.enum_values().unwrap().len(), 3);
    assert_eq!(status.default_value().unwrap().as_str(), "\"pending\"");
    let customer = model.field("customer").unwrap();
    assert_eq!(customer.reference(), Some("#/$defs/customer"));
    let referrer = customer
        .children()
        .find(|f| f.name() == "referrer")
        .unwrap();
    assert!(matches!(
        referrer.unexpanded(),
        Some(schema::Unexpanded::Recursive(r)) if r == "#/$defs/customer"
    ));
    let items = model.field("items").unwrap();
    let DataType::List(item) = items.data_type() else {
        panic!("{:?}", items.data_type());
    };
    assert_eq!(item.kind(), FieldKind::Items);
    assert_eq!(item.reference(), Some("#/$defs/item"));
    let notes = model.field("notes").unwrap();
    assert_eq!(notes.data_type(), &DataType::String);
    assert!(notes.is_nullable());
    let branches: Vec<(&str, FieldKind)> = model
        .field("payment")
        .unwrap()
        .children()
        .map(|f| (f.name(), f.kind()))
        .collect();
    let one_of = FieldKind::Branch(schema::Composition::OneOf);
    assert_eq!(branches, [("[1] Card", one_of), ("[2] Invoice", one_of)]);
}

#[test]
fn ddl_draws_as_a_tree_of_tables() {
    let parsed = sql::parse(SHOP_V1).unwrap();
    let notes: Vec<String> = parsed.notes.iter().map(ToString::to_string).collect();
    assert_eq!(notes, ["line 13: orders.total: CHECK constraint skipped"]);
    let tree = SchemaTree::from_model(parsed.schema).title("shop");
    assert_eq!(
        plain(80, &tree),
        concat!(
            "shop  2 tables\n",
            "├── customers  table\n",
            "│   ├── id (required)  BIGINT  primary key\n",
            "│   ├── email (required)  VARCHAR(255)  unique\n",
            "│   └── name  TEXT\n",
            "└── orders  table\n",
            "    ├── id (required)  BIGINT  primary key\n",
            "    ├── customer_id (required)  BIGINT  → customers.id\n",
            "    ├── status  VARCHAR(16)  default='new'\n",
            "    └── total  NUMERIC(10, 2)",
        )
    );
}

#[test]
fn ddl_versions_diff() {
    let old = sql::parse(SHOP_V1).unwrap().schema;
    let new = sql::parse(SHOP_V2).unwrap().schema;
    let diff = SchemaDiff::models(&old, &new).names("v1", "v2");
    assert_eq!(
        plain(100, &diff),
        concat!(
            "┏━━━┳━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━┓\n",
            "┃   ┃ Where            ┃ Change                                 ┃          ┃\n",
            "┡━━━╇━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━┩\n",
            "│ ~ │ customers.name   │ became required                        │ breaking │\n",
            "│ ~ │ customers.email  │ type VARCHAR(255) → VARCHAR(320)       │ breaking │\n",
            "│ ~ │ orders.status    │ default 'new' → 'pending'              │          │\n",
            "│ ~ │ orders.total     │ type NUMERIC(10, 2) → NUMERIC(12, 2)   │ breaking │\n",
            "│ + │ orders.placed_at │ field added (TIMESTAMP WITH TIME ZONE) │          │\n",
            "│ + │ order_lines      │ table added (3 fields)                 │          │\n",
            "└───┴──────────────────┴────────────────────────────────────────┴──────────┘\n",
            "v1 → v2: 6 changes, 3 breaking                                              ",
        )
    );
}

#[test]
fn json_schema_and_ddl_compare_through_the_model() {
    let json = serde_json::json!({
        "title": "customers",
        "required": ["id", "email"],
        "properties": {
            "id": {"type": "integer"},
            "email": {"type": "string", "maxLength": 255}
        }
    });
    let ddl =
        sql::parse("CREATE TABLE customers (id INTEGER NOT NULL, email TEXT NOT NULL, name TEXT);")
            .unwrap()
            .schema;
    let diff = SchemaDiff::models(&json::to_model(&json), ddl.table("customers").unwrap());
    assert_eq!(
        plain(100, &diff),
        concat!(
            "┏━━━┳━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━┓\n",
            "┃   ┃ Where ┃ Change                ┃          ┃\n",
            "┡━━━╇━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━┩\n",
            "│ ~ │ email │ type string → TEXT    │ breaking │\n",
            "│ - │ email │ maxLength 255 removed │          │\n",
            "│ + │ name  │ field added (TEXT)    │          │\n",
            "└───┴───────┴───────────────────────┴──────────┘\n",
            "old → new: 3 changes, 1 breaking                ",
        )
    );
}

#[test]
fn a_timeline_marks_each_version() {
    let timeline = SchemaTimeline::new()
        .push("v1", sql::parse(SHOP_V1).unwrap().schema)
        .push("v2", sql::parse(SHOP_V2).unwrap().schema)
        .charset(Charset::Ascii);
    assert_eq!(
        plain(80, &timeline),
        concat!(
            "customers.id         #############################==============================\n",
            "customers.email      #############################==============================\n",
            "customers.name       #############################==============================\n",
            "orders.id            #############################==============================\n",
            "orders.customer_id   #############################==============================\n",
            "orders.status        #############################==============================\n",
            "orders.total         #############################==============================\n",
            "orders.placed_at                                  ##############################\n",
            "order_lines.order_id                              ##############################\n",
            "order_lines.line                                  ##############################\n",
            "order_lines.sku                                   ##############################\n",
            "                     * v1                         * v2: +2 ~4, 3 breaking       \n",
            "                     +----------------------------+----------------------------+\n",
            "                     0                            1                            2\n",
            "v1 → v2: 6 changes, 3 breaking\n",
            "├── ~ customers.name  became required  breaking\n",
            "├── ~ customers.email  type VARCHAR(255) → VARCHAR(320)  breaking\n",
            "├── ~ orders.status  default 'new' → 'pending'\n",
            "├── ~ orders.total  type NUMERIC(10, 2) → NUMERIC(12, 2)  breaking\n",
            "├── + orders.placed_at  field added (TIMESTAMP WITH TIME ZONE)\n",
            "└── + order_lines  table added (3 fields)",
        )
    );
}

#[test]
fn json_schema_versions_on_a_timeline() {
    let v1 = schema::parse(&fixture("order-v1.schema.json")).unwrap();
    let v2 = schema::parse(&fixture("order-v2.schema.json")).unwrap();
    let timeline = SchemaTimeline::new()
        .push_json("2024-01", v1)
        .at(0.0)
        .push_json("2024-06", v2)
        .at(5.0)
        .charset(Charset::Ascii);
    assert_eq!(plain(90, &timeline), concat!(
            "id       ########################################=========================================\n",
            "status   ########################################=========================================\n",
            "customer ########################################=========================================\n",
            "items    ########################################=========================================\n",
            "payment  ########################################=========================================\n",
            "notes    #########################################                                        \n",
            "currency                                         #########################################\n",
            "         * 2024-01                               * 2024-06: +4 -2 ~3, 5 breaking          \n",
            "         +-------+-------+-------+-------+-------+-------+-------+-------+-------+-------+\n",
            "         0       1       2       3       4       5       6       7       8       9      10\n",
            "2024-01 → 2024-06: 9 changes, 5 breaking\n",
            "├── ~ currency  became required  breaking\n",
            "├── - notes  property removed (string | null)  breaking\n",
            "├── + status  enum value \"refunded\" added\n",
            "├── + items  maxItems 100 added  breaking\n",
            "├── ~ items[].price  type number → string  breaking\n",
            "├── + items[].price  pattern /^[0-9]+\\.[0-9]{2}$/ added  breaking\n",
            "├── - items[].price  exclusiveMinimum 0 removed\n",
            "├── ~ payment  one of: 2 → 3 branches\n",
            "└── + currency  property added (string)",
        )
    );
}
