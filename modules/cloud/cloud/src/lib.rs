//! The cloud module: the cloud accounts you connected, once, for every
//! part of Link that uses them. `lnk storage` keeps its storage there
//! and `lnk box` makes machines there.
//!
//! Each cloud is an adapter module (`lnk-aws`, `lnk-gcp`; the contract is
//! `link_cloud_contract`): the adapter signs in and describes the account,
//! and this module keeps it. Other modules read an account through `lnk
//! cloud env <name> --json`, never this file.

pub mod cli;

use link_format::WithinExt;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::ensure;
use link_cloud_contract::Account;
use serde::{Deserialize, Serialize};

/// Names are also bucket and machine names: short, lowercase, simple.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 30
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !name.ends_with('-'),
        "{name:?} can't be a name: lowercase letters, digits and dashes, starting with a letter"
    );
    Ok(())
}

/// A name a cloud goes by, its kind `<name>` or `<name>-cloud`, is that
/// cloud's own account's: an account of another kind never takes it.
/// `is_kind` says whether a kind is one this installation has an adapter
/// for.
pub fn check_account(name: &str, kind: &str, is_kind: impl Fn(&str) -> bool) -> anyhow::Result<()> {
    check_name(name)?;
    let theirs = [name.to_string(), format!("{name}-cloud")]
        .into_iter()
        .find(|cloud| cloud != kind && is_kind(cloud));
    ensure!(
        theirs.is_none(),
        "{name:?} is the name of a cloud's own account: name this {kind} account something else"
    );
    Ok(())
}

/// Whether `kind` is a cloud whose adapter, `lnk-<kind>`, is found here.
pub fn adapter_found(kind: &str) -> bool {
    link_cloud_contract::check_kind(kind).is_ok() && link_core::find_module(kind).is_some()
}

/// `~/.lnk/safe/clouds/accounts.toml`: the accounts, with what their
/// clouds' CLIs need to act as them, access keys included (mode 600). On a
/// Mac the secret ones ([`is_secret`]) are in the Keychain instead, and
/// each account's `keychain` lists which.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Accounts {
    #[serde(default, rename = "account")]
    pub accounts: Vec<Account>,
}

link_files::specification!(Accounts, include_str!("accounts.schema.toml"));

/// The cloud accounts, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Accounts>()
}

impl Accounts {
    /// Include accounts an installed module exposes from an existing
    /// sign-in, without replacing a saved account's identity.
    pub fn discover() -> anyhow::Result<Self> {
        let mut accounts = Self::load()?;
        for answer in link_cloud_contract::ACCOUNTS.ask(&link_cloud_contract::AccountQuestion {}) {
            let supplied = answer
                .reply
                .within(format!("discovering {}'s cloud accounts", answer.module))?;
            accounts.include(supplied.accounts)?;
        }
        Ok(accounts)
    }

