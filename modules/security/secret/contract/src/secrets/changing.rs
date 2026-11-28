//! What changes the file: a secret added, a service's own kept or
//! forgotten, and its copies followed where it goes.

use super::*;

/// What [`Secrets::own`] and [`Secrets::forget`] changed, for the
/// Keychain and the file to follow: whether anything did, the values set
/// (the Keychain takes each where there's one), and the entries gone, by
/// ID.
#[derive(Default)]
pub(super) struct Changed {
    any: bool,
    set: Vec<String>,
    gone: Vec<String>,
}

impl Secrets {
    /// Writes what `changed` says: each value set to the Keychain where
    /// there's one, each entry gone from it, and the file.
    pub(super) fn apply(&mut self, store: &Store, changed: Changed) -> anyhow::Result<()> {
        for id in &changed.set {
            if let Some(kept) = self.entry.get_mut(id) {
                let value = kept.value.take();
                kept.value = crate::keychain::stash(&account(id), value.as_ref())?;
            }
        }
        if changed.any {
            store.save(self)?;
        }
        // Once the file no longer names them: a save that fails leaves an
        // item the Keychain holds for nothing, never an entry with no value.
        for id in &changed.gone {
            crate::keychain::stash(&account(id), None)?;
        }
        Ok(())
    }

