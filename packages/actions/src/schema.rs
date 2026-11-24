//! A shape as the catalog holds it: the subset of JSON Schema that maps
//! one for one to a type in Rust and TypeScript and to one form, with
//! every name resolved to the shape it means.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::names::TypeName;

/// One schema: what a value is, and what is said about it wherever it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    pub node: Node,
    pub meta: Meta,
}

/// What a value is.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// A named shape, declared once: `stage`, `box.id`.
    Named(TypeName),
    /// Named fields, closed to others.
    Record(Record),
    /// Keys of one text shape (any, unless it names one), each value of
    /// one shape.
    Map(Map),
    List(List),
    /// One of these values.
    Enum(Vec<Value>),
    /// Exactly this value.
    Const(Value),
    /// One of these records, told apart by the one property each holds
    /// as a `const`.
    OneOf(OneOf),
    String(Text),
    Integer(Bounds<i64>),
    Number(Bounds<f64>),
    Boolean,
    /// Any JSON value, as a document another program writes: `x-link-any
    /// = true`, JSON Schema's `{}`.
    Any,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub properties: BTreeMap<String, Schema>,
    pub required: BTreeSet<String>,
    /// Any other key's shape, for a record open to keys it doesn't name,
    /// as a tool table passing on a tool from outside Link: none for a
    /// closed one.
    pub others: Option<Box<Schema>>,
    /// `x-link-shorthand`: the property a bare value on the record's own
    /// flag sets, so `--engine v8` is `--engine.name v8`.
    pub shorthand: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    /// The text shape every key is (`propertyNames`): any text when none.
    pub names: Option<Box<Schema>>,
    pub values: Box<Schema>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub items: Box<Schema>,
    pub min_items: Option<u64>,
    pub max_items: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OneOf {
    /// How a value says which choice it is.
    pub told: Told,
    pub variants: Vec<Schema>,
}

/// How a choice is told apart.
#[derive(Clone, Debug, PartialEq)]
pub enum Told {
    /// By a tag: the property every choice holds as its `const`, the
    /// default for every shape.
    Tag(String),
    /// By a marking key, `x-link-markers`: each choice, in order, by the
    /// required key it alone has, where a tag would be clearly worse for
    /// the person writing the file.
    Markers(Vec<String>),
    /// By its value alone: a whole number or one of the words its choices
    /// name as a `const`, as a port is `8080` or `"*"`.
    Value,
}

/// How a path is resolved: from the file's folder or the current one,
/// and for a confined path never outside the spec's own folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Plain,
    Confined,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Text {
    pub format: Option<Format>,
    pub pattern: Option<String>,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
    /// `x-link-kind`: this string is an ID of the kind named.
    pub kind: Option<String>,
    /// `x-link-path`: this string is a path, `true` resolved from the
    /// file's folder or the current folder, `"confined"` also kept inside
    /// the spec's own folder.
    pub path: Option<PathKind>,
    /// `x-link-ref`: this string names a module, a command or a tool,
    /// read as the name it has today.
    pub reference: Option<link_files::schema::references::Reference>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bounds<T> {
    pub minimum: Option<T>,
    pub maximum: Option<T>,
}

impl<T> Default for Bounds<T> {
    fn default() -> Self {
        Self {
            minimum: None,
            maximum: None,
        }
    }
}

/// What is said about a value wherever its shape is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub description: Option<String>,
    pub default: Option<Value>,
    pub examples: Vec<Value>,
    /// `x-link-sensitive`: a secret's name or source, never logged or
    /// printed, and never given as a flag's value.
    pub sensitive: bool,
    pub display: Option<Display>,
    /// `x-link-unavailable`: why a run refuses a value given here, as an
    /// agent's grant under a sandbox run. The shape keeps the field, so
    /// one spec reads alike everywhere, and the run that can't honor it
    /// refuses it with this reason.
    pub unavailable: Option<String>,
    /// `x-link-summary`: a tool action's argument that says what its call
    /// works on, by its place among them, from 1: the call's title shows
    /// it there.
    pub summary: Option<u32>,
    /// `x-link-fill`: a tool action's text argument whose choices the
    /// program gives at runtime, by the fill's name in `[tools].fills`.
    pub fill: Option<String>,
    /// `x-link-asks`: a tool action's boolean argument whose call asks you
    /// first unless it's given false, whatever lets the call through: a
    /// recurring schedule, as the model's trained shape writes it with a
    /// flag.
    pub asks: bool,
    /// `x-link-values`: what each of an enum's words means, every word
    /// said once.
    pub values: BTreeMap<String, String>,
    /// `x-link-rust`: a part of a hidden argument's payload the format can't say
    /// yet, read with the named Rust type's own serde. Any value, as
    /// every surface shows it, on the shapes check's short list.
    pub rust: Option<String>,
}

