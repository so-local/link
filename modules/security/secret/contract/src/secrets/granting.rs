//! Who a secret is given to ([`Grant`]) and how ([`Use`]).

use super::*;

/// A grant: a principal, an action and a resource, the secret's
/// `secret:<id>`, the entry it's kept under (`[entry.<secret id>]`). The
/// principal is a service: never a workload, so a secret made again with
/// the name, or a workload made again with its name, is reached by no old
/// one. Made only by [`Grant::new`], for the secret it's kept in.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// Who's given it, `service:<id>`.
    pub principal: Principal,
    /// What it may do with it: pass a sealed secret, read a plain one.
    pub action: Action,
    /// The secret, `secret:<id>`.
    pub resource: Resource,
    /// The workload acting as its principal, `<kind>:<id>`, to show: it
    /// names what runs and grants nothing.
    pub workload: Workload,
    /// The workload's name as it was granted, to show.
    pub name: String,
    /// How it's given it.
    #[serde(rename = "as")]
    pub r#as: Use,
}

/// What a grant lets its principal do with a secret, an action, which
/// the secret module checks itself: set by how the secret
/// is kept ([`Action::of`]), never chosen apart from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    /// Sealed use: its proxy adds a sealed secret, and what runs sees a
    /// placeholder.
    #[serde(rename = "secret.secret.pass")]
    Pass,
    /// Common use: the value itself, of a plain secret.
    #[serde(rename = "secret.secret.read")]
    Read,
}

impl Action {
    /// The action a secret kept `sealed`, or plain, is given by.
    pub fn of(sealed: bool) -> Action {
        match sealed {
            true => Action::Pass,
            false => Action::Read,
        }
    }
}

/// The action's name, `secret.secret.pass`.
impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Action::Pass => "secret.secret.pass",
            Action::Read => "secret.secret.read",
        })
    }
}

/// How a principal is given a secret granted to it, tagged by its kind.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Use {
    /// As a variable of that name: plain, what it runs reads it; sealed,
    /// its proxy adds it for its host (`lnk secret create --internet-host`).
    Variable,
    /// Its own secret for a purpose the module that keeps it names
    /// ([`Use::own`]): kept as an own secret, found by its principal and
    /// this purpose, and never granted by hand.
    Own { purpose: String },
}

/// The longest purpose an own secret has.
const PURPOSE_MAX: usize = 120;

impl std::fmt::Display for Use {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Use::Variable => write!(f, "a variable"),
            Use::Own { purpose } => write!(f, "its own {purpose}"),
        }
    }
}

impl Use {
    /// Whether it's the [`Use::Own`] kind: an own secret, by its tag.
    pub fn is_own(&self) -> bool {
        matches!(self, Use::Own { .. })
    }

    /// Its own secret for `purpose`: one or more parts joined by `:`,
    /// each of lowercase letters, digits, `_`, `-` and `.` (`model`,
    /// `connection:telegram:bot_token`), at most 120 characters, and never
    /// `variable`.
    pub fn own(purpose: &str) -> anyhow::Result<Use> {
        let part = |p: &str| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
        };
        ensure!(
            purpose.len() <= PURPOSE_MAX && purpose != "variable" && purpose.split(':').all(part),
            "{:?} isn't an own secret's purpose: parts of lowercase letters, digits, _, - and ., \
             joined by :",
            link_format::clean(purpose)
        );
        Ok(Use::Own {
            purpose: purpose.to_string(),
        })
    }

    /// How a command line names it (`lnk secret delete --here --own`):
    /// `variable`, or an own secret's purpose.
    pub fn flag(&self) -> String {
        match self {
            Use::Variable => "variable".into(),
            Use::Own { purpose } => purpose.clone(),
        }
    }

    /// The use [`Use::flag`] wrote.
    pub fn from_flag(text: &str) -> anyhow::Result<Use> {
        match text {
            "variable" => Ok(Use::Variable),
            _ => Use::own(text),
        }
    }

    /// An own secret's purpose: none for a variable.
    pub fn purpose(&self) -> Option<&str> {
        match self {
            Use::Variable => None,
            Use::Own { purpose } => Some(purpose),
        }
    }
}

