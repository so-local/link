//! What `access` and `own` read that TOML can't hold: the grammar of an
//! outside ID, as a connection names its chats and users, and the words of
//! an own secret's purpose.

/// The longest id a connection gives a chat or a user, in bytes.
pub const CHAT_ID_MAX: usize = 128;

/// The bytes besides ASCII letters and digits such an id may hold:
/// Slack's `T0ACME/U0ALICE`, Telegram's `-1001234`.
pub const CHAT_ID_PUNCTUATION: &str = "._:/-";

/// The words of an own secret's purpose (`own`): its model's key, its
/// fallback model's key, a connection's field, a webhook's secret, and the
/// one before it.
pub const MODEL: &str = "model";
pub const FALLBACK: &str = "fallback";
pub const CONNECTION: &str = "connection";
pub const WEBHOOK: &str = "webhook";
pub const PREVIOUS: &str = "previous";

/// The keys of a tool call's `_meta` a harness says who the call is for
/// by: the conversation it's in (its thread's name, `telegram:42`), and
/// how far the message its turn answers is let in ([`crate::access::Standing`]).
pub const THREAD_META: &str = "link.local/thread";
pub const STANDING_META: &str = "link.local/standing";
