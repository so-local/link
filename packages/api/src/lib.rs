//! Link's API client: the account service's API
//! (`schemas/account.yaml` and `schemas/remote.yaml`), one typed function a route, on
//! [`link_fetch`].
//!
//! - **Its types are the spec's.** [`types`] is made at every build from
//!   the owning schemas (`build.rs`, typify): objects are closed, string
//!   patterns checked and credentials held as [`Secret`]. Callers enforce
//!   semantic and numeric bounds the generated type does not represent.
//! - **One function a route,** named as the spec's `operationId`, each
//!   taking the route's request type and answering its answer type.
//!   [`ROUTES`] lists them, and a test holds them to the spec's paths.
//! - **No credential of its own.** A call that needs one is handed it,
//!   a `&Secret`, by its caller. [`Api::sign_for`] uses a caller-supplied
//!   installation signer; this client chooses no local credential store.
//! - **Access tokens, made and checked in one place** ([`jwt`]): the
//!   assertion an installation signs, and the check a relay or a service makes
//!   of a token, with the account service's public keys alone.
//! - **link-fetch's rules:** `https://` only (or `http://` on this
//!   installation), no redirects, an answer's size capped, and errors mapped
//!   once: a 401 is [`Error::SignedOut`], a 429 [`Error::Later`]. What a
//!   refusal says, in the spec's words, is [`refused`].
//!
//! The one answer read entry by entry, not closed, is the report's
//! ([`Asked`]): an action of a kind this installation doesn't know is refused
//! alone, so a newer service never stops an older installation's actions.

use link_fetch::Client;
pub use link_fetch::Error;
use link_secretive::Secret;
use serde_json::Value;

/// What the generated types are made of, for their callers to name.
pub use jiff;
pub use uuid;

/// The spec's schemas, as Rust types.
pub mod types {
    pub use link_principal::{Principal, PrincipalKind};
    include!(concat!(env!("OUT_DIR"), "/types.rs"));
}

pub mod jwt;
pub mod stopped;

use types::{
    AccessToken, Account, AuditLog, DeviceLogin, ErrorCode, GrantInstallation, InstallationId,
    Installations, PollDevice, RelayAccounts, Report, SigningKeys, StartDevice, Stopped,
    StoppedAccounts, TokenRequest, User, Username,
};

/// The account API's protocol, as `protocol.toml` declares it: the
/// version this release's calls state in their path ([`base_path`]).
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> =
    std::sync::LazyLock::new(|| link_version::Protocol::shipped(include_str!("protocol.toml")));

/// Where every route is, under a service's URL: this release's version's,
/// `/api/v1`, stating the protocol version of every call.
pub fn base_path() -> String {
    format!("/api/v{}", PROTOCOL.current)
}

/// Every route, as the spec names it: its method, its path under
/// [`base_path`], and the function that calls it.
pub const ROUTES: &[(&str, &str, &str)] = &[
    ("GET", "/relay", "relay_accounts"),
    ("POST", "/device", "start_device"),
    ("POST", "/device/token", "poll_device"),
    ("GET", "/account", "account"),
    ("DELETE", "/account", "delete_account"),
    ("POST", "/logout", "logout"),
    ("GET", "/installations", "list_installations"),
    ("POST", "/installations", "grant_installation"),
    ("DELETE", "/installations/{id}", "sign_out_installation"),
    ("POST", "/token", "token"),
    ("GET", "/keys", "keys"),
    ("POST", "/agents", "report_agents"),
    ("GET", "/users/{username}", "user"),
    ("GET", "/iam/log", "iam_log"),
    ("POST", "/iam/stop", "stop"),
    ("GET", "/stopped", "stopped_accounts"),
];

/// The account service at one URL. Holds no credential: each call that
/// needs one is handed it. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Api {
    http: Client,
    url: String,
}

impl Api {
    /// `GET /relay`: public discovery for a registered relay, served
    /// independently of that relay's box. The caller checks its audience.
    pub async fn relay_accounts(&self, audience: &str) -> Result<RelayAccounts, Error> {
        self.http
            .get(&self.at("/relay"))
            .query(&[("audience", audience)])
            .answer()
            .await
    }

