//! Secret values are held in this package, in types chosen
//! by where they may go:
//!
//! - [`OpaqueSecret`]: one Link uses and nobody reads (an agent's proof
//!   secret, a service's signing secret). It signs and checks; there is
//!   no way to its bytes.
//! - [`Secret`]: a value another program or service must be given (a
//!   model's API key, a connection's token, a password). It's read only by
//!   asking for it by name, [`Secret::expose`], at a place a
//!   review sees; it's never serialized but by
//!   [`bare`], never copied but by [`Secret::duplicate`].
//! - [`SigningSecret`]: an Ed25519 key pair's private half (a machine's
//!   key), which signs, says its public half, and exports a [`Secret`]
//!   only through reviewed [`SigningSecret::keep`] persistence.
//! - [`Sealed`]: either, encrypted to one recipient (HPKE, RFC 9180:
//!   X25519, HKDF-SHA256, ChaCha20-Poly1305), the only form that's
//!   saved or sent as it is.
//!
//! None prints what it holds: `Debug` writes `<redacted>`, so a log line,
//! an error or a struct printed with `{:?}` carries none. None has
//! `Display`: what takes one (`format!("{}")`, `bearer_auth`, a header)
//! would send `<redacted>` as the value, so it doesn't compile; the value
//! is exposed there, by name.
//! [`Secret`] zeroes its held value on drop; byte secrets compare in
//! constant time (`subtle`). [`SigningSecret`] keeps its PKCS #8 bytes
//! in a secret, but ring's temporary signing key pairs are not zeroed.
//!
//! Beside them are shared HMAC signatures ([`hmac`]) and signed inbound
//! post verification ([`inbound`]); Ed25519 uses [`signing`].

use std::fmt;
use std::io::BufRead;

use serde::{Deserialize, Deserializer};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

pub mod hmac;
pub mod inbound;
pub mod opaque;
pub mod process;
mod sealed;
pub mod signing;
pub mod stream;

pub use link_secretive_derive::Duplicate;

/// Variable `name`, read by a debug build alone: what a test sets in place
/// of something a release build never lets a variable choose (a sudo, a
/// trust anchor, a resolver, a file root writes). `None` in a release
/// build, or where it's unset.
pub fn test_var(name: &str) -> Option<std::ffi::OsString> {
    cfg!(debug_assertions)
        .then(|| std::env::var_os(name))
        .flatten()
}
pub use opaque::OpaqueSecret;
pub use sealed::{ANNOUNCED, About, CONNECT_VERSION, Recipient, Refused, SealTo, Sealable, Sealed};
pub use signing::SigningSecret;

/// JSON that holds a secret among plain values (a connection's settings,
/// its token among them), held whole as `Secret<Json>`: read into a type
/// whose secret fields are [`Secret`]s ([`Json::read`]), or looked into
/// as a `serde_json::Value`. Zeroed when dropped, strings and keys alike.
#[derive(Clone, Default, PartialEq, serde::Serialize, Deserialize)]
#[serde(transparent)]
pub struct Json(pub serde_json::Value);

impl Json {
    /// What it holds, as `T`: one whose secret fields are [`Secret`]s.
    pub fn read<T: serde::de::DeserializeOwned>(&self) -> serde_json::Result<T> {
        T::deserialize(&self.0)
    }
}

impl std::ops::Deref for Json {
    type Target = serde_json::Value;

    fn deref(&self) -> &serde_json::Value {
        &self.0
    }
}

impl std::ops::DerefMut for Json {
    fn deref_mut(&mut self) -> &mut serde_json::Value {
        &mut self.0
    }
}

impl Zeroize for Json {
    fn zeroize(&mut self) {
        fn wipe(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::String(s) => s.zeroize(),
                serde_json::Value::Array(all) => all.iter_mut().for_each(wipe),
                serde_json::Value::Object(map) => {
                    for (mut key, mut value) in std::mem::take(map) {
                        key.zeroize();
                        wipe(&mut value);
                    }
                }
                _ => {}
            }
            *value = serde_json::Value::Null;
        }
        wipe(&mut self.0);
    }
}

