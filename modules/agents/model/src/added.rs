//! Runtime addresses the user gave Link (`lnk model runtime add <url>`), such as a
//! model they're building on a port of its own. Discovery asks them as it
//! asks the default ports, so an agent and the tunnel see that model as
//! they see Ollama's.
//!
//! They're kept in `~/Link/Safe/Models/models.toml`, one `[[runtime]]` each.

use std::path::{Path, PathBuf};

use anyhow::bail;
use link_format::WithinExt;
use serde::{Deserialize, Serialize};

use crate::discover::KNOWN_RUNTIMES;

/// One remembered address.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Added {
    /// What the user called it, shown as its runtime (`lnk model list`).
    pub name: String,
    /// Its base URL, without `/v1` (`http://localhost:5000`).
    pub base_url: String,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    runtime: Vec<Added>,
}

/// Where the added addresses are kept.
pub fn path() -> PathBuf {
    link_files::layout::Home::Models.path()
}

/// The added addresses, oldest first. None when the file is missing or
/// unreadable: discovery still finds the default ports.
pub fn load() -> Vec<Added> {
    load_from(&path()).unwrap_or_default()
}

link_files::specification!(File, include_str!("added.schema.toml"));

fn load_from(path: &Path) -> anyhow::Result<Vec<Added>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).within(format!("reading {}", path.display())),
    };
    let file: File =
        link_files::saved::parse(&text).within(format!("reading {}", path.display()))?;
    // Only what `add` would have written.
    Ok(file
        .runtime
        .into_iter()
        .filter_map(|added| {
            let base_url = base_url(&added.base_url).ok()?;
            Some(Added { base_url, ..added })
        })
        .collect())
}

fn save_to(path: &Path, runtimes: Vec<Added>) -> anyhow::Result<()> {
    let text = link_files::saved::print(&File { runtime: runtimes })?;
    link_files::write_private(path, text.as_bytes()).within(format!("writing {}", path.display()))
}

/// The base URL of a runtime on this machine, from what the user typed:
/// `localhost:5000`, `http://127.0.0.1:5000/` and `http://localhost:5000/v1`
/// are all `http://localhost:5000`-style addresses. Anything off this
/// machine, or with a path, query or password, is refused.
pub fn base_url(given: &str) -> anyhow::Result<String> {
    let given = given.trim();
    let with_scheme = if given.contains("://") {
        given.to_string()
    } else {
        format!("http://{given}")
    };
    let url = reqwest::Url::parse(&with_scheme)
        .map_err(|_| anyhow::anyhow!("{given:?} isn't an address like http://localhost:5000"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("{given:?} isn't an http:// or https:// address");
    }
    let host = url.host_str().unwrap_or_default();
    if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        bail!(
            "{given:?} isn't on this machine: Link adds only a runtime at localhost, \
             127.0.0.1 or [::1]"
        );
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("{given:?} holds a user or password; give the address alone");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("{given:?} has a query; give the address alone");
    }
    let path = url.path().trim_end_matches('/');
    if !matches!(path, "" | "/v1") {
        bail!("{given:?} has a path; give the address its /v1 API is under");
    }
    let port = url
        .port_or_known_default()
        .within("that address has no port")?;
    Ok(format!("{}://{host}:{port}", url.scheme()))
}

/// What `add` did.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Added,
    /// It was added already; its name is now the one given.
    Renamed,
    /// Added, at a runtime's default port: listed under that runtime's
    /// name, as discovery finds it, and marked the owner's.
    Known(&'static str),
}

/// Remembers the runtime at `given` as `name`.
pub fn add(given: &str, name: &str) -> anyhow::Result<(String, Outcome)> {
    add_with(given, name, || {
        let home = link_settings::folders::need()?;
        Ok(link_files::layout::Home::Models.at(&home))
    })
}

fn add_with(
    given: &str,
    name: &str,
    destination: impl FnOnce() -> anyhow::Result<PathBuf>,
) -> anyhow::Result<(String, Outcome)> {
    let base_url = base_url(given)?;
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        bail!("give it a name of plain characters (--name)");
    }
    // The harness takes an `Ollama` runtime for Ollama itself.
    if KNOWN_RUNTIMES
        .iter()
        .any(|(known, _)| known.eq_ignore_ascii_case(name))
    {
        bail!("{name} names the runtime at its default port; give yours another name (--name)");
    }
    // At localhost, 127.0.0.1 or [::1] alike.
    let port = |url: &str| url.rsplit_once(':').map(|(_, port)| port.to_string());
    let known = KNOWN_RUNTIMES
        .iter()
        .find(|(_, url)| base_url.starts_with("http://") && port(url) == port(&base_url));
    let path = destination()?;
    let mut runtimes = load_from(&path)?;
    let outcome = match runtimes.iter_mut().find(|r| same(&r.base_url, &base_url)) {
        Some(runtime) => {
            runtime.name = name.to_string();
            match known {
                Some((known, _)) => Outcome::Known(known),
                None => Outcome::Renamed,
            }
        }
        None => {
            runtimes.push(Added {
                name: name.to_string(),
                base_url: base_url.clone(),
            });
            match known {
                Some((known, _)) => Outcome::Known(known),
                None => Outcome::Added,
            }
        }
    };
    save_to(&path, runtimes)?;
    Ok((base_url, outcome))
}

/// Whether `a` and `b`, addresses [`base_url`] made, are one runtime:
/// each is this machine's, so its port says which.
pub fn same(a: &str, b: &str) -> bool {
    let port = |url: &str| url.rsplit_once(':').map(|(_, port)| port.to_string());
    a == b || port(a).is_some() && port(a) == port(b)
}

