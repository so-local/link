//! [`Sealed`]: a secret encrypted to one recipient, the only form one is
//! saved or sent in as it is.
//!
//! HPKE (RFC 9180) in its base mode: X25519, HKDF-SHA256 and
//! ChaCha20-Poly1305. What a sealed value is ([`About`]: whose, what
//! kind, which version) is its additional data, so it opens only as what
//! it was sealed as. A fresh session [`Recipient`] can be dropped after
//! a transfer; recorded ciphertext then needs the discarded private key
//! to open. [`Recipient::keep`] also supports durable receivers: keeping
//! that key retains the ability to open earlier ciphertext.

use std::fmt;
use std::io::{BufRead, Write};
use std::marker::PhantomData;

use base64::Engine;
use hpke::{Deserializable, Kem as _, Serializable};
use serde::{Deserialize, Serialize};

use crate::{OpaqueSecret, Secret};

type Kem = hpke::kem::X25519HkdfSha256;
type Kdf = hpke::kdf::HkdfSha256;
type Aead = hpke::aead::ChaCha20Poly1305;

/// What every sealed value's context starts with, so it opens as
/// nothing else HPKE seals.
const INFO: &[u8] = b"link-secret sealed";

/// What a sealed value is, sealed with it: whose it is (an account's or
/// an agent's id), what kind of secret (`model-key`, `connection-token`)
/// and the version of its format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct About {
    pub owner: String,
    pub kind: String,
    pub version: u32,
}

impl About {
    /// Each field after its length (8 bytes, big-endian), so no two
    /// `About`s give the same bytes, whatever their fields hold; and the
    /// kind of value sealed (`of`, a [`Sealable::KIND`]), so it opens as
    /// that kind alone.
    fn additional_data(&self, of: &str) -> Vec<u8> {
        let mut out = b"link-secret about".to_vec();
        out.extend(self.version.to_be_bytes());
        for field in [of.as_bytes(), self.owner.as_bytes(), self.kind.as_bytes()] {
            out.extend((field.len() as u64).to_be_bytes());
            out.extend(field);
        }
        out
    }
}

impl About {
    /// What a connect's secrets (a connection's tokens, sealed by the account
    /// service as you ask) are: the receiver's (a machine's id at the
    /// account service, or an account's owner where agents are hosted)
    /// agent `<receiver>/<agent>`, the action's kind, and
    /// [`CONNECT_VERSION`], so a copy of one receiver's settings elsewhere
    /// opens none sealed for another.
    pub fn connect(receiver: &str, agent: &str, kind: &str) -> About {
        About {
            owner: format!("{receiver}/{agent}"),
            kind: kind.to_string(),
            version: CONNECT_VERSION,
        }
    }
}

/// The version of what a connect seals: a JSON object of its fields.
pub const CONNECT_VERSION: u32 = 1;

/// What only this crate makes: a [`Sealable`]'s methods take one, so no
/// code outside reaches a value's bytes through them, or implements it.
mod only {
    pub struct Here(pub(super) ());
}

use only::Here;

/// What can be sealed: a [`Secret`] (text or bytes) or an
/// [`OpaqueSecret`]. Its methods are this crate's alone:
///
/// ```compile_fail,E0603
/// // The token they take is made nowhere else.
/// let _ = link_secretive::sealed::only::Here(());
/// ```
///
/// ```compile_fail,E0061
/// use link_secretive::{Sealable, Secret};
/// let key = Secret::new("a key".to_string());
/// let _bytes = key.with_bytes(|bytes| bytes.to_vec());
/// ```
pub trait Sealable: Sized {
    /// What kind of value it is, sealed with it: one opens as no other
    /// (an [`OpaqueSecret`] never as a `Secret` of its bytes).
    #[doc(hidden)]
    const KIND: &'static str;
    #[doc(hidden)]
    fn with_bytes<R>(&self, here: Here, f: impl FnOnce(&[u8]) -> R) -> R;
    #[doc(hidden)]
    fn from_opened(here: Here, bytes: Secret<Vec<u8>>) -> Result<Self, Refused>;
}

impl Sealable for Secret {
    const KIND: &'static str = "text";

