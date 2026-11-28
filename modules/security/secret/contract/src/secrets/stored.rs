//! The file of secrets: what each entry keeps, how it's read and written,
//! and what it grants a workload.

use super::*;

/// The largest store [`Store::save`] writes: well past any person's
/// secrets, and short of what one caller could fill the keeper's disk
/// with, a request at a time.
pub(super) const STORE_MAX: usize = 8 << 20;

/// Which kind of secret one is, said by a tag, never told from which of
/// its other values it has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretKind {
    /// Kept by `lnk secret create` under a name, a variable's.
    #[default]
    Named,
    /// A service's own, kept by the module that keeps that service, with
    /// no name: found by its service's ID and its use.
    Own,
}

/// One secret as it's kept, under its ID: its kind, its name, its value
/// (none when the Keychain has it), how it's given, and where.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Kept {
    pub(super) kind: SecretKind,
    /// Its name, `lnk secret create`'s: a named secret's alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "link_secretive::bare")]
    pub(super) value: Option<Secret>,
    /// How it's given when it's sealed; none, plain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sealed: Option<Sealing>,
    pub(super) to: Vec<Grant>,
    /// Where its owner gave a copy of it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) copies: BTreeSet<Place>,
    /// Own grants retained here while the service's workload is away.
    /// These grant nothing that runs.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) retained: BTreeSet<Grant>,
}

/// Whose own secret one is, and for what: a service, by its ID.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub principal: Principal,
    #[serde(rename = "as")]
    pub r#as: Use,
}

/// The copies elsewhere of an own secret no longer kept here
/// (`[left.<id>]`, by the secret's retired ID): those that left with its
/// service, and those `remove` couldn't reach, for the next `remove`. They
/// follow the service where it goes next; a workload made later with its
/// name has another service, so one's copies never touch another's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Left {
    /// The service whose own secret it is.
    pub(super) principal: Principal,
    /// Which of its own secrets.
    #[serde(rename = "as")]
    pub(super) r#as: Use,
    pub(super) copies: BTreeSet<Place>,
}

impl Left {
    pub(super) fn owner(&self) -> Owner {
        Owner {
            principal: self.principal.clone(),
            r#as: self.r#as.clone(),
        }
    }
}

/// A secret here, as `lnk secret list` shows it: never its value. One
/// no longer kept here, but with copies left elsewhere, isn't `kept`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    /// Its ID, `secret:<id>`'s id.
    pub id: String,
    pub kind: SecretKind,
    /// Its name: none for an own secret.
    pub name: Option<String>,
    /// Whose own secret it is, for an own secret.
    pub owner: Option<Owner>,
    /// How it's given when it's sealed; none, plain.
    pub sealed: Option<Sealing>,
    pub to: Vec<Grant>,
    /// Where its owner gave a copy of it.
    pub copies: Vec<Place>,
    pub kept: bool,
}

impl Listed {
    /// Its ID as it's written, `secret:<id>`.
    pub fn resource(&self) -> String {
        resource(&self.id)
    }

    /// What it's called in words: its name, or whose own secret it is.
    pub fn called(&self) -> String {
        match (&self.name, &self.owner) {
            (Some(name), _) => name.clone(),
            (None, Some(owner)) => format!("{}'s own, {}", owner.principal, owner.r#as),
            (None, None) => self.resource(),
        }
    }

    /// Whether `secret`, a name or `secret:<id>`, names it.
    pub fn is(&self, secret: &Called) -> bool {
        match secret {
            Called::Id(id) => *id == self.id,
            Called::Name(name) => self.name.as_deref() == Some(name.as_str()),
        }
    }
}

/// A secret's ID as it's written, `secret:<id>`.
pub fn resource(id: &str) -> String {
    format!("{SECRET_KIND}:{id}")
}

/// A secret as a command names it: by its name, or by its ID,
/// `secret:<id>`. A name is a variable's, which never holds `:`, so the
/// two are never told apart by an ID's shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Called {
    Name(String),
    Id(String),
}

impl Called {
    /// `text`, a secret's name or `secret:<id>`.
    pub fn parse(text: &str) -> anyhow::Result<Called> {
        match text
            .strip_prefix(SECRET_KIND)
            .and_then(|t| t.strip_prefix(':'))
        {
            Some(id) => {
                ensure!(
                    link_id::is_id(id),
                    "{:?} isn't a secret's ID: secret:<its id>",
                    link_format::clean(text)
                );
                Ok(Called::Id(id.to_string()))
            }
            None => {
                link_spec::check_variable(text)?;
                Ok(Called::Name(text.to_string()))
            }
        }
    }
}

