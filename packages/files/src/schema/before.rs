//! Shapes that predate Link 0.0.1, which starts every schema again at 1 and
//! keeps no install made earlier: each declared in its owner's
//! `schema.toml` under `[before]` and refused by name with what replaced
//! it, never read into the current shape and never taken for a newer one.

use std::collections::BTreeMap;

use anyhow::ensure;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `[before]`: the schema numbers and keys a file written before 0.0.1 may
/// carry, each with what replaced it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Before {
    /// Schema numbers retired by the restart, each with what to write
    /// instead: TOML keys, so `"2" = "…"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, String>,
    /// Dotted key paths gone from the current shape, each with what
    /// replaced it. A `*` stands for any one key or list entry.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, String>,
    /// The release that brought them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<link_version::Version>,
    /// The release that deletes them, after which such a file is refused
    /// as unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<link_version::Version>,
}

impl Before {
    /// Whether it declares nothing.
    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty() && self.keys.is_empty()
    }

    /// Checked as its owner's `schema.toml` is parsed.
    pub fn check(&self, kind: &str, current: u32) -> anyhow::Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        let (Some(since), Some(until)) = (&self.since, &self.until) else {
            anyhow::bail!("{kind}'s [before] needs its since and until");
        };
        ensure!(
            since < until,
            "{kind}'s [before] is deleted at {until}, before or as it came at {since}"
        );
        for (schema, why) in &self.schemas {
            let number: u32 = schema.parse().map_err(|_| {
                anyhow::anyhow!("{kind}'s [before.schemas] {schema:?} isn't a schema number")
            })?;
            ensure!(
                number > current,
                "{kind}'s [before.schemas] {number} is a schema this lnk reads"
            );
            ensure!(
                !why.trim().is_empty(),
                "{kind}'s [before.schemas] {number} needs what replaced it"
            );
        }
        for (path, why) in &self.keys {
            ensure!(
                path != "schema" && path != "kind" && super::path::check_pattern(path),
                "invalid [before.keys] path {path:?}"
            );
            ensure!(
                !path.starts_with('*'),
                "{kind}'s [before.keys] {path} starts with *, which would match any top-level key"
            );
            ensure!(
                !why.trim().is_empty(),
                "{kind}'s [before.keys] {path} needs what replaced it"
            );
        }
        Ok(())
    }

    /// The refusal of a file naming schema `from`, when it's one from
    /// before the restart.
    pub fn schema(&self, kind: &str, from: u32) -> anyhow::Result<()> {
        match self.schemas.get(&from.to_string()) {
            Some(why) => anyhow::bail!(
                "{kind} schema {from} predates Link 0.0.1, which starts every schema again at 1: {why}"
            ),
            None => Ok(()),
        }
    }

    /// The refusal of a document, at the current schema, that still holds
    /// a key retired by the restart.
    pub fn keys(&self, kind: &str, document: &Value) -> anyhow::Result<()> {
        for (path, why) in &self.keys {
            let parts = super::path::keys(path).unwrap_or_default();
            if let Some(found) = find(document, &parts, String::new()) {
                anyhow::bail!("{kind} {found} predates Link 0.0.1: {why}");
            }
        }
        Ok(())
    }
}

