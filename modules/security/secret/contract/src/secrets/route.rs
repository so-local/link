//! The secret route, which `lnk-secret`'s bridge serves
//! (`/_link/secret/<name>/...`): a service's call, its secret added
//! outside the sandbox by a rule that is data ([`SecretRule`], from the
//! module that owns the service, or a secret's own sealing), and only
//! for a call showing its placeholder.

use anyhow::Context;
use serde::{Deserialize, Serialize};

use link_sandbox_route::{Bridge, Placeholders, Route};
use link_secretive::Duplicate;

mod address;
pub use address::Address;

/// Where the secret route is on a proxy.
pub const PREFIX: &str = "/_link/secret/";

/// The bridge that serves the secret route here: `lnk secret bridge`.
pub fn bridge() -> anyhow::Result<Bridge> {
    Bridge::of("secret", None)
}

/// What the secret route's bridge is given: each secret by name, granted
/// to the route's caller, whose value the bridge reads from the store at
/// each call; or each with its value, handed with a run that
/// starts its proxy at once (`lnk sandbox run --with`), never in a file.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Given {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[duplicate]
    pub secrets: Vec<Sealed>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[duplicate]
    pub granted: Vec<Named>,
}

impl Given {
    /// The secret named in `target` (a path on the proxy) with its value:
    /// one handed as it is, one granted read from `granted`, what the
    /// store grants the route's caller of `which` kind, and the caller's
    /// own token made by `token` for a host and grants ([`Kept::Token`]).
    /// None when the target names none; an error when the caller isn't
    /// granted it, or it holds no value its placeholder was made for.
    pub fn find(
        &self,
        target: &str,
        granted: impl Fn(super::Which) -> anyhow::Result<Vec<super::ForService>>,
        token: impl Fn(&str, &str, Option<&Assertion>) -> anyhow::Result<link_secretive::Secret>,
    ) -> anyhow::Result<Option<Sealed>> {
        let Some((name, _)) = target.strip_prefix(PREFIX).and_then(|t| t.split_once('/')) else {
            return Ok(None);
        };
        if let Some(secret) = self.secrets.iter().find(|s| s.name == name) {
            return Ok(Some(secret.duplicate()));
        }
        let Some(named) = self.granted.iter().find(|s| s.name == name) else {
            return Ok(None);
        };
        named.resolved(granted, token).map(Some)
    }
}

/// A secret the route adds by name: granted to its caller in the store
/// (`kept`), the program holding `placeholder` in its place, sent by
/// `rule`.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Named {
    /// Its name on the route ([`check_secret_name`]).
    pub name: String,
    pub kept: Kept,
    /// A caller-specific assertion held by the proxy, when its machine
    /// cannot sign for it: for a token alone. Never passed to the runtime.
    #[duplicate]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assertion: Option<Assertion>,
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub placeholder: link_secretive::Secret,
    #[serde(flatten)]
    pub rule: SecretRule,
}

/// A trusted token issuer and the binding-specific proof it accepts.
/// The issuer rechecks authority at every trade; the bridge follows no redirect.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub url: String,
    #[duplicate]
    #[serde(with = "link_secretive::bare")]
    pub proof: link_secretive::Secret,
}

/// Which secret the route adds: one of the store's granted to the route's
/// caller, its own or a variable `lnk secret add --to` gave it, by name;
/// or the caller's own access token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kept {
    Own(super::Use),
    Variable(String),
    /// None of the store's: the caller's own access token for the rule's
    /// host, as its own workload, granted `grants` there, made for each
    /// call outside the sandbox by the bridge's token maker, so the caller
    /// reaches a service across a network as itself and never holds the
    /// token.
    Token {
        grants: String,
    },
}

/// `lnk secret bridge`'s question to the modules that keep workloads: the
/// caller's own access token ([`Kept::Token`]), made as its machine signs
/// for it, for the workload whose ID its binding names. A module answers
/// none for a workload not its own.
pub const TOKEN: link_core::answers::Question<TokenAsk, Option<Token>> =
    link_core::answers::Question::new("secret.token", include_str!("token.toml"));

/// What [`TOKEN`] asks: a token for `workload`, by the ID its binding
/// names, for `host`, granted `grants` there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenAsk {
    pub workload: String,
    pub host: String,
    pub grants: String,
}

