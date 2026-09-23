//! A serde `Serializer` that builds a [`Node`] directly, so `u64` values and
//! non-string map keys survive (a detour through `serde_json::Value` would
//! reject the keys).

use std::collections::HashMap;

use serde::ser::{self, Serialize};

use super::{DataError, Format, Node, Value};

impl ser::Error for DataError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        DataError::new(Format::Json, msg.to_string(), None)
    }
}

pub(crate) struct NodeSerializer;

fn node(value: Value) -> Result<Node, DataError> {
    Ok(Node::new(value))
}

/// A map key as a string: strings as-is, scalars as displayed, containers as
/// compact JSON.
fn key_string(key: Node) -> String {
    match key.value {
        Value::String(s) | Value::DateTime(s) => s,
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => rich::pyformat::float_repr(f),
        Value::Seq(_) | Value::Map(_) => key.to_json().to_string(),
    }
}

pub(crate) struct SeqBuilder {
    items: Vec<Node>,
    variant: Option<&'static str>,
}

pub(crate) struct MapBuilder {
    entries: Vec<(String, Node)>,
    /// Key → slot, so repeated keys stay linear.
    index: HashMap<String, usize>,
    key: Option<String>,
    variant: Option<&'static str>,
}

fn wrap(variant: Option<&'static str>, value: Value) -> Node {
    match variant {
        Some(name) => Node::new(Value::Map(vec![(name.to_string(), Node::new(value))])),
        None => Node::new(value),
    }
}

impl ser::Serializer for NodeSerializer {
    type Ok = Node;
    type Error = DataError;
    type SerializeSeq = SeqBuilder;
    type SerializeTuple = SeqBuilder;
    type SerializeTupleStruct = SeqBuilder;
    type SerializeTupleVariant = SeqBuilder;
    type SerializeMap = MapBuilder;
    type SerializeStruct = MapBuilder;
    type SerializeStructVariant = MapBuilder;