/// `x-link-display`: how the formatter shows a shape when its verb's way
/// doesn't suit.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Display {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// A string's standard format, or Link's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    DateTime,
    Date,
    Time,
    Duration,
    /// A size, `16 MB` ([`crate::value::Size`]).
    Size,
    /// How many in each period, `10/s` ([`crate::value::Rate`]).
    Rate,
    Email,
    Hostname,
    Ipv4,
    Ipv6,
    Uri,
    UriReference,
    Uuid,
    Regex,
    /// A Link ID, `<kind>:<id>`, of the kind `x-link-kind` names.
    LinkId,
    /// A public key a secret is sealed to, X25519 in base64
    /// ([`link_secretive::SealTo`]).
    SealTo,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DateTime => "date-time",
            Self::Date => "date",
            Self::Time => "time",
            Self::Duration => "duration",
            Self::Size => "size",
            Self::Rate => "rate",
            Self::Email => "email",
            Self::Hostname => "hostname",
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
            Self::Uri => "uri",
            Self::UriReference => "uri-reference",
            Self::Uuid => "uuid",
            Self::Regex => "regex",
            Self::LinkId => "link-id",
            Self::SealTo => "seal-to",
        }
    }
}

impl Schema {
    pub fn new(node: Node) -> Self {
        Self {
            node,
            meta: Meta::default(),
        }
    }

    /// Every name this schema refers to, at any depth.
    pub fn names(&self) -> Vec<&TypeName> {
        let mut out = Vec::new();
        self.walk(&mut |schema| {
            if let Node::Named(name) = &schema.node {
                out.push(name);
            }
        });
        out
    }

    /// Calls `visit` on this schema and every schema inside it.
    pub fn walk<'a>(&'a self, visit: &mut impl FnMut(&'a Schema)) {
        visit(self);
        match &self.node {
            Node::Record(record) => {
                record.properties.values().for_each(|s| s.walk(visit));
                if let Some(others) = &record.others {
                    others.walk(visit);
                }
            }
            Node::Map(map) => {
                if let Some(keys) = &map.names {
                    keys.walk(visit);
                }
                map.values.walk(visit);
            }
            Node::List(list) => list.items.walk(visit),
            Node::OneOf(one) => one.variants.iter().for_each(|s| s.walk(visit)),
            _ => {}
        }
    }

    /// Calls `visit` on this schema and every schema inside it, to change
    /// them.
    pub fn walk_mut(&mut self, visit: &mut impl FnMut(&mut Schema)) {
        visit(self);
        match &mut self.node {
            Node::Record(record) => {
                record
                    .properties
                    .values_mut()
                    .for_each(|s| s.walk_mut(visit));
                if let Some(others) = &mut record.others {
                    others.walk_mut(visit);
                }
            }
            Node::Map(map) => {
                if let Some(keys) = &mut map.names {
                    keys.walk_mut(visit);
                }
                map.values.walk_mut(visit);
            }
            Node::List(list) => list.items.walk_mut(visit),
            Node::OneOf(one) => one.variants.iter_mut().for_each(|s| s.walk_mut(visit)),
            _ => {}
        }
    }
}
