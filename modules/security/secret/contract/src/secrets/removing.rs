//! A secret deleted: here and from each place a copy reached ([`remove`]),
//! and what this machine still holds ([`held`]).

use super::*;

/// What reaching a copy found ([`Reach`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Reached {
    /// It's gone from there: removed now, or gone with its box or service.
    Removed,
    /// It's held there with its service, which an account removes with it.
    WithService,
    /// It couldn't be reached, and why.
    Unreached(String),
}

/// How [`remove`] reaches the copies: what it found of each, in order.
pub type Reach<'a> = dyn Fn(&[Copy]) -> Vec<Reached> + 'a;

/// `lnk secret delete`'s question to the modules that give a service's
/// own secrets' copies to other places: reach each copy
/// ([`Copies`]), what each found, in order.
pub const COPIES: link_core::answers::Question<Copies, Vec<Reached>> =
    link_core::answers::Question::new("secret.copies", include_str!("copies.toml"));

/// The copies [`COPIES`] asks about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copies {
    pub copies: Vec<Copy>,
}

/// The copies `copies`, reached by every module that answers [`COPIES`]:
/// each copy gone where any removed it, else held with its service where any
/// says so, else unreached, with each one's why. With none answering,
/// each is unreached.
pub fn reach(copies: &[Copy]) -> Vec<Reached> {
    let asked = Copies {
        copies: copies.to_vec(),
    };
    let answers = COPIES.ask(&asked);
    copies
        .iter()
        .enumerate()
        .map(|(at, _)| {
            let found: Vec<Reached> = answers
                .iter()
                .map(|answer| match &answer.reply {
                    Ok(reached) => reached.get(at).cloned().unwrap_or_else(|| {
                        Reached::Unreached(format!("the {} module didn't say", answer.module))
                    }),
                    Err(err) => Reached::Unreached(format!("{err:#}")),
                })
                .collect();
            if found.contains(&Reached::Removed) {
                return Reached::Removed;
            }
            if found.contains(&Reached::WithService) {
                return Reached::WithService;
            }
            let why: Vec<String> = found
                .into_iter()
                .filter_map(|r| match r {
                    Reached::Unreached(why) => Some(why),
                    _ => None,
                })
                .collect();
            Reached::Unreached(match why.is_empty() {
                true => "no module here reaches it: the one that gave it".into(),
                false => why.join("; "),
            })
        })
        .collect()
}

/// What [`remove`] removes: a secret, by its name or ID, here and from
/// each place its owner gave a copy to; or, on a box given one, the own
/// secret it holds for that service and use, here alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Removal {
    Secret(Called),
    Own(Owner),
}

impl std::fmt::Display for Removal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Removal::Secret(secret) => write!(f, "{secret}"),
            Removal::Own(owner) => write!(f, "{}'s own, {}", owner.principal, owner.r#as),
        }
    }
}

/// What [`remove`] did.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Removed {
    /// The ID of the secret removed, kept here or left elsewhere: none
    /// when there's none.
    pub id: Option<String>,
    /// Whether it was kept here.
    pub here: bool,
    /// The places it's gone from.
    pub reached: Vec<Place>,
    /// The boxes it couldn't be removed from, each with why: left noted
    /// for the next `remove`.
    pub unreached: Vec<(Place, String)>,
    /// The cloud accounts holding a copy with its service there, which
    /// goes with the service: left noted until it's gone from there.
    pub with_service: Vec<Place>,
}

/// Removes `what` from this machine, its value and its grants, its ID
/// retired, then from each place a move gave a copy to, by `reach` (all
/// of them asked at once, [`reach`] asks the modules that answer
/// [`COPIES`]); with none, no place is tried and every copy stays noted.
/// Every copy is noted as left (`[left]`) before any is tried, and each
/// reached is taken off after, so nothing is forgotten if it's cut short.
pub fn remove(what: &Removal, reach: Option<&Reach<'_>>) -> anyhow::Result<Removed> {
    let holder = holder()?;
    let Removing {
        mut removed,
        copies,
    } = holder
        .ask(Request::Remove { what: what.clone() })?
        .removing()?;
    let (Some(reach), Some(id)) = (reach, removed.id.clone()) else {
        return Ok(removed);
    };
    let mut reached = Vec::new();
    let found = match copies.is_empty() {
        true => Vec::new(),
        false => reach(&copies),
    };
    for (copy, found) in copies.into_iter().zip(
        found
            .into_iter()
            .chain(std::iter::repeat(Reached::Unreached("not reached".into()))),
    ) {
        match found {
            Reached::Removed => {
                removed.reached.push(copy.place.clone());
                reached.push(copy);
            }
            Reached::WithService => removed.with_service.push(copy.place),
            // An account removes it with its service, whether it answers or not.
            Reached::Unreached(_) if copy.place.cloud => removed.with_service.push(copy.place),
            Reached::Unreached(why) => removed.unreached.push((copy.place, why)),
        }
    }
    if !reached.is_empty() {
        holder.ask(Request::Reached { id, reached })?.done()?;
    }
    Ok(removed)
}