impl std::fmt::Display for Called {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Called::Name(name) => f.write_str(name),
            Called::Id(id) => f.write_str(&resource(id)),
        }
    }
}

/// The file as it's written and read: the retired IDs, then each secret
/// kept by its ID (`[entry.<id>]`), then the copies left of those that
/// aren't (`[left.<id>]`).
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Secrets {
    /// Each ID a secret here had and no longer has: never given again.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) retired: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) entry: BTreeMap<String, Kept>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) left: BTreeMap<String, Left>,
}

link_files::specification!(Secrets, include_str!("stored.schema.toml"));

impl Store {
    /// Those it keeps: none when there's no file.
    pub(super) fn load(&self) -> anyhow::Result<Secrets> {
        let file = self.file.display();
        match at_rest::read(&self.file, keeper::running_as_keeper())? {
            Some(text) => {
                parse(std::str::from_utf8(text.expose()).context("the secrets store isn't text")?)
                    .with_context(|| format!("reading {file}"))
            }
            None => Ok(Secrets::default()),
        }
    }

    pub(super) fn save(&self, secrets: &Secrets) -> anyhow::Result<()> {
        // A value is written by the serializer, so it can't add a line.
        let text = Secret::new(link_files::saved::print(secrets).context("writing the secrets")?);
        let bytes = text.expose().as_bytes();
        ensure!(
            bytes.len() <= STORE_MAX,
            "the secrets kept would be over {} MB: delete some with lnk secret delete first",
            STORE_MAX >> 20
        );
        (if keeper::running_as_keeper() {
            at_rest::write(&self.file, bytes)
        } else {
            link_files::write_private(&self.file, bytes).map_err(Into::into)
        })
        .with_context(|| format!("writing {}", self.file.display()))
    }

    /// Its lock, held while it's read, changed and written back, so two
    /// at once never lose one's secret: let go when it's dropped.
    pub(super) fn lock(&self) -> anyhow::Result<std::fs::File> {
        link_files::lock_file(&self.file.with_extension("toml.lock"))
    }
}

/// The IDs of a store, as `link_id`'s reservation reads them: live while
/// an entry or the copies left have it, retired once deleted.
struct Ids<'a> {
    store: &'a Secrets,
    held: BTreeSet<String>,
}

impl link_id::Ledger for Ids<'_> {
    fn is_live(&self, id: &str) -> bool {
        self.store.entry.contains_key(id)
            || self.store.left.contains_key(id)
            || self.held.contains(id)
    }
    fn is_retired(&self, id: &str) -> bool {
        self.store.retired.contains(id)
    }
    fn hold(&mut self, id: &str) {
        self.held.insert(id.to_string());
    }
    fn retire(&mut self, _: &str) {}
}

impl Secrets {
    /// A new secret's ID: never one a secret here has, or had.
    pub(super) fn mint(&self) -> anyhow::Result<String> {
        self.mint_from(link_id::candidate)
    }

    /// [`Secrets::mint`], drawing candidates from `draw`.
    pub(super) fn mint_from(
        &self,
        draw: impl FnMut() -> Result<String, link_id::NoRandomness>,
    ) -> anyhow::Result<String> {
        let mut ids = Ids {
            store: self,
            held: BTreeSet::new(),
        };
        Ok(link_id::reserve_from(&mut ids, draw)?)
    }

    /// The ID of the secret kept here that `which` names.
    pub(super) fn find(&self, which: &Called) -> Option<String> {
        match which {
            Called::Id(id) => self.entry.contains_key(id).then(|| id.clone()),
            Called::Name(name) => self
                .entry
                .iter()
                .find(|(_, kept)| kept.name.as_deref() == Some(name.as_str()))
                .map(|(id, _)| id.clone()),
        }
    }

