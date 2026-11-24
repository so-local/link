//! The secrets a person keeps on this machine (`lnk secret`): each value
//! in the Keychain where there is one (as bucket keys are), else in
//! Link's settings (mode 600), with how it's given and where it's
//! granted kept beside it. A secret goes only where a grant names, so a
//! file naming one is never enough to be given it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use link_id::{Resource, Workload};
use link_principal::Principal;
use serde::{Deserialize, Serialize};

pub use crate::place::Place;
use link_secretive::Secret;

pub mod at_rest;
pub mod keeper;
pub mod route;
use keeper::{Holder, Own, Request, holder};
mod changing;
mod granting;
mod removing;
mod sealing;
mod stored;
pub use changing::{Added, LeftItem};
use changing::{add_in, copied_in, gone_from_in, grant_in, owner};
pub use granting::{Action, GRANTEE, Grant, Grantee, GranteeAt, Use, grantee, service_principal};
pub use removing::{
    COPIES, Copies, Copy, Reach, Reached, Removal, Removed, Removing, held, reach, remove,
};
use removing::{held_in, reached, remove_here};
pub use sealing::Sealing;
#[cfg(test)]
use stored::parse;
pub use stored::{
    Called, ForService, Listed, Owner, SecretKind, Which, Yours, check_store, resource,
};
use stored::{Kept, Left, Secrets};

/// The kind a secret's ID is written with: `secret:<id>`.
pub const SECRET_KIND: &str = "secret";

/// Your secrets' file, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<stored::Secrets>()
}

/// Where secrets are kept: `~/.lnk/safe/secrets/secrets.toml`.
pub fn path() -> PathBuf {
    link_files::layout::Hidden::Secrets.path()
}

/// A user's secrets kept as files: `secrets.toml` (mode 600) and its
/// lock beside it. Yours ([`Store::yours`]) in Link's settings, or one
/// the keeper keeps for a user ([`keeper`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    file: PathBuf,
}

impl Store {
    /// Yours, in Link's settings: [`path`].
    pub fn yours() -> Store {
        Store { file: path() }
    }

    /// The store whose file is `file`.
    pub fn at(file: PathBuf) -> Store {
        Store { file }
    }

