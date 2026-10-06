//! Entity-relationship diagrams (#247): tables, their columns and keys, and
//! the relationships between them, drawn through the layered layout.
//!
//! [`ErModel`] is a small, format-neutral model built in code: plain public
//! structs with chaining constructors, so a reader of SQL DDL or of another
//! schema model can fill it without this crate knowing that format.
//! [`ErDiagram`] draws it:
//!
//! - each [`Entity`] as a table box: its name as a centred header, ruled off
//!   from one row per [`Column`] (`name  type  keys`, aligned);
//! - each [`Relationship`] as an edge from the referencing entity to the
//!   referenced one, an arrow at the referenced end, labelled with its
//!   columns (`user_id → id`) and [`Cardinality`] (`N:1`);
//! - each [`Group`] as a cluster frame around its entities.
//!
//! In a column row, `PK`, `FK` and `UQ` mark a primary key, a foreign key and
//! a unique column, and `?` after the type marks a nullable one.
//!
//! ```
//! use rich::Console;
//! use rich_diagram::er::{Cardinality, Column, Entity, ErDiagram, ErModel, Relationship};
//!
//! let model = ErModel::new()
//!     .entity(
//!         Entity::new("users")
//!             .column(Column::new("id").data_type("int").primary_key())
//!             .column(Column::new("email").data_type("text").unique()),
//!     )
//!     .entity(
//!         Entity::new("orders")
//!             .column(Column::new("id").data_type("int").primary_key())
//!             .column(Column::new("user_id").data_type("int").foreign_key())
//!             .column(Column::new("note").data_type("text").nullable()),
//!     )
//!     .relationship(
//!         Relationship::new("orders", "users")
//!             .columns(["user_id"], ["id"])
//!             .cardinality(Cardinality::ManyToOne),
//!     );
//! let console = Console::builder().width(80).color_system(None).build();
//! let out = console.render_export(&ErDiagram::new(model));
//! assert_eq!(
//!     out,
//!     "┌────────────────────┐                       ┌─────────────────┐\n\
//!      │       orders       │                       │      users      │\n\
//!      ├────────────────────┤                       ├─────────────────┤\n\
//!      │ id       int    PK │ ┌─user_id → id (N:1)─►│ id     int   PK │\n\
//!      │ user_id  int    FK ├─┘                     │ email  text  UQ │\n\
//!      │ note     text?     │                       └─────────────────┘\n\
//!      └────────────────────┘\n"
//! );
//! ```

use std::collections::HashMap;
use std::sync::OnceLock;

use rich::cells::cell_len;
use rich::console::{Console, ConsoleOptions};
use rich::measure::Measurement;
use rich::protocol::Renderable;
use rich::segment::Segment;

use crate::dot::{prefixed_note, trim_final_newline};
use crate::graph::{Cluster, Direction, Edge, Graph, Head, Node, Shape};
use crate::layout::{draw, DrawError, Drawing};

/// A column of an [`Entity`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// Its type as the source spells it (`int`, `varchar(80)`), if known.
    pub data_type: Option<String>,
    /// Part of the primary key: marked `PK`.
    pub primary_key: bool,
    /// Part of a foreign key: marked `FK`.
    pub foreign_key: bool,
    /// Unique: marked `UQ`.
    pub unique: bool,
    /// May be null: `?` after the type.
    pub nullable: bool,
}

impl Column {
    /// A column with no type and no markers.
    pub fn new(name: impl Into<String>) -> Self {
        Column {
            name: name.into(),
            ..Column::default()
        }
    }

    /// Set the type.
    pub fn data_type(mut self, data_type: impl Into<String>) -> Self {
        self.data_type = Some(data_type.into());
        self
    }