/// A token [`TOKEN`]'s answer made, with when it ends, in seconds since
/// the epoch, when it says.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Token {
    #[duplicate]
    #[serde(with = "link_secretive::bare")]
    pub token: link_secretive::Secret,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

impl Default for Kept {
    fn default() -> Self {
        Kept::Variable(String::new())
    }
}

impl Named {
    /// `name`'s secret granted as `kept`, its placeholder made for `value`
    /// (read only for that) among `placeholders`, sent by `rule`.
    pub fn new(
        placeholders: &Placeholders,
        name: &str,
        kept: Kept,
        value: &link_secretive::Secret,
        rule: SecretRule,
    ) -> Self {
        Named {
            name: name.to_string(),
            placeholder: placeholder(placeholders, name, value, rule.public.as_deref()),
            kept,
            assertion: None,
            rule,
        }
    }

    /// The caller's own token for `rule`'s host as `name`, granted
    /// `grants` there ([`Kept::Token`]), its placeholder among
    /// `placeholders`: a token is made at each call, so none of it is
    /// public (`rule.public`).
    pub fn token(
        placeholders: &Placeholders,
        name: &str,
        grants: &str,
        rule: SecretRule,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            rule.public.is_none(),
            "{name}: a token's placeholder keeps none of it"
        );
        let none: link_secretive::Secret = String::new().into();
        Ok(Named::new(
            placeholders,
            name,
            Kept::Token {
                grants: grants.to_string(),
            },
            &none,
            rule,
        ))
    }

    /// It with its value, of those `granted` gives of its kind: refused
    /// when the store grants the caller none, or holds one its
    /// placeholder wasn't made for. A variable goes only by the sealing
    /// the store holds for it now: one created again for another host,
    /// header or format is refused until the route is made again, never
    /// sent by the rule its route was made with.
    pub fn resolved(
        &self,
        granted: impl Fn(super::Which) -> anyhow::Result<Vec<super::ForService>>,
        token: impl Fn(&str, &str, Option<&Assertion>) -> anyhow::Result<link_secretive::Secret>,
    ) -> anyhow::Result<Sealed> {
        use super::{Use, Which};
        let which = match &self.kept {
            Kept::Own(Use::Variable) => anyhow::bail!("a variable is kept by its name"),
            Kept::Own(_) => Which::Own,
            Kept::Variable(_) => Which::Variables,
            Kept::Token { grants } => {
                let value = token(&self.rule.host, grants, self.assertion.as_ref())
                    .with_context(|| format!("{}: this proxy's caller's token", self.name))?;
                return self.sealed(value);
            }
        };
        let value = granted(which)?
            .into_iter()
            .find(|g| match &self.kept {
                Kept::Own(own) => g.r#as == *own,
                Kept::Variable(name) => {
                    !g.r#as.is_own() && g.name.as_deref() == Some(name.as_str())
                }
                Kept::Token { .. } => false,
            })
            .with_context(|| format!("{} isn't granted to this proxy's caller", self.name))?;
        if let Kept::Variable(_) = &self.kept {
            anyhow::ensure!(
                value.sealed.as_ref().map(super::Sealing::rule).as_ref() == Some(&self.rule),
                "{} is sealed now for another host or header than this proxy's route: \
                 start this proxy's caller again to send it as it's sealed",
                self.name
            );
        }
        self.sealed(value.value)
    }

    /// It with `value`, its placeholder checked as made for it.
    fn sealed(&self, value: link_secretive::Secret) -> anyhow::Result<Sealed> {
        let sealed = Sealed {
            name: self.name.clone(),
            value,
            placeholder: self.placeholder.duplicate(),
            rule: self.rule.clone(),
        };
        sealed.check_placeholder()?;
        Ok(sealed)
    }
}