/// `lnk secret add --to <kind>:<name>`'s question to the modules that keep
/// workloads: where the one of that kind and name is, in this module's
/// types.
pub const GRANTEE: link_core::answers::Question<String, GranteeAt> =
    link_core::answers::Question::new("secret.grantee", include_str!("grantee.toml"));

/// Where [`GRANTEE`]'s workload is, in Stable IDs' answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GranteeAt {
    /// Here, by its workload's ID, `<kind>:<id>`, acting as its
    /// principal, `service:<id>`: a secret is granted to it here.
    Here {
        workload: Workload,
        principal: Principal,
    },
    /// Of a kind the module keeps, but with no ID yet, and the command
    /// that gives it one.
    Unstarted { how: String },
    /// On another machine, by the name it's reached by.
    On { place: String },
    /// Of a kind the module keeps, but nothing has that name.
    None,
    /// Not a kind the module keeps.
    Unkept,
}

/// Who a grant names: the service it gives to, the workload acting as it,
/// by its ID, and its name. Made by [`Grantee::new`], which checks each,
/// and carried to the keeper as one, which checks each again as it reads
/// the request ([`Grantee::of`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "Carried")]
pub struct Grantee {
    service: Principal,
    workload: Workload,
    name: String,
}

/// A [`Grantee`] as a request carries it, before it's checked.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Carried {
    service: Principal,
    workload: Workload,
    name: String,
}

impl TryFrom<Carried> for Grantee {
    type Error = anyhow::Error;
    fn try_from(carried: Carried) -> anyhow::Result<Grantee> {
        Grantee::of(carried.workload, &carried.name, carried.service)
    }
}

impl Grantee {
    /// The workload named `name` whose ID is `workload` (`<kind>:<id>`),
    /// acting as the service whose ID is `service`: an error unless each
    /// ID is minted and the name is a name.
    pub fn new(workload: &str, name: &str, service: &str) -> anyhow::Result<Grantee> {
        crate::machine::check_id(service)?;
        Grantee::of(workload.parse()?, name, service_principal(service)?)
    }

    /// The workload named `name` whose ID is `workload`, acting as
    /// `principal`: an error unless it's a service's and each ID minted.
    pub fn of(workload: Workload, name: &str, principal: Principal) -> anyhow::Result<Grantee> {
        crate::machine::check_name(name)?;
        crate::machine::check_id(workload.id())?;
        ensure!(
            principal.kind() == link_principal::PrincipalKind::Service,
            "{principal} isn't a service: a secret is given to a service alone"
        );
        crate::machine::check_id(principal.id())?;
        Ok(Grantee {
            service: principal,
            workload,
            name: name.to_string(),
        })
    }

    /// The service it acts as, `service:<id>`.
    pub fn service(&self) -> &Principal {
        &self.service
    }

    /// The workload, `<kind>:<id>`.
    pub fn workload(&self) -> &Workload {
        &self.workload
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// A flag's `text`: `<kind>:<name>`, the workload here of that kind
    /// and name, by the service it acts as, as `grantee` says (an error
    /// for one that isn't here).
    pub fn parse(
        text: &str,
        grantee: &dyn Fn(&str) -> anyhow::Result<Grantee>,
    ) -> anyhow::Result<Grantee> {
        named(text)?;
        grantee(text)
    }
}

/// `main (<kind>:<id>)`.
impl std::fmt::Display for Grantee {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.workload)
    }
}

/// A workload named `name` of kind `run` acting as the service whose ID
/// is `service`, for tests: its own ID is the service's backwards, so the
/// same service is always the same workload.
#[cfg(test)]
pub(crate) fn test_grantee(service: &str, name: &str) -> Grantee {
    let id: String = service.chars().rev().collect();
    Grantee::new(&format!("run:{id}"), name, service).unwrap()
}