    /// Mark it part of the primary key.
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self
    }

    /// Mark it part of a foreign key.
    pub fn foreign_key(mut self) -> Self {
        self.foreign_key = true;
        self
    }

    /// Mark it unique.
    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }

    /// Mark it nullable.
    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    /// `PK`, `FK` and `UQ`, as they apply, space-separated.
    pub fn markers(&self) -> String {
        let marks = [
            (self.primary_key, "PK"),
            (self.foreign_key, "FK"),
            (self.unique, "UQ"),
        ];
        marks
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, mark)| *mark)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A table (or any entity) and its columns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entity {
    /// What relationships and groups refer to it by, and its header.
    pub name: String,
    pub columns: Vec<Column>,
}

impl Entity {
    /// An entity with no columns.
    pub fn new(name: impl Into<String>) -> Self {
        Entity {
            name: name.into(),
            columns: Vec::new(),
        }
    }

    /// Add a column.
    pub fn column(mut self, column: Column) -> Self {
        self.columns.push(column);
        self
    }

    /// Add columns.
    pub fn columns(mut self, columns: impl IntoIterator<Item = Column>) -> Self {
        self.columns.extend(columns);
        self
    }
}

/// How many rows on each side of a [`Relationship`] relate, from its `from`
/// entity to its `to` entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cardinality {
    /// `1:1`.
    OneToOne,
    /// `1:N`.
    OneToMany,
    /// `N:1`: a foreign key's usual reading, many referencing rows to one
    /// referenced row.
    ManyToOne,
    /// `N:M`.
    ManyToMany,
}

impl Cardinality {
    /// `1:1`, `1:N`, `N:1` or `N:M`.
    pub fn notation(self) -> &'static str {
        match self {
            Cardinality::OneToOne => "1:1",
            Cardinality::OneToMany => "1:N",
            Cardinality::ManyToOne => "N:1",
            Cardinality::ManyToMany => "N:M",
        }
    }
}

/// A relationship from the `from` entity's columns to the `to` entity's (a
/// foreign key reads `from` the referencing table `to` the referenced one).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relationship {
    pub from: String,
    /// The `from` entity's columns, in order; empty when not known.
    pub from_columns: Vec<String>,
    pub to: String,
    /// The `to` entity's columns, in order; empty when not known.
    pub to_columns: Vec<String>,
    pub cardinality: Option<Cardinality>,
    /// Text in place of the columns (the cardinality still follows).
    pub label: Option<String>,
}

impl Relationship {
    /// A relationship from entity `from` to entity `to`, columns unknown.
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Relationship {
            from: from.into(),
            to: to.into(),
            ..Relationship::default()
        }
    }

    /// Set the columns at each end (more than one for a composite key).
    pub fn columns<F, T>(mut self, from: F, to: T) -> Self
    where
        F: IntoIterator,
        F::Item: Into<String>,
        T: IntoIterator,
        T::Item: Into<String>,
    {
        self.from_columns = from.into_iter().map(Into::into).collect();
        self.to_columns = to.into_iter().map(Into::into).collect();
        self
    }

    /// Set the cardinality.
    pub fn cardinality(mut self, cardinality: Cardinality) -> Self {
        self.cardinality = Some(cardinality);
        self
    }

    /// Set the label, drawn in place of the columns.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The edge's text: the label or the columns, then the cardinality.
    fn text(&self, ascii: bool) -> Option<String> {
        let arrow = if ascii { "->" } else { "→" };
        let columns = match (
            &self.label,
            self.from_columns.is_empty(),
            self.to_columns.is_empty(),
        ) {
            (Some(label), _, _) => label.clone(),
            (None, true, true) => String::new(),
            (None, false, true) => self.from_columns.join(", "),
            (None, true, false) => format!("{arrow} {}", self.to_columns.join(", ")),
            (None, false, false) => format!(
                "{} {arrow} {}",
                self.from_columns.join(", "),
                self.to_columns.join(", ")
            ),
        };
        let text = match (columns.is_empty(), self.cardinality) {
            (true, None) => return None,
            (true, Some(c)) => c.notation().to_string(),
            (false, None) => columns,
            (false, Some(c)) => format!("{columns} ({})", c.notation()),
        };
        Some(text)
    }
}

/// Entities drawn in one frame, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    /// The frame's label.
    pub label: String,
    pub entities: Vec<String>,
}

