//! DNS in a sandbox (Linux), where its policy names raw endpoints
//! (`[internet] tcp`): a resolver at `127.0.0.1:53` in its own network,
//! answered by its proxy outside, for the names its rules allow and no
//! others.
//!
//! - A name a `tcp` endpoint holds (`db.example.com`, or one under
//!   `*.abc.mongodb.net`) gets an address of the sandbox's own: its
//!   `/etc/hosts` one for a name listed outright, else one from
//!   `127.1.0.1` on ([`Names`]). The proxy's end inside listens on the
//!   endpoint's port there and sends each connection to the proxy as
//!   `CONNECT name:port`, which it checks and logs as any other. No IPv6
//!   address.
//! - A name its hosts allow gets its public addresses (never this
//!   machine's, its network's or a metadata service's), which reach
//!   nothing directly: its traffic still goes through the proxy.
//! - SRV and TXT, for such a name or a service of it (`_mongodb._tcp.<name>`,
//!   so `mongodb+srv://` works), get what the machine's resolver answers,
//!   never for a local network's names (`.local`, one label, ...).
//!   An SRV answer's targets are names the program then looks up and
//!   connects to, checked as any other.
//! - Every other name is refused (`REFUSED`), before anything is asked.
//!
//! Each query is a call on the wire: logged (`dns`, the name and type).
//! What asks outside (a name's A, SRV, TXT) counts in its calls a minute.
//! A refused one is logged once a minute at most and never counts toward
//! a pause: a program looking up a name it may not reach is no attack,
//! and learns nothing. A name resolving only to a metadata service
//! pauses the wire, as connecting to it does.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Mutex;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::filter::Filter;

mod message;
mod rules;
mod serve;
#[cfg(target_os = "linux")]
mod sockets;
#[cfg(test)]
mod tests;
mod upstream;

pub use message::*;
pub use rules::*;
pub use serve::*;
#[cfg(target_os = "linux")]
pub use sockets::*;
pub use upstream::*;
