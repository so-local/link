//! What a run opens a sandbox from, whichever command takes it: its spec
//! file, the field flags over it, where its internet traffic leaves from
//! and whether it says yes to what it would ask. `lnk sandbox run`,
//! `check` and `learn` each read it from their own arguments.

use std::path::PathBuf;

use link_sandbox_contract::net::Upstream;

/// A sandbox to open, as a command's arguments give it.
pub struct Opening {
    /// The spec file, its field flags laid over it.
    pub spec: Option<PathBuf>,
    /// The field flags, by the spec's key paths.
    pub fields: link_spec::FieldFlags,
    /// Where its internet traffic leaves from, as [`Upstream::parse`]
    /// reads it.
    pub upstream: String,
    /// Yes to what it would ask.
    pub yes: bool,
}

impl Opening {
    /// A command's spec file, field flags, upstream and yes: no upstream
    /// given is this machine's own network.
    pub fn of(
        spec: Option<PathBuf>,
        fields: link_spec::FieldFlags,
        upstream: Option<String>,
        yes: bool,
    ) -> Self {
        Self {
            spec,
            fields,
            upstream: upstream.unwrap_or_else(|| Upstream::Direct.text()),
            yes,
        }
    }
}
