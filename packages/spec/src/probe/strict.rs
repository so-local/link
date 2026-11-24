//! Strict leaf inputs use TOML's real deserializer; example coercion stays separate.
use super::*;
#[derive(Clone, Copy)]
pub(super) enum LeafValue<'a> {
    Example(&'a serde_json::Value),
    Toml(&'a toml::Value),
}
impl LeafValue<'_> {
    pub(super) fn seed<'de, S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, Error> {
        match self {
            Self::Example(value) => seed.deserialize(Given(value)),
            Self::Toml(value) => seed.deserialize(value.clone()).map_err(de::Error::custom),
        }
    }
    pub(super) fn any<'de, V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Self::Example(value) => serde::Deserializer::deserialize_any(Given(value), visitor),
            Self::Toml(value) => serde::Deserializer::deserialize_any(value.clone(), visitor)
                .map_err(de::Error::custom),
        }
    }
    pub(super) fn enumeration<'de, V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self {
            Self::Example(value) => {
                serde::Deserializer::deserialize_enum(Given(value), name, variants, visitor)
            }
            Self::Toml(value) => {
                serde::Deserializer::deserialize_enum(value.clone(), name, variants, visitor)
                    .map_err(de::Error::custom)
            }
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LeafRead {
    Accepted,
    Refused(String),
    Unreached(String),
}
/// Only the leaf result supplies proof; surrounding completion and Kind/check do not.
pub(crate) fn read_strict_at<T: DeserializeOwned>(path: &str, value: &toml::Value) -> LeafRead {
    if path.is_empty() {
        return match T::deserialize(value.clone()) {
            Ok(_) => LeafRead::Accepted,
            Err(e) => LeafRead::Refused(e.to_string()),
        };
    }
    let path: Vec<String> = path.split('.').map(String::from).collect();
    // Serde may consume an unknown key with IgnoredAny. That is not a reached
    // owning field, even though its seed would accept any value.
    let trace = Trace::at(path.clone());
    let _ = T::deserialize(Probe {
        remaining: &path,
        seen: Rc::new(RefCell::new(None)),
        trace: trace.clone(),
    });
    if trace
        .get()
        .is_none_or(|request| request.method == "deserialize_ignored_any")
    {
        return LeafRead::Unreached(format!("no owning field request at {}", path.join(".")));
    }
    let outcome: Leaf = Rc::new(RefCell::new(None));
    let _ = T::deserialize(Reach {
        probe: Probe {
            remaining: &path,
            seen: Rc::new(RefCell::new(None)),
            trace: Trace::default(),
        },
        value: LeafValue::Toml(value),
        outcome: outcome.clone(),
    });
    match outcome.borrow_mut().take() {
        Some(Ok(())) => LeafRead::Accepted,
        Some(Err(error)) => LeafRead::Refused(error),
        None => LeafRead::Unreached(format!("the owning seed never reached {}", path.join("."))),
    }
}