/// The first path in `value` matching `parts`, a `*` matching any key or
/// list entry, written as it's found.
fn find(value: &Value, parts: &[&str], at: String) -> Option<String> {
    let Some((head, rest)) = parts.split_first() else {
        return Some(at);
    };
    let join = |key: &str| match at.as_str() {
        "" => super::path::key(key),
        _ => format!("{at}.{}", super::path::key(key)),
    };
    match value {
        Value::Object(object) if *head == "*" => object
            .iter()
            .find_map(|(key, inner)| find(inner, rest, join(key))),
        Value::Object(object) => find(object.get(*head)?, rest, join(head)),
        Value::Array(list) if *head == "*" => list
            .iter()
            .enumerate()
            .find_map(|(i, inner)| find(inner, rest, join(&i.to_string()))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::Format;

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code, reason = "read only to prove the shape parses")]
    struct Spec {
        name: Option<String>,
        jobs: Option<Vec<Job>>,
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code, reason = "read only to prove the shape parses")]
    struct Job {
        name: String,
    }

    fn format() -> Format {
        Format::parse(
            r#"
schema = 1
kind = "example"
current = 1
command = "lnk example spec show"
[before]
since = "0.0.1"
until = "0.0.2"
schemas = { "2" = "write it as schema 1" }
keys = { owner = "leave owner out", "jobs.*.id" = "name it with name" }
"#,
        )
        .unwrap()
    }

    #[test]
    fn a_schema_from_before_the_restart_is_refused_by_name_never_as_newer() {
        let error = format()
            .read::<Spec>("schema = 2\nname = \"a\"\n")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("example schema 2 predates Link 0.0.1"),
            "{error}"
        );
        assert!(error.contains("write it as schema 1"), "{error}");
        assert!(!error.contains("newer"), "{error}");
        let error = format()
            .read::<Spec>("schema = 3\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("needs a newer lnk"), "{error}");
    }

    #[test]
    fn a_key_from_before_the_restart_is_refused_by_its_path() {
        let error = format()
            .read::<Spec>("schema = 1\nowner = \"me\"\n")
            .unwrap_err()
            .to_string();
        assert_eq!(error, "example owner predates Link 0.0.1: leave owner out");
        let text = "schema = 1\n[[jobs]]\nname = \"a\"\n[[jobs]]\nname = \"b\"\nid = \"j-1\"\n";
        let error = format().read::<Spec>(text).unwrap_err().to_string();
        assert_eq!(
            error,
            "example jobs.1.id predates Link 0.0.1: name it with name"
        );
        let error = format()
            .read_json::<Spec>(br#"{"schema":1,"jobs":[{"name":"a","id":"j"}]}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("jobs.0.id predates Link 0.0.1"), "{error}");
        assert!(
            format()
                .read::<Spec>("schema = 1\n[[jobs]]\nname = \"a\"\n")
                .is_ok()
        );
    }

    /// A `[before]` key is a path as a step's is: a key holding a dot is
    /// quoted, and one that's no path is refused.
    #[test]
    fn a_key_from_before_the_restart_may_be_quoted() {
        let base = "schema = 1\nkind = \"example\"\ncurrent = 1\ncommand = \"lnk example spec show\"\n\
                    [before]\nsince = \"0.0.1\"\nuntil = \"0.0.2\"\n";
        let quoted = format!(
            "{base}keys = {{ 'locations.\"lnk.old\"' = \"it's lnk.migration-journal\" }}\n"
        );
        let error = Format::parse(&quoted)
            .unwrap()
            .read::<Spec>("schema = 1\n[locations.\"lnk.old\"]\npath = \"x\"\n")
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "example locations.\"lnk.old\" predates Link 0.0.1: it's lnk.migration-journal"
        );
        let broken = format!("{base}keys = {{ 'locations.\"lnk' = \"x\" }}\n");
        assert!(Format::parse(&broken).is_err());
    }

    #[test]
    fn a_declaration_of_a_schema_this_lnk_reads_or_without_its_window_is_refused() {
        let base = "schema = 1\nkind = \"example\"\ncurrent = 2\ncommand = \"lnk example spec show\"\n\
                    [[steps]]\nfrom = 1\nsince = \"0.0.1\"\nuntil = \"0.0.2\"\n";
        let reads = format!(
            "{base}[before]\nsince = \"0.0.1\"\nuntil = \"0.0.2\"\nschemas = {{ \"2\" = \"x\" }}\n"
        );
        assert!(Format::parse(&reads).is_err());
        let windowless = format!("{base}[before]\nkeys = {{ owner = \"x\" }}\n");
        assert!(Format::parse(&windowless).is_err());
        let any = format!(
            "{base}[before]\nsince = \"0.0.4\"\nuntil = \"0.0.6\"\nkeys = {{ \"*.id\" = \"x\" }}\n"
        );
        let refused = Format::parse(&any).unwrap_err().to_string();
        assert!(refused.contains("starts with *"), "{refused}");
    }
}
