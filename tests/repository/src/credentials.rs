//! No other company's credential, real or made up, in a tracked file: a
//! line matching a shape in `credentials.toml` fails, naming the file,
//! the line and the provider. GitHub's push protection refuses a push
//! holding one and admits no exception, so no list admits one here. A
//! test builds such a value from parts, so its source never matches.

use std::collections::BTreeMap;
use std::path::Path;

/// The shapes, by provider, read from a `credentials.toml`.
pub struct Shapes(Vec<(String, regex::Regex)>);

impl Shapes {
    /// The shapes `text`, a `credentials.toml`, declares.
    pub fn parse(text: &str) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            shapes: BTreeMap<String, String>,
        }
        let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
        file.shapes
            .into_iter()
            .map(|(provider, shape)| {
                regex::Regex::new(&shape)
                    .map(|re| (provider.clone(), re))
                    .map_err(|e| format!("{provider}: {e}"))
            })
            .collect::<Result<_, _>>()
            .map(Shapes)
    }

    /// The provider whose credential `line` holds, if any.
    pub fn provider(&self, line: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(_, re)| re.is_match(line))
            .map(|(provider, _)| provider.as_str())
    }

    /// Each line of `text` holding a credential, by its number, with the
    /// provider.
    pub fn lines<'a>(&'a self, text: &'a str) -> impl Iterator<Item = (usize, &'a str)> + 'a {
        text.lines()
            .enumerate()
            .filter_map(|(at, line)| self.provider(line).map(|p| (at + 1, p)))
    }
}

/// The text files git tracks under `root`, by their paths from it.
#[expect(clippy::disallowed_methods, reason = "git lists the tracked files")]
pub(crate) fn tracked(root: &Path) -> Vec<String> {
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|file| !file.is_empty())
        .map(str::to_string)
        .collect()
}

/// The check on `root`, by the shapes at `list`.
pub fn check(root: &Path, list: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(list).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    let shapes = Shapes::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    tracked(root)
        .into_iter()
        .flat_map(|file| {
            // A file that isn't text holds no line to match.
            let text = std::fs::read_to_string(root.join(&file)).unwrap_or_default();
            shapes
                .lines(&text)
                .map(|(line, provider)| {
                    format!(
                        "{file}:{line}: holds a {provider} credential's shape, which GitHub \
                         refuses to publish even made up: build it from parts with concat!"
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shapes() -> Shapes {
        Shapes::parse(include_str!("../credentials.toml")).unwrap()
    }

    #[test]
    fn each_providers_shape_is_found_whole_and_never_in_parts() {
        let shapes = shapes();
        for (provider, whole, parts) in [
            (
                "discord",
                concat!(
                    "MTIzNDU2Nzg5MDEyMzQ1Njc4",
                    ".GabcDe.",
                    "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
                ),
                r#"concat!("MTIzNDU2Nzg5MDEyMzQ1Njc4", ".GabcDe.", "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-")"#,
            ),
            (
                "slack",
                concat!("xoxb", "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx"),
                r#"concat!("xoxb", "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx")"#,
            ),
            (
                "telegram",
                concat!("123456", ":AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw"),
                r#"concat!("123456", ":AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw")"#,
            ),
            (
                "github",
                concat!("ghp", "_abcdefghijklmnopqrstuvwxyz0123456789"),
                r#"concat!("ghp", "_abcdefghijklmnopqrstuvwxyz0123456789")"#,
            ),
            (
                "aws",
                concat!("AKIA", "ABCDEFGHIJKLMNOP"),
                r#"concat!("AKIA", "ABCDEFGHIJKLMNOP")"#,
            ),
        ] {
            assert_eq!(shapes.provider(whole), Some(provider), "{whole}");
            assert_eq!(shapes.provider(parts), None, "{parts}");
        }
    }

    #[test]
    fn short_placeholders_and_public_keys_are_not_credentials() {
        let shapes = shapes();
        for line in [
            "api_key: \"sk-ant-api03-secret\"",
            "const TOKEN: &str = \"ghs_SEALEDgittoken123\";",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIH9+OiZ/SeNGnTeS94BYhtoALcCIHMNqITeJujSzZeHW",
            "Target::Aarch64UnknownLinuxMusl",
            "sha512-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789==",
        ] {
            assert_eq!(shapes.provider(line), None, "{line}");
        }
        assert_eq!(
            shapes
                .lines("a\nAKIAABCDEFGHIJKLMNOP\n")
                .collect::<Vec<_>>(),
            [(2, "aws")]
        );
    }
}