impl Group {
    pub fn new<I, S>(label: impl Into<String>, entities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Group {
            label: label.into(),
            entities: entities.into_iter().map(Into::into).collect(),
        }
    }
}

/// Entities, the relationships between them, and groups of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErModel {
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    pub groups: Vec<Group>,
}

impl ErModel {
    /// An empty model.
    pub fn new() -> Self {
        ErModel::default()
    }

    /// Add an entity.
    pub fn entity(mut self, entity: Entity) -> Self {
        self.entities.push(entity);
        self
    }

    /// Add a relationship.
    pub fn relationship(mut self, relationship: Relationship) -> Self {
        self.relationships.push(relationship);
        self
    }

    /// Add a group.
    pub fn group(mut self, group: Group) -> Self {
        self.groups.push(group);
        self
    }

    /// The model as a [`Graph`] flowing in `direction`, and a note for each
    /// thing it could not include: a repeated entity name (the first is
    /// drawn), and a relationship, group member or relationship column that
    /// names something that does not exist (the relationship and the member
    /// are left out; an unknown column is drawn as given). With `ascii`,
    /// relationship labels use `->` for `→`.
    pub fn to_graph(&self, direction: Direction, ascii: bool) -> (Graph, Vec<String>) {
        let mut notes = Vec::new();
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut nodes = Vec::new();
        let mut drawn: Vec<&Entity> = Vec::new();
        for entity in &self.entities {
            if index.contains_key(entity.name.as_str()) {
                notes.push(format!(
                    "entity `{}` is defined more than once; the first is drawn",
                    entity.name
                ));
                continue;
            }
            index.insert(&entity.name, nodes.len());
            nodes.push(Node::new(entity.name.clone(), table_label(entity)).shape(Shape::Table));
            drawn.push(entity);
        }
        let mut edges = Vec::new();
        for relationship in &self.relationships {
            let ends = (
                index.get(relationship.from.as_str()),
                index.get(relationship.to.as_str()),
            );
            let (Some(&from), Some(&to)) = ends else {
                let missing = if ends.0.is_none() {
                    &relationship.from
                } else {
                    &relationship.to
                };
                notes.push(format!(
                    "the relationship {} → {} is not drawn: there is no entity `{missing}`",
                    relationship.from, relationship.to
                ));
                continue;
            };
            for (entity, columns) in [
                (from, &relationship.from_columns),
                (to, &relationship.to_columns),
            ] {
                for column in columns {
                    if !drawn[entity].columns.iter().any(|c| &c.name == column) {
                        notes.push(format!(
                            "entity `{}` has no column `{column}`",
                            drawn[entity].name
                        ));
                    }
                }
            }
            let mut edge = Edge::new(from, to);
            edge.label = relationship.text(ascii);
            edge.start = Head::None;
            edge.end = Head::Arrow;
            edges.push(edge);
        }
        let mut graph = Graph::from_parts(direction, nodes, edges);
        for (number, group) in self.groups.iter().enumerate() {
            let mut members = Vec::new();
            for name in &group.entities {
                match index.get(name.as_str()) {
                    Some(&i) => members.push(i),
                    None => notes.push(format!(
                        "group `{}` names entity `{name}`, which does not exist",
                        group.label
                    )),
                }
            }
            graph.add_cluster(
                Cluster::new(format!("group{number}"))
                    .label(group.label.clone())
                    .nodes(members),
            );
        }
        (graph, notes)
    }
}