    /// [`keep_own`]'s change, `in_keychain` the value an entry the
    /// Keychain holds has, by its ID.
    pub(super) fn own(
        &mut self,
        grantee: &Grantee,
        own: Vec<(Use, Option<Secret>)>,
        in_keychain: &dyn Fn(&str) -> anyhow::Result<Option<Secret>>,
    ) -> anyhow::Result<Changed> {
        let of = grantee.service().clone();
        let name = grantee.name();
        let mut changed = Changed::default();
        let mut wanted: BTreeMap<Use, Option<Secret>> = BTreeMap::new();
        for (r#as, value) in own {
            ensure!(
                r#as.is_own(),
                "a variable is named by lnk secret create, not by what keeps it"
            );
            if let Some(value) = &value {
                if value.is_empty() {
                    continue;
                }
                if value.expose().contains(['\n', '\r', '\0']) {
                    bail!("{name}'s {as_} is one line", as_ = r#as);
                }
            }
            wanted.insert(r#as, value);
        }
        // What's no longer its own.
        let ids: Vec<String> = self.entry.keys().cloned().collect();
        for key in ids {
            let kept = self.entry.get_mut(&key).expect("listed");
            let was = owner(kept);
            let before = kept.to.len();
            kept.to
                .retain(|g| !g.is_own_of(&of) || wanted.contains_key(&g.r#as));
            if kept.to.len() != before {
                changed.any = true;
                if kept.to.is_empty() && kept.retained.is_empty() {
                    self.drop_kept(&key, was);
                    changed.gone.push(key);
                }
            }
        }
        for (r#as, value) in wanted {
            let found = self.own_of(&of, &r#as);
            // Not read: as it's kept, if it is.
            let Some(value) = value else {
                if let Some(key) = found {
                    let kept = self.entry.get_mut(&key).expect("found");
                    let grant = Grant::new(grantee, r#as, &key, kept.sealed.is_some())?;
                    changed.any |= kept.retained.remove(&grant);
                    changed.any |= granted(kept, grant);
                }
                continue;
            };
            let key = match found {
                Some(key) => key,
                None => {
                    let key = self.mint()?;
                    let own = Kept {
                        kind: SecretKind::Own,
                        ..Kept::default()
                    };
                    self.entry.insert(key.clone(), own);
                    key
                }
            };
            let now = match self.entry.get(&key).map(|kept| &kept.value) {
                Some(Some(value)) => Some(value.duplicate()),
                Some(None) => in_keychain(&key)?,
                None => None,
            };
            let kept = self.entry.get_mut(&key).expect("kept");
            let grant = Grant::new(grantee, r#as, &key, kept.sealed.is_some())?;
            kept.retained.retain(|g| !g.same_as(&grant));
            if now.as_ref() != Some(&value) {
                kept.value = Some(value);
                changed.set.push(key.clone());
                changed.any = true;
            }
            changed.any |= granted(kept, grant);
        }
        Ok(changed)
    }

    /// [`forget`]'s change.
    pub(super) fn forget(&mut self, id: &str) -> anyhow::Result<Changed> {
        let of = service_principal(id)?;
        let mut changed = Changed::default();
        let ids: Vec<String> = self.entry.keys().cloned().collect();
        for key in ids {
            let kept = self.entry.get_mut(&key).expect("listed");
            let was = owner(kept);
            let before = kept.to.len();
            kept.to.retain(|g| g.principal != of);
            if kept.to.len() == before {
                continue;
            }
            changed.any = true;
            // Its value stays only while something else is granted it.
            if kept.to.is_empty() && kept.retained.is_empty() {
                self.drop_kept(&key, was);
                changed.gone.push(key);
            }
        }
        Ok(changed)
    }

    /// Retention is separate from a live grant, so a parked own secret
    /// stays in its keeper and explicit removal still deletes it.
    pub(super) fn retain(&mut self, id: &str, uses: &[Use]) -> anyhow::Result<Changed> {
        let of = service_principal(id)?;
        let mut changed = Changed::default();
        let mut drop = Vec::new();
        for (key, kept) in &mut self.entry {
            let was = owner(kept);
            let before = kept.retained.clone();
            kept.retained
                .retain(|g| !(g.principal == of && !uses.contains(&g.r#as)));
            kept.retained.extend(
                kept.to
                    .iter()
                    .filter(|g| g.is_own_of(&of) && uses.contains(&g.r#as))
                    .cloned(),
            );
            changed.any |= before != kept.retained;
            if !before.is_empty() && kept.retained.is_empty() && kept.to.is_empty() {
                drop.push((key.clone(), was));
            }
        }
        for (key, was) in drop {
            self.drop_kept(&key, was);
            changed.gone.push(key);
        }
        Ok(changed)
    }

    /// Entry `id` gone from here, its ID retired: its copies elsewhere, if
    /// any, left for `remove` (`[left]`), following `was`, the service
    /// whose own it was before its grants went.
    pub(super) fn drop_kept(&mut self, id: &str, was: Option<Owner>) {
        let Some(kept) = self.entry.remove(id) else {
            return;
        };
        self.retired.insert(id.to_string());
        if let Some(owner) = was.or_else(|| owner(&kept))
            && !kept.copies.is_empty()
        {
            let left = self.left.entry(id.to_string()).or_insert(Left {
                principal: owner.principal,
                r#as: owner.r#as,
                copies: BTreeSet::new(),
            });
            left.copies.extend(kept.copies);
        }
    }

    /// `[left]` without an entry that has no copies.
    pub(super) fn tidy(&mut self) {
        self.left.retain(|_, left| !left.copies.is_empty());
    }

    /// The copies of the own secrets of the service whose ID is `id`, but those
    /// for a use in `without`, each to change by `change`: those of the
    /// entries kept here for it, and those left that follow it. Whether
    /// any changed; a left one with none is gone.
    fn copies_of(
        &mut self,
        id: &str,
        without: &[Use],
        change: &dyn Fn(&mut BTreeSet<Place>) -> bool,
    ) -> anyhow::Result<bool> {
        let of = service_principal(id)?;
        let mut any = false;
        for kept in self.entry.values_mut() {
            let own = kept
                .to
                .iter()
                .any(|g| g.is_own_of(&of) && !without.contains(&g.r#as));
            if own {
                any |= change(&mut kept.copies);
            }
        }
        for left in self.left.values_mut() {
            if left.principal == of && !without.contains(&left.r#as) {
                any |= change(&mut left.copies);
            }
        }
        self.tidy();
        Ok(any)
    }
}

/// Grants `grant` in `kept`, in place of one to the same workload the
/// same way (by a name since changed): whether that changed it.
fn granted(kept: &mut Kept, grant: Grant) -> bool {
    if kept.to.contains(&grant) {
        return false;
    }
    kept.to.retain(|g| !g.same_as(&grant));
    kept.to.push(grant);
    kept.to.sort();
    true
}

/// Whose own secret `kept` is, and for what, if it's a service's own:
/// by its kind, never by which grants it has.
pub(super) fn owner(kept: &Kept) -> Option<Owner> {
    if kept.kind != SecretKind::Own {
        return None;
    }
    kept.to
        .iter()
        .chain(&kept.retained)
        .find(|g| g.r#as.is_own())
        .map(|g| Owner {
            principal: g.principal.clone(),
            r#as: g.r#as.clone(),
        })
}

/// [`copied`], in `store`.
pub(super) fn copied_in(store: &Store, id: &str, to: Place, without: &[Use]) -> anyhow::Result<()> {
    to.check()?;
    let _locked = store.lock()?;
    let mut secrets = store.load()?;
    let noted = |copies: &mut BTreeSet<Place>| {
        if copies.contains(&to) {
            return false;
        }
        // A box of the name made since: this one is where it goes now.
        copies.retain(|c| !c.is(&to));
        copies.insert(to.clone())
    };
    if secrets.copies_of(id, without, &noted)? {
        store.save(&secrets)?;
    }
    Ok(())
}

/// [`gone_from`], in `store`.
pub(super) fn gone_from_in(
    store: &Store,
    id: &str,
    from: &Place,
    without: &[Use],
) -> anyhow::Result<()> {
    let _locked = store.lock()?;
    let mut secrets = store.load()?;
    let gone = |copies: &mut BTreeSet<Place>| {
        let before = copies.len();
        copies.retain(|c| !c.is(from));
        copies.len() != before
    };
    if secrets.copies_of(id, without, &gone)? {
        store.save(&secrets)?;
    }
    Ok(())
}

/// A secret kept by [`add`]: its new ID, what it replaced, and the
/// Keychain account of what it replaced when removing that item failed,
/// with why: the secret is kept all the same, and the old item holds a
/// value nothing reaches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Added {
    pub id: String,
    pub before: Option<Listed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<LeftItem>,
}

/// A Keychain item [`add`] couldn't remove: its account, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeftItem {
    pub account: String,
    pub why: String,
}

/// [`add`], in `store`.
pub(super) fn add_in(
    store: &Store,
    name: &str,
    value: &Secret,
    sealed: Option<Sealing>,
    to: Vec<Grantee>,
) -> anyhow::Result<Added> {
    link_spec::check_variable(name)?;
    if value.is_empty() || value.expose().contains(['\n', '\r', '\0']) {
        bail!("a secret is one line, not empty");
    }
    let _locked = store.lock()?;
    let mut secrets = store.load()?;
    ensure!(
        sealed.is_some() || to.is_empty(),
        "{name} is kept plain, and a workload is given a secret sealed only: create it with \
         --internet-host"
    );
    let named = Called::Name(name.to_string());
    let before = secrets.list().into_iter().find(|l| l.kept && l.is(&named));
    let id = secrets.mint()?;
    let mut to = to
        .iter()
        .map(|g| Grant::new(g, Use::Variable, &id, sealed.is_some()))
        .collect::<anyhow::Result<Vec<_>>>()?;
    to.sort();
    to.dedup();
    let value = crate::keychain::stash(&account(&id), Some(value))?;
    // What it replaces is deleted: its ID retired, its grants with it.
    if let Some(before) = &before {
        secrets.drop_kept(&before.id, None);
    }
    secrets.entry.insert(
        id.clone(),
        Kept {
            kind: SecretKind::Named,
            name: Some(name.to_string()),
            value,
            sealed,
            to,
            ..Kept::default()
        },
    );
    store.save(&secrets)?;
    // The new secret is kept and the old one's ID retired: an item the
    // Keychain won't let go of is a warning, never a failed create.
    let left = before.as_ref().and_then(|before| {
        let account = account(&before.id);
        crate::keychain::stash(&account, None)
            .err()
            .map(|err| LeftItem {
                account,
                why: link_format::clean(&format!("{err:#}")),
            })
    });
    Ok(Added { id, before, left })
}

/// Grants `add` to the secret `secret` names and takes `remove` from it,
/// in `store`: whether any grant changed. A workload is given a secret
/// sealed only, and an own secret is never granted by hand.
pub(super) fn grant_in(
    store: &Store,
    secret: &Called,
    add: Vec<Grantee>,
    remove: Vec<Grantee>,
) -> anyhow::Result<bool> {
    let _locked = store.lock()?;
    let mut secrets = store.load()?;
    let Some(id) = secrets.find(secret) else {
        bail!("there is no secret {secret} here");
    };
    let kept = secrets.entry.get_mut(&id).expect("found");
    let Some(name) = kept.name.clone() else {
        bail!(
            "{secret} is a service's own secret, kept by the module that keeps that service, \
             and granted to that service alone"
        );
    };
    ensure!(
        kept.sealed.is_some() || add.is_empty(),
        "{name} is kept plain, and a workload is given a secret sealed only: create it again with \
         --internet-host"
    );
    let before = kept.to.clone();
    let sealed = kept.sealed.is_some();
    kept.to
        .retain(|g| !remove.iter().any(|r| g.is_variable_of(r)));
    for grantee in add {
        granted(kept, Grant::new(&grantee, Use::Variable, &id, sealed)?);
    }
    kept.to.sort();
    kept.to.dedup();
    let changed = kept.to != before;
    if changed {
        store.save(&secrets)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::super::granting::test_grantee;
    use super::*;

    /// Everything the service whose ID is `id` is granted, its own first.
    fn all_for(secrets: &Secrets, id: &str) -> anyhow::Result<Vec<ForService>> {
        let mut all = secrets.for_service(id, Which::Own)?;
        all.extend(secrets.for_service(id, Which::Variables)?);
        Ok(all)
    }

    /// Never the Keychain: values here are the file's.
    fn none(_: &str) -> anyhow::Result<Option<Secret>> {
        Ok(None)
    }

    /// A plain secret `name`, given each of `to` as a variable.
    fn named(secrets: &mut Secrets, name: &str, to: Vec<Grantee>) -> String {
        let to = to.into_iter().map(|g| (g, Use::Variable)).collect();
        named_as(secrets, name, None, to)
    }

    /// A secret `name`, kept `sealed` or plain, given each of `to` as its
    /// use says.
    fn named_as(
        secrets: &mut Secrets,
        name: &str,
        sealed: Option<Sealing>,
        to: Vec<(Grantee, Use)>,
    ) -> String {
        let id = secrets.mint().unwrap();
        let to = to
            .into_iter()
            .map(|(g, r#as)| Grant::new(&g, r#as, &id, sealed.is_some()).unwrap())
            .collect();
        secrets.entry.insert(
            id.clone(),
            Kept {
                name: Some(name.into()),
                value: Some("npm_x".to_string().into()),
                sealed,
                to,
                ..Kept::default()
            },
        );
        id
    }

    #[test]
    fn a_services_own_are_kept_by_its_id_and_use_and_go_with_it() {
        let mut secrets = Secrets::default();
        let id = link_id::candidate().unwrap();
        let telegram = Use::own("connection:telegram:bot_token").unwrap();
        let own = |model: &str, bot: &str| {
            vec![
                (
                    Use::own("model").unwrap(),
                    Some(Secret::from(model.to_string())),
                ),
                (telegram.clone(), Some(Secret::from(bot.to_string()))),
            ]
        };
        assert!(
            secrets
                .own(&test_grantee(&id, "main"), own("sk-1", "123:abc"), &none)
                .unwrap()
                .any
        );
        // As it is: nothing to write.
        assert!(
            !secrets
                .own(&test_grantee(&id, "main"), own("sk-1", "123:abc"), &none)
                .unwrap()
                .any
        );
        // Kept by the service's ID and their use: no name, none built from
        // the service's.
        let listed = secrets.list();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|l| l.name.is_none()), "{listed:?}");
        let model = secrets
            .own_of(
                &service_principal(&id).unwrap(),
                &Use::own("model").unwrap(),
            )
            .unwrap();
        // Renamed, it keeps them: the same entries, the grants' names new.
        assert!(
            secrets
                .own(&test_grantee(&id, "work"), own("sk-1", "123:abc"), &none)
                .unwrap()
                .any
        );
        assert_eq!(secrets.entry.len(), 2);
        assert_eq!(secrets.entry[&model].to[0].name, "work");
        // Variables you granted it, beside its own: one granted another
        // service too, one granted it alone.
        let work = test_grantee(&link_id::candidate().unwrap(), "work");
        let to_main = test_grantee(&id, "main");
        let npm = named(
            &mut secrets,
            "NPM_TOKEN",
            vec![to_main.clone(), work.clone()],
        );
        named(&mut secrets, "ONLY_MAIN", vec![to_main]);
        let given = all_for(&secrets, &id).unwrap();
        assert_eq!(given.len(), 4);
        assert!(
            given
                .iter()
                .any(|g| g.r#as == Use::own("model").unwrap() && g.value.expose() == "sk-1")
        );
        // Another service, or one made later with the name, has none of it.
        assert!(
            all_for(&secrets, &link_id::candidate().unwrap())
                .unwrap()
                .is_empty()
        );
        // Its connection disconnected: that token goes, the rest stays.
        let changed = secrets
            .own(
                &test_grantee(&id, "main"),
                vec![(Use::own("model").unwrap(), Some("sk-2".to_string().into()))],
                &none,
            )
            .unwrap();
        assert!(changed.any);
        assert_eq!(changed.gone.len(), 1);
        assert_eq!(changed.set, std::slice::from_ref(&model));
        assert!(secrets.retired.contains(&changed.gone[0]));
        assert_eq!(secrets.entry.len(), 3);
        // Gone from here: its grants go, and a value stays only while
        // something else is granted it.
        assert!(secrets.forget(&id).unwrap().any);
        let listed = secrets.list();
        assert_eq!(listed.len(), 1);
        let work = Grant::new(&work, Use::Variable, &npm, false).unwrap();
        assert_eq!(
            (listed[0].name.as_deref(), &listed[0].to),
            (Some("NPM_TOKEN"), &vec![work])
        );
        assert!(secrets.retired.contains(&model));
        assert!(
            secrets
                .own(
                    &test_grantee(&id, "main"),
                    vec![(Use::Variable, Some("s".to_string().into()))],
                    &none
                )
                .is_err()
        );
    }

    /// A secret deleted and made again with its name is a new secret: no
    /// grant or copy of the first reaches it, and the first's ID is never
    /// minted again.
    #[test]
    fn a_name_made_again_reaches_no_old_grant_or_copy() {
        let dir = link_test_kit::TempDir::new("secret-made-again");
        let store = Store::at(dir.join("secrets.toml"));
        let main = link_id::candidate().unwrap();
        let sealing = Sealing::new("api.example.com", None, None, None).unwrap();
        let value = Secret::from("v1".to_string());
        let first = add_in(&store, "API_KEY", &value, Some(sealing.clone()), Vec::new())
            .unwrap()
            .id;
        let name = Called::Name("API_KEY".into());
        let to_main = test_grantee(&main, "main");
        assert!(grant_in(&store, &name, vec![to_main.clone()], Vec::new()).unwrap());
        // Made again: a new ID, the first's retired and its grant gone.
        let value = Secret::from("v2".to_string());
        let Added {
            id: second,
            before,
            left,
        } = add_in(&store, "API_KEY", &value, Some(sealing), Vec::new()).unwrap();
        assert_eq!(left, None);
        assert_ne!(first, second);
        assert_eq!(before.unwrap().id, first);
        let secrets = store.load().unwrap();
        assert!(secrets.retired.contains(&first));
        assert!(
            secrets
                .for_service(&main, Which::Variables)
                .unwrap()
                .is_empty()
        );
        // Its first ID names nothing now, and is never given again.
        let gone = Called::Id(first.clone());
        assert!(grant_in(&store, &gone, vec![to_main], Vec::new()).is_err());
        let mut drawn = vec![link_id::candidate().unwrap(), second.clone(), first.clone()];
        let minted = secrets.mint_from(|| Ok(drawn.pop().unwrap())).unwrap();
        assert!(minted != first && minted != second, "{minted}");
        assert!(drawn.is_empty());
        // The same for a service's own key: deleted, then kept again for
        // a workload made with the name, its copies stay the first's.
        let mut secrets = Secrets::default();
        let first = link_id::candidate().unwrap();
        let key = || {
            vec![(
                Use::own("model").unwrap(),
                Some(Secret::from("sk-1".to_string())),
            )]
        };
        secrets
            .own(&test_grantee(&first, "main"), key(), &none)
            .unwrap();
        secrets
            .copies_of(&first, &[], &|c| c.insert(Place::of_box("aws-1", None)))
            .unwrap();
        secrets.forget(&first).unwrap();
        let again = link_id::candidate().unwrap();
        secrets
            .own(&test_grantee(&again, "main"), key(), &none)
            .unwrap();
        let listed = secrets.list();
        let kept: Vec<_> = listed.iter().filter(|l| l.kept).collect();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].copies.is_empty());
        assert!(!secrets.retired.contains(&kept[0].id));
    }

    /// Its owner notes where it sends a service's own secrets before it sends
    /// them; when the service leaves, those copies are left, follow it to
    /// its next place, and go once it leaves there. Never a variable
    /// you granted it, which a move doesn't send.
    #[test]
    fn copies_are_noted_and_follow_the_service() {
        let id = link_id::candidate().unwrap();
        let mut secrets = Secrets::default();
        secrets
            .own(
                &test_grantee(&id, "main"),
                vec![(Use::own("model").unwrap(), Some("sk-1".to_string().into()))],
                &none,
            )
            .unwrap();
        let npm = named(&mut secrets, "NPM_TOKEN", vec![test_grantee(&id, "main")]);
        let model = secrets
            .own_of(
                &service_principal(&id).unwrap(),
                &Use::own("model").unwrap(),
            )
            .unwrap();
        let aws = Place::of_box("aws-1", None);
        assert!(
            secrets
                .copies_of(&id, &[], &|c| c.insert(aws.clone()))
                .unwrap()
        );
        let copies = |s: &Secrets, id: &str| {
            s.list()
                .into_iter()
                .find(|l| l.id == id)
                .map(|l| (l.kept, l.copies))
        };
        assert_eq!(copies(&secrets, &model), Some((true, vec![aws.clone()])));
        assert_eq!(copies(&secrets, &npm), Some((true, vec![])));
        // It leaves: its key goes from here, its copy is left, following it.
        secrets.forget(&id).unwrap();
        assert_eq!(copies(&secrets, &model), Some((false, vec![aws.clone()])));
        assert_eq!(copies(&secrets, &npm), None);
        // Written and read back.
        let text = link_files::saved::print(&secrets).unwrap();
        assert!(text.contains(&format!("[left.{model}]")), "{text}");
        assert!(
            text.contains(&format!("principal = \"service:{id}\"")),
            "{text}"
        );
        let mut secrets = parse(&text).unwrap();
        // From aws-1 to an account: noted there, then gone from aws-1.
        let cloud = Place::of_account("cloud-1", None);
        secrets
            .copies_of(&id, &[], &|c| c.insert(cloud.clone()))
            .unwrap();
        secrets.copies_of(&id, &[], &|c| c.remove(&aws)).unwrap();
        assert_eq!(copies(&secrets, &model), Some((false, vec![cloud.clone()])));
        // Another service's moves never touch it.
        let other = link_id::candidate().unwrap();
        assert!(
            !secrets
                .copies_of(&other, &[], &|c| c.remove(&cloud))
                .unwrap()
        );
        // Back here: nothing left anywhere.
        secrets.copies_of(&id, &[], &|c| c.remove(&cloud)).unwrap();
        assert!(secrets.list().is_empty());
        // A copy's place is a name, as Link's are, and its service an ID.
        let left = |rest: &str| parse(&format!("schema = 1\n[left.{model}]\n{rest}\n"));
        let owner =
            format!("principal = \"service:{id}\"\nas = {{ kind = \"own\", purpose = \"model\" }}");
        assert!(left(&format!("{owner}\ncopies = [{{ name = \"a\" }}]")).is_ok());
        assert!(left(&format!("{owner}\ncopies = [{{ name = \"a b\" }}]")).is_err());
        assert!(
            left(&format!(
                "{owner}\ncopies = [{{ name = \"a\", disk = \"a\" }}]"
            ))
            .is_err()
        );
        assert!(
            left(&format!(
                "{owner}\ncopies = [{{ name = \"a\", id = \"../x\" }}]"
            ))
            .is_err()
        );
        assert!(
            left("workload = \"main\"\nas = {{ kind = \"own\", purpose = \"model\" }}\ncopies = [{ name = \"a\" }]")
                .is_err()
        );
        assert!(
            left(&format!(
                "workload = \"env:{id}\"\nas = {{ kind = \"own\", purpose = \"model\" }}\ncopies = [{{ name = \"a\" }}]"
            ))
            .is_err()
        );
        assert!(
            left(&format!(
                "principal = \"service:{id}\"\ncopies = [{{ name = \"a\" }}]"
            ))
            .is_err()
        );
    }

    /// A service's own secret is kept by its module, for that service alone: a
    /// secret granted by hand never hands it to another.
    #[test]
    fn an_own_secret_is_never_granted_by_hand() {
        let dir = link_test_kit::TempDir::new("own-key-by-hand");
        let store = Store::at(dir.join("secrets.toml"));
        let (main, other) = (link_id::candidate().unwrap(), link_id::candidate().unwrap());
        let mut secrets = Secrets::default();
        secrets
            .own(
                &test_grantee(&main, "main"),
                vec![(
                    Use::own("model").unwrap(),
                    Some("sk-main".to_string().into()),
                )],
                &none,
            )
            .unwrap();
        store.save(&secrets).unwrap();
        let model = Called::Id(secrets.entry.keys().next().unwrap().clone());
        let kept = std::fs::read_to_string(store.file()).unwrap();
        let to_other = test_grantee(&other, "other");
        let err = grant_in(&store, &model, vec![to_other], Vec::new()).unwrap_err();
        assert!(err.to_string().contains("a service's own secret"), "{err}");
        let owner = test_grantee(&main, "main");
        assert!(grant_in(&store, &model, vec![owner.clone()], Vec::new()).is_err());
        assert!(grant_in(&store, &model, Vec::new(), vec![owner]).is_err());
        assert_eq!(std::fs::read_to_string(store.file()).unwrap(), kept);
    }

    /// A plain secret is created for no one: a workload is given one
    /// sealed only, however it's asked.
    #[test]
    fn a_plain_secret_is_created_with_no_grant() {
        let dir = link_test_kit::TempDir::new("add-plain");
        let store = Store::at(dir.join("secrets.toml"));
        let main = test_grantee(&link_id::candidate().unwrap(), "main");
        let value = Secret::from("hunter2".to_string());
        let refused = add_in(&store, "NPM_TOKEN", &value, None, vec![main]).unwrap_err();
        assert!(refused.to_string().contains("sealed only"), "{refused}");
        assert!(!store.file().exists());
    }

    /// A grant is told from another by the workload and how it's given the
    /// secret: a variable never replaces or removes a model's key.
    #[test]
    fn a_grant_is_matched_by_the_service_and_how_it_is_given() {
        let dir = link_test_kit::TempDir::new("grant-kinds");
        let store = Store::at(dir.join("secrets.toml"));
        let id = link_id::candidate().unwrap();
        let main = test_grantee(&id, "main");
        let mut secrets = Secrets::default();
        let sealing = Sealing::new("api.example.com", None, None, None).unwrap();
        let key = named_as(
            &mut secrets,
            "ANTHROPIC_KEY",
            Some(sealing),
            vec![(main.clone(), Use::own("model").unwrap())],
        );
        let grant = |r#as| Grant::new(&main, r#as, &key, true).unwrap();
        store.save(&secrets).unwrap();
        let name = Called::Name("ANTHROPIC_KEY".into());
        // Another way of being given it is another grant, kept beside it.
        assert!(grant_in(&store, &name, vec![main.clone()], Vec::new()).unwrap());
        let to = store.load().unwrap().entry[&key].to.clone();
        assert!(
            to.contains(&grant(Use::own("model").unwrap())) && to.contains(&grant(Use::Variable))
        );
        // And taking one away leaves the other, named by its ID alike.
        let by_id = Called::Id(key.clone());
        assert!(grant_in(&store, &by_id, Vec::new(), vec![main.clone()]).unwrap());
        assert_eq!(
            store.load().unwrap().entry[&key].to,
            vec![grant(Use::own("model").unwrap())]
        );
    }

    #[test]
    fn a_controller_keeps_excluded_model_copies_at_the_source_box() {
        let dir = link_test_kit::TempDir::new("copies-managed-models");
        let store = Store::at(dir.join("secrets.toml"));
        let id = link_id::candidate().unwrap();
        let telegram = Use::own("connection:telegram:bot_token").unwrap();
        let mut secrets = Secrets::default();
        secrets
            .own(
                &test_grantee(&id, "main"),
                vec![
                    (
                        Use::own("model").unwrap(),
                        Some("sk-source".to_string().into()),
                    ),
                    (telegram.clone(), Some("123:abc".to_string().into())),
                ],
                &none,
            )
            .unwrap();
        let of = service_principal(&id).unwrap();
        let model = secrets.own_of(&of, &Use::own("model").unwrap()).unwrap();
        let token = secrets.own_of(&of, &telegram).unwrap();
        store.save(&secrets).unwrap();
        let source = Place::of_box("b1", None);
        copied_in(&store, &id, source.clone(), &[]).unwrap();
        let mut secrets = store.load().unwrap();
        secrets.forget(&id).unwrap();
        store.save(&secrets).unwrap();
        let cloud = Place::of_account("host", None);
        let without = [Use::own("model").unwrap(), Use::own("fallback").unwrap()];
        copied_in(&store, &id, cloud.clone(), &without).unwrap();
        gone_from_in(&store, &id, &source, &without).unwrap();
        let secrets = store.load().unwrap();
        assert_eq!(secrets.left[&model].copies, BTreeSet::from([source]));
        assert_eq!(secrets.left[&token].copies, BTreeSet::from([cloud]));
    }

    /// A workload made again with a name another had: each one's copies are
    /// its own, so one's moves never take the other's off, nor point them
    /// elsewhere.
    #[test]
    fn a_name_used_again_never_takes_another_copies_of() {
        let key = |v: &str| {
            vec![(
                Use::own("model").unwrap(),
                Some(Secret::from(v.to_string())),
            )]
        };
        let at = |name: &str| Place::of_box(name, None);
        let (first, second) = (link_id::candidate().unwrap(), link_id::candidate().unwrap());
        let mut secrets = Secrets::default();
        // main moves to aws-1, then a new main to gcp-1.
        secrets
            .own(&test_grantee(&first, "main"), key("sk-1"), &none)
            .unwrap();
        secrets
            .copies_of(&first, &[], &|c| c.insert(at("aws-1")))
            .unwrap();
        secrets.forget(&first).unwrap();
        secrets
            .own(&test_grantee(&second, "main"), key("sk-2"), &none)
            .unwrap();
        secrets
            .copies_of(&second, &[], &|c| c.insert(at("gcp-1")))
            .unwrap();
        secrets.forget(&second).unwrap();
        assert_eq!(secrets.left.len(), 2, "{:?}", secrets.left);
        let of = |s: &Secrets, id: &str| {
            let of = service_principal(id).unwrap();
            s.left
                .values()
                .find(|l| l.principal == of)
                .map(|l| l.copies.iter().cloned().collect::<Vec<_>>())
        };
        assert_eq!(of(&secrets, &first), Some(vec![at("aws-1")]));
        assert_eq!(of(&secrets, &second), Some(vec![at("gcp-1")]));
        // The first leaves aws-1: the second's copy on gcp-1 stays noted.
        assert!(
            secrets
                .copies_of(&first, &[], &|c| c.remove(&at("aws-1")))
                .unwrap()
        );
        assert_eq!(of(&secrets, &first), None);
        assert_eq!(of(&secrets, &second), Some(vec![at("gcp-1")]));
        assert_eq!(secrets.list()[0].copies, vec![at("gcp-1")]);
    }
}