    /// The ID of the service `of`'s own secret for `r#as`, granted it now
    /// or retained for it.
    pub(super) fn own_of(&self, of: &Principal, r#as: &Use) -> Option<String> {
        self.entry
            .iter()
            .find(|(_, kept)| {
                kept.to
                    .iter()
                    .chain(&kept.retained)
                    .any(|g| g.principal == *of && g.r#as == *r#as)
            })
            .map(|(id, _)| id.clone())
    }

    /// Each kept here, by ID: how it's given and where, never its value.
    /// Those no longer kept here but with copies left elsewhere
    /// (`[left]`) are listed too, by their copies alone.
    pub(super) fn list(&self) -> Vec<Listed> {
        let kept = self.entry.iter().map(|(id, kept)| Listed {
            id: id.clone(),
            kind: kept.kind,
            name: kept.name.clone(),
            owner: owner(kept),
            sealed: kept.sealed.clone(),
            to: kept.to.clone(),
            copies: kept.copies.iter().cloned().collect(),
            kept: true,
        });
        let left = self.left.iter().map(|(id, left)| Listed {
            id: id.clone(),
            kind: SecretKind::Own,
            name: None,
            owner: Some(left.owner()),
            sealed: None,
            to: Vec::new(),
            copies: left.copies.iter().cloned().collect(),
            kept: false,
        });
        let mut listed: Vec<Listed> = kept.chain(left).collect();
        listed.sort_by_key(|l| (l.kind == SecretKind::Own, l.called(), l.id.clone()));
        listed
    }
}

/// Which of a service's grants [`for_service`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Which {
    /// Its own, each for its purpose.
    Own,
    /// What `lnk secret add --to` gave it, as variables.
    Variables,
}

impl Which {
    fn takes(self, r#as: &Use) -> bool {
        !r#as.is_own() == (self == Which::Variables)
    }
}

/// What a service is given of a secret granted to it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForService {
    /// The secret's name, a variable's: none for an own secret.
    pub name: Option<String>,
    pub r#as: Use,
    #[serde(with = "link_secretive::bare")]
    pub value: Secret,
    /// How it's given when it's sealed: for [`Use::Variable`] alone.
    pub sealed: Option<Sealing>,
}

impl Secrets {
    /// The value of entry `id`, `kept`: the file's, else the Keychain's.
    /// One the Keychain should hold and doesn't is an error, naming it.
    fn value(id: &str, kept: &Kept) -> anyhow::Result<Secret> {
        let in_file = kept.value.as_ref().map(Secret::duplicate);
        crate::keychain::fetch(&account(id), in_file)?.with_context(|| {
            format!(
                "{} is kept, but its value isn't",
                kept.name.clone().unwrap_or_else(|| resource(id))
            )
        })
    }

    /// What the service whose ID is `id` is granted for a use `which`
    /// takes, each as it's given: its own or what `lnk secret add --to`
    /// gave it. Only those are read.
    pub(super) fn for_service(&self, id: &str, which: Which) -> anyhow::Result<Vec<ForService>> {
        let of = service_principal(id)?;
        let mut given = Vec::new();
        for (key, kept) in &self.entry {
            for grant in &kept.to {
                if grant.principal != of || !which.takes(&grant.r#as) {
                    continue;
                }
                given.push(ForService {
                    name: kept.name.clone(),
                    r#as: grant.r#as.clone(),
                    value: Secrets::value(key, kept)?,
                    sealed: kept.sealed.clone(),
                });
            }
        }
        Ok(given)
    }
}

/// One of your secrets as [`for_you`] reads it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Yours {
    /// Its ID.
    pub id: String,
    /// Its name, a variable's.
    pub name: String,
    #[serde(with = "link_secretive::bare")]
    pub value: Secret,
    /// How it's given when it's sealed.
    pub sealed: Option<Sealing>,
}

impl Secrets {
    /// Your own secrets of `ids`, each still kept and named: a service's
    /// own (no name) is never yours to read.
    pub(super) fn for_you(&self, ids: &[String]) -> anyhow::Result<Vec<Yours>> {
        let mut given = Vec::new();
        for id in ids {
            let Some(kept) = self.entry.get(id) else {
                continue;
            };
            let Some(name) = &kept.name else {
                continue;
            };
            given.push(Yours {
                id: id.clone(),
                name: name.clone(),
                value: Secrets::value(id, kept)?,
                sealed: kept.sealed.clone(),
            });
        }
        Ok(given)
    }
}

/// Whether `text` is a `secrets.toml` as this `lnk` reads one, checked
/// as [`parse`] checks it: what a page showing the file is held to.
pub fn check_store(text: &str) -> anyhow::Result<()> {
    parse(text).map(|_| ())
}

