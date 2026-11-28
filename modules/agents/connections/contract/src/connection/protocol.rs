//! The connection protocol's version, on every line a connection module
//! reads that carries a [`super::Bridge`] and every answer of `connect`,
//! `describe` and `rule`: each states `protocol`, this release's
//! [`PROTOCOL`], beside its own keys ([`stated`], [`Stated`]), and its
//! reader takes the version out first, refuses one it doesn't read, then
//! reads the rest, refusing a key it doesn't know ([`read`]).

use anyhow::Context;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The key a line states its version in.
pub const VERSION_KEY: &str = "protocol";

/// This protocol, as `protocol.toml` declares it.
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> =
    std::sync::LazyLock::new(|| link_version::Protocol::shipped(include_str!("protocol.toml")));

/// A value as the protocol carries it: its own keys, and `protocol`.
#[derive(Serialize)]
pub struct Stated<'a, T: Serialize> {
    protocol: u32,
    #[serde(flatten)]
    value: &'a T,
}

impl<'a, T: Serialize> Stated<'a, T> {
    /// `value`, stating this release's version.
    pub fn new(value: &'a T) -> Self {
        Self {
            protocol: PROTOCOL.current,
            value,
        }
    }
}

/// `value` as one line of the protocol, ending in a newline.
pub fn stated(value: &impl Serialize) -> anyhow::Result<String> {
    let text = serde_json::to_string(&Stated::new(value)).context("writing a connection line")?;
    Ok(format!("{text}\n"))
}

/// The `T` that `peer` (as a person names it: `lnk-telegram`) wrote in
/// `text`: an error unless it states a version this release reads, and
/// holds nothing else `T` doesn't name.
pub fn read<T: DeserializeOwned>(peer: &str, text: &[u8]) -> anyhow::Result<T> {
    let mut object: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(text).with_context(|| format!("reading what {peer} said"))?;
    let version = object
        .remove(VERSION_KEY)
        .with_context(|| {
            format!(
                "{peer} states no {} protocol version: {}",
                PROTOCOL.name, PROTOCOL.upgrade
            )
        })?
        .as_u64()
        .and_then(|version| u32::try_from(version).ok())
        .with_context(|| format!("{peer}'s {VERSION_KEY} isn't a version"))?;
    PROTOCOL.read(peer, version)?;
    serde_json::from_value(serde_json::Value::Object(object))
        .with_context(|| format!("reading what {peer} said"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Said {
        text: String,
    }

    fn said() -> Said {
        Said { text: "hi".into() }
    }

    #[test]
    fn a_stated_line_is_read_back() {
        let line = stated(&said()).unwrap();
        assert!(line.ends_with('\n'));
        let json: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(json[VERSION_KEY], PROTOCOL.current);
        assert_eq!(read::<Said>("lnk-x", line.as_bytes()).unwrap(), said());
    }

    #[test]
    fn no_version_an_unread_one_and_an_unknown_key_are_refused() {
        let why = |text: &str| {
            read::<Said>("lnk-x", text.as_bytes())
                .unwrap_err()
                .to_string()
        };
        assert!(why(r#"{"text":"hi"}"#).contains("states no connection protocol version"));
        for version in [PROTOCOL.oldest - 1, PROTOCOL.current + 1] {
            let said = why(&format!(r#"{{"protocol":{version},"text":"hi"}}"#));
            assert!(said.contains("lnk-x"), "{said}");
        }
        let line = format!(r#"{{"protocol":{},"text":"hi","new":1}}"#, PROTOCOL.current);
        assert!(read::<Said>("lnk-x", line.as_bytes()).is_err());
    }
}
