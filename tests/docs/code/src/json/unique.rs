//! Reject duplicate keys even inside extensible metadata and map-valued fields.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::{collections::BTreeSet, fmt};

struct Unique;

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Keys;
        impl<'de> Visitor<'de> for Keys {
            type Value = Unique;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<Unique, A::Error> {
                while values.next_element::<Unique>()?.is_some() {}
                Ok(Unique)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<Unique, A::Error> {
                let mut seen = BTreeSet::new();
                while let Some(key) = values.next_key::<String>()? {
                    if !seen.insert(key.clone()) {
                        return Err(de::Error::custom(format!("duplicate JSON key {key:?}")));
                    }
                    values.next_value::<Unique>()?;
                }
                Ok(Unique)
            }
        }
        deserializer.deserialize_any(Keys)
    }
}

pub(super) fn check(text: &str) -> Result<(), String> {
    serde_json::from_str::<Unique>(text)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_maps_cannot_hide_repeated_or_escaped_duplicate_fields() {
        assert!(check(r#"{"meta":[{"key":true,"key":null}]}"#).is_err());
        assert!(check(r#"{"meta":{"key":1,"\u006bey":2}}"#).is_err());
        assert!(check(r#"{"meta":[{"key":true},{"key":null}],"numbers":[1,-1,1.5]}"#).is_ok());
    }
}
