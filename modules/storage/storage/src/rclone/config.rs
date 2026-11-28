//! rclone's config file and the Keychain: its sections written, renamed and
//! removed, and on a Mac the secret settings kept in the Keychain.

use super::*;
use link_format::WithinExt;

impl Rclone {
    /// Adds (or replaces) the config section `name` with `settings`.
    pub fn set_section(&self, name: &str, settings: &[(&str, &str)]) -> anyhow::Result<()> {
        self.changed();
        put_section(&self.config, name, settings)
    }

    pub fn remove_section(&self, name: &str) -> anyhow::Result<()> {
        self.changed();
        remove_section(&self.config, name)
    }

    /// Runs rclone's own setup for `name`, of rclone type `kind`, in this
    /// terminal: `settings` as given, a browser sign-in where the type has
    /// one, and with `ask`, rclone's questions for the rest.
    pub fn create_interactive(
        &self,
        name: &str,
        kind: &str,
        settings: &[(&str, &str)],
        ask: bool,
    ) -> anyhow::Result<()> {
        let mut command = self.command(&["config", "create", name, kind])?;
        for (key, value) in settings {
            command.arg(format!("{key}={value}"));
        }
        if ask {
            command.arg("--all");
        }
        self.changed();
        let status = command.status().within("running rclone")?;
        ensure!(status.success(), "setting up {name} didn't finish");
        Ok(())
    }

    /// The storage types rclone knows (`s3`, `b2`, `drive`, ...).
    pub fn backends(&self) -> anyhow::Result<Vec<String>> {
        Ok(self
            .providers()?
            .into_iter()
            .map(|(name, _)| name)
            .collect())
    }

    /// Each storage type, with the settings rclone treats as secret or
    /// sensitive (passwords, keys, account ids).
    pub fn providers(&self) -> anyhow::Result<Vec<(String, Vec<String>)>> {
        let output = self.run(&["config", "providers"])?;
        let providers: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).within("reading rclone's providers")?;
        Ok(providers
            .iter()
            .filter_map(|p| {
                let name = p["Name"].as_str()?.to_string();
                let secret = p["Options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|o| {
                        o["IsPassword"].as_bool().unwrap_or(false)
                            || o["Sensitive"].as_bool().unwrap_or(false)
                    })
                    .filter_map(|o| o["Name"].as_str().map(str::to_string))
                    .collect();
                Some((name, secret))
            })
            .collect())
    }

    /// Whether the storage under `remote` (`name:`) holds buckets (S3, B2,
    /// Azure) rather than folders (Drive, Dropbox, SFTP).
    pub fn bucket_based(&self, remote: &str) -> anyhow::Result<bool> {
        let features = self.features(remote)?;
        Ok(features["Features"]["BucketBased"]
            .as_bool()
            .unwrap_or(false))
    }

    /// Runs rclone with `args` as given, in this terminal, on the clouds
    /// Link connected (each is a remote named after its cloud). Returns its
    /// exit code.
    pub fn passthrough(&self, args: &[String]) -> anyhow::Result<i32> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let status = self.command(&args)?.status().within("running rclone")?;
        Ok(status.code().unwrap_or(1))
    }

    /// `password` as rclone keeps it in its config (obscured, which is
    /// not encryption: the file's permissions are what protect it).
    pub fn obscure(&self, password: &str) -> anyhow::Result<String> {
        let mut child = self
            .command(&["obscure", "-"])?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .within("running rclone")?;
        child
            .stdin
            .take()
            .within("no stdin")?
            .write_all(password.as_bytes())?;
        let output = child.wait_with_output()?;
        ensure!(output.status.success(), "{}", said(&output));
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }
}

/// The config file in the module's folder `dir`.
pub fn config_in(dir: &Path) -> PathBuf {
    dir.join("rclone.conf")
}

/// Deletes the section `name` from the config file `config`, if there;
/// needs no engine installed.
pub fn remove_section(config: &Path, name: &str) -> anyhow::Result<()> {
    let text = match std::fs::read_to_string(config) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    link_files::write_private(config, without_section(&text, name).as_bytes())?;
    forget_keychain(config, name)
}

/// Writes section `name` of `config` with `settings`, replacing it; on a
/// Mac the secret ones go to the Keychain.
pub(super) fn put_section(
    config: &Path,
    name: &str,
    settings: &[(&str, &str)],
) -> anyhow::Result<()> {
    for (key, value) in settings {
        ensure!(
            !key.contains(['\n', '\r', '=']) && !value.contains(['\n', '\r']),
            "a setting can't span lines"
        );
    }
    forget_keychain(config, name)?;
    let mut moved = Vec::new();
    let mut text = without_section(&std::fs::read_to_string(config)?, name);
    if !text.is_empty() && !text.ends_with("\n\n") {
        text.push('\n');
    }
    text += &format!("[{name}]\n");
    for (key, value) in settings {
        if keychain(config) && SECRET_KEYS.contains(key) {
            link_keychain::stash(&key_account(name, key), Some(&(*value).to_string().into()))?;
            moved.push(key.to_string());
            continue;
        }
        text += &format!("{key} = {value}\n");
    }
    if !moved.is_empty() {
        let mut listed = keychain_list(config)?;
        listed.insert(name.to_string(), moved);
        save_keychain_list(config, &listed)?;
    }
    link_files::write_private(config, text.as_bytes())?;
    Ok(())
}