    pub fn include(&mut self, discovered: Vec<Account>) -> anyhow::Result<()> {
        for account in discovered {
            check_account(&account.name, &account.kind, adapter_found)?;
            link_cloud_contract::check_kind(&account.kind)?;
            if let Some(saved) = self.get(&account.name) {
                ensure!(
                    saved.kind == account.kind && saved.identity == account.identity,
                    "a discovered account conflicts with {:?}: disconnect or rename the saved account first",
                    account.name
                );
                continue;
            }
            self.accounts.push(account);
        }
        Ok(())
    }
    pub fn path() -> anyhow::Result<PathBuf> {
        Ok(link_files::layout::Hidden::Clouds
            .try_path()?
            .join("accounts.toml"))
    }

    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&Self::path()?)
    }

    /// The accounts as their file has them, the Keychain unread: what
    /// `lnk secret list` needs, which is never a value.
    pub fn load_without_keychain() -> anyhow::Result<Self> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => link_files::saved::parse(&without_keychain(&text)?)
                .within(format!("reading {}", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err).within(format!("reading {}", path.display())),
        }
    }

    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err).within(format!("reading {}", path.display())),
        };
        let mut accounts: Self = link_files::saved::parse(&without_keychain(&text)?)
            .within(format!("reading {}", path.display()))?;
        for account in &mut accounts.accounts {
            account.split_settings();
        }
        if keychain(path) {
            let listed = in_keychain(&text);
            for account in &mut accounts.accounts {
                for var in listed.get(&account.name).into_iter().flatten() {
                    if let Some(secret) =
                        link_keychain::fetch(&key_account(&account.name, var), None)?
                    {
                        account.env.insert(var.clone(), secret);
                    }
                }
            }
        }
        Ok(accounts)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::path()?)
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        self.save_with(
            path,
            keychain(path),
            &mut |account, value| link_keychain::stash(account, Some(value)).map(drop),
            &mut |account| link_keychain::delete(account).map(drop),
        )
    }

    /// [`Accounts::save_to`], each secret stashed with `stash` and each
    /// one no longer kept deleted with `delete` when `keychain`: the file
    /// is written before any is deleted, so an interrupted save never
    /// leaves the file naming a secret the Keychain no longer holds.
    fn save_with(
        &self,
        path: &Path,
        keychain: bool,
        stash: &mut dyn FnMut(&str, &link_secretive::Secret) -> anyhow::Result<()>,
        delete: &mut dyn FnMut(&str) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            link_files::create_private_dir(dir)?;
        }
        let mut text = toml::Value::try_from(self)?;
        let mut stale = Vec::new();
        if keychain {
            // Accounts no longer here, and keys no longer secret, go once
            // the file no longer names them.
            let before = std::fs::read_to_string(path)
                .map(|old| in_keychain(&old))
                .unwrap_or_default();
            for (name, vars) in &before {
                for var in vars {
                    let kept = self
                        .get(name)
                        .is_some_and(|a| a.env.get(var).is_some_and(|v| !v.is_empty()));
                    if !kept {
                        stale.push(key_account(name, var));
                    }
                }
            }
            let tables = text
                .get_mut("account")
                .and_then(|a| a.as_array_mut())
                .into_iter()
                .flatten();
            for (account, table) in self.accounts.iter().zip(tables) {
                let mut moved = Vec::new();
                for (var, value) in account.env.iter().filter(|(var, _)| is_secret(var)) {
                    stash(&key_account(&account.name, var), value)?;
                    moved.push(toml::Value::from(var.as_str()));
                    if let Some(env) = table.get_mut("env").and_then(|e| e.as_table_mut()) {
                        env.remove(var);
                    }
                }
                if let (Some(table), false) = (table.as_table_mut(), moved.is_empty()) {
                    table.insert("keychain".into(), toml::Value::Array(moved));
                }
            }
        }
        if let Some(table) = text.as_table_mut() {
            use link_files::schema::Specification;
            table.insert(
                "schema".into(),
                toml::Value::Integer(Self::format().current.into()),
            );
        }
        link_files::write_private(path, toml::to_string(&text)?.as_bytes())
            .within(format!("writing {}", path.display()))?;
        for account in &stale {
            delete(account)?;
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.name == name)
    }

    /// Adds `account`, or replaces the one of that name (reconnecting).
    pub fn put(&mut self, mut account: Account) {
        account.split_settings();
        self.accounts.retain(|a| a.name != account.name);
        self.accounts.push(account);
        self.accounts.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn remove(&mut self, name: &str) -> Option<Account> {
        let index = self.accounts.iter().position(|a| a.name == name)?;
        Some(self.accounts.remove(index))
    }
}

/// Whether an account's variable is a secret, kept in the Keychain on a
/// Mac: its name says so (`AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`).
pub fn is_secret(var: &str) -> bool {
    ["SECRET", "TOKEN", "PASSWORD"]
        .iter()
        .any(|word| var.contains(word))
}

/// Whether `path`'s secrets go to the Keychain: there is one, and it is
/// the usual file (not a test's).
fn keychain(path: &Path) -> bool {
    link_keychain::available() && Accounts::path().is_ok_and(|usual| usual == path)
}

fn key_account(name: &str, var: &str) -> String {
    format!("cloud/{name}/{var}")
}

impl Accounts {
    /// The credentials the accounts hold, as `lnk secret list` shows them:
    /// each account's secret variables, where each is kept, never its value.
    pub fn held(&self) -> Vec<link_core::uninstall::Held> {
        let path = Self::path().unwrap_or_default();
        let listed = std::fs::read_to_string(&path)
            .map(|text| in_keychain(&text))
            .unwrap_or_default();
        self.accounts
            .iter()
            .flat_map(|account| {
                let listed = listed.get(&account.name).cloned().unwrap_or_default();
                let path = path.clone();
                let vars: std::collections::BTreeSet<String> = account
                    .env
                    .keys()
                    .filter(|var| is_secret(var))
                    .cloned()
                    .chain(listed.iter().cloned())
                    .collect();
                vars.into_iter().map(move |var| link_core::uninstall::Held {
                    what: link_core::uninstall::CredentialKind::CloudAccountKey.into(),
                    of: format!("{} ({var})", account.name),
                    kept: match listed.contains(&var) {
                        true => format!(
                            "the Keychain ({}:{})",
                            link_keychain::SERVICE,
                            key_account(&account.name, &var)
                        ),
                        false => format!("{} (only yours)", path.display()),
                    },
                    may: format!(
                        "Acts as your {} account: its boxes and buckets",
                        account.kind
                    ),
                })
            })
            .collect()
    }
}

/// Which variables of which account a saved file says are in the Keychain.
/// The accounts file's text with each account's `keychain` list taken
/// out: which secrets the keychain keeps is the file's (`in_keychain`),
/// never part of an account.
fn without_keychain(text: &str) -> anyhow::Result<String> {
    let mut table: toml::Table = toml::from_str(text)?;
    if let Some(toml::Value::Array(accounts)) = table.get_mut("account") {
        for account in accounts.iter_mut().filter_map(toml::Value::as_table_mut) {
            account.remove("keychain");
        }
    }
    Ok(toml::to_string(&table)?)
}

