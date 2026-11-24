//! A spec's fields as its reader takes them, found by probing the
//! owning `Deserialize` implementation rather than read from any
//! declaration: an independent check on the shape a kind declares
//! ([`crate::check_shape`]). Each probe follows one path and stops before
//! constructing a value.

mod requests;
mod strict;
use requests::Trace;
pub use requests::{Request, RequestKind};
use strict::LeafValue;
pub(crate) use strict::{LeafRead, read_strict_at};

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::rc::Rc;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A table: its type's serde name, its visitor's full type name,
    /// which tells apart types sharing a name, and its field names.
    Fields {
        name: String,
        identity: String,
        names: Vec<String>,
    },
    Map(String),
    Variants(Vec<String>),
    Scalar,
    Opaque,
}

#[derive(Debug)]
struct Error {
    message: String,
    fields: Option<Vec<String>>,
    scalar: bool,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self {
            message: msg.to_string(),
            fields: None,
            scalar: false,
        }
    }
    fn unknown_field(field: &str, expected: &'static [&'static str]) -> Self {
        Self {
            message: format!("unknown field {field}"),
            fields: Some(expected.iter().map(|s| (*s).into()).collect()),
            scalar: false,
        }
    }
    fn invalid_type(unexpected: de::Unexpected<'_>, expected: &dyn de::Expected) -> Self {
        Self {
            message: format!("invalid {unexpected}, expected {expected}"),
            fields: None,
            scalar: true,
        }
    }
}

type Seen = Rc<RefCell<Option<Shape>>>;

struct Probe<'a> {
    remaining: &'a [String],
    seen: Seen,
    trace: Trace,
}
impl Probe<'_> {
    fn stop<T>(self, shape: Shape) -> Result<T, Error> {
        *self.seen.borrow_mut() = Some(shape);
        Err(de::Error::custom("the field probe stopped"))
    }
}

struct Entry<'a> {
    probe: Probe<'a>,
    field: Option<String>,
}
impl<'de> MapAccess<'de> for Entry<'_> {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        self.field
            .take()
            .map(|field| seed.deserialize(field.into_deserializer()))
            .transpose()
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(Probe {
            remaining: &self.probe.remaining[1..],
            seen: self.probe.seen.clone(),
            trace: self.probe.trace.clone(),
        })
    }
}
struct Unknown {
    pending: bool,
    value_read: Rc<Cell<bool>>,
}
impl<'de> MapAccess<'de> for Unknown {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if std::mem::take(&mut self.pending) {
            seed.deserialize("__link_spec_field_probe__".into_deserializer())
                .map(Some)
        } else {
            Ok(None)
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        self.value_read.set(true);
        seed.deserialize(serde::de::value::UnitDeserializer::<Error>::new())
    }
}

struct Element<'a>(Option<Probe<'a>>);
impl<'de> SeqAccess<'de> for Element<'_> {
    type Error = Error;
    fn next_element_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<Option<V::Value>, Error> {
        self.0
            .take()
            .map(|probe| seed.deserialize(probe))
            .transpose()
    }
}

struct Variant<'a>(Probe<'a>);
impl<'de, 'a> EnumAccess<'de> for Variant<'a> {
    type Error = Error;
    type Variant = Self;
    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        self.0
            .trace
            .enums
            .borrow_mut()
            .push(self.0.remaining[0].clone());
        let value = seed.deserialize(self.0.remaining[0].clone().into_deserializer())?;
        Ok((
            value,
            Self(Probe {
                remaining: &self.0.remaining[1..],
                seen: self.0.seen,
                trace: self.0.trace,
            }),
        ))
    }
}
impl<'de> VariantAccess<'de> for Variant<'_> {
    type Error = Error;
    fn unit_variant(self) -> Result<(), Error> {
        self.0.stop(Shape::Scalar)
    }
    fn newtype_variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(self.0)
    }
    fn tuple_variant<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value, Error> {
        self.0.stop(Shape::Opaque)
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_struct(self.0, std::any::type_name::<V>(), fields, visitor)
    }
}