    fn with_bytes<R>(&self, _: Here, f: impl FnOnce(&[u8]) -> R) -> R {
        f(self.0.as_bytes())
    }

    fn from_opened(_: Here, bytes: Secret<Vec<u8>>) -> Result<Self, Refused> {
        let mut bytes = bytes;
        // Taken over without a copy; given back to be zeroed if it isn't text.
        String::from_utf8(std::mem::take(&mut bytes.0))
            .map(Secret::new)
            .map_err(|e| {
                bytes.0 = e.into_bytes();
                Refused
            })
    }
}

impl Sealable for Secret<Vec<u8>> {
    const KIND: &'static str = "bytes";

    fn with_bytes<R>(&self, _: Here, f: impl FnOnce(&[u8]) -> R) -> R {
        f(&self.0)
    }

    fn from_opened(_: Here, bytes: Secret<Vec<u8>>) -> Result<Self, Refused> {
        Ok(bytes)
    }
}

impl Sealable for OpaqueSecret {
    const KIND: &'static str = "opaque";

    fn with_bytes<R>(&self, _: Here, f: impl FnOnce(&[u8]) -> R) -> R {
        f(&self.0.0)
    }

    fn from_opened(_: Here, bytes: Secret<Vec<u8>>) -> Result<Self, Refused> {
        Ok(OpaqueSecret::from_bytes(bytes))
    }
}

/// Why a value didn't seal or open: never what it held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused;

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a sealed secret didn't seal or open: it's not this recipient's, or not what it says it is")
    }
}

impl std::error::Error for Refused {}

/// A `T` encrypted to one recipient's [`SealTo`]: it serializes, and may
/// be saved and sent anywhere; it opens only at that recipient's
/// [`Recipient`], as what its [`About`] says.
#[derive(Serialize, Deserialize)]
pub struct Sealed<T> {
    about: About,
    /// The sender's encapsulated key, base64.
    enc: String,
    /// The ciphertext and its tag, base64.
    data: String,
    #[serde(skip)]
    of: PhantomData<fn() -> T>,
}

impl<T> Sealed<T> {
    /// What it says it is: checked when it's opened, since it's sealed with it.
    pub fn about(&self) -> &About {
        &self.about
    }
}

impl<T: Sealable> Sealed<T> {
    /// `value`, sealed to `to` as `about`.
    pub fn seal(value: &T, to: &SealTo, about: About) -> Result<Self, Refused> {
        let (enc, data) = value.with_bytes(Here(()), |plain| {
            hpke::single_shot_seal::<Aead, Kdf, Kem>(
                &hpke::OpModeS::Base,
                &to.0,
                INFO,
                plain,
                &about.additional_data(T::KIND),
            )
            .map_err(|_| Refused)
        })?;
        Ok(Self {
            about,
            enc: base64(&enc.to_bytes()),
            data: base64(&data),
            of: PhantomData,
        })
    }
}

impl Sealed<Secret<Vec<u8>>> {
    /// `value` written as JSON, its secrets as their values (as it
    /// writes them: [`crate::bare`]), and sealed to `to` as `about`: the
    /// JSON is only ever in a [`Secret`], sized first so nothing grows
    /// and leaves a copy, and zeroed once sealed. Opened by
    /// [`Recipient::open_json`].
    pub fn seal_json(value: &impl Serialize, to: &SealTo, about: About) -> Result<Self, Refused> {
        let mut size = Counted(0);
        serde_json::to_writer(&mut size, value).map_err(|_| Refused)?;
        let mut json = Secret::new(Vec::with_capacity(size.0));
        serde_json::to_writer(json.expose_mut(), value).map_err(|_| Refused)?;
        Self::seal(&json, to, about)
    }
}

/// A writer that only counts what it's given.
struct Counted(usize);

impl Write for Counted {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<T> fmt::Debug for Sealed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sealed")
            .field("about", &self.about)
            .finish_non_exhaustive()
    }
}

/// The receiving side: a fresh pair for a session, or a durable receiver
/// restored by [`Recipient::from_kept`]. Private bytes leave only through
/// reviewed [`Recipient::keep`] persistence. The private key zeroes on drop.
pub struct Recipient {
    private: <Kem as hpke::Kem>::PrivateKey,
    public: SealTo,
}

