//! How an action travels over HTTP, from `transport.toml` beside the
//! crate: each effect's method and whether it takes a body, the formats a
//! body may be in, a stream's media type, and each named error's status.
//! Nothing here is written in an action's declaration: a transport
//! derives it from the action's effect and output.

use std::{collections::BTreeMap, sync::LazyLock};

use serde::Deserialize;

use crate::Effect;

/// `transport.toml`, read once.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transport {
    effects: BTreeMap<Effect, Method>,
    pub formats: Formats,
    pub streams: Streams,
    pub caching: Caching,
    /// Each named error's status, by its name.
    pub status: BTreeMap<String, u16>,
}

/// An effect's method, and whether its inputs other than what it acts on
/// travel in a body.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Method {
    pub method: String,
    pub body: bool,
}

/// The formats a body may be in.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Formats {
    /// The format of an answer whose request names none.
    pub default: String,
    /// The formats offered now; any other is refused.
    pub offered: Vec<String>,
    /// Each format's media type, by its name.
    pub media: BTreeMap<String, String>,
}

/// A stream's framing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Streams {
    pub media: String,
}

/// What caching headers are written from.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Caching {
    /// How long, in seconds, an answer that keeps forever is kept.
    pub forever_seconds: u64,
}

impl Transport {
    /// The `Cache-Control` an action's answer carries: a find's from what
    /// it keeps, `no-store` for a find that keeps nothing, and none for
    /// any other effect, whose answer no cache keeps.
    pub fn cache_control(&self, effect: Effect, keeps: Option<&crate::Keeps>) -> Option<String> {
        if effect != Effect::Find {
            return None;
        }
        let Some(keeps) = keeps else {
            return Some("no-store".into());
        };
        let whose = if keeps.shared { "public" } else { "private" };
        Some(match keeps.lasting {
            crate::Lasting::For(duration) => {
                format!("{whose}, max-age={}", duration.get().as_secs())
            }
            crate::Lasting::Forever => format!(
                "{whose}, max-age={}, immutable",
                self.caching.forever_seconds
            ),
        })
    }

    /// An effect's method and body.
    pub fn method(&self, effect: Effect) -> &Method {
        &self.effects[&effect]
    }

    /// A named error's status, where `name` is one.
    pub fn status(&self, name: &str) -> Option<u16> {
        self.status.get(name).copied()
    }
}

/// The transport, as `transport.toml` declares it.
pub fn transport() -> &'static Transport {
    &TRANSPORT
}

static TRANSPORT: LazyLock<Transport> = LazyLock::new(|| {
    let transport: Transport =
        toml::from_str(include_str!("../transport.toml")).expect("the built-in transport");
    assert!(
        Effect::ALL
            .iter()
            .all(|effect| transport.effects.contains_key(effect)),
        "transport.toml gives every effect its method"
    );
    let formats = &transport.formats;
    assert!(
        formats.offered.contains(&formats.default)
            && formats
                .offered
                .iter()
                .all(|format| formats.media.contains_key(format)),
        "transport.toml offers its default format, and each offered one has a media type"
    );
    transport
});

#[cfg(test)]
mod tests {
    use super::*;

    /// A find and a delete take no body; an insert and an update do.
    #[test]
    fn each_effect_has_its_method() {
        let transport = transport();
        for (effect, method, body) in [
            (Effect::Find, "GET", false),
            (Effect::Insert, "POST", true),
            (Effect::Update, "PUT", true),
            (Effect::Delete, "DELETE", false),
        ] {
            let declared = transport.method(effect);
            assert_eq!((declared.method.as_str(), declared.body), (method, body));
        }
        assert_eq!(transport.formats.media["json"], "application/json");
    }

    /// A find's answer keeps as it declares, one caller's own unless it's
    /// shared; one that declares nothing is never kept; and no other
    /// effect's answer says anything of caching.
    #[test]
    fn caching_follows_what_a_find_keeps() {
        let keeps = |text: &str| -> crate::Keeps { toml::from_str(text).unwrap() };
        let transport = transport();
        assert_eq!(
            transport.cache_control(Effect::Find, Some(&keeps("for = \"5m\""))),
            Some("private, max-age=300".into())
        );
        assert_eq!(
            transport.cache_control(
                Effect::Find,
                Some(&keeps("for = \"forever\"\nshared = true"))
            ),
            Some("public, max-age=31536000, immutable".into())
        );
        assert_eq!(
            transport.cache_control(Effect::Find, None),
            Some("no-store".into())
        );
        assert_eq!(transport.cache_control(Effect::Insert, None), None);
        assert!(toml::from_str::<crate::Keeps>("for = \"soon\"").is_err());
    }

    /// Every error core's `error` names has a status here, and nothing
    /// else does: one list of names, core's.
    #[test]
    fn every_named_error_has_its_status() {
        let core = crate::catalog::core_shapes().unwrap();
        let error = &core.types["error"];
        let crate::schema::Node::Record(record) = &error.schema.node else {
            panic!("core's error is a record")
        };
        let crate::schema::Node::Enum(words) = &record.properties["name"].node else {
            panic!("core's error names its error by a word")
        };
        let names: Vec<&str> = words.iter().filter_map(|word| word.as_str()).collect();
        let statuses: Vec<&str> = transport().status.keys().map(String::as_str).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, statuses);
        assert_eq!(transport().status("conflict"), Some(409));
        assert_eq!(transport().status("unavailable"), Some(503));
        assert!(
            transport()
                .status
                .values()
                .all(|status| (400..600).contains(status))
        );
    }
}
