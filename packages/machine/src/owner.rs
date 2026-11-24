//! Whose a machine's work is: an owner. An owner is an account
//! (`acct-<id>`) once one is signed in; before that, the user running
//! Link on this machine stands in, `<user>@<machine>` ([`local_owner`]), and is replaced by
//! the account at the next start of what it owns. Saved and printed, it
//! says its kind ([`Owner`]). Which account is
//! signed in is the accounts module's to say, which the core asks
//! (`link_core::owner`).

use anyhow::bail;
use serde::{Deserialize, Serialize};

/// The longest an owner's ID is.
pub const OWNER_MAX: usize = 64;

/// Whose a machine's work is, by its kind: an account, or the user
/// running Link on a machine, saved and printed with its kind's tag.
/// Its text ([`Owner::text`]) is what a path or a wire between programs
/// names it by: the account's ID, or `<user>@<machine>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Owner {
    /// An account, by the ID its account service gives it.
    Account { id: String },
    /// The user `user` running Link on the machine `machine`.
    MachineUser { user: String, machine: String },
}

impl Owner {
    /// The owner whose text is `text`, checked ([`check_owner`]): a
    /// machine's user when it holds `@`, else an account.
    pub fn of(text: &str) -> anyhow::Result<Owner> {
        check_owner(text)?;
        Ok(match text.split_once('@') {
            Some((user, machine)) => Owner::MachineUser {
                user: user.to_string(),
                machine: machine.to_string(),
            },
            None => Owner::Account {
                id: text.to_string(),
            },
        })
    }

    /// Its text: an account's ID, or `<user>@<machine>`.
    pub fn text(&self) -> String {
        match self {
            Owner::Account { id } => id.clone(),
            Owner::MachineUser { user, machine } => format!("{user}@{machine}"),
        }
    }

    /// It, checked as [`Owner::of`] checks its text: an account's ID holds
    /// no `@`, and a machine's user and machine none either.
    pub fn checked(self) -> anyhow::Result<Owner> {
        let parts_plain = match &self {
            Owner::Account { id } => !id.contains('@'),
            Owner::MachineUser { user, machine } => {
                !user.is_empty() && !machine.is_empty() && !format!("{user}{machine}").contains('@')
            }
        };
        match parts_plain {
            true => Owner::of(&self.text()),
            false => bail!("{:?} isn't an owner", link_format::clean(&self.text())),
        }
    }
}

impl std::fmt::Display for Owner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

/// An error unless `owner` is an owner's ID: an account's
/// (`acct-<id>`) or a local one (`<user>@<machine>`), letters,
/// digits, `.`, `_` and `-`, with at most one `@`, at most
/// [`OWNER_MAX`] long.
pub fn check_owner(owner: &str) -> anyhow::Result<()> {
    let plain = (1..=OWNER_MAX).contains(&owner.len())
        && owner.starts_with(|c: char| c.is_ascii_alphanumeric())
        && owner.matches('@').count() <= 1
        && !owner.ends_with('@')
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b));
    match plain {
        true => Ok(()),
        false => bail!(
            "{:?} isn't an owner (an account's ID, or <user>@<machine>)",
            link_format::clean(owner)
        ),
    }
}

/// Whether `owner` is a local stand-in (`<user>@<machine>`), not an
/// account.
pub fn is_local(owner: &str) -> bool {
    owner.contains('@')
}

/// The local owner: the user running Link here, at this installation's name
/// ([`crate::installation::installation`], made from its host name once and kept,
/// so renaming the host changes no owner).
pub fn local_owner() -> anyhow::Result<String> {
    let installation = crate::installation::installation()?;
    Ok(local_owner_of(&user_name(), &installation.name))
}

/// `<user>@<machine>`, the user's name made to fit an owner.
pub fn local_owner_of(user: &str, machine: &str) -> String {
    let user: String = user
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() || "._-".contains(c) {
            true => c.to_ascii_lowercase(),
            false => '-',
        })
        .take(OWNER_MAX - machine.len() - 1)
        .collect();
    let user = user.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    let user = match user.is_empty() {
        true => "user",
        false => user,
    };
    format!("{user}@{machine}")
}

/// The name of the user running this program.
fn user_name() -> String {
    crate::machine::user_name().unwrap_or_default()
}

/// Whether a machine signed in as `account` (none: signed out) takes an
/// agent of `owner`'s. Two kinds of owner, both whole: an agent of
/// someone who never signed in (`<user>@<machine>`) moves to any
/// machine, signed in or not; an account's (`acct-<id>`) only to
/// a machine signed in as that account, never one signed out, which
/// can't tell its owner from anyone.
pub fn serves(owner: &str, account: Option<&str>) -> bool {
    is_local(owner) || account == Some(owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners() {
        for good in [
            "acct-0123456789abcdef0123456789abcdef",
            "github-22600052",
            "dana@danas-mbp",
            "a.b_c@x-1",
        ] {
            check_owner(good).unwrap();
        }
        for bad in [
            "",
            "@x",
            "a@b@c",
            "a@",
            "a b",
            "a/b",
            "../x",
            &"x".repeat(65),
        ] {
            assert!(check_owner(bad).is_err(), "{bad:?}");
        }
        assert!(is_local("dana@danas-mbp"));
        assert!(!is_local("github-1"));
        assert!(serves("github-1", Some("github-1")));
        assert!(!serves("github-1", Some("github-2")));
        assert!(serves("dana@box", Some("github-2")));
        assert!(serves("dana@box", None));
        assert!(!serves("github-1", None));
    }

    /// Saved and printed by its kind's tag, and read back only as one.
    #[test]
    fn an_owner_says_its_kind() {
        let account = Owner::of("github-22600052").unwrap();
        assert_eq!(
            account,
            Owner::Account {
                id: "github-22600052".into()
            }
        );
        let local = Owner::of("dana@danas-mbp").unwrap();
        assert_eq!(
            serde_json::to_value(&local).unwrap(),
            serde_json::json!({"kind": "machine-user", "user": "dana", "machine": "danas-mbp"})
        );
        assert_eq!(
            serde_json::to_value(&account).unwrap(),
            serde_json::json!({"kind": "account", "id": "github-22600052"})
        );
        assert_eq!(local.to_string(), "dana@danas-mbp");
        for owner in [&account, &local] {
            let text = toml::to_string(owner).unwrap();
            assert_eq!(&toml::from_str::<Owner>(&text).unwrap(), owner);
            assert_eq!(&owner.clone().checked().unwrap(), owner);
        }
        assert!(serde_json::from_value::<Owner>(serde_json::json!("github-1")).is_err());
        assert!(serde_json::from_value::<Owner>(serde_json::json!({"id": "github-1"})).is_err());
        let wrong = Owner::Account { id: "a@b".into() };
        assert!(wrong.checked().is_err());
        let wrong = Owner::MachineUser {
            user: String::new(),
            machine: "box".into(),
        };
        assert!(wrong.checked().is_err());
        assert!(Owner::of("a b").is_err());
    }

    #[test]
    fn a_local_owner_is_the_user_at_the_machine() {
        assert_eq!(local_owner_of("dana", "danas-mbp"), "dana@danas-mbp");
        assert_eq!(local_owner_of("Dana Q", "box"), "dana-q@box");
        assert_eq!(local_owner_of("", "box"), "user@box");
        assert_eq!(local_owner_of("-x", "box"), "x@box");
        let long = local_owner_of(&"u".repeat(100), "box");
        check_owner(&long).unwrap();
        check_owner(&local_owner_of("Dana Q", "box")).unwrap();
    }
}