/// Forgets the runtimes `which` names, the name given to `add`. Returns
/// the ones forgotten.
pub fn forget(which: &str) -> anyhow::Result<Vec<Added>> {
    forget_from(&path(), which)
}

fn forget_from(path: &Path, which: &str) -> anyhow::Result<Vec<Added>> {
    let runtimes = load_from(path)?;
    let (gone, kept): (Vec<_>, Vec<_>) = runtimes.into_iter().partition(|r| r.name == which.trim());
    if gone.is_empty() {
        bail!("no runtime added as {which:?}; lnk model runtime list shows the ones that are");
    }
    save_to(path, kept)?;
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_to(path: &Path, given: &str, name: &str) -> anyhow::Result<(String, Outcome)> {
        add_with(given, name, || Ok(path.to_path_buf()))
    }

    fn scratch(name: &str) -> link_test_kit::TempDir {
        link_test_kit::TempDir::new(&format!("models-{name}"))
    }

    #[test]
    fn addresses_are_this_machines_and_bare() {
        for (given, want) in [
            ("localhost:5000", "http://localhost:5000"),
            ("http://localhost:5000/", "http://localhost:5000"),
            ("http://localhost:5000/v1", "http://localhost:5000"),
            ("http://localhost:5000/v1/", "http://localhost:5000"),
            ("http://127.0.0.1:9000", "http://127.0.0.1:9000"),
            ("http://[::1]:9000", "http://[::1]:9000"),
            ("https://localhost", "https://localhost:443"),
            (" localhost:5000 ", "http://localhost:5000"),
        ] {
            assert_eq!(base_url(given).unwrap(), want, "{given}");
        }
        for bad in [
            "http://example.com:5000",
            "http://192.168.1.2:5000",
            "http://localhost.example.com:5000",
            "ftp://localhost:5000",
            "http://user:pw@localhost:5000",
            "http://localhost:5000/api",
            "http://localhost:5000/v1/models",
            "http://localhost:5000/?x=1",
            "not an address",
        ] {
            assert!(base_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn invalid_addresses_and_names_never_prepare_state() {
        for (url, name) in [
            ("https://example.com", "mine"),
            ("localhost:5000", " "),
            ("localhost:5000", "ollama"),
        ] {
            let result = add_with(url, name, || panic!("invalid input must not need Home"));
            assert!(result.is_err());
        }
    }

    #[test]
    fn added_addresses_are_kept_renamed_and_forgotten() {
        let dir = scratch("added");
        let path = dir.join("models.toml");
        assert_eq!(load_from(&path).unwrap(), []);

        assert_eq!(
            add_to(&path, "localhost:5000", "mymodel").unwrap(),
            ("http://localhost:5000".into(), Outcome::Added)
        );
        add_to(&path, "http://127.0.0.1:5001/v1", "my server").unwrap();
        assert_eq!(
            add_to(&path, "http://localhost:5000/", "renamed")
                .unwrap()
                .1,
            Outcome::Renamed
        );
        // A default port is kept too, so it counts as yours.
        assert_eq!(
            add_to(&path, "localhost:11434", "mine").unwrap().1,
            Outcome::Known("Ollama")
        );
        assert_eq!(
            add_to(&path, "127.0.0.1:8000", "mine").unwrap().1,
            Outcome::Known("vLLM")
        );
        assert!(add_to(&path, "localhost:5002", " ").is_err());
        assert!(add_to(&path, "localhost:5002", "ollama").is_err());
        assert_eq!(
            load_from(&path).unwrap(),
            [
                Added {
                    name: "renamed".into(),
                    base_url: "http://localhost:5000".into()
                },
                Added {
                    name: "my server".into(),
                    base_url: "http://127.0.0.1:5001".into()
                },
                Added {
                    name: "mine".into(),
                    base_url: "http://localhost:11434".into()
                },
                Added {
                    name: "mine".into(),
                    base_url: "http://127.0.0.1:8000".into()
                },
            ]
        );
        // The same runtime, at another of this machine's names.
        assert_eq!(
            add_to(&path, "127.0.0.1:11434", "again").unwrap().1,
            Outcome::Known("Ollama")
        );
        assert_eq!(load_from(&path).unwrap().len(), 4);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // By the name it was added as, never its address.
        assert_eq!(forget_from(&path, "renamed").unwrap().len(), 1);
        let err = forget_from(&path, "renamed").unwrap_err().to_string();
        assert!(err.contains("no runtime added"), "{err}");
        assert!(forget_from(&path, "127.0.0.1:5001").is_err());
        assert_eq!(forget_from(&path, "my server").unwrap().len(), 1);
        assert_eq!(forget_from(&path, "again").unwrap().len(), 1);
        assert_eq!(forget_from(&path, "mine").unwrap().len(), 1);
        assert_eq!(load_from(&path).unwrap(), []);
    }

    #[test]
    fn a_hand_edited_address_off_this_machine_is_ignored() {
        let dir = scratch("edited");
        let path = dir.join("models.toml");
        std::fs::write(
            &path,
            "[[runtime]]\nname = \"far\"\nbase_url = \"http://example.com:5000\"\n\n\
             [[runtime]]\nname = \"near\"\nbase_url = \"http://localhost:5000/v1\"\n",
        )
        .unwrap();
        assert_eq!(
            load_from(&path).unwrap(),
            [Added {
                name: "near".into(),
                base_url: "http://localhost:5000".into()
            }]
        );
    }
}