/// The secret route adding `secrets`, handed with their values, and
/// `granted`, by name, each to its own service's calls, served by
/// `bridge`: none named twice, each one its route can hold to.
pub fn route(bridge: Bridge, secrets: Vec<Sealed>, granted: Vec<Named>) -> anyhow::Result<Route> {
    let mut named = std::collections::BTreeSet::new();
    for secret in &secrets {
        secret.check_placeholder()?;
    }
    for named in &granted {
        anyhow::ensure!(
            named.assertion.is_none() || matches!(named.kept, Kept::Token { .. }),
            "an assertion belongs only to a token"
        );
    }
    let all = secrets
        .iter()
        .map(|s| (&s.name, &s.rule))
        .chain(granted.iter().map(|s| (&s.name, &s.rule)));
    for (name, rule) in all {
        check_secret_name(name)?;
        rule.check()?;
        anyhow::ensure!(named.insert(name), "two secrets named {name}");
    }
    let handed = !secrets.is_empty() || granted.iter().any(|named| named.assertion.is_some());
    let given = serde_json::to_string(&Given { secrets, granted })?;
    Ok(Route {
        name: "secret".into(),
        prefix: PREFIX.into(),
        bridge,
        given: given.into(),
        handed,
        ..Route::default()
    })
}

/// A secret the route adds to one service's calls: the program calls
/// `<its proxy>/_link/secret/<name>/<path>` ([`url`]) with a
/// placeholder in place of the secret, and the route sends
/// `https://<host>/<path>` with the real one, if [`SecretRule`] allows the
/// path.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sealed {
    /// Its name on the route: lowercase letters, digits and `-`
    /// ([`check_secret_name`]).
    pub name: String,
    /// The secret itself.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub value: link_secretive::Secret,
    /// What the program holds in its place ([`placeholder`]):
    /// the proxy adds the secret only for a call showing it. None given
    /// to `lnk sandbox run --with-fd`, which makes its own.
    #[serde(with = "link_secretive::bare", default)]
    #[duplicate]
    pub placeholder: link_secretive::Secret,
    #[serde(flatten)]
    pub rule: SecretRule,
}

/// Where a secret may go, and how: the data a module that owns the
/// service gives (a connection module's `rule --json`), which the proxy
/// enforces, knowing nothing of the service.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SecretRule {
    /// The one host it goes to, over HTTPS (`api.example.com`).
    pub host: String,
    /// The paths a call may take, after the proxy's
    /// `/_link/secret/<name>/`; none, none may.
    pub paths: Vec<SecretPath>,
    /// The header the secret goes in, when it goes in one rather than in
    /// a path's `{secret}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<SecretHeader>,
    /// Paths refused, compared without case, `{secret}` where the
    /// secret is (`bot{secret}/setwebhook`): calls that would hand what
    /// the secret guards to something outside Link.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refuse: Vec<String>,
    /// The request methods allowed; none given, GET and POST.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<String>,
    /// A separator ending the part of the secret that isn't secret (a
    /// bot's id, `123456` of `123456:ABC...`, before `:`), which the
    /// placeholder keeps so the program can read it: only digits are
    /// kept ([`placeholder`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<String>,
    /// A WebSocket the secret goes in the first message of (Discord's
    /// Gateway), on a host of its own: a service whose calls take the
    /// secret in a path or a header and whose socket takes it in its
    /// first message keeps one rule for both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket: Option<SecretSocket>,
}

/// A WebSocket the secret goes in: the program opens it at
/// `<its proxy>/_link/secret/<name>/<path>`, which the route opens at
/// `wss://<host>/`, the query kept, and the program's first message, a
/// JSON object, carries the placeholder at `pointer`, where the route
/// puts the secret. Nothing after the first message is read. One shape
/// covers the one service that needs it; a socket that takes its secret
/// elsewhere would be a kind of its own, tagged.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSocket {
    /// The one host it opens at, over TLS (`gateway.discord.gg`).
    pub host: String,
    /// The path on the proxy after the secret's name that opens it, no
    /// secret in it (`gateway`).
    pub path: String,
    /// A JSON pointer (RFC 6901) into the first message where the
    /// placeholder is (`/d/token`).
    pub pointer: String,
}

/// A path a secret's call may take: `prefix` (`bot{secret}/`, holding
/// `{secret}` once unless the secret goes in a header), then `rest`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SecretPath {
    pub prefix: String,
    pub rest: SecretRest,
}

