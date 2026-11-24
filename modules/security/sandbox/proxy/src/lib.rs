//! The filtering proxy: a sandbox with no network of its own but one
//! proxy, which Link runs outside it.
//!
//! On Linux the sandbox gets a network namespace of its own
//! (`--unshare-net`): its own loopback, and no way out. Small relays cross
//! it, through unix sockets in a folder both sides see:
//!
//! - **Out:** inside, `sandbox proxy-inside` listens on
//!   `127.0.0.1:`[`PROXY_PORT`] (the program's `HTTPS_PROXY`) and passes
//!   each connection to `out.sock`, where `sandbox proxy`, outside, speaks
//!   HTTP proxy: `CONNECT host:port`, or a plain `http://` request. It
//!   resolves the host itself, refuses loopback, private and link-local
//!   addresses (this machine, its network, a cloud's metadata service),
//!   and dials the rest directly or through a SOCKS5 upstream: the home
//!   exit, `ssh -R` from home.
//! - **Local:** inside, the relay listens on each of this machine's ports
//!   the policy allows (a local model's) and passes each connection to
//!   `local.sock`, naming the port; outside, it's joined to that port on
//!   the machine's loopback, if it's one of them.
//! - **In:** outside, `sandbox proxy` listens on the program's own ports on
//!   the machine's loopback and passes each connection to `in.sock`,
//!   where the inside relay connects to that port in the sandbox: Link's
//!   health checks reach a harness as before.
//!
//! On macOS there are no network namespaces: `sandbox proxy --listen`
//! serves the proxy on a port of this machine's loopback, and the
//! sandbox's Seatbelt profile, without `network`, allows connecting to
//! that port and the policy's own, nothing else. The program serves its
//! ports on the machine's loopback itself.
//!
//! What doesn't use the proxy has no network at all: it fails closed.
//!
//! The proxy names no way out but the network: each other one (a model,
//! a secret, a tool, a page to open) is a route ([`routes`]) its bind
//! mounts at a prefix of `/_link/`, which the proxy checks, logs and
//! hands to the route's bridge outside ([`bridged`]).

pub mod bind;
pub mod bridged;
pub mod client;
pub mod dial;
pub mod dns;
pub mod filter;
pub mod http;
pub mod lan;
pub mod listen;
pub mod outside;
pub mod relay;
pub mod routes;
pub mod run;
pub mod serve;
#[cfg(test)]
mod tests;
pub mod wire;

pub use bind::bound;
pub use client::{PROXY_PORT, connect, git_ssh_command, proxy_env};
pub use filter::Filter;
pub use outside::{Bound, blank, listening, outside};
pub use relay::{inside, loopback_inside, loopback_outside};
pub use routes::{Client, Peer, Routes};
pub use serve::proxy;

#[cfg(test)]
link_test_kit::program!(system);
