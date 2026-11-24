//! Arguments for a program re-entering the guest-service protocol.
use serde::{Deserialize, Serialize};

pub const ENTRY: &str = "guest-service";

#[derive(clap::Args, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestEntry {
    #[arg(value_enum)]
    pub role: Role,
    pub unit: String,
    pub epoch: String,
}

#[derive(clap::ValueEnum, Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Supervisor,
    Worker,
}