macro_rules! scalar {
    ($($name:ident => $kind:ident),*) => {$ (
        fn $name<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Error> { self.requested(RequestKind::$kind, stringify!($name)); self.stop(Shape::Scalar) }
    )*};
}
impl<'de> de::Deserializer<'de> for Probe<'_> {
    type Error = Error;
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.requested(RequestKind::Object, "deserialize_struct");
        if self.remaining.is_empty() {
            return self.stop(Shape::Fields {
                name: name.into(),
                identity: std::any::type_name::<V>().into(),
                names: fields.iter().map(|s| (*s).into()).collect(),
            });
        }
        visitor.visit_map(Entry {
            field: Some(self.remaining[0].clone()),
            probe: self,
        })
    }
    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.requested(RequestKind::Object, "deserialize_map");
        if self.remaining.is_empty() {
            return self.stop(Shape::Map(std::any::type_name::<V>().to_owned()));
        }
        visitor.visit_map(Entry {
            field: Some(FREE_KEY.into()),
            probe: self,
        })
    }
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.requested(RequestKind::Array, "deserialize_seq");
        visitor.visit_seq(Element(Some(self)))
    }
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.trace.some(self.remaining.len());
        visitor.visit_some(self)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.requested(RequestKind::Enum, "deserialize_enum");
        if self.remaining.is_empty() {
            return self.stop(Shape::Variants(
                variants.iter().map(|s| (*s).into()).collect(),
            ));
        }
        visitor.visit_enum(Variant(self))
    }
    fn deserialize_tuple<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value, Error> {
        self.stop(Shape::Opaque)
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: usize,
        _: V,
    ) -> Result<V::Value, Error> {
        self.stop(Shape::Opaque)
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: V,
    ) -> Result<V::Value, Error> {
        self.stop(Shape::Scalar)
    }
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.requested(RequestKind::Opaque, "deserialize_any");
        if !self.remaining.is_empty() {
            self.trace.opaque.set(true);
            return visitor.visit_map(Entry {
                field: Some(self.remaining[0].clone()),
                probe: self,
            });
        }
        let name = std::any::type_name::<V>().to_owned();
        let value_read = Rc::new(Cell::new(false));
        let outcome = visitor.visit_map(Unknown {
            pending: true,
            value_read: value_read.clone(),
        });
        let shape = match outcome {
            Err(Error {
                fields: Some(keys), ..
            }) if !value_read.get() => Shape::Fields {
                identity: name.clone(),
                name,
                names: keys,
            },
            Err(Error { scalar: true, .. }) if !value_read.get() => Shape::Scalar,
            _ => Shape::Opaque,
        };
        self.stop(shape)
    }
    scalar!(
        deserialize_bool => Boolean,
        deserialize_i8 => Integer,
        deserialize_i16 => Integer,
        deserialize_i32 => Integer,
        deserialize_i64 => Integer,
        deserialize_i128 => Integer,
        deserialize_u8 => Integer,
        deserialize_u16 => Integer,
        deserialize_u32 => Integer,
        deserialize_u64 => Integer,
        deserialize_u128 => Integer,
        deserialize_f32 => Number,
        deserialize_f64 => Number,
        deserialize_char => String,
        deserialize_str => String,
        deserialize_string => String,
        deserialize_bytes => Array,
        deserialize_byte_buf => Array,
        deserialize_unit => Opaque,
        deserialize_identifier => String,
        deserialize_ignored_any => Opaque
    );
}

/// The tables a reader takes, by dotted path (`""` for the top, `*` for a
/// free key, a variant's name for its fields): each with its sorted keys.
pub type Fields = BTreeMap<String, Vec<String>>;

/// The key a free-key map is probed with, a path's `*`.
const FREE_KEY: &str = "example";

/// One owning type binding can replace a custom deserializer at a path.
/// A scalar binding is explicit, so an opaque table is never silently dropped.
pub type Binding = fn() -> Result<Fields, String>;

/// How often a type met at several paths is expanded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expand {
    /// Its children once, at the first path met: what a reference lists.
    Once,
    /// At every path, stopping only where a type recurs inside itself:
    /// what a schema declares, path by path.
    Everywhere,
}

/// What a reader takes: its tables, and the paths it reads by hand,
/// where no probe sees inside, each with why.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub fields: Fields,
    pub opaque: BTreeMap<String, String>,
    pub requests: BTreeMap<String, Request>,
}