/// The workload `text` names (`<kind>:<name>`), as the modules that answer
/// [`GRANTEE`] say: an error for one that isn't here, naming the machine
/// it's on, where its secrets are given it.
pub fn grantee(text: &str) -> anyhow::Result<Grantee> {
    let (_, name) = named(text)?;
    let (mut on, mut unstarted) = (None, None);
    for answer in GRANTEE.ask(&text.to_string()) {
        match answer.reply {
            Ok(GranteeAt::Here {
                workload,
                principal,
            }) => {
                return Grantee::of(workload, name, principal);
            }
            Ok(GranteeAt::Unstarted { how }) => unstarted = Some(how),
            Ok(GranteeAt::On { place }) => on = Some(place),
            Ok(GranteeAt::None | GranteeAt::Unkept) | Err(_) => {}
        }
    }
    let text = link_format::clean(text);
    match (unstarted, on) {
        (Some(how), _) => bail!("{text} has no ID yet: {}", link_format::clean(&how)),
        (None, Some(on)) => bail!(
            "{text} is on {on}: a secret is granted to it there, lnk box ssh {on}, then \
             lnk secret add"
        ),
        (None, None) => bail!("nothing here is {text}"),
    }
}

/// `text`'s kind and name, `<kind>:<name>`.
fn named(text: &str) -> anyhow::Result<(&str, &str)> {
    let Some((kind, name)) = text.split_once(':') else {
        bail!(
            "{:?}: a secret is given to --to <kind>:<name>, as lnk secret add's help says",
            link_format::clean(text)
        );
    };
    link_id::WorkloadKind::new(kind)?;
    crate::machine::check_name(name)?;
    Ok((kind, name))
}

impl Grant {
    /// A grant to `grantee`, by the service it acts as, for `as`, of the
    /// secret whose ID is `id`, kept `sealed` or plain.
    pub fn new(grantee: &Grantee, r#as: Use, id: &str, sealed: bool) -> anyhow::Result<Grant> {
        crate::machine::check_name(&grantee.name)?;
        Ok(Grant {
            principal: grantee.service.clone(),
            action: Action::of(sealed),
            resource: Resource::of(SECRET_KIND, id)?,
            workload: grantee.workload.clone(),
            name: grantee.name.clone(),
            r#as,
        })
    }

    /// Whether it gives `grantee` the secret as a variable.
    pub fn is_variable_of(&self, grantee: &Grantee) -> bool {
        self.principal == grantee.service && !self.r#as.is_own()
    }

    /// Whether it's an own secret of the principal `of`, never a variable.
    pub fn is_own_of(&self, of: &Principal) -> bool {
        self.principal == *of && self.r#as.is_own()
    }

    /// Whether it gives the same principal the secret the same way as
    /// `other`, whatever name it was granted by.
    pub fn same_as(&self, other: &Grant) -> bool {
        self.principal == other.principal && self.r#as == other.r#as
    }
}

/// The service whose ID is `id`, as a principal: `service:<id>`.
pub fn service_principal(id: &str) -> anyhow::Result<Principal> {
    // A minted ID only: never a name standing in for one.
    anyhow::ensure!(
        link_id::is_candidate(id),
        "{:?} isn't a service's ID",
        link_format::clean(id)
    );
    Ok(Principal::new(&link_principal::PrincipalKind::Service, id)?)
}