    /// Its file.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

/// Where its Keychain accounts are: [`account`]'s prefix.
pub const KEYCHAIN: &str = "secret/";

/// The Keychain account of the secret whose ID is `id`: `secret/<id>`.
fn account(id: &str) -> String {
    format!("{KEYCHAIN}{id}")
}

/// Each secret kept for you, by ID: how it's given and where, never its
/// value. Those no longer kept but with copies left elsewhere are listed
/// too, by their copies alone.
pub fn list() -> anyhow::Result<Vec<Listed>> {
    holder()?.ask(Request::List)?.listed()
}

/// What the service whose ID is `id` is granted, of `which`, each as
/// it's given: its own or what `lnk secret add --to` gave it. Only those
/// are read. A value the Keychain should hold and doesn't is an error,
/// naming it.
pub fn for_service(id: &str, which: Which) -> anyhow::Result<Vec<ForService>> {
    holder()?
        .ask(Request::ForService {
            id: id.to_string(),
            which,
        })?
        .for_service()
}

/// Your own secrets whose IDs are `ids`, read as their holder, for what
/// you run yourself (an environment you approved them for): each still
/// kept, with its name and how it's given; one deleted since is left
/// out. Only those are read.
pub fn for_you(ids: &[String]) -> anyhow::Result<Vec<Yours>> {
    holder()?
        .ask(Request::ForYou { ids: ids.to_vec() })?
        .for_you()
}

/// Keeps secret `name` as `value`, under a new ID, sealed when there's
/// `sealed`, given each of `to` as a variable: replacing whatever was kept
/// as `name`, whose ID is retired with its grants ([`Added`]).
pub fn add(
    name: &str,
    value: &Secret,
    sealed: Option<Sealing>,
    to: Vec<Grantee>,
) -> anyhow::Result<Added> {
    holder()?
        .ask(Request::Add {
            name: name.to_string(),
            value: value.duplicate(),
            sealed,
            to,
        })?
        .replaced()
}

/// Grants each of `add` the secret `secret` names, as a variable, and
/// takes it from each of `remove`, by the action its kind gives: whether
/// any grant changed.
pub fn grant(secret: &Called, add: Vec<Grantee>, remove: Vec<Grantee>) -> anyhow::Result<bool> {
    holder()?
        .ask(Request::Grant {
            called: secret.clone(),
            add,
            remove,
        })?
        .granted()
}

/// Keeps `grantee` its own secrets as `own` holds them now, each by the
/// service it acts as and its use, given to it as its use says (none: as
/// it's kept, a value not read): one no longer in `own` goes, and so does
/// an entry no grant is left for. What `lnk secret add` gave it is left
/// as it is. Written only when something changed.
pub fn keep_own(grantee: &Grantee, own: Vec<(Use, Option<Secret>)>) -> anyhow::Result<()> {
    holder()?
        .ask(Request::KeepOwn {
            id: grantee.service().id().to_string(),
            workload: grantee.workload().to_string(),
            name: grantee.name().to_string(),
            own: own
                .into_iter()
                .map(|(r#as, value)| Own { r#as, value })
                .collect(),
        })?
        .done()
}

/// Takes back every grant to the service whose ID is `id`, when what acts
/// as it leaves this machine or is removed: an entry nothing else is
/// granted goes, its value with it, and its ID is retired. The copies
/// given elsewhere of its own secrets are left (`[left]`), following the
/// service, for `lnk secret delete`.
pub fn forget(id: &str) -> anyhow::Result<()> {
    holder()?
        .ask(Request::Forget { id: id.to_string() })?
        .done()
}

/// Notes `to` as where the service whose ID is `id` is given a copy of
/// each of its own secrets, before its owner sends them there: those kept
/// for it, and those it took when it left here before (from a box it went
/// to, to the next). Never a secret `lnk secret add` gave it, which is
/// never sent.
pub fn copied(id: &str, to: Place) -> anyhow::Result<()> {
    copied_without(id, to, Vec::new())
}

/// Record only the own credentials actually sent to the destination.
pub fn copied_without(id: &str, to: Place, without: Vec<Use>) -> anyhow::Result<()> {
    holder()?
        .ask(Request::Copied {
            id: id.to_string(),
            to,
            without,
        })?
        .done()
}

/// Keep selected own credentials on this machine while the service's
/// workload is away, without granting access. An empty list releases the
/// retention.
pub fn retain_own(id: &str, uses: Vec<Use>) -> anyhow::Result<()> {
    holder()?
        .ask(Request::RetainOwn {
            id: id.into(),
            uses,
        })?
        .done()
}

/// The service whose ID is `id` has left `from`, a box or an account by
/// its name (its owner there deleted its copy, its secrets with it, or the
/// account removed it): no longer noted there.
pub fn gone_from(id: &str, from: &Place) -> anyhow::Result<()> {
    gone_from_without(id, from, Vec::new())
}

/// Forget only credentials that left the source, keeping excluded uses
/// noted there when a host supplied their replacements.
pub fn gone_from_without(id: &str, from: &Place, without: Vec<Use>) -> anyhow::Result<()> {
    holder()?
        .ask(Request::GoneFrom {
            id: id.to_string(),
            from: from.clone(),
            without,
        })?
        .done()
}

/// Every secret kept for you gone, and their store: by `lnk uninstall`,
/// which removes your own files itself, and so the keeper's for you.
/// First each copy a move left elsewhere is removed through `reach`, as
/// [`remove`] does: one it can't reach is refused, its note and the
/// store kept, so the store is never cleared of the only note that a key
/// is still on a box. A copy on a cloud account goes with the workload
/// it was moved for, as [`remove`] says, and doesn't hold it.
pub fn clear(reach: &Reach<'_>) -> anyhow::Result<()> {
    let mut left = Vec::new();
    for listed in list()?.into_iter().filter(|l| !l.copies.is_empty()) {
        let removed = remove(&Removal::Secret(Called::Id(listed.id)), Some(reach))?;
        left.extend(
            removed
                .unreached
                .into_iter()
                .map(|(place, why)| format!("{place}: {why}")),
        );
    }
    anyhow::ensure!(
        left.is_empty(),
        "your secrets stay until the copies a move left are removed, so a key on a box \
         is never forgotten: {}",
        left.join("; ")
    );
    holder()?.ask(Request::Clear)?.done()
}

/// The name a sealed secret has on the proxy: `stripe-key` for
/// `STRIPE_KEY`.
pub fn proxy_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

/// The variable what runs calls a sealed secret's service at, on its
/// proxy, in place of `https://<host>`: `LNK_SEALED_<NAME>_URL`.
pub fn sealed_url_var(name: &str) -> String {
    format!("LNK_SEALED_{name}_URL")
}

/// [`keep_own`], in `store`.
fn keep_own_in(
    store: &Store,
    grantee: &Grantee,
    own: Vec<(Use, Option<Secret>)>,
) -> anyhow::Result<()> {
    let _locked = store.lock()?;
    let mut secrets = store.load()?;
    let kept = |id: &str| crate::keychain::fetch(&account(id), None);
    let changed = secrets.own(grantee, own, &kept)?;
    secrets.apply(store, changed)
}

/// [`forget`], in `store`.
fn forget_in(store: &Store, id: &str) -> anyhow::Result<()> {
    let _locked = store.lock()?;
    let mut secrets = store.load()?;
    let changed = secrets.forget(id)?;
    secrets.apply(store, changed)
}
