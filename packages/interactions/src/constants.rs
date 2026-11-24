/// Names the machine's interaction store for a harness whose `HOME` is
/// its own, which `lnk-agent` hands it where it can reach the store
/// ([`crate::Host`]).
pub const STORE_VAR: &str = "LNK_INTERACTIONS";
pub const STATE_FILE: &str = "interactions.json";
pub const TEMP_FILE: &str = "interactions.next";
pub const CODE_LENGTH: usize = 6;
pub const CODE_ATTEMPTS: usize = 128;
pub const CODE_ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
pub const MAX_STATE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_RECORDS: usize = 10_000;
pub const MAX_EVENTS: usize = 100_000;
pub const MAX_ITEMS: usize = 32;
pub const MAX_OPTIONS: usize = 64;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_WAIT_SECS: u64 = 24 * 60 * 60;
pub const DEFAULT_WAIT_SECS: u64 = 120;
pub const POLL: std::time::Duration = std::time::Duration::from_millis(100);
pub const POLICY_REVISION: &str = "source-wait";
pub const CONFIRM_ITEM: &str = "confirmation";
pub const DOMAINS: &str = include_str!("../domains.toml");
pub(crate) const DISPATCH_LOCK: &str = "dispatch.lock";
pub(crate) const DESKTOP_ENV: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "LANG",
];
pub(crate) const SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
