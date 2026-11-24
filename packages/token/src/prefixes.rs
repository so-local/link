//! link-token's prefixes, read once from `prefixes.toml`: each secret
//! IAM makes opens with its type's.

use std::sync::LazyLock;

use serde::Deserialize;

/// Each type's prefix, as `prefixes.toml` declares it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prefixes {
    pub access: String,
    pub refresh: String,
    pub client_secret: String,
    pub api_key: String,
    pub code: String,
}

/// link-token's prefixes.
pub fn prefixes() -> &'static Prefixes {
    static PREFIXES: LazyLock<Prefixes> = LazyLock::new(|| {
        let prefixes: Prefixes =
            toml::from_str(include_str!("prefixes.toml")).expect("link-token's prefixes");
        prefixes.check().expect("link-token's prefixes");
        prefixes
    });
    &PREFIXES
}

impl Prefixes {
    fn all(&self) -> [&str; 5] {
        [
            &self.access,
            &self.refresh,
            &self.client_secret,
            &self.api_key,
            &self.code,
        ]
    }

    /// Each is `lnk_`, whole lowercase words joined by underscores, and an
    /// underscore last, so a double-click selects the whole secret; none
    /// begins another, so a text is of one type alone.
    fn check(&self) -> Result<(), String> {
        let all = self.all();
        for prefix in all {
            let words = prefix
                .strip_prefix("lnk_")
                .and_then(|rest| rest.strip_suffix('_'))
                .ok_or_else(|| format!("{prefix:?}: lnk_, its words, and an underscore"))?;
            if words.is_empty()
                || !words
                    .split('_')
                    .all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase()))
            {
                return Err(format!("{prefix:?}: whole lowercase words"));
            }
            if all
                .iter()
                .any(|other| *other != prefix && other.starts_with(prefix))
            {
                return Err(format!("{prefix:?} begins another prefix"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_prefixes_read_and_hold_together() {
        assert_eq!(super::prefixes().access, "lnk_access_");
    }
}