/// What a store of names leaves in the Keychain, which nothing here
/// reads or removes: each name's item, `link:secret/<name>`, where
/// there's a Keychain; nothing where the file held the values.
fn left_by_names(named: &[&str]) -> String {
    if !crate::keychain::available() {
        return String::new();
    }
    let items = named
        .iter()
        .map(|name| {
            format!(
                "{}:{}",
                crate::keychain::SERVICE,
                super::account(&link_format::clean(name))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "; the Keychain keeps their values, which nothing reads now: delete {items} with Keychain Access"
    )
}

/// `secrets.toml` read, each sealed one's way checked again (the file is
/// only text), its error never the parser's: that names the line, or a
/// value of the wrong type, and on Linux the file holds values. A file of
/// the shape an earlier `lnk` wrote, each secret by its name, is refused.
pub(super) fn parse(text: &str) -> anyhow::Result<Secrets> {
    // Its schema advanced first; a file that isn't TOML is refused below,
    // by line, never by the parser's words, which could show a value.
    let advanced = match text.parse::<toml::Table>() {
        Ok(_) => link_files::saved::advance::<Secrets>(text)?,
        Err(_) => text.to_string(),
    };
    let text = advanced.as_str();
    let mut secrets: Secrets = match toml::from_str(text) {
        Ok(secrets) => secrets,
        Err(err) => {
            if let Ok(table) = text.parse::<toml::Table>() {
                let named: Vec<&str> = table
                    .keys()
                    .map(String::as_str)
                    .filter(|k| !["retired", "entry", "left", "schema"].contains(k))
                    .collect();
                if !named.is_empty() {
                    bail!(
                        "it keeps secrets by their names, as an earlier lnk did, which this one \
                         doesn't read: remove it, then add your secrets again with lnk secret \
                         create, and start what keeps its own again{}",
                        left_by_names(&named)
                    );
                }
            }
            let line = err
                .span()
                .map(|span| format!("line {}: ", text[..span.start].matches('\n').count() + 1))
                .unwrap_or_default();
            bail!("{line}not as lnk secret create writes it");
        }
    };
    for id in secrets.retired.iter().chain(secrets.left.keys()) {
        ensure!(link_id::is_candidate(id), "{id:?} isn't a secret's ID");
    }
    for (id, kept) in &mut secrets.entry {
        ensure!(link_id::is_candidate(id), "{id:?} isn't a secret's ID");
        let called = kept.name.clone().unwrap_or_else(|| resource(id));
        match (kept.kind, &kept.name) {
            (SecretKind::Named, Some(name)) => link_spec::check_variable(name)?,
            (SecretKind::Named, None) => bail!("{called} is a named secret with no name"),
            (SecretKind::Own, Some(_)) => bail!("{called} is a service's own, which has no name"),
            // An own secret: granted its service alone, for its use.
            (SecretKind::Own, None) => ensure!(
                owner(kept).is_some_and(|owner| kept
                    .to
                    .iter()
                    .chain(&kept.retained)
                    .all(|g| g.is_own_of(&owner.principal) && g.r#as == owner.r#as)),
                "{called} is an own secret, and isn't a service's own"
            ),
        }
        if let Some(sealing) = &kept.sealed {
            kept.sealed = Some(
                sealing
                    .checked()
                    .with_context(|| format!("how {called} is given"))?,
            );
        }
        let (action, secret) = (Action::of(kept.sealed.is_some()), resource(id));
        for grant in kept.to.iter().chain(&kept.retained) {
            ensure!(
                grant.action == action && grant.resource.to_string() == secret,
                "{called} is given to {} by {}, never {action} on {secret}: a grant is the action \
                 its secret's kind gives, on that secret",
                grant.principal,
                grant.action
            );
            ensure!(
                grant.principal.kind() == link_principal::PrincipalKind::Service,
                "{called} is given to {}, never a service: a secret is given to a service alone",
                grant.principal
            );
            ensure!(
                link_id::is_candidate(grant.workload.id()),
                "{called} is given to {}, which isn't a workload's ID",
                grant.workload
            );
            crate::machine::check_name(&grant.name)
                .with_context(|| format!("who {called} is given to"))?;
        }
        for grant in &kept.retained {
            ensure!(grant.r#as.is_own(), "{called} retains only an own secret");
        }
        for copy in &kept.copies {
            copy.check()
                .with_context(|| format!("where {called} has a copy"))?;
        }
    }
    for (id, left) in &secrets.left {
        ensure!(
            left.principal.kind() == link_principal::PrincipalKind::Service && left.r#as.is_own(),
            "the copies left of {} are an own secret's",
            resource(id)
        );
        for copy in &left.copies {
            copy.check()
                .with_context(|| format!("where {} has a copy", resource(id)))?;
        }
    }
    Ok(secrets)
}

#[cfg(test)]
mod tests {
    use super::super::granting::test_grantee;
    use super::*;

    const ID: &str = "a1b2c3d4e5f6";

    #[test]
    fn a_broken_file_of_secrets_never_shows_its_line() {
        let err = parse(&format!(
            "[entry.{ID}]\nname = \"NPM_TOKEN\"\nvalue = \"npm_hunter2\" junk\n"
        ))
        .err()
        .unwrap();
        let shown = format!("{err:#}");
        assert!(!shown.contains("npm_hunter2"), "{shown}");
        assert!(shown.starts_with("line 3: "), "{shown}");
    }

    #[test]
    fn a_wrongly_typed_value_is_never_echoed_and_a_bad_way_is_refused() {
        let err = parse(&format!(
            "schema = 1\n[entry.{ID}]\nkind = \"named\"\nname = \"X\"\nto = \"npm_hunter2\"\n"
        ))
        .err()
        .unwrap();
        let shown = format!("{err:#}");
        assert!(!shown.contains("npm_hunter2"), "{shown}");
        assert!(shown.starts_with("line 4: not as"), "{shown}");
        // A sealed one's way, edited by hand to any host, is refused as read.
        let err = parse(&format!(
            "schema = 1\n[entry.{ID}]\nkind = \"named\"\nname = \"X\"\nto = []\nsealed = {{ host = \"*.example.com\" }}\n"
        ))
        .err()
        .unwrap();
        assert!(format!("{err:#}").contains("how X is given"), "{err:#}");
        let read = parse(&format!(
            "schema = 1\n[entry.{ID}]\nkind = \"named\"\nname = \"X\"\nto = []\nsealed = {{ host = \"API.Stripe.com\" }}\n"
        ))
        .unwrap();
        assert_eq!(
            read.list()[0].sealed.as_ref().unwrap().host,
            "api.stripe.com"
        );
    }

    /// A file an earlier `lnk` wrote, each secret under its name, is
    /// refused saying what to do, never read or converted, and never
    /// showing a value.
    #[test]
    fn a_store_of_the_shape_before_ids_is_refused() {
        let err = parse("schema = 1\n[NPM_TOKEN]\nvalue = \"npm_hunter2\"\nto = []\n")
            .err()
            .unwrap();
        let shown = format!("{err:#}");
        assert!(shown.contains("lnk secret create"), "{shown}");
        assert!(!shown.contains("npm_hunter2"), "{shown}");
    }

    #[test]
    fn a_kept_secret_is_written_and_read_back_with_its_grants() {
        let service = link_id::candidate().unwrap();
        let mut secrets = Secrets::default();
        secrets.retired.insert("r1b2c3d4e5f6".into());
        secrets.entry.insert(
            ID.into(),
            Kept {
                name: Some("STRIPE_KEY".into()),
                value: Some("sk_live".to_string().into()),
                sealed: Some(Sealing::new("api.stripe.com", None, None, None).unwrap()),
                to: vec![
                    Grant::new(&test_grantee(&service, "main"), Use::Variable, ID, true).unwrap(),
                ],
                ..Kept::default()
            },
        );
        let text = link_files::saved::print(&secrets).unwrap();
        assert!(text.contains(&format!("[entry.{ID}]")), "{text}");
        assert!(
            text.contains(&format!("principal = \"service:{service}\"")),
            "{text}"
        );
        let read = parse(&text).unwrap();
        assert_eq!(read.list(), secrets.list());
        assert_eq!(read.retired, secrets.retired);
        assert_eq!(
            read.list()[0].to[0].to_string(),
            format!("main ({})", test_grantee(&service, "main").workload())
        );
        assert_eq!(read.list()[0].resource(), format!("secret:{ID}"));
        // Written as a principal, an action and a resource, each kind tagged.
        assert!(text.contains("kind = \"named\""), "{text}");
        assert!(text.contains("action = \"secret.secret.pass\""), "{text}");
        assert!(
            text.contains(&format!("resource = \"secret:{ID}\"")),
            "{text}"
        );
        let refused = |entry: &str| parse(&format!("schema = 1\n[entry.{ID}]\n{entry}\n")).is_err();
        let grant = |principal: &str, action: &str, resource: &str, workload: &str| {
            format!(
                "kind = \"named\"\nname = \"X\"\nto = [{{ principal = \"{principal}\", action = \
                 \"{action}\", resource = \"{resource}\", workload = \"{workload}\", name = \
                 \"app\", as = {{ kind = \"variable\" }} }}]"
            )
        };
        let (svc, run, secret) = (
            format!("service:{service}"),
            format!("run:{service}"),
            format!("secret:{ID}"),
        );
        // As written, a plain one's grant reads it.
        assert!(!refused(&grant(&svc, "secret.secret.read", &secret, &run)));
        // A grant to no workload is refused as it's read.
        assert!(refused(
            "kind = \"named\"\nname = \"X\"\nto = [{ kind = \"group\", name = \"ops\" }]"
        ));
        // A grant to a workload, never a principal.
        assert!(refused(&grant(
            &format!("env:{service}"),
            "secret.secret.read",
            &secret,
            &run
        )));
        // Another action than its kind gives, an action no grant names,
        // or another secret.
        assert!(refused(&grant(&svc, "secret.secret.pass", &secret, &run)));
        assert!(refused(&grant(&svc, "use", &secret, &run)));
        assert!(refused(&grant(
            &svc,
            "secret.secret.read",
            &format!("secret:{service}"),
            &run
        )));
        // A use that says no kind.
        assert!(refused(
            &grant(&svc, "secret.secret.read", &secret, &run)
                .replace("{ kind = \"variable\" }", "\"variable\"")
        ));
        // A name a variable can't have, a named secret with no name, an own
        // one with a name, and one saying no kind.
        assert!(refused("kind = \"named\"\nname = \"lower\"\nto = []"));
        assert!(refused("kind = \"named\"\nto = []"));
        assert!(refused("kind = \"own\"\nname = \"X\"\nto = []"));
        assert!(refused("kind = \"own\"\nto = []"));
        assert!(refused("name = \"X\"\nto = []"));
        // A grant whose workload is a name.
        assert!(refused(&grant(
            &svc,
            "secret.secret.read",
            &secret,
            "run:main"
        )));
        assert!(parse("[entry.not-an-id]\nname = \"X\"\nto = []\n").is_err());
    }

    /// What you read as the holder is your own named secrets of the IDs
    /// asked, never a service's own, and none deleted since.
    #[test]
    fn you_read_only_your_named_secrets_of_the_ids_asked() {
        let (own, gone) = (link_id::candidate().unwrap(), link_id::candidate().unwrap());
        let mut secrets = Secrets::default();
        secrets.entry.insert(
            ID.into(),
            Kept {
                name: Some("NPM_TOKEN".into()),
                value: Some("npm_x".to_string().into()),
                ..Kept::default()
            },
        );
        secrets.entry.insert(
            own.clone(),
            Kept {
                kind: SecretKind::Own,
                value: Some("sk".to_string().into()),
                to: vec![
                    Grant::new(
                        &test_grantee(&link_id::candidate().unwrap(), "main"),
                        Use::own("model").unwrap(),
                        &own,
                        false,
                    )
                    .unwrap(),
                ],
                ..Kept::default()
            },
        );
        let read = parse(&link_files::saved::print(&secrets).unwrap()).unwrap();
        let yours = read.for_you(&[ID.to_string(), own, gone]).unwrap();
        assert_eq!(yours.len(), 1);
        assert_eq!(yours[0].name, "NPM_TOKEN");
        assert_eq!(yours[0].value.expose(), "npm_x");
    }

    #[test]
    fn a_secret_is_named_by_its_name_or_its_id_never_told_by_shape() {
        assert_eq!(
            Called::parse("NPM_TOKEN").unwrap(),
            Called::Name("NPM_TOKEN".into())
        );
        assert_eq!(
            Called::parse(&format!("secret:{ID}")).unwrap(),
            Called::Id(ID.into())
        );
        // A bare ID is a name, and not one a variable has.
        assert!(Called::parse(ID).is_err());
        assert!(Called::parse("secret:").is_err());
        assert!(Called::parse("run:a1b2c3d4e5f6").is_err());
    }
}