/// The settings rclone reads that Link writes and that are secret: cloud
/// keys and encryption passwords (obscured, as rclone keeps them).
pub const SECRET_KEYS: &[&str] = &[
    "secret_access_key",
    "session_token",
    "password",
    "password2",
    "service_account_credentials",
];

/// Whether `config`'s secrets go to the Keychain: there is one, and it is
/// the usual file (not a test's).
pub(super) fn keychain(config: &Path) -> bool {
    link_keychain::available()
        && crate::files::config_dir().is_ok_and(|dir| config_in(&dir) == config)
}

fn key_account(section: &str, key: &str) -> String {
    format!("storage/{section}/{key}")
}

/// Where a secret setting of the engine's config is kept, never its
/// value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// In the config file itself, readable only by you.
    File(PathBuf),
    /// In the Keychain, under this account of Link's service.
    Keychain(String),
}

impl Kept {
    /// Where it is, for people and `lnk secret list`.
    pub fn describe(&self) -> String {
        match self {
            Kept::File(path) => format!("{} (only yours)", path.display()),
            Kept::Keychain(account) => {
                format!("the Keychain ({}:{account})", link_keychain::SERVICE)
            }
        }
    }
}

/// Each secret setting `config` holds, by section: its key and where it
/// is kept.
fn secrets(config: &Path) -> BTreeMap<String, Vec<(String, Kept)>> {
    let text = std::fs::read_to_string(config).unwrap_or_default();
    let mut found: BTreeMap<String, Vec<(String, Kept)>> = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_string();
        } else if let Some((key, _)) = line.split_once('=')
            && SECRET_KEYS.contains(&key.trim())
        {
            found
                .entry(section.clone())
                .or_default()
                .push((key.trim().into(), Kept::File(config.to_path_buf())));
        }
    }
    // Shown, never saved: a list that can't be read shows none of its own.
    for (section, keys) in keychain_list(config).unwrap_or_default() {
        for key in keys {
            let kept = Kept::Keychain(key_account(&section, &key));
            found.entry(section.clone()).or_default().push((key, kept));
        }
    }
    found
}

/// Where setting `key` of section `name` in `config` is kept; none when
/// it isn't set.
pub fn kept(config: &Path, name: &str, key: &str) -> Option<Kept> {
    secrets(config)
        .remove(name)?
        .into_iter()
        .find_map(|(k, kept)| (k == key).then_some(kept))
}

/// The secrets `config` holds, by section, as `lnk secret list` shows them:
/// where each is kept, never its value.
pub fn held(config: &Path) -> Vec<link_core::uninstall::Held> {
    secrets(config)
        .into_iter()
        .flat_map(|(section, keys)| {
            keys.into_iter().map(move |(key, kept)| {
                let password = key.starts_with("password");
                link_core::uninstall::Held {
                    what: match password {
                        true => {
                            link_core::uninstall::CredentialKind::BucketEncryptionPassword.into()
                        }
                        false => link_core::uninstall::CredentialKind::BucketKeys.into(),
                    },
                    of: format!("{section} ({key})"),
                    kept: kept.describe(),
                    may: match password {
                        true => format!("Reads and writes what's encrypted in {section}"),
                        false => format!("Reaches {section}'s storage as your cloud account"),
                    },
                }
            })
        })
        .collect()
}

/// `rclone-keychain.toml`: which settings of which section are in the
/// Keychain.
pub(super) fn keychain_list_path(config: &Path) -> PathBuf {
    config.with_file_name("rclone-keychain.toml")
}

/// `rclone-keychain.toml`, as read: each section's settings in the
/// Keychain, by the section's name, under `[sections]` so no name meets
/// its `schema`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct KeychainList {
    #[serde(default)]
    sections: std::collections::BTreeMap<String, Vec<String>>,
}

link_files::specification!(KeychainList, include_str!("keychain.schema.toml"));

/// The storage keychain list, which an upgrade converts.
pub fn keychain_saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<KeychainList>()
}

/// The list: empty only when its file is missing. One that can't be read
/// is refused by name, never read as empty, since the next save would
/// drop what it lists and orphan those Keychain settings.
fn keychain_list(config: &Path) -> anyhow::Result<std::collections::BTreeMap<String, Vec<String>>> {
    let path = keychain_list_path(config);
    Ok(link_files::saved::read_optional::<KeychainList>(&path)?
        .unwrap_or_default()
        .sections)
}