/// Every table `T`'s reader takes, found by probing it path by path
/// rather than read from a declaration. `overrides` answers for a path
/// whose reader is written by hand.
pub fn discover<T: DeserializeOwned>(
    overrides: &BTreeMap<String, Binding>,
    expand: Expand,
) -> Result<Found, String> {
    let mut found = Found::default();
    let mut pending = VecDeque::from([(Vec::<String>::new(), Vec::<String>::new())]);
    let mut expanded = BTreeSet::new();
    let mut probes = 0;
    while let Some((path, ancestors)) = pending.pop_front() {
        probes += 1;
        if found.fields.len() > crate::constants::MAX_TABLES
            || probes > crate::constants::MAX_PROBES
            || path.len() > crate::constants::MAX_DEPTH
        {
            return Err("field discovery exceeded its bounded traversal".into());
        }
        let at = path.join(".");
        if let Some(binding) = overrides.get(&at) {
            for (relative, fields) in binding()? {
                found.fields.insert(
                    [at.as_str(), relative.as_str()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("."),
                    fields,
                );
            }
            continue;
        }
        let seen = Rc::new(RefCell::new(None));
        let trace = Trace::at(path.clone());
        let outcome = T::deserialize(Probe {
            remaining: &path,
            seen: seen.clone(),
            trace: trace.clone(),
        });
        let shape = seen.borrow_mut().take().ok_or_else(|| {
            format!(
                "{} at {at}: introspection stopped without classifying its shape: {:?}",
                std::any::type_name::<T>(),
                outcome.err()
            )
        })?;
        if let Some(request) = trace.get() {
            found.requests.insert(at.clone(), request);
        }
        let push = |pending: &mut VecDeque<_>, key: String, ancestors: Vec<String>| {
            let mut next = path.clone();
            next.push(key);
            pending.push_back((next, ancestors));
        };
        match shape {
            Shape::Fields {
                name,
                identity,
                names: mut keys,
            } => {
                keys.sort();
                keys.dedup();
                found.fields.insert(at.clone(), keys.clone());
                // A recursive type has the same fields at every occurrence:
                // each is listed, its children expanded where `expand` says.
                let again = match expand {
                    Expand::Once => !expanded.insert(name),
                    Expand::Everywhere => ancestors.contains(&identity),
                };
                if !again {
                    let mut inner = ancestors.clone();
                    inner.push(identity);
                    for key in keys {
                        push(&mut pending, key, inner.clone());
                    }
                }
            }
            Shape::Map(name) => {
                // Serde's own maps; any other visitor is a reader by hand.
                if name.contains("MapVisitor") {
                    push(&mut pending, "*".into(), ancestors);
                } else {
                    found.opaque.insert(at, "its table is read by hand".into());
                }
            }
            Shape::Variants(variants) => {
                for variant in variants {
                    push(&mut pending, variant, ancestors.clone());
                }
            }
            Shape::Scalar => {}
            Shape::Opaque => {
                found.opaque.insert(at, "its value is read by hand".into());
            }
        }
    }
    Ok(found)
}

/// Whether `T`'s reader takes `value` at `path`, a path [`discover`]
/// names: the reader is followed down the path as for discovery, then
/// handed `value` there. What else the reader needs around it is never
/// asked for.
pub fn read_at<T: DeserializeOwned>(path: &str, value: &serde_json::Value) -> Result<(), String> {
    let path: Vec<String> = match path {
        "" => Vec::new(),
        _ => path.split('.').map(String::from).collect(),
    };
    let outcome: Leaf = Rc::new(RefCell::new(None));
    let seen = Rc::new(RefCell::new(None));
    let result = T::deserialize(Reach {
        probe: Probe {
            remaining: &path,
            seen,
            trace: Trace::default(),
        },
        value: LeafValue::Example(value),
        outcome: outcome.clone(),
    });
    match outcome.borrow_mut().take() {
        Some(read) => read,
        // A value at the top is the whole document.
        None if path.is_empty() => result.map(|_| ()).map_err(|e| e.message),
        None => Err(format!("the reader never reached {}", path.join("."))),
    }
}

/// What the reader made of the value it was handed.
type Leaf = Rc<RefCell<Option<Result<(), String>>>>;

/// A probe carrying a value to hand over where its path ends.
struct Reach<'a> {
    probe: Probe<'a>,
    value: LeafValue<'a>,
    outcome: Leaf,
}

impl<'a> Reach<'a> {
    fn next(&self, remaining: &'a [String]) -> Reach<'a> {
        Reach {
            probe: Probe {
                remaining,
                seen: self.probe.seen.clone(),
                trace: self.probe.trace.clone(),
            },
            value: self.value,
            outcome: self.outcome.clone(),
        }
    }
}

struct ReachEntry<'a> {
    reach: Reach<'a>,
    field: Option<String>,
}
impl<'de> MapAccess<'de> for ReachEntry<'_> {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        self.field
            .take()
            .map(|field| seed.deserialize(field.into_deserializer()))
            .transpose()
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let remaining = &self.reach.probe.remaining[1..];
        if remaining.is_empty() {
            let read = self.reach.value.seed(seed);
            let ended = read.as_ref().map(|_| ()).map_err(|e| e.message.clone());
            *self.reach.outcome.borrow_mut() = Some(ended);
            return read;
        }
        seed.deserialize(self.reach.next(remaining))
    }
}

struct ReachElement<'a>(Option<Reach<'a>>);
impl<'de> SeqAccess<'de> for ReachElement<'_> {
    type Error = Error;
    fn next_element_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<Option<V::Value>, Error> {
        self.0
            .take()
            .map(|reach| seed.deserialize(reach))
            .transpose()
    }
}