impl Recipient {
    /// A new pair, for one transfer.
    pub fn new() -> Self {
        let (private, public) = Kem::gen_keypair();
        Self {
            private,
            public: SealTo(public),
        }
    }

    /// One kept as [`Recipient::keep`] wrote it; none when it isn't one.
    pub fn from_kept(kept: &Secret) -> Option<Self> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(kept.expose().trim())
            .ok()
            .map(Secret::new)?;
        let private = <Kem as hpke::Kem>::PrivateKey::from_bytes(bytes.expose()).ok()?;
        let public = SealTo(Kem::sk_to_pk(&private));
        Some(Self { private, public })
    }

    /// As text where it's kept, for a receiver that's sealed to while
    /// it's away (a machine's actions, which wait for it): its private
    /// half's 32 bytes, base64url. Every use is
    /// reviewed.
    pub fn keep(&self) -> Secret {
        let mut bytes = self.private.to_bytes();
        let kept = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes.as_slice());
        zeroize::Zeroize::zeroize(bytes.as_mut_slice());
        Secret::new(kept)
    }

    /// What a sender seals to: sent to it, in the clear.
    pub fn seal_to(&self) -> &SealTo {
        &self.public
    }

    /// What `sealed` holds, if it was sealed to this recipient as
    /// `about`, and as a `T`; refused otherwise, whatever it says it is.
    /// The value is back here: every call is reviewed.
    pub fn open_sealed<T: Sealable>(
        &self,
        sealed: &Sealed<T>,
        about: &About,
    ) -> Result<T, Refused> {
        self.open_sealed_within(sealed, about, about.version)
    }

    /// What `sealed` holds, as [`Recipient::open_sealed`] opens it, but
    /// sealed at any version of its format from `oldest` to `about`'s,
    /// the current one: a format's owner declares both, so a value sealed
    /// before its format changed still opens, and its owner seals it
    /// again at the current one ([`Sealed::about`] says which it was).
    /// Its version is sealed with it, so none passes as another.
    pub fn open_sealed_within<T: Sealable>(
        &self,
        sealed: &Sealed<T>,
        about: &About,
        oldest: u32,
    ) -> Result<T, Refused> {
        let at = &sealed.about;
        if at.owner != about.owner
            || at.kind != about.kind
            || !(oldest..=about.version).contains(&at.version)
        {
            return Err(Refused);
        }
        let about = at;
        let enc = from_base64(&sealed.enc)?;
        let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&enc).map_err(|_| Refused)?;
        let data = from_base64(&sealed.data)?;
        let opened = hpke::single_shot_open::<Aead, Kdf, Kem>(
            &hpke::OpModeR::Base,
            &self.private,
            &enc,
            INFO,
            &data,
            &about.additional_data(T::KIND),
        )
        .map_err(|_| Refused)?;
        T::from_opened(Here(()), Secret::new(opened))
    }

    /// What [`Sealed::seal_json`] sealed, read back as a `T` (one whose
    /// secret fields are [`Secret`]s), if it was sealed to this recipient
    /// as `about`; refused otherwise, or when it isn't a `T`. Every call
    /// is reviewed, as `open_sealed`'s are.
    pub fn open_json<T: serde::de::DeserializeOwned>(
        &self,
        sealed: &Sealed<Secret<Vec<u8>>>,
        about: &About,
    ) -> Result<T, Refused> {
        let json = self.open_sealed(sealed, about)?;
        serde_json::from_slice(json.expose()).map_err(|_| Refused)
    }

    /// Says what a sender seals to, on one line of `out` that
    /// [`SealTo::read_announced`] finds among whatever else the other
    /// end of a session sees first (a login's banner): the receiving side
    /// of a transfer over a session both ways, before anything is sent.
    pub fn announce(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(out, "{ANNOUNCED}{}", self.public.text())?;
        out.flush()
    }
}

/// What a line [`Recipient::announce`] writes starts with.
pub const ANNOUNCED: &str = "lnk-seal-to ";

/// The most a sender reads looking for an announced [`SealTo`].
const ANNOUNCED_MOST: usize = 64 * 1024;