impl From<serde_json::Value> for Secret<Json> {
    fn from(value: serde_json::Value) -> Self {
        Secret::new(Json(value))
    }
}

/// Equal when what they hold writes alike, compared in constant time.
impl PartialEq for Secret<Json> {
    fn eq(&self, other: &Self) -> bool {
        let text = |json: &Json| Secret::new(serde_json::to_vec(&json.0).unwrap_or_default());
        text(&self.0) == text(&other.0)
    }
}

impl Eq for Secret<Json> {}

/// Whether `a` and `b` are the same bytes, in a time that doesn't depend
/// on where they differ (`subtle`): for a tag or a hash checked against
/// one given, where neither is held in a [`Secret`].
pub fn equal(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// What a secret prints as, whatever it holds.
pub const REDACTED: &str = "<redacted>";

/// A secret: text by default (`Secret`), or bytes (`Secret<Vec<u8>>`,
/// `Secret<[u8; 32]>`). See the [crate]'s docs.
pub struct Secret<T: Zeroize = String>(T);

impl<T: Zeroize> Secret<T> {
    /// `value`, held as a secret from now on.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// The value itself, for where it's used. Never print it, and never
    /// copy it out (`expose().clone()`): hand on the `Secret`.
    pub fn expose(&self) -> &T {
        &self.0
    }

    /// A plain copy of the value, for what must own one (a table written
    /// to a file of mode 600): said so, and reviewed as
    /// `expose`'s are. Anywhere else, hand on the `Secret`.
    pub fn expose_owned(&self) -> T
    where
        T: Clone,
    {
        self.0.clone()
    }

    /// The value itself, to change in place (a field of settings that
    /// hold it). Every use is reviewed, as `expose`'s are.
    pub fn expose_mut(&mut self) -> &mut T {
        &mut self.0
    }

    /// The value itself, taken out, to go whole into what holds it as a
    /// part (a `Secret<Json>` it's a field of). Every use is
    /// reviewed, as `expose`'s are.
    pub fn into_exposed(mut self) -> T
    where
        T: Default,
    {
        std::mem::take(&mut self.0)
    }

    /// A secret made from this one's value, as a part of it (`USER` of
    /// `USER:PASSWORD`) or a header made of it: its value read, so it's an
    /// expose like any other, and what's made is a secret too.
    pub fn expose_map<U: Zeroize>(&self, f: impl FnOnce(&T) -> U) -> Secret<U> {
        Secret(f(&self.0))
    }

    /// A second copy, said so: a `Secret` has no `Clone`.
    pub fn duplicate(&self) -> Self
    where
        T: Clone,
    {
        Self(self.0.clone())
    }
}

impl Secret {
    /// From the variable `name`, when it holds one: a secret given off
    /// the command line, where every user of the machine sees it.
    /// Taken: no child Link runs is given it ([`process::command`]), and
    /// it's out of the environment once read while the process has one
    /// thread, as a service's start reads it; read again, it's the same.
    pub fn from_env(name: &str) -> Option<Self> {
        let value = process::take(name)?;
        (!value.is_empty()).then(|| value.expose_map(|v| without_line_end(v).to_string()))
    }

    /// The first line of `reader`, its line ending taken off: a secret
    /// on standard input, or in a file. None when the line is empty.
    pub fn read_line(reader: &mut impl BufRead) -> std::io::Result<Option<Self>> {
        let mut line = Self::new(String::new());
        reader.read_line(&mut line.0)?;
        let secret = line.expose_map(|v| without_line_end(v).to_string());
        Ok((!secret.is_empty()).then_some(secret))
    }

    /// The whole of what `reader` holds, its last line ending taken off:
    /// a secret in a file, or on standard input.
    pub fn read_all(reader: &mut impl std::io::Read) -> std::io::Result<Self> {
        let mut all = Self::new(String::new());
        reader.read_to_string(&mut all.0)?;
        Ok(all.expose_map(|v| without_line_end(v).to_string()))
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `value` without the one line ending a file or a terminal leaves:
/// everything else, spaces included, is the secret.
fn without_line_end(value: &str) -> &str {
    let value = value.strip_suffix('\n').unwrap_or(value);
    value.strip_suffix('\r').unwrap_or(value)
}

impl<T: Zeroize> Drop for Secret<T> {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl<T: Zeroize> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// A copy said so: what a type holding a secret has in place of `Clone`
/// (`#[derive(Duplicate)]`, its fields that hold one marked
/// `#[duplicate]`).
pub trait Duplicate {
    fn duplicate(&self) -> Self;
}

impl<T: Zeroize + Clone> Duplicate for Secret<T> {
    fn duplicate(&self) -> Self {
        Secret::duplicate(self)
    }
}

impl<X: Duplicate> Duplicate for Option<X> {
    fn duplicate(&self) -> Self {
        self.as_ref().map(X::duplicate)
    }
}

impl<X: Duplicate> Duplicate for Vec<X> {
    fn duplicate(&self) -> Self {
        self.iter().map(X::duplicate).collect()
    }
}

impl<X: Duplicate> Duplicate for Box<X> {
    fn duplicate(&self) -> Self {
        Box::new(X::duplicate(self))
    }
}

impl<X: Duplicate> Duplicate for std::cell::RefCell<X> {
    fn duplicate(&self) -> Self {
        std::cell::RefCell::new(self.borrow().duplicate())
    }
}

impl<K: Clone + Ord, X: Duplicate> Duplicate for std::collections::BTreeMap<K, X> {
    fn duplicate(&self) -> Self {
        self.iter()
            .map(|(k, v)| (k.clone(), v.duplicate()))
            .collect()
    }
}

/// What a tuple holding a secret holds beside it.
macro_rules! plainly {
    ($($t:ty),*) => {$(
        impl Duplicate for $t {
            fn duplicate(&self) -> Self {
                self.clone()
            }
        }
    )*};
}

plainly!(String, std::path::PathBuf, bool, u16, u32, u64, usize, i64);

impl<A: Duplicate, B: Duplicate> Duplicate for (A, B) {
    fn duplicate(&self) -> Self {
        (self.0.duplicate(), self.1.duplicate())
    }
}

impl<A: Duplicate, B: Duplicate, C: Duplicate> Duplicate for (A, B, C) {
    fn duplicate(&self) -> Self {
        (self.0.duplicate(), self.1.duplicate(), self.2.duplicate())
    }
}

impl<T: Zeroize + Default> Default for Secret<T> {
    fn default() -> Self {
        Self(T::default())
    }
}

/// Equal when their bytes are, compared in a time that doesn't depend on
/// where they first differ (only on their lengths).
impl<T: Zeroize + AsRef<[u8]>> PartialEq for Secret<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref().ct_eq(other.0.as_ref()).into()
    }
}

impl<T: Zeroize + AsRef<[u8]>> Eq for Secret<T> {}

/// Taken over: the value moves in, nothing is copied.
impl<T: Zeroize> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

/// Read from a file it's kept in, or a message that carries it: getting
/// one in. Writing one out is [`bare`]'s, said so where it's done.
impl<'de, T: Zeroize + Deserialize<'de>> Deserialize<'de> for Secret<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self)
    }
}

