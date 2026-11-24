//! Content identity: the SHA-256 of a value's canonical JSON, so two
//! programs holding the same value name it alike, whatever their types.

use serde::{Deserialize, Serialize};

/// A lowercase SHA-256 digest naming content, refused when malformed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct ContentId(String);
impl ContentId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("content identity must be a lowercase SHA-256 digest".into());
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for ContentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for ContentId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// The identity of `value`: its JSON with object keys sorted and arrays
/// kept in order, so a typed value and the same value carried as plain
/// JSON digest alike.
pub fn content_digest<T: Serialize>(value: &T) -> Result<ContentId, serde_json::Error> {
    fn sorted(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();
                format!(
                    "{{{}}}",
                    keys.into_iter()
                        .map(|key| format!(
                            "{}:{}",
                            serde_json::Value::from(key.as_str()),
                            sorted(&map[key])
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
            serde_json::Value::Array(items) => format!(
                "[{}]",
                items.iter().map(sorted).collect::<Vec<_>>().join(",")
            ),
            other => other.to_string(),
        }
    }
    let value = serde_json::to_value(value)?;
    Ok(ContentId(crate::sha256(sorted(&value).as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Typed {
        zebra: u32,
        apple: Vec<&'static str>,
    }

    #[test]
    fn a_typed_value_and_its_plain_json_digest_alike() {
        let typed = Typed {
            zebra: 1,
            apple: vec!["b", "a"],
        };
        let plain = serde_json::json!({"apple": ["b", "a"], "zebra": 1});
        assert_eq!(
            content_digest(&typed).unwrap(),
            content_digest(&plain).unwrap()
        );
        let reordered = serde_json::json!({"apple": ["a", "b"], "zebra": 1});
        assert_ne!(
            content_digest(&typed).unwrap(),
            content_digest(&reordered).unwrap()
        );
    }

    #[test]
    fn a_malformed_identity_is_refused() {
        assert!(ContentId::new("A".repeat(64)).is_err());
        assert!(ContentId::new("a".repeat(63)).is_err());
        let id = ContentId::new("a".repeat(64)).unwrap();
        let read: ContentId = serde_json::from_str(&format!("\"{id}\"")).unwrap();
        assert_eq!(read, id);
        assert!(serde_json::from_str::<ContentId>("\"nope\"").is_err());
    }
}