    /// The service at `url` (`https://accounts.example.com`, as a relay names
    /// it), its routes under [`base_path`].
    pub fn new(url: &str) -> Result<Self, Error> {
        Ok(Self::on(Client::new()?, url))
    }

    /// [`Api::new`], its calls on `http`: one client for a caller's life,
    /// so its connection is kept.
    pub fn on(http: Client, url: &str) -> Self {
        Self {
            http,
            url: url.trim_end_matches('/').to_string(),
        }
    }

    /// The service's URL, as it was given.
    pub fn url(&self) -> &str {
        &self.url
    }

    fn at(&self, path: &str) -> String {
        format!("{}{}{path}", self.url, base_path())
    }

    /// `POST /device`: a device login started, for a person to approve
    /// in the browser.
    pub async fn start_device(&self, start: &StartDevice) -> Result<DeviceLogin, Error> {
        self.http
            .post(&self.at("/device"))
            .json(start)
            .answer()
            .await
    }

    /// `POST /device/token`: the login's answer, once the person gave
    /// it: the installation, signed in with the key it started with. Until
    /// then a 400 [`Error::Refused`], [`refused`] saying
    /// [`ErrorCode::AuthorizationPending`] or why never.
    pub async fn poll_device(&self, poll: &PollDevice) -> Result<Account, Error> {
        self.http
            .post(&self.at("/device/token"))
            .json(poll)
            .answer()
            .await
    }

    /// `GET /account`: who `token` is.
    pub async fn account(&self, token: &Secret) -> Result<Account, Error> {
        self.http
            .get(&self.at("/account"))
            .bearer(token)
            .answer()
            .await
    }

    /// `DELETE /account`: `token`'s account deleted for good, its plan
    /// ended and what the services hold of it erased first; asked again
    /// after a failure, it goes on from where it stopped.
    pub async fn delete_account(&self, token: &Secret) -> Result<(), Error> {
        self.http
            .delete(&self.at("/account"))
            .bearer(token)
            .answer()
            .await
    }

    /// `POST /logout`: the installation `token` is for, signed out for good.
    pub async fn logout(&self, token: &Secret) -> Result<(), Error> {
        self.http
            .post(&self.at("/logout"))
            .bearer(token)
            .answer()
            .await
    }

    /// `GET /iam/log`: `token`'s account's audit log, newest first.
    pub async fn iam_log(&self, token: &Secret) -> Result<AuditLog, Error> {
        self.http
            .get(&self.at("/iam/log"))
            .bearer(token)
            .answer()
            .await
    }

    /// `POST /iam/stop`: every installation of `token`'s account but its own
    /// signed out, and the account stopped from now.
    pub async fn stop(&self, token: &Secret) -> Result<Stopped, Error> {
        self.http
            .post(&self.at("/iam/stop"))
            .bearer(token)
            .answer()
            .await
    }

    /// `GET /stopped`: every stopped account, with when it stopped.
    pub async fn stopped_accounts(&self) -> Result<StoppedAccounts, Error> {
        self.http.get(&self.at("/stopped")).answer().await
    }

    /// `GET /installations`: every installation signed in to `token`'s account.
    pub async fn list_installations(&self, token: &Secret) -> Result<Installations, Error> {
        self.http
            .get(&self.at("/installations"))
            .bearer(token)
            .answer()
            .await
    }

    /// `POST /installations`: a new installation of `token`'s account, with the
    /// public half of the key pair it made, signed in by this one.
    pub async fn grant_installation(
        &self,
        token: &Secret,
        grant: &GrantInstallation,
    ) -> Result<Account, Error> {
        self.http
            .post(&self.at("/installations"))
            .bearer(token)
            .json(grant)
            .answer()
            .await
    }