/// A copy of a service's own secret elsewhere: where, and whose and for
/// what, which is how it's found there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    pub owner: Owner,
    pub place: Place,
}

/// [`remove`]'s first step, here: what it did, and the copies elsewhere
/// it's to reach, each noted as left before any is tried.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Removing {
    pub removed: Removed,
    pub copies: Vec<Copy>,
}

/// [`remove`]'s step here: the value and grants gone, the ID retired,
/// every copy noted as left.
pub(super) fn remove_here(store: &Store, what: &Removal) -> anyhow::Result<Removing> {
    let mut removed = Removed::default();
    let _locked = store.lock()?;
    let mut secrets = store.load()?;
    let kept = match what {
        Removal::Secret(secret) => secrets.find(secret),
        Removal::Own(owner) => secrets.own_of(&owner.principal, &owner.r#as),
    };
    let id = match (what, &kept) {
        (_, Some(id)) => Some(id.clone()),
        // Not kept here, but left elsewhere: reached by its ID alone.
        (Removal::Secret(Called::Id(id)), None) => {
            secrets.left.contains_key(id).then(|| id.clone())
        }
        _ => None,
    };
    removed.here = kept.is_some();
    if let Some(kept) = &kept {
        secrets.drop_kept(kept, None);
    }
    let copies = id
        .as_ref()
        .and_then(|id| secrets.left.get(id))
        .map(|left| {
            left.copies
                .iter()
                .map(|place| Copy {
                    owner: left.owner(),
                    place: place.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    removed.id = id;
    if let Some(kept) = &kept {
        store.save(&secrets)?;
        // Once the file no longer names it, as `apply` does: an item the
        // Keychain won't let go of is named, since nothing here names it
        // any more.
        let item = account(kept);
        crate::keychain::stash(&item, None).with_context(|| {
            format!(
                "the secret is deleted, but the Keychain kept its item, link:{item}, which \
                 holds its value and nothing reaches: delete it with Keychain Access"
            )
        })?;
    }
    Ok(Removing { removed, copies })
}

/// [`remove`]'s last step: the copies `reached` of the secret whose ID is
/// `id` no longer noted as left.
pub(super) fn reached(store: &Store, id: &str, reached: &[Copy]) -> anyhow::Result<()> {
    let _locked = store.lock()?;
    let mut secrets = store.load()?;
    if let Some(left) = secrets.left.get_mut(id) {
        for copy in reached {
            if left.owner() == copy.owner {
                left.copies.remove(&copy.place);
            }
        }
    }
    secrets.tidy();
    store.save(&secrets)
}

/// The secrets kept for you, as `lnk secret list` shows them: where each
/// is kept and where it's given, never its value. A store that can't be
/// read, or a keeper that doesn't answer, is one entry saying so.
pub fn held() -> Vec<crate::uninstall::Held> {
    let asked = holder().and_then(|holder| holder.ask(Request::Held)?.held());
    asked.unwrap_or_else(|err| {
        vec![crate::uninstall::Held {
            what: link_core::uninstall::CredentialKind::UnreadableSecrets.into(),
            of: String::new(),
            kept: holder()
                .map_or_else(|_| Holder::Files(Store::yours()).kept_by(), |h| h.kept_by()),
            may: format!("Nothing until they're read: {err:#}"),
        }]
    })
}

/// The secrets kept here, as `lnk secret list` shows them: where each is
/// kept, where it's given and where a move left a copy, never its value.
/// One no longer kept here, with copies left elsewhere, is listed by
/// those copies. A store that can't be read is one entry saying so,
/// never none.
pub(super) fn held_in(store: &Store, kept_by: &str) -> Vec<crate::uninstall::Held> {
    let secrets = match store.load() {
        Ok(secrets) => secrets,
        Err(err) => {
            return vec![crate::uninstall::Held {
                what: link_core::uninstall::CredentialKind::UnreadableSecrets.into(),
                of: String::new(),
                kept: kept_by.to_string(),
                may: format!("Nothing until it's read: {err:#}"),
            }];
        }
    };
    secrets
        .list()
        .into_iter()
        .map(|listed| {
            let copies = listed
                .copies
                .iter()
                .map(Place::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let (kept, may) = match secrets.entry.get(&listed.id) {
                None => (
                    format!("Not here: copies left on {copies}"),
                    "Whatever runs there, until lnk secret delete reaches them".to_string(),
                ),
                Some(here) => {
                    // One given to nothing yet says so, never an empty list.
                    let to = match listed.to.is_empty() {
                        true => "no one yet".to_string(),
                        false => listed
                            .to
                            .iter()
                            .map(Grant::to_string)
                            .collect::<Vec<_>>()
                            .join(", "),
                    };
                    let may = match &listed.sealed {
                        Some(sealing) => format!(
                            "Added by the proxy to calls to {} alone, in {}: {}, for {to}",
                            sealing.host,
                            sealing.header,
                            sealing.format()
                        ),
                        None if listed.to.is_empty() => format!("Given to {to}"),
                        None => format!("Given to what runs in {to}"),
                    };
                    let kept = match here.value {
                        Some(_) => kept_by.to_string(),
                        None => format!(
                            "the Keychain ({}:{})",
                            crate::keychain::SERVICE,
                            account(&listed.id)
                        ),
                    };
                    match copies.is_empty() {
                        true => (kept, may),
                        false => (kept, format!("{may}; copies on {copies}")),
                    }
                }
            };
            crate::uninstall::Held {
                what: link_core::uninstall::CredentialKind::Secret.into(),
                of: listed.called(),
                kept,
                may,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {

    /// The question this owns declares its protocol beside it.

    #[test]

    fn its_question_declares_its_protocol() {
        let protocol = COPIES.protocol();

        assert!(protocol.oldest <= protocol.current, "{protocol:?}");
    }
    use super::*;

    #[test]
    fn held_names_a_secrets_copies_and_one_left_only_elsewhere() {
        let dir = link_test_kit::TempDir::new("held-copies");
        let store = Store::at(dir.join("secrets.toml"));
        let (kept, gone, service, workload) = (
            link_id::candidate().unwrap(),
            link_id::candidate().unwrap(),
            link_id::candidate().unwrap(),
            link_id::candidate().unwrap(),
        );
        let text = format!(
            "schema = 1\n[entry.{kept}]\nkind = \"named\"\nname = \"KEPT\"\nvalue = \"the-kept-value\"\n\
             to = [{{ principal = \"service:{service}\", action = \"secret.secret.read\", resource = \"secret:{kept}\", workload = \"run:{workload}\", name = \"main\", as = {{ kind = \"variable\" }} }}]\n\n\
             [left.{gone}]\nprincipal = \"service:{service}\"\nas = {{ kind = \"own\", purpose = \"model\" }}\n\
             copies = [{{ name = \"gcp-1\" }}]\n"
        );
        std::fs::write(store.file(), text).unwrap();
        let held = held_in(&store, "the file");
        let of = |name: &str| held.iter().find(|h| h.of.starts_with(name)).unwrap();
        assert_eq!(of("KEPT").kept, "the file");
        assert_eq!(
            of("KEPT").may,
            format!("Given to what runs in main (run:{workload})")
        );
        assert_eq!(
            of(&format!("service:{service}")).kept,
            "Not here: copies left on gcp-1"
        );
        assert!(!format!("{held:?}").contains("the-kept-value"));
    }

    #[test]
    fn held_says_a_secret_given_to_nothing_is_given_to_no_one_yet() {
        let dir = link_test_kit::TempDir::new("held-ungiven");
        let store = Store::at(dir.join("secrets.toml"));
        let kept = link_id::candidate().unwrap();
        let text = format!(
            "schema = 1\n[entry.{kept}]\nkind = \"named\"\nname = \"IDLE\"\nvalue = \"the-idle-value\"\nto = []\n"
        );
        std::fs::write(store.file(), text).unwrap();
        let held = held_in(&store, "the file");
        let idle = held.iter().find(|h| h.of.starts_with("IDLE")).unwrap();
        assert_eq!(idle.may, "Given to no one yet");
    }
}