struct ReachVariant<'a>(Reach<'a>);
impl<'de> EnumAccess<'de> for ReachVariant<'_> {
    type Error = Error;
    type Variant = Self;
    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        let reach = &self.0;
        let value = seed.deserialize(reach.probe.remaining[0].clone().into_deserializer())?;
        let rest = reach.next(&reach.probe.remaining[1..]);
        Ok((value, Self(rest)))
    }
}
impl<'de> VariantAccess<'de> for ReachVariant<'_> {
    type Error = Error;
    fn unit_variant(self) -> Result<(), Error> {
        Err(de::Error::custom("a unit variant holds no value"))
    }
    fn newtype_variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<V::Value, Error> {
        if self.0.probe.remaining.is_empty() {
            let read = self.0.value.seed(seed);
            *self.0.outcome.borrow_mut() =
                Some(read.as_ref().map(|_| ()).map_err(|e| e.message.clone()));
            return read;
        }
        seed.deserialize(self.0)
    }
    fn tuple_variant<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value, Error> {
        Err(de::Error::custom("a tuple variant is no path"))
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_struct(self.0, "variant", fields, visitor)
    }
}

impl<'de> de::Deserializer<'de> for Reach<'_> {
    type Error = Error;
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if self.probe.remaining.is_empty() {
            return self.value.any(visitor);
        }
        // A free key is reached by the key discovery names it by.
        let key = match self.probe.remaining[0].as_str() {
            "*" => FREE_KEY.to_string(),
            key => key.to_string(),
        };
        visitor.visit_map(ReachEntry {
            field: Some(key),
            reach: self,
        })
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_any(visitor)
    }
    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_any(visitor)
    }
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_seq(ReachElement(Some(self)))
    }
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        if self.probe.remaining.is_empty() {
            return self.value.enumeration(name, variants, visitor);
        }
        visitor.visit_enum(ReachVariant(self))
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct tuple tuple_struct identifier ignored_any
    }
}

/// A value handed to a reader as a TOML file would give it: never
/// null, and one element standing for a list of it.
#[derive(Clone, Copy)]
struct Given<'a>(&'a serde_json::Value);

fn json(error: serde_json::Error) -> Error {
    de::Error::custom(error)
}

impl<'de> de::Deserializer<'de> for Given<'_> {
    type Error = Error;
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.0.clone().deserialize_any(visitor).map_err(json)
    }
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            serde_json::Value::Array(_) => self.deserialize_any(visitor),
            _ => visitor.visit_seq(One(Some(self))),
        }
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.0
            .clone()
            .deserialize_enum(name, variants, visitor)
            .map_err(json)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct tuple tuple_struct map struct identifier
        ignored_any
    }
}

/// A list of the one value given.
struct One<'a>(Option<Given<'a>>);
impl<'de> SeqAccess<'de> for One<'_> {
    type Error = Error;
    fn next_element_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<Option<V::Value>, Error> {
        self.0
            .take()
            .map(|given| seed.deserialize(given))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Root {
        #[expect(
            dead_code,
            reason = "only deserialized, for the fields discovery finds"
        )]
        nested: Option<Nested>,
    }
    #[derive(Deserialize)]
    struct Nested {
        #[expect(
            dead_code,
            reason = "only deserialized, for the fields discovery finds"
        )]
        added_later: bool,
    }
    #[test]
    fn a_new_nested_field_is_discovered_without_a_second_field_list() {
        let found = discover::<Root>(&BTreeMap::new(), Expand::Once)
            .unwrap()
            .fields;
        assert_eq!(found["nested"], ["added_later"]);
    }
    #[derive(Deserialize)]
    enum Choice {
        Old,
        Added {
            #[expect(
                dead_code,
                reason = "only deserialized, for the fields discovery finds"
            )]
            nested: Nested,
        },
    }
    #[test]
    fn enum_struct_variants_are_walked_instead_of_treated_as_scalars() {
        let found = discover::<Choice>(&BTreeMap::new(), Expand::Once)
            .unwrap()
            .fields;
        assert_eq!(found["Added.nested"], ["added_later"]);
    }
    #[derive(Deserialize)]
    struct Custom {
        #[expect(
            dead_code,
            reason = "only deserialized, for the fields discovery finds"
        )]
        value: serde_json::Value,
    }
    #[test]
    fn opaque_values_do_not_silently_look_like_complete_discovery() {
        let found = discover::<Custom>(&BTreeMap::new(), Expand::Once).unwrap();
        assert_eq!(found.opaque.keys().collect::<Vec<_>>(), ["value"]);
    }
}