/// A [`Secret`] written as its value, said so where it's done:
/// `#[serde(with = "link_secretive::bare")]`, on a `Secret` or an `Option`,
/// `Vec` or map of them. Only where it's kept (a file of mode 600, the
/// keyring) or, until each is [`Sealed`], a crossing; every use is
/// reviewed.
pub mod bare {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use zeroize::Zeroize;

    use crate::Secret;

    /// What `bare` writes: a secret, or what holds them.
    pub trait Bare {
        fn write_bare<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
    }

    impl<T: Zeroize + Serialize> Bare for Secret<T> {
        fn write_bare<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.0.serialize(serializer)
        }
    }

    impl<X: Bare> Bare for Option<X> {
        fn write_bare<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                Some(x) => serializer.serialize_some(&Written(x)),
                None => serializer.serialize_none(),
            }
        }
    }

    impl<X: Bare> Bare for Vec<X> {
        fn write_bare<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(self.iter().map(Written))
        }
    }

    impl<K: Serialize, X: Bare> Bare for BTreeMap<K, X> {
        fn write_bare<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_map(self.iter().map(|(k, v)| (k, Written(v))))
        }
    }

    struct Written<'a, X>(&'a X);

    impl<X: Bare> Serialize for Written<'_, X> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.0.write_bare(serializer)
        }
    }

    pub fn serialize<S: Serializer, X: Bare>(value: &X, serializer: S) -> Result<S::Ok, S::Error> {
        value.write_bare(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, X: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<X, D::Error> {
        X::deserialize(deserializer)
    }
}

/// The secret in file `path`, or on standard input for `-`, its ends
/// trimmed: what a command takes for a secret it would otherwise be
/// given on its command line, where every user of the machine sees it.
pub fn read_secret(path: &std::path::Path) -> anyhow::Result<Secret> {
    use std::io::Read;
    let mut raw = String::new();
    let read = match path.to_str() {
        Some("-") => std::io::stdin().read_to_string(&mut raw).map(drop),
        _ => std::fs::File::open(path).and_then(|mut file| file.read_to_string(&mut raw).map(drop)),
    };
    let secret = Secret::new(raw).expose_map(|raw| raw.trim().to_string());
    read.map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    fn secret(value: &str) -> Secret {
        Secret::new(value.to_string())
    }

    // What it may never do, at compile time.
    static_assertions::assert_not_impl_any!(Secret: Clone, Serialize, Copy, fmt::Display);
    static_assertions::assert_not_impl_any!(Secret<Vec<u8>>: Clone, Serialize);
    static_assertions::assert_not_impl_any!(Secret: From<&'static str>, From<&'static String>);

    #[test]
    fn it_never_prints_what_it_holds() {
        let held = secret("sk-ant-api03-hunter2");
        assert_eq!(format!("{held:?}"), "<redacted>");
        assert_eq!(format!("{held:#?}"), "<redacted>");
        let bytes: Secret<Vec<u8>> = Secret::new(b"hunter2".to_vec());
        assert_eq!(format!("{bytes:?}"), "<redacted>");
        // Nor inside what holds it.
        #[derive(Debug)]
        #[expect(
            dead_code,
            reason = "its fields are read only by Debug, which is what the test shows"
        )]
        struct Login {
            user: String,
            token: Secret,
            key: Option<Secret<[u8; 4]>>,
        }
        let login = Login {
            user: "alice".into(),
            token: secret("lnk_0123"),
            key: Some(Secret::new(*b"abcd")),
        };
        let printed = format!("{login:?} {login:#?}");
        assert!(printed.contains("alice"));
        for leaked in ["lnk_0123", "abcd", "97, 98"] {
            assert!(!printed.contains(leaked), "{printed}");
        }
    }

    #[test]
    fn it_is_read_only_by_name_and_copied_only_by_saying_so() {
        let held = secret("lnk_0123");
        assert_eq!(held.expose(), "lnk_0123");
        let user = secret("alice:pw").expose_map(|v| v.split(':').next().unwrap().to_string());
        assert_eq!(user.expose(), "alice");
        let copy = held.duplicate();
        assert_eq!(copy, held);
    }

    #[test]
    fn what_holds_one_is_copied_only_by_saying_so() {
        #[derive(Duplicate, Debug, PartialEq)]
        struct Login {
            user: String,
            #[duplicate]
            token: Secret,
            #[duplicate]
            others: Vec<(String, Secret)>,
        }
        #[derive(Duplicate, Debug, PartialEq)]
        enum Held {
            One(#[duplicate] Login),
            None,
        }
        static_assertions::assert_not_impl_any!(Login: Clone);
        let login = Login {
            user: "alice".into(),
            token: secret("lnk_0123"),
            others: vec![("b".into(), secret("x"))],
        };
        assert_eq!(login.duplicate(), login);
        let held = Held::One(login);
        assert_eq!(held.duplicate(), held);
        assert_eq!(Held::None.duplicate(), Held::None);
    }

    #[test]
    fn its_memory_is_zeroed_when_dropped() {
        // Dropped in place, its buffer still allocated: the bytes it held
        // are zero.
        let mut slot = std::mem::ManuallyDrop::new(Secret::new(*b"hunter2!"));
        let at = slot.expose().as_ptr();
        // SAFETY: dropped once, and read while the slot still holds it.
        unsafe {
            std::mem::ManuallyDrop::drop(&mut slot);
            assert_eq!(std::ptr::read_volatile(at as *const [u8; 8]), [0; 8]);
        }
        let mut text = secret("hunter2");
        text.0.zeroize();
        assert!(text.is_empty());
    }

    #[test]
    fn json_holding_one_is_a_secret_too() {
        #[derive(Deserialize)]
        struct Telegram {
            bot_token: Secret,
            allowed_users: Vec<i64>,
        }
        let settings: Secret<Json> =
            serde_json::json!({"bot_token": "123:hunter2", "allowed_users": [42]}).into();
        assert_eq!(format!("{settings:?}"), "<redacted>");
        let read: Telegram = settings.expose().read().unwrap();
        assert_eq!(read.bot_token.expose(), "123:hunter2");
        assert_eq!(read.allowed_users, [42]);
        assert_eq!(settings.expose()["allowed_users"][0], 42);
        assert_eq!(settings.duplicate(), settings);
        let mut json = Json(serde_json::json!({"a": ["b", {"c": "d"}]}));
        json.zeroize();
        assert_eq!(json.0, serde_json::Value::Null);
    }

    #[test]
    fn bytes_compare_alike() {
        assert!(equal(b"tag", b"tag"));
        assert!(!equal(b"tag", b"tah"));
        assert!(!equal(b"tag", b"tags"));
    }

    #[test]
    fn it_compares_by_value() {
        assert_eq!(secret("a"), secret("a"));
        assert_ne!(secret("a"), secret("b"));
        assert_ne!(secret("a"), secret("ab"));
        assert_ne!(secret(""), secret("a"));
    }

    #[test]
    fn it_is_written_only_bare_by_name() {
        #[derive(Serialize, Deserialize)]
        struct Kept {
            #[serde(with = "crate::bare")]
            token: Secret,
            #[serde(with = "crate::bare", default)]
            other: Option<Secret>,
        }
        let kept: Kept = toml::from_str("token = \"lnk_0123\"").unwrap();
        assert_eq!(kept.token.expose(), "lnk_0123");
        assert!(kept.other.is_none());
        assert_eq!(
            serde_json::to_string(&kept).unwrap(),
            r#"{"token":"lnk_0123","other":null}"#
        );
    }

    #[test]
    fn it_is_read_as_given_but_for_its_line_ending() {
        let mut given = std::io::Cursor::new(" lnk 0123 \r\nmore\n");
        let read = Secret::read_line(&mut given).unwrap().unwrap();
        assert_eq!(read.expose(), " lnk 0123 ");
        assert!(
            Secret::read_line(&mut std::io::Cursor::new("\n"))
                .unwrap()
                .is_none()
        );
        let all = Secret::read_all(&mut std::io::Cursor::new("a\nb\n")).unwrap();
        assert_eq!(all.expose(), "a\nb");
        assert!(Secret::from_env("LNK_SECRET_TEST_NEVER_SET").is_none());
    }
}

#[cfg(test)]
link_test_kit::program!(system);
