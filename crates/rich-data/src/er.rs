//! ER diagrams from the schema model (#247), behind the `er` feature.
//!
//! [`model`] turns a [`Schema`] (from SQL DDL through
//! [`rich_ext::schema::sql`], from JSON Schema, Arrow or code) into
//! `rs-rich-diagram`'s [`ErModel`]: a box per table with its columns and
//! keys, and an edge per foreign key. [`from_sql`] does both steps for DDL.
//! The two crates do not depend on each other, so the bridge lives here.
//!
//! - A schema with tables gives an entity per table; one without gives a
//!   single entity, named after the schema (`schema` when it has no name).
//! - A column is a primary key when the table's key names it, a foreign key
//!   when a key starting from it does, unique when it is marked so, and
//!   nullable unless it is required.
//! - A foreign key is drawn from the referring table to the one it names,
//!   `N:1`, or `1:1` when its columns are the referring table's whole primary
//!   key or a unique column. A table named `users` matches `public.users`
//!   when that is the only table so named; otherwise the diagram notes that
//!   the table is missing.
//!
//! ```
//! use rich::Console;
//! use rich_data::er;
//!
//! let ddl = "CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE);
//!            CREATE TABLE posts (id INT PRIMARY KEY,
//!                                author INT NOT NULL REFERENCES users (id));";
//! let (diagram, notes) = er::from_sql(ddl).unwrap();
//! assert!(notes.is_empty());
//! let model = diagram.model();
//! assert_eq!(model.entities.len(), 2);
//! assert_eq!(model.relationships[0].from, "posts");
//! assert_eq!(model.relationships[0].to, "users");
//!
//! let console = Console::builder().width(80).color_system(None).build();
//! let out = console.render_to_string(&diagram);
//! assert!(out.contains("users") && out.contains("author"));
//! ```

use rich_diagram::er::{Cardinality, Column, Entity, Relationship};
use rich_diagram::{ErDiagram, ErModel};
use rich_ext::schema::sql::{self, Note, SqlError};
use rich_ext::schema::{ForeignKey, Schema};

/// The ER model of `schema`. See the [module docs](self).
pub fn model(schema: &Schema) -> ErModel {
    let tables: Vec<(String, &Schema)> = if schema.tables().is_empty() {
        vec![(schema.name().unwrap_or("schema").to_string(), schema)]
    } else {
        schema
            .tables()
            .iter()
            .enumerate()
            .map(|(index, table)| {
                let name = table
                    .name()
                    .map_or_else(|| format!("table {}", index + 1), str::to_string);
                (name, table)
            })
            .collect()
    };
    let names: Vec<&str> = tables.iter().map(|(name, _)| name.as_str()).collect();
    let mut er = ErModel::new();
    for (name, table) in &tables {
        let primary = table.primary_key();
        let keys = table.foreign_keys();
        let entity = Entity::new(name.as_str()).columns(table.fields().iter().map(|field| {
            let mut column = Column::new(field.name()).data_type(field.type_label());
            if primary.contains(&field.name()) {
                column = column.primary_key();
            }
            if keys
                .iter()
                .any(|key| key.columns.iter().any(|c| c == field.name()))
            {
                column = column.foreign_key();
            }
            if field.is_unique() {
                column = column.unique();
            }
            if !field.is_required() && !field.is_primary_key() {
                column = column.nullable();
            }
            column
        }));
        er = er.entity(entity);
        for key in &keys {
            er = er.relationship(relationship(name, table, key, &names));
        }
    }
    er
}

/// Read SQL DDL and draw it: the diagram, and the notes on what the reader
/// skipped. See [`rich_ext::schema::sql::parse`] for what it reads.
pub fn from_sql(ddl: &str) -> Result<(ErDiagram, Vec<Note>), SqlError> {
    let parsed = sql::parse(ddl)?;
    Ok((ErDiagram::new(model(&parsed.schema)), parsed.notes))
}

/// The edge for one foreign key of table `from`.
fn relationship(from: &str, table: &Schema, key: &ForeignKey, names: &[&str]) -> Relationship {
    let to = resolve(&key.table, names);
    let primary = table.primary_key();
    let one = (!primary.is_empty()
        && primary.len() == key.columns.len()
        && key.columns.iter().all(|c| primary.contains(&c.as_str())))
        || (key.columns.len() == 1
            && table
                .fields()
                .iter()
                .any(|f| f.name() == key.columns[0] && f.is_unique()));
    let mut edge = Relationship::new(from, to).cardinality(if one {
        Cardinality::OneToOne
    } else {
        Cardinality::ManyToOne
    });
    if !key.columns.is_empty() {
        edge = edge.columns(key.columns.clone(), key.references.clone());
    }
    edge
}

/// The table a key names: as written when a table has that name, else the
/// one table whose last dotted part matches it, else as written (and the
/// diagram notes that it is missing).
fn resolve(name: &str, names: &[&str]) -> String {
    if names.contains(&name) {
        return name.to_string();
    }
    let last = |n: &str| n.rsplit('.').next().unwrap_or(n).to_string();
    let wanted = last(name);
    let mut matches = names.iter().filter(|n| last(n) == wanted);
    match (matches.next(), matches.next()) {
        (Some(only), None) => (*only).to_string(),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_cardinality_come_from_the_ddl() {
        let (diagram, notes) = from_sql(
            "CREATE TABLE public.users (id INT PRIMARY KEY, email TEXT UNIQUE NOT NULL, bio TEXT);
             CREATE TABLE profiles (user_id INT PRIMARY KEY REFERENCES users (id));
             CREATE TABLE posts (id INT, author INT, PRIMARY KEY (id),
                                 FOREIGN KEY (author) REFERENCES public.users (id));
             CREATE INDEX posts_author ON posts (author);",
        )
        .unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
        let model = diagram.model();
        let users = &model.entities[0];
        assert_eq!(users.name, "public.users");
        assert!(users.columns[0].primary_key && !users.columns[0].nullable);
        assert!(users.columns[1].unique && !users.columns[1].nullable);
        assert!(users.columns[2].nullable);
        let edges: Vec<_> = model
            .relationships
            .iter()
            .map(|r| (r.from.as_str(), r.to.as_str(), r.cardinality))
            .collect();
        // `users` resolves to the one table so named, `public.users`.
        assert_eq!(
            edges,
            [
                ("profiles", "public.users", Some(Cardinality::OneToOne)),
                ("posts", "public.users", Some(Cardinality::ManyToOne)),
            ]
        );
        assert!(model.entities[2].columns[1].foreign_key);
    }

    #[test]
    fn a_schema_without_tables_is_one_entity() {
        let schema = Schema::new(vec![rich_ext::schema::Field::new(
            "id",
            rich_ext::schema::DataType::Integer,
        )]);
        let er = model(&schema);
        assert_eq!(er.entities.len(), 1);
        assert_eq!(er.entities[0].name, "schema");
        assert!(er.relationships.is_empty());
    }

    #[test]
    fn an_ambiguous_or_missing_table_is_kept_as_written() {
        assert_eq!(resolve("users", &["a.users", "b.users"]), "users");
        assert_eq!(resolve("ghost", &["users"]), "ghost");
        assert_eq!(resolve("users", &["users", "a.users"]), "users");
    }
}