/// What may follow a [`SecretPath`]'s prefix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretRest {
    /// One name of letters and digits (a method: `getUpdates`).
    #[default]
    Name,
    /// A relative path: parts of letters, digits, `_`, `-` and `.`,
    /// none empty, `.` or `..` (a file: `photos/file_1.jpg`).
    Path,
}

/// A header a secret goes in: `name: <value, {secret} in it>`
/// (`Authorization`, `Bearer {secret}`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SecretHeader {
    pub name: String,
    pub value: String,
    /// A user: `{secret}` is then HTTP Basic's credentials, the base64 of
    /// `<user>:<secret>`, as git over HTTPS takes a token
    /// (`x-access-token` for GitHub). The placeholder is sent the same
    /// way, as `<user>:<placeholder>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic: Option<String>,
}

impl SecretHeader {
    /// Header `name`, the secret after `prefix` in its value
    /// (`Authorization`, `Bearer `).
    pub fn prefixed(name: &str, prefix: &str) -> SecretHeader {
        SecretHeader {
            name: name.into(),
            value: format!("{prefix}{{secret}}"),
            basic: None,
        }
    }

    /// The header's value with `secret` (the value, or its placeholder)
    /// where `{secret}` is, encoded as [`SecretHeader::basic`] says.
    pub fn filled(&self, secret: &str) -> String {
        match &self.basic {
            None => self.value.replace("{secret}", secret),
            Some(user) => {
                use base64::Engine;
                let pair = format!("{user}:{secret}");
                let encoded = base64::engine::general_purpose::STANDARD.encode(pair.as_bytes());
                self.value.replace("{secret}", &encoded)
            }
        }
    }
}

/// An error unless `name` can name a secret on the proxy: 1 to 40
/// lowercase letters, digits and `-`, not starting with `-`.
pub fn check_secret_name(name: &str) -> anyhow::Result<()> {
    let plain = !name.is_empty()
        && name.len() <= 40
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    anyhow::ensure!(
        plain,
        "{name:?} isn't a secret's name: lowercase letters, digits and -"
    );
    Ok(())
}

impl SecretRule {
    /// An error unless the proxy can hold to it: a host, and `{secret}`
    /// once in each path's prefix, or in the header's value.
    pub fn check(&self) -> anyhow::Result<()> {
        crate::net::check_host(&self.host)?;
        anyhow::ensure!(
            !self.host.starts_with("*.") && self.host.parse::<std::net::IpAddr>().is_err(),
            "{} isn't one host's name",
            self.host
        );
        anyhow::ensure!(!self.paths.is_empty(), "a secret's rule with no paths");
        let once = |text: &str| text.matches("{secret}").count() == 1;
        for path in &self.paths {
            let plain = path
                .prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-./{}".contains(&b))
                && !path.prefix.starts_with('/')
                && !path.prefix.split('/').any(|p| p == "..");
            anyhow::ensure!(plain, "{:?} isn't a path's prefix", path.prefix);
            match &self.header {
                None => anyhow::ensure!(
                    once(&path.prefix),
                    "{:?} needs {{secret}} once",
                    path.prefix
                ),
                Some(_) => anyhow::ensure!(
                    !path.prefix.contains('{'),
                    "{:?}: the secret goes in the header",
                    path.prefix
                ),
            }
        }
        if let Some(header) = &self.header {
            let name_ok = !header.name.is_empty()
                && header
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-');
            let value_ok =
                once(&header.value) && header.value.bytes().all(|b| (0x20..0x7f).contains(&b));
            anyhow::ensure!(
                name_ok && value_ok,
                "{:?} isn't a secret's header",
                header.name
            );
            if let Some(user) = &header.basic {
                anyhow::ensure!(
                    !user.is_empty()
                        && user.len() <= 64
                        && user
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
                    "{:?} isn't a user for Basic: letters, digits, ., _ and -",
                    user
                );
            }
        }
        for method in &self.methods {
            anyhow::ensure!(
                !method.is_empty() && method.bytes().all(|b| b.is_ascii_uppercase()),
                "{method:?} isn't a request method"
            );
        }
        if let Some(socket) = &self.socket {
            socket.check()?;
            anyhow::ensure!(
                !self
                    .paths
                    .iter()
                    .any(|path| path.prefix.starts_with(&socket.path)),
                "{:?}: a socket's path is no call's",
                socket.path
            );
        }
        Ok(())
    }
}