/// An entity's box text: its name, then one aligned row per column.
fn table_label(entity: &Entity) -> String {
    // Line breaks would split a row; the layout scrubs control characters.
    let flat = |text: &str| text.replace(['\r', '\n'], " ");
    let rows: Vec<[String; 3]> = entity
        .columns
        .iter()
        .map(|column| {
            let mut data_type = column.data_type.as_deref().map(flat).unwrap_or_default();
            if column.nullable {
                data_type.push('?');
            }
            [flat(&column.name), data_type, column.markers()]
        })
        .collect();
    let width = |i: usize| rows.iter().map(|row| cell_len(&row[i])).max().unwrap_or(0);
    let widths = [width(0), width(1), width(2)];
    let mut lines = vec![flat(&entity.name)];
    for row in &rows {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            if widths[i] == 0 {
                continue;
            }
            if !line.is_empty() {
                line.push_str("  ");
            }
            line.push_str(cell);
            line.push_str(&" ".repeat(widths[i] - cell_len(cell)));
        }
        lines.push(line.trim_end().to_string());
    }
    // Rows pad to one width, so the box's rows line up with the header.
    let widest = lines[1..].iter().map(|l| cell_len(l)).max().unwrap_or(0);
    for line in &mut lines[1..] {
        let pad = widest - cell_len(line);
        line.push_str(&" ".repeat(pad));
    }
    lines.join("\n")
}

/// The notes on a model, and its drawing.
type Drawn = (Vec<String>, Result<Drawing, DrawError>);

/// An [`ErModel`] as a renderable: the drawing, cropped to the width it is
/// given like [`Diagram`](crate::Diagram), then a dim `ER:` note for each
/// thing it could not draw.
///
/// Entities flow left to right by default ([`ErDiagram::direction`]):
/// referencing tables before the tables they reference.
#[derive(Clone, Debug)]
pub struct ErDiagram {
    model: ErModel,
    direction: Direction,
    ascii: Option<bool>,
    /// The Unicode and ASCII graphs and drawings, made on first use.
    drawn: [OnceLock<Drawn>; 2],
}

impl ErDiagram {
    pub fn new(model: ErModel) -> Self {
        ErDiagram {
            model,
            direction: Direction::LeftRight,
            ascii: None,
            drawn: Default::default(),
        }
    }

    /// Set the direction relationships flow in.
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self.drawn = Default::default();
        self
    }

    /// Draw with ASCII only (`true`) or box drawing (`false`), whatever the
    /// console's encoding.
    pub fn ascii(mut self, ascii: bool) -> Self {
        self.ascii = Some(ascii);
        self
    }

    pub fn model(&self) -> &ErModel {
        &self.model
    }

    /// The notes on the model and the whole drawing, uncropped.
    fn drawn(&self, ascii: bool) -> &Drawn {
        self.drawn[usize::from(ascii)].get_or_init(|| {
            let (graph, notes) = self.model.to_graph(self.direction, ascii);
            (notes, draw(&graph, ascii))
        })
    }

    /// The whole drawing, uncropped.
    pub fn drawing(&self, ascii: bool) -> Result<&Drawing, &DrawError> {
        self.drawn(ascii).1.as_ref()
    }

    fn ascii_for(&self, console: &Console) -> bool {
        self.ascii.unwrap_or_else(|| console.ascii_only())
    }
}

impl From<ErModel> for ErDiagram {
    fn from(model: ErModel) -> Self {
        ErDiagram::new(model)
    }
}

impl Renderable for ErDiagram {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let ascii = self.ascii_for(console);
        let (model_notes, drawing) = self.drawn(ascii);
        let mut segments = Vec::new();
        let mut notes = model_notes.clone();
        match drawing {
            Ok(drawing) => {
                for line in drawing.cropped(options.max_width) {
                    segments.push(Segment::new(line, None));
                    segments.push(Segment::line());
                }
                notes.extend(drawing.notes.iter().cloned());
                if drawing.width > options.max_width {
                    notes.push(format!(
                        "cropped to {} of {} columns",
                        options.max_width, drawing.width
                    ));
                }
            }
            Err(error) => notes.push(format!("too large to draw: {error}")),
        }
        for note in notes {
            segments.extend(prefixed_note("ER", &note, ascii, console, options));
        }
        trim_final_newline(segments)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        match self.drawing(self.ascii_for(console)) {
            Ok(drawing) => Measurement::new(drawing.width, drawing.width),
            Err(_) => Measurement::new(options.max_width, options.max_width),
        }
    }
}