impl Default for Recipient {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Recipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Recipient")
            .field("seal_to", &self.public)
            .finish_non_exhaustive()
    }
}

/// A recipient's public half: what a sender seals to. Public, so it's
/// sent and printed as it is (base64).
#[derive(Clone)]
pub struct SealTo(<Kem as hpke::Kem>::PublicKey);

impl SealTo {
    fn text(&self) -> String {
        base64(&self.0.to_bytes())
    }
}

impl SealTo {
    /// What the other end of a session announced ([`Recipient::announce`])
    /// on `input`: each line before it handed to `before` as it was (a
    /// login's banner, shown or dropped by the caller). An error when
    /// `input` ends first, after 64 KiB without one, or when the line
    /// isn't one.
    pub fn read_announced(
        input: &mut impl BufRead,
        mut before: impl FnMut(&str),
    ) -> std::io::Result<SealTo> {
        let invalid =
            |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_string());
        let mut read = 0;
        loop {
            let mut line = String::new();
            let n = std::io::Read::take(&mut *input, (ANNOUNCED_MOST - read) as u64 + 1)
                .read_line(&mut line)?;
            read += n;
            if n == 0 || read > ANNOUNCED_MOST {
                return Err(invalid("the other end said nothing to seal to"));
            }
            let line = line.strip_suffix('\n').unwrap_or(&line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            match line.strip_prefix(ANNOUNCED) {
                Some(text) => {
                    let bytes = from_base64(text).map_err(|_| invalid("not a key to seal to"))?;
                    return <Kem as hpke::Kem>::PublicKey::from_bytes(&bytes)
                        .map(SealTo)
                        .map_err(|_| invalid("not an X25519 public key"));
                }
                None => before(line),
            }
        }
    }
}

impl PartialEq for SealTo {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bytes() == other.0.to_bytes()
    }
}

impl Eq for SealTo {}

/// As its text (base64), as it's sent: it's public.
impl fmt::Display for SealTo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

/// From its text, as [`fmt::Display`] writes it (a flag of a command).
impl std::str::FromStr for SealTo {
    type Err = Refused;

    fn from_str(text: &str) -> Result<Self, Refused> {
        let bytes = from_base64(text)?;
        <Kem as hpke::Kem>::PublicKey::from_bytes(&bytes)
            .map(SealTo)
            .map_err(|_| Refused)
    }
}

impl fmt::Debug for SealTo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

impl Serialize for SealTo {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.text().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SealTo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = from_base64(&text).map_err(serde::de::Error::custom)?;
        <Kem as hpke::Kem>::PublicKey::from_bytes(&bytes)
            .map(SealTo)
            .map_err(|_| serde::de::Error::custom("not an X25519 public key"))
    }
}

fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn from_base64(text: &str) -> Result<Vec<u8>, Refused> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| Refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn about() -> About {
        About {
            owner: "acct-1".into(),
            kind: "model-key".into(),
            version: 1,
        }
    }

    static_assertions::assert_not_impl_any!(Recipient: Clone, Serialize);

    #[test]
    fn no_two_abouts_are_the_same_bytes() {
        // Whatever a field holds, a NUL among it, it can't pass as another's.
        let one = About {
            owner: "a\0b".into(),
            kind: "c".into(),
            version: 1,
        };
        let other = About {
            owner: "a".into(),
            kind: "b\0c".into(),
            version: 1,
        };
        assert_ne!(one.additional_data("text"), other.additional_data("text"));
        let recipient = Recipient::new();
        let secret = Secret::new("s".to_string());
        let sealed = Sealed::seal(&secret, recipient.seal_to(), one).unwrap();
        assert_eq!(recipient.open_sealed(&sealed, &other), Err(Refused));
    }

    #[test]
    fn it_opens_only_at_its_recipient_as_what_it_was_sealed_as() {
        let recipient = Recipient::new();
        let secret = Secret::new("sk-ant-hunter2".to_string());
        let sealed = Sealed::seal(&secret, recipient.seal_to(), about()).unwrap();
        // What crosses holds nothing readable.
        let wire = serde_json::to_string(&sealed).unwrap();
        assert!(!wire.contains("hunter2"), "{wire}");
        assert!(!format!("{sealed:?}").contains("hunter2"));
        let sealed: Sealed<Secret> = serde_json::from_str(&wire).unwrap();
        assert_eq!(recipient.open_sealed(&sealed, &about()).unwrap(), secret);
        // Not at another recipient.
        assert_eq!(
            Recipient::new().open_sealed(&sealed, &about()),
            Err(Refused)
        );
        // Not as anything else, even if it says so.
        let other = About {
            kind: "connection-token".into(),
            ..about()
        };
        assert_eq!(recipient.open_sealed(&sealed, &other), Err(Refused));
        let relabelled = wire.replace("model-key", "connection-token");
        let relabelled: Sealed<Secret> = serde_json::from_str(&relabelled).unwrap();
        assert_eq!(recipient.open_sealed(&relabelled, &other), Err(Refused));
        // Nor tampered with.
        let mut bent = serde_json::from_str::<Sealed<Secret>>(&wire).unwrap();
        let mut data = from_base64(&bent.data).unwrap();
        data[0] ^= 1;
        bent.data = base64(&data);
        assert_eq!(recipient.open_sealed(&bent, &about()), Err(Refused));
    }

    /// A value sealed at an older version of its format opens while its
    /// owner still declares that version, and never as a newer one.
    #[test]
    fn a_value_sealed_at_an_older_version_opens_within_its_oldest() {
        let recipient = Recipient::new();
        let secret = Secret::new("sk-ant-hunter2".to_string());
        let current = About {
            version: 2,
            ..about()
        };
        let sealed = Sealed::seal(&secret, recipient.seal_to(), about()).unwrap();
        // Exactly as `about` says: an older one isn't it.
        assert_eq!(recipient.open_sealed(&sealed, &current), Err(Refused));
        assert_eq!(
            recipient.open_sealed_within(&sealed, &current, 1).unwrap(),
            secret
        );
        assert_eq!(sealed.about().version, 1);
        // Past its oldest, refused.
        assert_eq!(
            recipient.open_sealed_within(&sealed, &current, 2),
            Err(Refused)
        );
        // One that says it's newer than it was sealed doesn't open.
        let wire = serde_json::to_string(&sealed)
            .unwrap()
            .replace("\"version\":1", "\"version\":2");
        let relabelled: Sealed<Secret> = serde_json::from_str(&wire).unwrap();
        assert_eq!(
            recipient.open_sealed_within(&relabelled, &current, 1),
            Err(Refused)
        );
        // Nor one sealed at a version newer than the current.
        let newer = Sealed::seal(
            &secret,
            recipient.seal_to(),
            About {
                version: 3,
                ..about()
            },
        )
        .unwrap();
        assert_eq!(
            recipient.open_sealed_within(&newer, &current, 1),
            Err(Refused)
        );
    }

    #[test]
    fn each_seal_is_new() {
        let recipient = Recipient::new();
        let secret = Secret::new("same".to_string());
        let a = Sealed::seal(&secret, recipient.seal_to(), about()).unwrap();
        let b = Sealed::seal(&secret, recipient.seal_to(), about()).unwrap();
        assert_ne!(a.data, b.data);
        assert_ne!(a.enc, b.enc);
    }

    #[test]
    fn an_opaque_secret_and_bytes_seal_too() {
        let recipient = Recipient::new();
        let opaque = OpaqueSecret::random(32).unwrap();
        let sealed = Sealed::seal(&opaque, recipient.seal_to(), about()).unwrap();
        let opened = recipient.open_sealed(&sealed, &about()).unwrap();
        let tag = opaque.sign(b"x");
        assert!(opened.verify(b"x", &tag));
        let bytes = Secret::new(vec![0u8, 1, 255]);
        let sealed = Sealed::seal(&bytes, recipient.seal_to(), about()).unwrap();
        assert_eq!(recipient.open_sealed(&sealed, &about()).unwrap(), bytes);
    }

    #[test]
    fn each_kind_is_its_own() {
        let kinds = [
            <Secret as Sealable>::KIND,
            <Secret<Vec<u8>> as Sealable>::KIND,
            <OpaqueSecret as Sealable>::KIND,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for b in &kinds[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn it_opens_as_the_kind_it_was_sealed_as_alone() {
        // An opaque secret, passed off as bytes or text on the way: its
        // bytes never come back as a Secret.
        let recipient = Recipient::new();
        let opaque = OpaqueSecret::random(32).unwrap();
        let wire =
            serde_json::to_string(&Sealed::seal(&opaque, recipient.seal_to(), about()).unwrap())
                .unwrap();
        let as_bytes: Sealed<Secret<Vec<u8>>> = serde_json::from_str(&wire).unwrap();
        assert_eq!(recipient.open_sealed(&as_bytes, &about()), Err(Refused));
        let as_text: Sealed<Secret> = serde_json::from_str(&wire).unwrap();
        assert_eq!(recipient.open_sealed(&as_text, &about()), Err(Refused));
        // Nor text as bytes.
        let text = Secret::new("sk-ant-hunter2".to_string());
        let wire =
            serde_json::to_string(&Sealed::seal(&text, recipient.seal_to(), about()).unwrap())
                .unwrap();
        let as_bytes: Sealed<Secret<Vec<u8>>> = serde_json::from_str(&wire).unwrap();
        assert_eq!(recipient.open_sealed(&as_bytes, &about()), Err(Refused));
    }

    #[test]
    fn a_recipient_announces_itself_past_a_banner() {
        let recipient = Recipient::new();
        let mut said = b"Welcome to the box\r\nlast login: never\n".to_vec();
        recipient.announce(&mut said).unwrap();
        said.extend(b"after\n");
        let mut input = std::io::Cursor::new(said);
        let mut banner = Vec::new();
        let to = SealTo::read_announced(&mut input, |line| banner.push(line.to_string())).unwrap();
        assert_eq!(&to, recipient.seal_to());
        assert_eq!(banner, ["Welcome to the box", "last login: never"]);
        // What follows is left to read.
        let mut rest = String::new();
        input.read_line(&mut rest).unwrap();
        assert_eq!(rest, "after\n");
        // Nothing announced, or not a key: an error, never a guess.
        for wrong in [&b"just a banner\n"[..], b"lnk-seal-to AAAA\n", b""] {
            let got = SealTo::read_announced(&mut std::io::Cursor::new(wrong), |_| {});
            assert!(got.is_err(), "{wrong:?}");
        }
        let endless = vec![b'x'; 70 * 1024];
        assert!(SealTo::read_announced(&mut std::io::Cursor::new(endless), |_| {}).is_err());
    }

    #[test]
    fn json_holding_secrets_is_sealed_whole() {
        #[derive(Serialize, Deserialize)]
        struct Given {
            #[serde(with = "crate::bare")]
            token: Secret,
            name: String,
        }
        let recipient = Recipient::new();
        let given = Given {
            token: Secret::new("sk-hunter2".to_string()),
            name: "app".into(),
        };
        let sealed = Sealed::seal_json(&given, recipient.seal_to(), about()).unwrap();
        let wire = serde_json::to_string(&sealed).unwrap();
        // Base64 holds no `-` or `"`, so neither can turn up by chance.
        assert!(
            !wire.contains("sk-hunter2") && !wire.contains("\"app\""),
            "{wire}"
        );
        let wire: Sealed<Secret<Vec<u8>>> = serde_json::from_str(&wire).unwrap();
        let opened: Given = recipient.open_json(&wire, &about()).unwrap();
        assert_eq!(opened.token.expose(), "sk-hunter2");
        assert_eq!(opened.name, "app");
        assert_eq!(
            Recipient::new().open_json::<Given>(&wire, &about()).err(),
            Some(Refused)
        );
    }

    #[test]
    fn a_kept_recipient_opens_what_was_sealed_to_it() {
        let recipient = Recipient::new();
        let secret = Secret::new("sk-ant-hunter2".to_string());
        let sealed = Sealed::seal(&secret, recipient.seal_to(), about()).unwrap();
        let kept = recipient.keep();
        assert_eq!(kept.expose().len(), 43);
        let read = Recipient::from_kept(&kept).unwrap();
        assert_eq!(read.seal_to(), recipient.seal_to());
        assert_eq!(read.open_sealed(&sealed, &about()).unwrap(), secret);
        assert!(!format!("{read:?}").contains(kept.expose().as_str()));
        for wrong in ["", "not a key", "AAAA"] {
            assert!(Recipient::from_kept(&Secret::new(wrong.to_string())).is_none());
        }
    }

    /// A connect's additional data, byte for byte, for each kind the
    /// account service and a host of agents seal: the kinds are named from the
    /// spec's `ConnectKind`, and what they seal with never changes unseen.
    #[test]
    fn a_connects_additional_data_is_pinned() {
        assert_eq!(
            About::connect("m-1", "main", "connect-telegram").additional_data("bytes"),
            b"link-secret about\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x05bytes\x00\x00\x00\x00\x00\x00\x00\x08m-1/main\x00\x00\x00\x00\x00\x00\x00\x10connect-telegram"
        );
        assert_eq!(
            About::connect("m-1", "main", "connect-slack").additional_data("bytes"),
            b"link-secret about\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x05bytes\x00\x00\x00\x00\x00\x00\x00\x08m-1/main\x00\x00\x00\x00\x00\x00\x00\x0dconnect-slack"
        );
        assert_eq!(
            About::connect("m-1", "main", "connect-discord").additional_data("bytes"),
            b"link-secret about\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x05bytes\x00\x00\x00\x00\x00\x00\x00\x08m-1/main\x00\x00\x00\x00\x00\x00\x00\x0fconnect-discord"
        );
    }

    #[test]
    fn a_value_sealed_elsewhere_by_the_spec_opens() {
        // Sealed by another implementation of the same construction (HPKE's
        // base mode, this crate's info and additional data), to a recipient
        // kept here, as the account service seals a connect's secrets.
        let recipient = Recipient::from_kept(&Secret::new(
            "b1_nEj4tCMskbnWSbYqvDZKCcTxGO6IhjDg36_CtmvU".into(),
        ))
        .unwrap();
        assert_eq!(
            recipient.seal_to().to_string(),
            "o3qIUHlJZe4dAoYa9z+XBWcI8mKeRBKfm/dDHP7puV8="
        );
        let wire = r#"{"about":{"owner":"7d0c4b1e-5b8f-4e2a-9c3d-2a1b0c9d8e7f/ada","kind":"connect-telegram","version":1},"enc":"Q5h/wh7plnhKxAU8EW71k3sIy3u4QajfZOhSCtKpaAA=","data":"IY6CwHtXkpmfMZi4ctBNAK8rQeEjNftjVmDsOnaXqHIYhuZ9VoJ/UaPZubb8mGI6cRjjvePpDEoE4JcEig=="}"#;
        let sealed: Sealed<Secret<Vec<u8>>> = serde_json::from_str(wire).unwrap();
        let about = About::connect(
            "7d0c4b1e-5b8f-4e2a-9c3d-2a1b0c9d8e7f",
            "ada",
            "connect-telegram",
        );
        #[derive(Deserialize)]
        struct Given {
            #[serde(with = "crate::bare")]
            token: Secret,
        }
        let opened: Given = recipient.open_json(&sealed, &about).unwrap();
        assert_eq!(opened.token.expose(), "123456:ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        // Not as the agent of another machine, nor as the agent alone.
        for owner in ["00000000-0000-4000-8000-000000000001/ada", "ada"] {
            let other = About {
                owner: owner.into(),
                ..about.clone()
            };
            assert!(recipient.open_json::<Given>(&sealed, &other).is_err());
        }
    }

    #[test]
    fn its_public_half_crosses_as_text() {
        let recipient = Recipient::new();
        let sent = serde_json::to_string(recipient.seal_to()).unwrap();
        let got: SealTo = serde_json::from_str(&sent).unwrap();
        assert_eq!(&got, recipient.seal_to());
        let text = recipient.seal_to().to_string();
        assert_eq!(text.parse::<SealTo>().unwrap(), got);
        assert!("AAAA".parse::<SealTo>().is_err());
        assert!(serde_json::from_str::<SealTo>(r#""AAAA""#).is_err());
    }
}