fn save_keychain_list(
    config: &Path,
    listed: &std::collections::BTreeMap<String, Vec<String>>,
) -> anyhow::Result<()> {
    let path = keychain_list_path(config);
    if listed.is_empty() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => return Ok(()),
        }
    }
    let listed = KeychainList {
        sections: listed.clone(),
    };
    link_files::write_private(&path, link_files::saved::print(&listed)?.as_bytes())?;
    Ok(())
}

/// Removes section `name`'s settings from the Keychain.
fn forget_keychain(config: &Path, name: &str) -> anyhow::Result<()> {
    let mut listed = keychain_list(config)?;
    let Some(keys) = listed.remove(name) else {
        return Ok(());
    };
    for key in keys {
        link_keychain::delete(&key_account(name, &key))?;
    }
    save_keychain_list(config, &listed)
}

/// Every setting of `config` that is in the Keychain: (section, key,
/// value).
pub(super) fn keychain_settings(config: &Path) -> anyhow::Result<Vec<(String, String, Secret)>> {
    if !keychain(config) {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for (section, keys) in keychain_list(config)? {
        for key in keys {
            if let Some(value) = link_keychain::fetch(&key_account(&section, &key), None)? {
                found.push((section.clone(), key, value));
            }
        }
    }
    Ok(found)
}

/// The settings of section `name` in the config file `config`, in order;
/// none when it isn't there.
pub fn section(config: &Path, name: &str) -> anyhow::Result<Option<Vec<(String, String)>>> {
    let mut found = file_section(config, name)?;
    if let Some(settings) = found.as_mut()
        && keychain(config)
    {
        for key in keychain_list(config)?.remove(name).unwrap_or_default() {
            // Back among the section's settings, as rclone reads them.
            if let Some(value) = link_keychain::fetch(&key_account(name, &key), None)? {
                settings.push((key, value.expose_owned()));
            }
        }
    }
    Ok(found)
}

/// The settings of section `name` as the file `config` holds them, in
/// order, none of the Keychain's; none when it isn't there.
pub(super) fn file_section(
    config: &Path,
    name: &str,
) -> anyhow::Result<Option<Vec<(String, String)>>> {
    let text = match std::fs::read_to_string(config) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let header = format!("[{name}]");
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if found.is_some() {
                break;
            }
            if line == header {
                found = Some(Vec::new());
            }
            continue;
        }
        if let (Some(settings), Some((key, value))) = (found.as_mut(), line.split_once('=')) {
            settings.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    Ok(found)
}

/// `text` (an rclone config) without the section `name`.
fn without_section(text: &str, name: &str) -> String {
    let header = format!("[{name}]");
    let mut out = String::new();
    let mut skipping = false;
    for line in text.lines() {
        if line.trim_start().starts_with('[') {
            skipping = line.trim() == header;
        }
        if !skipping {
            out += line;
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_config_sections() {
        let text = "[a]\ntype = s3\n\n[b]\ntype = alias\nremote = /x\n";
        assert_eq!(
            without_section(text, "a"),
            "[b]\ntype = alias\nremote = /x\n"
        );
        assert_eq!(without_section(text, "b"), "[a]\ntype = s3\n\n");
        assert_eq!(without_section(text, "c"), text);
    }

    /// The list names its schema, and a section named `schema` is one
    /// like any other.
    #[test]
    fn the_keychain_list_round_trips() {
        let dir = link_test_kit::TempDir::new("rclone-keychain");
        let config = dir.join("rclone.conf");
        let listed = std::collections::BTreeMap::from([
            ("schema".to_string(), vec!["password".to_string()]),
            ("aws".to_string(), vec!["secret_access_key".to_string()]),
        ]);
        save_keychain_list(&config, &listed).unwrap();
        let text = std::fs::read_to_string(keychain_list_path(&config)).unwrap();
        assert!(text.starts_with("schema = 1\n"), "{text}");
        assert_eq!(keychain_list(&config).unwrap(), listed);
        save_keychain_list(&config, &Default::default()).unwrap();
        assert!(!keychain_list_path(&config).exists());
        assert!(keychain_list(&config).unwrap().is_empty());
    }

    /// A list that can't be read is refused by name, never read as empty,
    /// so no save drops what it lists.
    #[test]
    fn an_unreadable_keychain_list_is_refused_by_name() {
        let dir = link_test_kit::TempDir::new("rclone-keychain-torn");
        let config = dir.join("rclone.conf");
        std::fs::write(keychain_list_path(&config), "schema = 1\n[sections\n").unwrap();
        let refused = keychain_list(&config).unwrap_err();
        assert!(
            format!("{refused:#}").contains("rclone-keychain.toml"),
            "{refused:#}"
        );
        assert!(forget_keychain(&config, "aws").is_err());
    }
}
