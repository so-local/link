//! The one HTTP client Link's own calls go out on: to the account
//! service and other HTTP services. Clients start with shared defaults
//! and callers explicitly select transport or typed-answer exceptions:
//!
//! - **Link's user agent** ([`USER_AGENT`]), and timeouts: [`TIMEOUT`]
//!   for a whole call unless the caller says another, [`CONNECT_TIMEOUT`]
//!   to connect.
//! - **No redirects by default.** A redirect is an error
//!   ([`Error::Redirected`]); [`Builder::follow_redirects`] permits bounded
//!   HTTPS redirects for credential-free downloads.
//! - **`https://` only**, or `http://` to this machine
//!   ([`is_this_machine`]) by default ([`Error::Insecure`]). A plain
//!   `http://` call uses no proxy; `localhost` and its subdomains resolve
//!   directly to loopback ([`Loopback`]). Explicit [`Builder::plain_http`]
//!   permits remote HTTP with normal DNS; [`Builder::metadata_server`]
//!   instead permits the fixed metadata endpoint.
//! - **An answer's size capped** ([`MAX_ANSWER`], or the call's own
//!   [`Request::limit`]), read so: a longer one is an error, never read
//!   whole first.
//! - **Typed both ways.** A request's body is a `Serialize` type, an
//!   answer a `Deserialize` type ([`Request::answer`]), and every answer
//!   is closed by default: a field its type doesn't name is refused as
//!   `deny_unknown_fields` would, whether or not the type says so. An
//!   answer that doesn't parse is an error naming the route
//!   ([`Error::Unexpected`]), never a guess.
//! - **Errors map once:** 401 to signed out ([`Error::SignedOut`]), 429 to
//!   try again later ([`Error::Later`], with its `Retry-After`), a 5xx to the service's fault
//!   ([`Error::Failed`]), any other refusal with what it said
//!   ([`Error::Refused`]). None prints a credential, a query string or a
//!   value the answer held.
//! - **Credentials go in as [`Secret`]s** ([`Request::bearer`],
//!   [`Request::basic`]), into headers marked sensitive; nothing here
//!   prints one.
//!
//! It is for Link's own calls. What an agent fetches (a page, a model)
//! doesn't go through it.

pub mod blocking;

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use link_secretive::Secret;
pub use reqwest::Method;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use serde::Serialize;
use serde::de::DeserializeOwned;

mod addresses;
mod client;
mod errors;
mod request;
pub use addresses::{
    HttpsUrl, Loopback, SendUrl, is_on_this_machine, is_public, is_this_machine, may_send_to,
};
use addresses::{PublicOnly, is_public_url, may_send};
pub use client::{Builder, Client};
use client::{Plain, Rules};
pub use errors::{Error, Refusal, Route, printable};
pub use request::{Request, Streamed};
use request::{Target, parse};

/// What Link's calls say they are: `lnk/<version>`.
pub const USER_AGENT: &str = concat!("lnk/", env!("CARGO_PKG_VERSION"));

/// The longest a call takes, connecting to the last byte of its answer,
/// unless its client says another ([`Builder::timeout`]).
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The longest a connection takes to open, unless its client says
/// another ([`Builder::connect_timeout`]).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most of an answer read, unless the call says another
/// ([`Request::limit`]): 1 MiB.
pub const MAX_ANSWER: u64 = 1 << 20;

/// The most of a refusal's body kept, to read ([`Refusal::read`]) or show.
const MAX_REFUSAL: usize = 16 << 10;

/// The most of a refusal's body shown ([`Refusal::reason`]).
const MAX_REASON: usize = 512;

/// The most of a field's path shown when an answer has one its type
/// doesn't name.
const MAX_PATH: usize = 200;

/// Selects rustls's `ring` crypto backend for this process. Call before
/// building any TLS client; repeated calls are harmless.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
link_test_kit::program!(unit);
