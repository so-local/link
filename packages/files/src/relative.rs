//! A path relative to the folder it's in, the only kind of path Link
//! stores: a stored path never names where a folder is, so the folder
//! can move, be copied or migrated and every path in it still holds. A
//! bare absolute path, or one that climbs out (`..`), is refused when
//! read.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A path relative to its folder: never absolute, never out of it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Relative(String);

impl Relative {
    /// `text` as a relative path, or why it isn't one.
    pub fn new(text: &str) -> anyhow::Result<Self> {
        let path = Path::new(text);
        anyhow::ensure!(!text.is_empty(), "a stored path can't be empty");
        anyhow::ensure!(
            !path.is_absolute(),
            "{text} is an absolute path: Link stores a path relative to its folder"
        );
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(part) => parts.push(
                    part.to_str()
                        .ok_or_else(|| anyhow::anyhow!("{text} isn't UTF-8"))?,
                ),
                Component::CurDir => {}
                _ => anyhow::bail!("{text} leaves its folder: Link stores a path inside it"),
            }
        }
        anyhow::ensure!(!parts.is_empty(), "{text} names no path in its folder");
        Ok(Self(parts.join("/")))
    }

    /// `path`, inside folder `base`, as a path relative to it; None when
    /// it isn't inside.
    pub fn within(base: &Path, path: &Path) -> Option<Self> {
        let rest = path.strip_prefix(base).ok()?;
        Self::new(rest.to_str()?).ok()
    }

    /// Where it is in folder `base`.
    pub fn under(&self, base: &Path) -> PathBuf {
        base.join(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Relative {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Relative {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Relative {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Relative::new(&text).map_err(serde::de::Error::custom)
    }
}

/// A path a person picked, stored as Link stores paths: `~/...` when
/// it's in the home folder, so it holds when the home folder's path
/// changes (another machine, another user); absolute only when it's
/// outside, where no folder of Link's or the person's holds it. A
/// relative one not under `~` is refused when read; an absolute one into
/// the home folder, as a file written by hand holds, is read as it is
/// and written as `~/...` the next time. For `#[serde(with = "...")]` on
/// an `Option<PathBuf>`.
pub mod in_home {
    use std::path::PathBuf;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match path {
            None => serializer.serialize_none(),
            Some(path) => serializer.serialize_str(&to_text(path, &crate::home_dir())),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| from_text(&text, &crate::home_dir()).map_err(serde::de::Error::custom))
            .transpose()
    }

    /// `path` as it's stored, for home folder `home`.
    pub fn to_text(path: &std::path::Path, home: &std::path::Path) -> String {
        match path.strip_prefix(home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        }
    }

    /// The path `text` stores, for home folder `home`.
    pub fn from_text(text: &str, home: &std::path::Path) -> anyhow::Result<PathBuf> {
        if text == "~" {
            return Ok(home.to_path_buf());
        }
        if let Some(rest) = text.strip_prefix("~/") {
            return Ok(super::Relative::new(rest)?.under(home));
        }
        let path = PathBuf::from(text);
        anyhow::ensure!(
            path.is_absolute(),
            "{text} is a relative path: Link stores one in your home folder as ~/..."
        );
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_inside_their_folder_are_relative() {
        for good in ["Link", "Documents/Link", "./a/b", "a//b"] {
            assert!(Relative::new(good).is_ok(), "{good}");
        }
        assert_eq!(Relative::new("./a//b").unwrap().as_str(), "a/b");
        for bad in ["", "/home/me/Link", "../Link", "a/../../b", "."] {
            assert!(Relative::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_path_picked_in_the_home_folder_is_stored_under_tilde() {
        let home = Path::new("/home/me");
        for (path, text) in [
            ("/home/me/Documents/x", "~/Documents/x"),
            ("/home/me", "~"),
            ("/srv/repo", "/srv/repo"),
        ] {
            assert_eq!(in_home::to_text(Path::new(path), home), text);
            assert_eq!(in_home::from_text(text, home).unwrap(), Path::new(path));
        }
        assert_eq!(
            in_home::from_text("~/x", Path::new("/home/you")).unwrap(),
            Path::new("/home/you/x")
        );
        // Written by hand: read, and written as Link writes it.
        let by_hand = in_home::from_text("/home/me/Documents", home).unwrap();
        assert_eq!(in_home::to_text(&by_hand, home), "~/Documents");
        for bad in ["Documents", "~/../etc"] {
            assert!(in_home::from_text(bad, home).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_absolute_path_is_refused_when_read() {
        #[derive(Deserialize)]
        struct Stored {
            #[expect(
                dead_code,
                reason = "only deserialized, to show an absolute path is refused"
            )]
            place: Relative,
        }
        let error = toml::from_str::<Stored>("place = \"/home/me/Link\"")
            .err()
            .unwrap();
        assert!(error.to_string().contains("absolute"), "{error}");
        assert!(toml::from_str::<Stored>("place = \"Documents/Link\"").is_ok());
    }

    #[test]
    fn a_path_inside_a_folder_is_stored_relative_to_it() {
        let home = Path::new("/home/me");
        let place = Relative::within(home, Path::new("/home/me/Documents/Link")).unwrap();
        assert_eq!(place.as_str(), "Documents/Link");
        assert_eq!(
            place.under(Path::new("/home/you")),
            Path::new("/home/you/Documents/Link")
        );
        assert!(Relative::within(home, Path::new("/srv/Link")).is_none());
        assert!(Relative::within(home, home).is_none());
    }
}