impl SecretSocket {
    /// An error unless the route can hold to it: one host, a plain path
    /// part, and a pointer into an object.
    pub fn check(&self) -> anyhow::Result<()> {
        crate::net::check_host(&self.host)?;
        anyhow::ensure!(
            !self.host.starts_with("*.") && self.host.parse::<std::net::IpAddr>().is_err(),
            "{} isn't one host's name",
            self.host
        );
        anyhow::ensure!(
            !self.path.is_empty()
                && self
                    .path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "{:?} isn't a socket's path: letters, digits, _ and -",
            self.path
        );
        anyhow::ensure!(
            self.pointer.len() > 1
                && self.pointer.starts_with('/')
                && self.pointer[1..].split('/').all(|part| !part.is_empty()),
            "{:?} isn't a pointer into the first message",
            self.pointer
        );
        Ok(())
    }
}

/// Where a program behind a proxy reaches the service of secret `name`,
/// its proxy on `port`: the service's root, in place of
/// `https://<host>` (`<root>/<path>`).
pub fn url(port: u16, name: &str) -> String {
    format!("http://127.0.0.1:{port}{PREFIX}{name}")
}

impl Sealed {
    /// `value` for a route to add by `rule`, as `name`, with its
    /// placeholder among `placeholders`.
    pub fn new(
        placeholders: &Placeholders,
        name: &str,
        value: link_secretive::Secret,
        rule: SecretRule,
    ) -> Self {
        Sealed {
            name: name.to_string(),
            placeholder: placeholder(placeholders, name, &value, rule.public.as_deref()),
            value,
            rule,
        }
    }

    /// An error unless its placeholder is one Link made for it, by its
    /// rule ([`check_placeholder`](link_sandbox_route::check_placeholder)).
    pub fn check_placeholder(&self) -> anyhow::Result<()> {
        let prefix = secret_prefix(&self.value, self.rule.public.as_deref());
        link_sandbox_route::check_placeholder(&self.placeholder, &prefix)
    }
}

/// What a program is given in place of the secret named `name` holding
/// `value`, one of `placeholders`: a tag, none of the secret, after the
/// part a rule says isn't secret (`public`: the digits before that
/// separator, or `0`), which a client may read.
pub fn placeholder(
    placeholders: &Placeholders,
    name: &str,
    value: &link_secretive::Secret,
    public: Option<&str>,
) -> link_secretive::Secret {
    placeholders.placeholder(&secret_prefix(value, public), &format!("secret/{name}"))
}