    fn serialize_bool(self, v: bool) -> Result<Node, DataError> {
        node(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Node, DataError> {
        node(Value::Int(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<Node, DataError> {
        node(Value::Int(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<Node, DataError> {
        node(Value::Int(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<Node, DataError> {
        node(Value::Int(v))
    }
    fn serialize_i128(self, v: i128) -> Result<Node, DataError> {
        if let Ok(i) = i64::try_from(v) {
            node(Value::Int(i))
        } else if let Ok(u) = u64::try_from(v) {
            node(Value::UInt(u))
        } else {
            node(Value::Float(v as f64))
        }
    }
    fn serialize_u8(self, v: u8) -> Result<Node, DataError> {
        node(Value::Int(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<Node, DataError> {
        node(Value::Int(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<Node, DataError> {
        node(Value::Int(v.into()))
    }
    fn serialize_u64(self, v: u64) -> Result<Node, DataError> {
        node(match i64::try_from(v) {
            Ok(i) => Value::Int(i),
            Err(_) => Value::UInt(v),
        })
    }
    fn serialize_u128(self, v: u128) -> Result<Node, DataError> {
        if let Ok(u) = u64::try_from(v) {
            self.serialize_u64(u)
        } else {
            node(Value::Float(v as f64))
        }
    }
    fn serialize_f32(self, v: f32) -> Result<Node, DataError> {
        node(Value::Float(v.into()))
    }
    fn serialize_f64(self, v: f64) -> Result<Node, DataError> {
        node(Value::Float(v))
    }
    fn serialize_char(self, v: char) -> Result<Node, DataError> {
        node(Value::String(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<Node, DataError> {
        node(Value::String(v.to_string()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Node, DataError> {
        node(Value::Seq(
            v.iter()
                .map(|b| Node::new(Value::Int((*b).into())))
                .collect(),
        ))
    }
    fn serialize_none(self) -> Result<Node, DataError> {
        node(Value::Null)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Node, DataError> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Node, DataError> {
        node(Value::Null)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Node, DataError> {
        node(Value::Null)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Node, DataError> {
        node(Value::String(variant.to_string()))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Node, DataError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Node, DataError> {
        let inner = value.serialize(NodeSerializer)?;
        node(Value::Map(vec![(variant.to_string(), inner)]))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqBuilder, DataError> {
        Ok(SeqBuilder {
            items: Vec::with_capacity(len.unwrap_or(0)),
            variant: None,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqBuilder, DataError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SeqBuilder, DataError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SeqBuilder, DataError> {
        Ok(SeqBuilder {
            items: Vec::with_capacity(len),
            variant: Some(variant),
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<MapBuilder, DataError> {
        Ok(MapBuilder {
            entries: Vec::with_capacity(len.unwrap_or(0)),
            index: HashMap::new(),
            key: None,
            variant: None,
        })
    }
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<MapBuilder, DataError> {
        self.serialize_map(Some(len))
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<MapBuilder, DataError> {
        Ok(MapBuilder {
            entries: Vec::with_capacity(len),
            index: HashMap::new(),
            key: None,
            variant: Some(variant),
        })
    }
}

impl SeqBuilder {
    fn push<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DataError> {
        self.items.push(value.serialize(NodeSerializer)?);
        Ok(())
    }
    fn finish(self) -> Result<Node, DataError> {
        Ok(wrap(self.variant, Value::Seq(self.items)))
    }
}

impl ser::SerializeSeq for SeqBuilder {
    type Ok = Node;
    type Error = DataError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DataError> {
        self.push(value)
    }
    fn end(self) -> Result<Node, DataError> {
        self.finish()
    }
}

impl ser::SerializeTuple for SeqBuilder {
    type Ok = Node;
    type Error = DataError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DataError> {
        self.push(value)
    }
    fn end(self) -> Result<Node, DataError> {
        self.finish()
    }
}

impl ser::SerializeTupleStruct for SeqBuilder {
    type Ok = Node;
    type Error = DataError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DataError> {
        self.push(value)
    }
    fn end(self) -> Result<Node, DataError> {
        self.finish()
    }
}

impl ser::SerializeTupleVariant for SeqBuilder {
    type Ok = Node;
    type Error = DataError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DataError> {
        self.push(value)
    }
    fn end(self) -> Result<Node, DataError> {
        self.finish()
    }
}

impl MapBuilder {
    /// Insert, letting a repeated key replace the earlier value in place.
    fn insert(&mut self, key: String, value: Node) {
        match self.index.get(&key) {
            Some(&slot) => self.entries[slot].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }
    fn finish(self) -> Result<Node, DataError> {
        Ok(wrap(self.variant, Value::Map(self.entries)))
    }
}

impl ser::SerializeMap for MapBuilder {
    type Ok = Node;
    type Error = DataError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), DataError> {
        self.key = Some(key_string(key.serialize(NodeSerializer)?));
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DataError> {
        let key = self
            .key
            .take()
            .ok_or_else(|| <DataError as ser::Error>::custom("map value without a key"))?;
        let value = value.serialize(NodeSerializer)?;
        self.insert(key, value);
        Ok(())
    }
    fn end(self) -> Result<Node, DataError> {
        self.finish()
    }
}

impl ser::SerializeStruct for MapBuilder {
    type Ok = Node;
    type Error = DataError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), DataError> {
        let value = value.serialize(NodeSerializer)?;
        self.insert(key.to_string(), value);
        Ok(())
    }
    fn end(self) -> Result<Node, DataError> {
        self.finish()
    }
}

impl ser::SerializeStructVariant for MapBuilder {
    type Ok = Node;
    type Error = DataError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), DataError> {
        let value = value.serialize(NodeSerializer)?;
        self.insert(key.to_string(), value);
        Ok(())
    }
    fn end(self) -> Result<Node, DataError> {
        self.finish()
    }
}