/// `main (<kind>:<id>)`, and how it's given when it isn't a variable.
impl std::fmt::Display for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.r#as {
            Use::Variable => write!(f, "{} ({})", self.name, self.workload),
            r#as => write!(f, "{} ({}), as {as}", self.name, self.workload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grantee read from a request is checked as one made here: a
    /// principal that isn't a service, or a name that isn't one, refused.
    #[test]
    fn a_carried_grantee_is_checked_again() {
        let id = link_id::candidate().unwrap();
        let made = Grantee::new(&format!("run:{id}"), "main", &id).unwrap();
        let text = serde_json::to_string(&made).unwrap();
        assert_eq!(serde_json::from_str::<Grantee>(&text).unwrap(), made);
        let user = text.replace("service:", "user:");
        assert!(serde_json::from_str::<Grantee>(&user).is_err(), "{user}");
        let named = text.replace("\"main\"", "\"not a name\"");
        assert!(serde_json::from_str::<Grantee>(&named).is_err(), "{named}");
    }

    /// The question this owns declares its protocol beside it.
    #[test]
    fn its_question_declares_its_protocol() {
        let protocol = GRANTEE.protocol();
        assert!(protocol.oldest <= protocol.current, "{protocol:?}");
    }

    #[test]
    fn to_is_a_workloads_service_here() {
        let none =
            |text: &str| -> anyhow::Result<Grantee> { anyhow::bail!("nothing here is {text}") };
        let (id, service) = (link_id::candidate().unwrap(), link_id::candidate().unwrap());
        let here = Grantee::new(&format!("run:{id}"), "main", &service).unwrap();
        let main = |text: &str| match text {
            "run:main" => Ok(here.clone()),
            _ => anyhow::bail!("nothing here is {text}"),
        };
        let grantee = Grantee::parse("run:main", &main).unwrap();
        assert_eq!(grantee, here);
        // Shown by its name and its ID, never a name in an ID's form.
        assert_eq!(grantee.to_string(), format!("main (run:{id})"));
        let secret = link_id::candidate().unwrap();
        let grant = Grant::new(&grantee, Use::Variable, &secret, true).unwrap();
        assert_eq!(grant.principal.to_string(), format!("service:{service}"));
        assert_eq!(grant.action, Action::Pass);
        assert_eq!(grant.to_string(), format!("main (run:{id})"));
        // Saved as a principal, an action and a resource, with a tagged use.
        let text = toml::to_string(&grant).unwrap();
        assert!(text.contains("action = \"secret.secret.pass\""), "{text}");
        assert!(
            text.contains(&format!("resource = \"secret:{secret}\"")),
            "{text}"
        );
        assert!(text.contains(&format!("workload = \"run:{id}\"")), "{text}");
        assert!(text.contains("kind = \"variable\""), "{text}");
        let plain = Grant::new(&grantee, Use::own("model").unwrap(), &secret, false).unwrap();
        assert_eq!(plain.action, Action::Read);
        let text = toml::to_string(&plain).unwrap();
        assert!(text.contains("action = \"secret.secret.read\""), "{text}");
        assert!(text.contains("purpose = \"model\""), "{text}");
        assert_eq!(toml::from_str::<Grant>(&text).unwrap(), plain);
        assert!(Grantee::parse("run:work", &main).is_err());
        for refused in [
            "user:web-1/alice",
            "run:main",
            "app",
            "run:",
            ":main",
            "Run:main",
        ] {
            assert!(Grantee::parse(refused, &none).is_err(), "{refused}");
        }
        assert!(Grantee::new("run:main", "main", &service).is_err());
        assert!(Grantee::new(&format!("run:{id}"), "main", "main").is_err());
        assert!(Grantee::new(&id, "main", &service).is_err());
    }

    #[test]
    fn an_own_use_is_written_for_a_command_line_and_read_back() {
        for purpose in [
            "model",
            "connection:telegram:bot_token",
            "webhook:ci:previous",
        ] {
            let r#use = Use::own(purpose).unwrap();
            assert_eq!(Use::from_flag(&r#use.flag()).unwrap(), r#use);
            assert_eq!(r#use.purpose(), Some(purpose));
        }
        assert_eq!(Use::from_flag("variable").unwrap(), Use::Variable);
        for refused in ["", "connection:", "a b", "Model", "x::y", &"a".repeat(121)] {
            assert!(Use::from_flag(refused).is_err(), "{refused}");
        }
        assert!(Use::own("variable").is_err());
    }
}