    /// `GET /users/{username}`: the account `username` names now, to let
    /// it in by its id. A 404 [`Error::Refused`] when none does.
    pub async fn user(&self, token: &Secret, username: &Username) -> Result<User, Error> {
        // A username: letters, digits and dashes, nothing that leaves the path.
        self.http
            .get(&self.at(&format!("/users/{}", username.as_str())))
            .bearer(token)
            .answer()
            .await
    }

    /// `DELETE /installations/{id}`: installation `id` of `token`'s account signed
    /// out. A 404 [`Error::Refused`] when no such installation is signed in.
    pub async fn sign_out_installation(
        &self,
        token: &Secret,
        id: &InstallationId,
    ) -> Result<(), Error> {
        // A uuid: nothing in it leaves the path.
        self.http
            .delete(&self.at(&format!("/installations/{id}")))
            .bearer(token)
            .answer()
            .await
    }

    /// `POST /token`: an access token, for an assertion an installation
    /// signed ([`Api::sign_for`] makes one and asks). A 400
    /// [`Error::Refused`], [`refused`] saying [`ErrorCode::InvalidGrant`],
    /// when the service doesn't take it: the installation signed out among
    /// its reasons.
    pub async fn token(&self, request: &TokenRequest) -> Result<AccessToken, Error> {
        self.http
            .post(&self.at("/token"))
            .json(request)
            .answer()
            .await
    }

    /// `GET /keys`: the public keys access tokens are checked with.
    pub async fn keys(&self) -> Result<SigningKeys, Error> {
        self.http.get(&self.at("/keys")).answer().await
    }

    /// An access token of `scope` for `resource`, signed for by
    /// `installation` with its key: a [`jwt::assertion`] for this service,
    /// traded at [`Api::token`].
    pub async fn sign_for(
        &self,
        installation: &jwt::Installation<'_>,
        resource: &str,
        scope: types::Scope,
    ) -> Result<AccessToken, Error> {
        let assertion = jwt::assertion(installation, &self.url, resource, scope, jwt::now());
        let request = TokenRequest { assertion };
        self.token(&request).await
    }

    /// An access token for `installation`'s agent `agent`, as itself, granted
    /// `grants` at `resource` ([`jwt::agent_assertion`]).
    pub async fn sign_for_agent(
        &self,
        installation: &jwt::Installation<'_>,
        resource: &str,
        agent: &types::AgentId,
        grants: &types::Grants,
    ) -> Result<AccessToken, Error> {
        let assertion =
            jwt::agent_assertion(installation, &self.url, resource, agent, grants, jwt::now());
        let request = TokenRequest { assertion };
        self.token(&request).await
    }

    /// `POST /agents`: `report` kept as the installation `token` is, in place
    /// of its last. What's asked of it, when `report.actions` said it
    /// takes actions; none when it doesn't (`204`).
    pub async fn report_agents(&self, token: &Secret, report: &Report) -> Result<Asked, Error> {
        let said: Option<Value> = self
            .http
            .post(&self.at("/agents"))
            .bearer(token)
            .json(report)
            .answer()
            .await?;
        Ok(said.as_ref().map(Asked::read).unwrap_or_default())
    }
}

/// What a refusal says it is, in the spec's words (its `Error`'s
/// `error`): what a program reads, never `error_description`. None for an
/// error that isn't a refusal, or one that says no such word.
pub fn refused(err: &Error) -> Option<ErrorCode> {
    let said: types::Error = err.refusal()?.read().ok()?;
    Some(said.error)
}

/// What the service asked of an installation with its report (`Actions`): each
/// action as it came, for the installation to read on its own. The one answer
/// read so: anything else it holds is passed over, and an action this
/// installation can't read is refused alone, said in its next report, so a
/// service newer than the installation never stops its actions nor has it
/// post the same outcomes again and again.
#[derive(Debug, Default)]
pub struct Asked {
    pub actions: Vec<Value>,
}

impl Asked {
    /// The actions in what the service answered, read entry by entry.
    pub fn read(said: &Value) -> Self {
        let actions = said["actions"].as_array().cloned().unwrap_or_default();
        Self { actions }
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