fn in_keychain(text: &str) -> BTreeMap<String, Vec<String>> {
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return BTreeMap::new();
    };
    value
        .get("account")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter_map(|account| {
            let name = account.get("name")?.as_str()?.to_string();
            let vars = account
                .get("keychain")?
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            Some((name, vars))
        })
        .collect()
}

/// An account as `lnk cloud list --json` shows it: without `env`, which
/// holds its keys.
pub fn listed(account: &Account) -> serde_json::Value {
    serde_json::json!({
        "name": account.name,
        "kind": account.kind,
        "place": account.place,
        "identity": account.identity,
        "login": account.login,
        "machine": account.machine == Some(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn aws(region: &str) -> Account {
        Account {
            name: "aws".into(),
            kind: "aws".into(),
            place: region.into(),
            env: BTreeMap::from([("AWS_SECRET_ACCESS_KEY".into(), "s3cret".to_string().into())]),
            ..Account::default()
        }
    }

    /// A Keychain secret no longer kept is deleted only once the file no
    /// longer names it: an interrupted save leaves no account naming a
    /// secret that's gone.
    #[test]
    fn the_file_is_written_before_a_keychain_secret_is_deleted() {
        let dir = link_test_kit::TempDir::new("cloud-accounts-order");
        let path = dir.join("accounts.toml");
        let mut stashed = Vec::new();
        let mut accounts = Accounts::default();
        accounts.put(aws("us-east-1"));
        accounts
            .save_with(
                &path,
                true,
                &mut |a, _| {
                    stashed.push(a.to_string());
                    Ok(())
                },
                &mut |_| panic!("nothing to delete yet"),
            )
            .unwrap();
        assert_eq!(stashed, ["cloud/aws/AWS_SECRET_ACCESS_KEY"]);
        assert!(std::fs::read_to_string(&path).unwrap().contains("keychain"));
        accounts.remove("aws");
        let mut deleted = Vec::new();
        accounts
            .save_with(&path, true, &mut |_, _| Ok(()), &mut |a| {
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(!text.contains("AWS_SECRET_ACCESS_KEY"), "{text}");
                deleted.push(a.to_string());
                Ok(())
            })
            .unwrap();
        assert_eq!(deleted, ["cloud/aws/AWS_SECRET_ACCESS_KEY"]);
    }

    #[test]
    fn discovering_an_account_preserves_saved_credentials_and_refuses_name_conflicts() {
        let mut accounts = Accounts::default();
        accounts.put(aws("eu-west-1"));
        accounts
            .include(vec![Account {
                name: "host".into(),
                kind: "fake".into(),
                identity: "signed-in-owner".into(),
                ..Default::default()
            }])
            .unwrap();
        assert_eq!(accounts.accounts.len(), 2);
        assert_eq!(accounts.get("aws").unwrap().place, "eu-west-1");
        assert!(
            accounts
                .include(vec![Account {
                    name: "aws".into(),
                    kind: "fake".into(),
                    ..Default::default()
                }])
                .is_err()
        );
        assert_eq!(accounts.get("aws").unwrap().kind, "aws");
        assert!(
            accounts
                .include(vec![Account {
                    name: "../bad".into(),
                    kind: "fake".into(),
                    ..Default::default()
                }])
                .is_err()
        );
    }

    #[test]
    fn accounts_are_saved_privately_and_replaced_by_name() {
        use std::os::unix::fs::PermissionsExt;
        let dir = link_test_kit::TempDir::new("cloud");
        let path = dir.join("cloud/accounts.toml");
        let mut accounts = Accounts::default();
        accounts.put(aws("eu-west-1"));
        accounts.put(aws("us-east-1"));
        assert_eq!(accounts.accounts.len(), 1);
        accounts.save_to(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let loaded = Accounts::load_from(&path).unwrap();
        assert_eq!(loaded.get("aws").unwrap().place, "us-east-1");
        assert!(
            !listed(loaded.get("aws").unwrap())
                .to_string()
                .contains("s3cret")
        );
    }

    #[test]
    fn names_are_simple() {
        assert!(check_name("aws").is_ok());
        assert!(check_name("work-gcp2").is_ok());
        for bad in ["", "AWS", "2aws", "a_b", "a-", "a b", &"a".repeat(31)] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_clouds_own_name_belongs_to_its_own_account() {
        let kinds = |kind: &str| ["aws", "gcp", "home", "lab-cloud"].contains(&kind);
        assert!(check_account("lab", "lab-cloud", kinds).is_ok());
        assert!(check_account("home", "home", kinds).is_ok());
        assert!(check_account("aws", "aws", kinds).is_ok());
        assert!(check_account("lab", "aws", kinds).is_err());
        assert!(check_account("home", "gcp", kinds).is_err());
        assert!(check_account("gcp", "aws", kinds).is_err());
        assert!(check_account("work", "aws", kinds).is_ok());
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
