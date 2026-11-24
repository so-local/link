//! Link web: a page fetched as markdown, and a web search through a
//! search API with a key of yours, for an agent to call as a tool
//! ([`mcp`]).
//!
//! It runs outside the agent's sandbox, as every tool does, so it keeps
//! the sandbox's rule itself: a page on this machine or its networks is
//! never fetched ([`link_sandbox_contract::net::forbidden`]), unless you name
//! its host (`--private-host`). Each name is resolved here, every address
//! it gives checked, and the connection made to those addresses only, so
//! a name can't change what it points at between the check and the
//! fetch. A redirect is followed the same way, one at a time.

pub mod cli;
pub mod limits;
pub use limits::limits;
pub mod mcp;

use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use anyhow::{Context, bail, ensure};
use link_secretive::Duplicate;
use reqwest::Url;
use serde_json::{Value, json};

mod clients;
mod converting;
mod fetching;
mod inflating;
mod page;
mod searching;
mod writing;
pub use clients::Clients;
use converting::{CONVERTING, title};
pub use converting::{Slot, Slots, markdown, part, plain};
use fetching::without_url;
pub use searching::{Freshness, Hit, Provider, Search};

/// Who it says it is: a program, not a browser.
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; lnk-web-tool/",
    env!("CARGO_PKG_VERSION"),
    "; +",
    link_core::docs!("agents/tools/web"),
    ")"
);

/// The variable naming the search API when both keys are set.
pub const SEARCH_ENV: &str = "LNK_WEB_SEARCH";

/// The variable giving the search API's address, for a server that
/// answers as one does (or a test's).
pub const SEARCH_URL_ENV: &str = "LNK_WEB_SEARCH_URL";

/// The tool: what it searches with, and which private hosts you let it
/// fetch.
#[derive(Duplicate, Debug, Default)]
pub struct Web {
    #[duplicate]
    pub search: Option<Search>,
    /// Hosts it fetches even on this machine or its networks, by name
    /// or address, as you named them (`--private-host`).
    pub private_hosts: Vec<String>,
    /// The last pages fetched, for their next parts ([`Web::page`]).
    pub pages: Pages,
    /// Its HTTP clients, made once and kept for the session.
    pub clients: Clients,
}

/// The last pages fetched (`limits.toml`'s `pages`), each by the address
/// asked for and when, kept `page-kept-seconds`: a page read a part at a
/// time is fetched and converted once, not once a part.
#[derive(Clone, Debug, Default)]
pub struct Pages(std::sync::Arc<std::sync::Mutex<Vec<(String, std::time::Instant, Page)>>>);

/// A page, fetched.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    /// Where it was at last, after its redirects.
    pub url: String,
    pub title: Option<String>,
    /// Its text: markdown for HTML, as it is for any other text.
    pub text: String,
    /// Whether it was longer than the most it reads of a page
    /// (`page-max-bytes`), and read only that far.
    pub cut: bool,
}

#[cfg(test)]
link_test_kit::program!(system);