/// What a placeholder for a secret holding `value` starts with: the
/// digits before `public`, the separator ending the part that isn't
/// secret, and the separator (`0` when there are none), or nothing.
pub fn secret_prefix(value: &link_secretive::Secret, public: Option<&str>) -> String {
    let Some(separator) = public.filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let id = value
        .expose()
        .split_once(separator)
        .map(|(id, _)| id)
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or("0");
    format!("{id}{separator}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placeholder_keeps_what_isnt_secret_alone() {
        let made = Placeholders::new().unwrap();
        let token: link_secretive::Secret = "123456:AAH-secret_part".to_string().into();
        let held = placeholder(&made, "chat", &token, Some(":"));
        assert!(held.expose().starts_with("123456:link-wire-"));
        assert!(!held.expose().contains("secret"));
        link_sandbox_route::check_placeholder(&held, "123456:").unwrap();
        let other = |v: &str, public| placeholder(&made, "chat", &v.to_string().into(), public);
        assert!(
            other("nonsense", Some(":"))
                .expose()
                .starts_with("0:link-wire-")
        );
        // Without the separator nothing is public, digits or not.
        assert!(
            other("123456", Some(":"))
                .expose()
                .starts_with("0:link-wire-")
        );
        assert!(other("123456:x", None).expose().starts_with("link-wire-"));
        let secret = Sealed::new(
            &made,
            "chat",
            "123:secret".to_string().into(),
            SecretRule::default(),
        );
        assert_eq!(
            secret.placeholder,
            placeholder(&made, "chat", &secret.value, None)
        );
        assert!(!format!("{secret:?}").contains("123:secret"));
        assert_eq!(
            url(40781, "chat"),
            "http://127.0.0.1:40781/_link/secret/chat"
        );
        // Each secret of a run its own, and none a model key's.
        let key: link_secretive::Secret = "sk-ant-api03-the-key".to_string().into();
        assert_ne!(
            placeholder(&made, "chat", &key, None),
            placeholder(&made, "mail", &key, None)
        );
        assert_ne!(
            placeholder(&made, "chat", &key, None),
            made.placeholder("", "model")
        );
    }

    #[test]
    fn a_placeholder_is_the_same_tag_of_its_secret() {
        // Pinned: a holder's placeholders, kept for its run, are the
        // same tags whatever version of Link reads them.
        let made = Placeholders::from_kept(link_secretive::OpaqueSecret::from_text(
            "pinned-placeholders-secret".to_string().into(),
        ));
        assert_eq!(
            placeholder(&made, "chat", &"42:t".to_string().into(), Some(":")).expose(),
            "42:link-wire-e5a4756cb3f1c01a97ca56f72c9eea2c"
        );
    }

    /// A route kept in a file names its secrets: one holding a value is
    /// marked handed, and refused there; one naming them alone isn't.
    #[test]
    fn a_route_in_a_file_refuses_a_value() {
        let made = Placeholders::new().unwrap();
        let rule = || SecretRule {
            host: "api.example.com".into(),
            paths: vec![SecretPath {
                prefix: "v1/{secret}/".into(),
                rest: SecretRest::Name,
            }],
            ..SecretRule::default()
        };
        let value: link_secretive::Secret = "the-value".to_string().into();
        let named = Named::new(&made, "svc", Kept::Variable("SVC".into()), &value, rule());
        let handed = Sealed::new(&made, "svc", value.duplicate(), rule());
        let names = route(Bridge::default(), Vec::new(), vec![named]).unwrap();
        names.names_only().unwrap();
        assert!(!names.given.expose().contains("the-value"));
        let valued = route(Bridge::default(), vec![handed], Vec::new()).unwrap();
        let refused = valued.names_only().unwrap_err().to_string();
        assert!(
            refused.contains("secret route holds a secret's value"),
            "{refused}"
        );
    }

    /// The bridge adds a named secret only for a caller the store grants
    /// it, of its kind and name, and only one its placeholder was made
    /// for: a grant removed, or the value changed, stops it.
    #[test]
    fn a_named_secret_is_added_only_for_a_caller_granted_it() {
        use crate::secrets::{ForService, Use, Which};
        let made = Placeholders::new().unwrap();
        let rule = SecretRule {
            host: "api.telegram.org".into(),
            paths: vec![SecretPath {
                prefix: "bot{secret}/".into(),
                rest: SecretRest::Name,
            }],
            public: Some(":".into()),
            ..SecretRule::default()
        };
        let token: link_secretive::Secret = "42:the-token".to_string().into();
        let own = Use::own("connection:telegram:bot_token").unwrap();
        let named = Named::new(&made, "telegram", Kept::Own(own.clone()), &token, rule);
        let given = Given {
            granted: vec![named],
            ..Given::default()
        };
        let target = "/_link/secret/telegram/getMe";
        let grant = |r#as: Use, value: &str| ForService {
            name: None,
            r#as,
            value: value.to_string().into(),
            sealed: None,
        };
        // Granted: its value, asked only of the caller's own.
        let granted = |which| {
            assert_eq!(which, Which::Own);
            Ok(vec![
                grant(Use::own("model").unwrap(), "sk-x"),
                grant(own.clone(), "42:the-token"),
            ])
        };
        let found = given
            .find(target, granted, |_, _, _| panic!("no token"))
            .unwrap()
            .unwrap();
        assert_eq!(found.value, token);
        // Granted none of it, another of its own, or another value.
        for others in [
            vec![],
            vec![grant(Use::own("model").unwrap(), "42:the-token")],
            vec![grant(own.clone(), "43:another-token")],
        ] {
            let refused = given.find(
                target,
                |_| Ok(others.iter().map(dup).collect()),
                |_, _, _| panic!("no token"),
            );
            assert!(refused.is_err(), "{others:?}");
        }
        // A target naming none is no secret, and nothing is read.
        let none = given.find(
            "/_link/secret/other/getMe",
            |_| panic!("read"),
            |_, _, _| panic!("made"),
        );
        assert!(none.unwrap().is_none());
    }

    /// A variable goes only as the store seals it now: created again for
    /// another host, or kept plain, it's refused by a route made for the
    /// old host, never sent there.
    #[test]
    fn a_variable_goes_only_by_its_sealing_now() {
        use crate::secrets::{ForService, Sealing, Use};
        let made = Placeholders::new().unwrap();
        let first = Sealing::new("api.example.com", None, None, None).unwrap();
        let value: link_secretive::Secret = "the-value".to_string().into();
        let named = Named::new(
            &made,
            "svc",
            Kept::Variable("SVC".into()),
            &value,
            first.rule(),
        );
        let grant = |sealed: Option<Sealing>| ForService {
            name: Some("SVC".into()),
            r#as: Use::Variable,
            value: "the-value".to_string().into(),
            sealed,
        };
        let resolve = |sealed: Option<Sealing>| {
            named.resolved(
                |_| Ok(vec![grant(sealed.clone())]),
                |_, _, _| panic!("no token"),
            )
        };
        let sent = resolve(Some(first.clone())).unwrap();
        assert_eq!(sent.rule.host, "api.example.com");
        for now in [
            Some(Sealing::new("api.other.example", None, None, None).unwrap()),
            Some(Sealing::new("api.example.com", Some("X-Api-Key"), None, None).unwrap()),
            None,
        ] {
            let refused = resolve(now.clone()).unwrap_err().to_string();
            assert!(
                refused.contains("sealed now for another"),
                "{now:?}: {refused}"
            );
        }
    }

    #[test]
    fn a_token_is_the_callers_own_made_at_each_call() {
        let made = Placeholders::new().unwrap();
        let rule = SecretRule {
            host: "dispatch.example.com".into(),
            header: Some(SecretHeader {
                name: "authorization".into(),
                value: "Bearer {secret}".into(),
                basic: None,
            }),
            ..SecretRule::default()
        };
        let named = Named::token(&made, "dispatch", "reader@docs:r7", rule.clone()).unwrap();
        let given = Given {
            granted: vec![named],
            ..Given::default()
        };
        let target = "/_link/secret/dispatch/v1/pages";
        let token = |host: &str, grants: &str, assertion: Option<&Assertion>| {
            assert!(assertion.is_none());
            assert_eq!((host, grants), ("dispatch.example.com", "reader@docs:r7"));
            Ok("eyJ.the.token".to_string().into())
        };
        let found = given
            .find(target, |_| panic!("the store read"), token)
            .unwrap()
            .unwrap();
        assert_eq!(found.value.expose(), "eyJ.the.token");
        // No token made, no call.
        let none = given.find(
            target,
            |_| panic!("read"),
            |_, _, _| anyhow::bail!("signed out"),
        );
        assert!(none.is_err());
        // A token keeps none of it public.
        let public = SecretRule {
            public: Some(":".into()),
            ..rule
        };
        assert!(Named::token(&made, "dispatch", "", public).is_err());
    }

    fn dup(given: &crate::secrets::ForService) -> crate::secrets::ForService {
        crate::secrets::ForService {
            name: given.name.clone(),
            r#as: given.r#as.clone(),
            value: given.value.duplicate(),
            sealed: given.sealed.clone(),
        }
    }

    #[test]
    fn a_placeholder_is_refused_unless_its_whole_shape_holds() {
        let one = Placeholders::new().unwrap();
        let tag = "0123456789abcdef0123456789abcdef";
        let token: link_secretive::Secret = "42:the-token".to_string().into();
        let rule = SecretRule {
            public: Some(":".into()),
            ..SecretRule::default()
        };
        let mut secret = Sealed::new(&one, "chat", token, rule);
        secret.check_placeholder().unwrap();
        for wrong in [format!("43:link-wire-{tag}"), format!("link-wire-{tag}")] {
            secret.placeholder = wrong.clone().into();
            assert!(secret.check_placeholder().is_err(), "{wrong}");
        }
    }
}
