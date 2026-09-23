//! TOML through the `toml` crate's span-aware `DeTable`. Its tables are not
//! insertion-ordered without the `preserve_order` feature (which would leak
//! into every other `toml` user in the build), so entries are put back in
//! document order by their keys' spans.

use toml::de::{DeTable, DeValue};
use toml::Spanned;

use super::{DataError, Format, LineIndex, Meta, Node, Value};

struct Converter<'a> {
    source: &'a str,
    lines: LineIndex<'a>,
}

impl Converter<'_> {
    fn table(&self, table: &DeTable<'_>) -> Result<Value, DataError> {
        let mut entries: Vec<(&Spanned<_>, &Spanned<DeValue<'_>>)> = table.iter().collect();
        entries.sort_by_key(|(key, _)| key.span().start);
        let mut out = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let mut node = self.value(value)?;
            node.meta.position = Some(self.lines.position(key.span().start));
            out.push((key.get_ref().to_string(), node));
        }
        Ok(Value::Map(out))
    }

    fn value(&self, value: &Spanned<DeValue<'_>>) -> Result<Node, DataError> {
        let span = value.span();
        let position = Some(self.lines.position(span.start));
        let converted = match value.get_ref() {
            DeValue::String(s) => Value::String(s.to_string()),
            DeValue::Integer(i) => i64::from_str_radix(i.as_str(), i.radix())
                .map(Value::Int)
                .or_else(|_| u64::from_str_radix(i.as_str(), i.radix()).map(Value::UInt))
                .map_err(|_| DataError::new(Format::Toml, "integer out of range", position))?,
            DeValue::Float(f) => Value::Float(
                f.as_str()
                    .parse()
                    .map_err(|_| DataError::new(Format::Toml, "invalid float", position))?,
            ),
            DeValue::Boolean(b) => Value::Bool(*b),
            DeValue::Datetime(d) => Value::DateTime(
                self.source
                    .get(span.clone())
                    .map_or_else(|| d.to_string(), str::to_string),
            ),
            DeValue::Array(items) => Value::Seq(
                items
                    .iter()
                    .map(|item| self.value(item))
                    .collect::<Result<_, _>>()?,
            ),
            DeValue::Table(table) => self.table(table)?,
        };
        Ok(Node::with_meta(
            converted,
            Meta {
                position,
                ..Meta::default()
            },
        ))
    }
}

pub(crate) fn parse(content: &str) -> Result<Node, DataError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let converter = Converter {
        source: content,
        lines: LineIndex::new(content),
    };
    match DeTable::parse(content) {
        Ok(table) => Ok(Node::new(converter.table(table.get_ref())?)),
        Err(error) => Err(DataError::new(
            Format::Toml,
            error.message().trim_end().to_string(),
            error
                .span()
                .map(|span| converter.lines.position(span.start)),
        )),
    }
}

/// Detection: parses and defines at least one key or table.
pub(crate) fn looks_like(content: &str) -> bool {
    parse(content).is_ok_and(|node| !node.is_empty())
}
