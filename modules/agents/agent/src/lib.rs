//! The harness module: `lnk agent start`, `use`, `stop`, `log show`, `connect`
//! (telegram, slack, discord), `disconnect`, `permissions`, `allow`,
//! `deny`, and each harness's own, `lnk agent harness-permission list`,
//! `allow`, `deny`, `reset`, `check`, `run`, `show` and `remove`.
//!
//! It owns what stays the same when the harness changes: the settings
//! (model key, files folder, connections), the background service, what each
//! harness may do in its sandbox, and switching. The sandbox itself is the
//! sandbox module's (`lnk sandbox`, through `link_sandbox_client`). Everything specific to one harness is in its
//! adapter module (`lnk-harness`, or one from outside Link), run through the commands
//! in `link_harness_contract::harness`.

/// The files this library writes that an upgrade converts, each by its
/// format.
pub const SAVED: [fn() -> link_files::upgrade::Saved; 4] = [
    grants::saved,
    settings::saved,
    machine_settings::saved,
    journal::saved,
];

pub mod adapter;
pub mod archives;
pub mod background;
pub mod bridge;
pub mod bridges;
pub mod carried;
pub mod chat;
pub mod checks;
pub mod cli;
pub mod connections;
pub mod databases;
pub mod deleted;
pub mod grants;
pub mod home;
pub mod hosted;
pub mod hosted_teams;
pub mod identity;
pub mod idle;
pub mod interaction_mcp;
pub mod journal;
pub mod kept;
pub mod layout;
pub mod machine_settings;
pub mod migrating;
pub mod moved;
pub mod owners;
pub mod programs;
pub mod pushed;
pub mod sandbox_file;
pub mod schedules;
pub mod service;
pub mod settings;
pub mod skills;
pub mod times;
pub mod traces;
pub mod transfer;
pub mod waits;
pub mod webhooks;

#[cfg(test)]
link_test_kit::program!(system);
