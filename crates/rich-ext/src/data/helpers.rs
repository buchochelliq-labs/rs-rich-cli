//! Zero-boilerplate printing for `Serialize` values: pretty JSON, a table of
//! records, or an explorer tree, each as a renderable or printed directly.

use rich::{Console, Json};
use serde::Serialize;

use super::{from_serialize, DataError, Explorer, Format, TableView};

/// `value` as core's highlighted [`Json`] renderable.
///
/// ```
/// use rich::Console;
///
/// #[derive(serde::Serialize)]
/// struct Point { x: i32, y: i32 }
///
/// let json = rich_ext::data::json(&Point { x: 1, y: 2 }).unwrap();
/// let out = Console::builder().width(40).build().render_export(&json);
/// assert_eq!(out, "{\n  \"x\": 1,\n  \"y\": 2\n}\n");
/// ```
pub fn json<T: Serialize + ?Sized>(value: &T) -> Result<Json, DataError> {
    let text = serde_json::to_string(value)
        .map_err(|e| DataError::new(Format::Json, e.to_string(), None))?;
    Json::new(&text).map_err(|e| DataError::new(Format::Json, e.to_string(), None))
}

/// `records` as a [`TableView`]: one row per record, one column per field.
/// Adjust it with the view's builder methods or
/// [`TableOptions`](super::TableOptions).
///
/// ```
/// use rich::Console;
///
/// #[derive(serde::Serialize)]
/// struct Server { name: &'static str, port: u16 }
///
/// let servers = [Server { name: "web", port: 80 }, Server { name: "db", port: 5432 }];
/// let view = rich_ext::data::table(&servers).unwrap().header("port", "Port");
/// let out = Console::builder().width(40).build().render_export(&view);
/// assert!(out.contains("┃ name ┃ Port ┃"), "{out}");
/// assert!(out.contains("│ db   │ 5432 │"), "{out}");
/// ```
pub fn table<T: Serialize>(records: &[T]) -> Result<TableView<'static>, DataError> {
    Ok(TableView::new(from_serialize(records)?))
}

/// `value` as an [`Explorer`] tree.
pub fn tree<T: Serialize + ?Sized>(value: &T) -> Result<Explorer<'static>, DataError> {
    Ok(Explorer::new(from_serialize(value)?))
}

/// Print `value` as highlighted JSON to stdout.
pub fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<(), DataError> {
    print_json_to(&Console::new(), value)
}

/// Print `value` as highlighted JSON to `console`.
pub fn print_json_to<T: Serialize + ?Sized>(console: &Console, value: &T) -> Result<(), DataError> {
    console.print(&json(value)?);
    Ok(())
}

/// Print `records` as a table to stdout.
pub fn print_table<T: Serialize>(records: &[T]) -> Result<(), DataError> {
    print_table_to(&Console::new(), records)
}

/// Print `records` as a table to `console`.
pub fn print_table_to<T: Serialize>(console: &Console, records: &[T]) -> Result<(), DataError> {
    console.print(&table(records)?);
    Ok(())
}

/// Print `value` as a tree to stdout.
pub fn print_tree<T: Serialize + ?Sized>(value: &T) -> Result<(), DataError> {
    print_tree_to(&Console::new(), value)
}

/// Print `value` as a tree to `console`.
pub fn print_tree_to<T: Serialize + ?Sized>(console: &Console, value: &T) -> Result<(), DataError> {
    console.print(&tree(value)?);
    Ok(())
}
