//! What a program holds in place of a credential a route adds outside
//! its sandbox: a placeholder, a tag of what it stands for, made for
//! each run and holder. Each route's credential makes its own with
//! [`Placeholders::placeholder`]: a model's key in
//! `link_model_contract::models::route`, a secret in
//! `link_secret_contract::secrets::route`.

/// What every placeholder carries before its tag.
pub const PLACEHOLDER: &str = "link-wire-";

/// What a holder's placeholders are made with: an [`OpaqueSecret`]
/// random for each run and holder, so a placeholder is a tag of what it
/// stands for under it ([`OpaqueSecret::sign`]), and says nothing of the
/// secret. On macOS the proxy is on the machine's loopback, where any
/// program, or another user, can connect: only the sandboxed program was
/// given the placeholder, and one read in a holder's home is no use for
/// another holder, or after its next start.
///
/// [`OpaqueSecret`]: link_secretive::OpaqueSecret
/// [`OpaqueSecret::sign`]: link_secretive::OpaqueSecret::sign
#[derive(Debug)]
pub struct Placeholders(link_secretive::OpaqueSecret);

impl Placeholders {
    /// New ones, for a run.
    pub fn new() -> std::io::Result<Self> {
        link_secretive::OpaqueSecret::random(32).map(Self)
    }

    /// Those a holder keeps for its run, where its program's settings and
    /// its proxy are both made from them ([`Placeholders::kept`]).
    pub fn from_kept(secret: link_secretive::OpaqueSecret) -> Self {
        Self(secret)
    }

    /// What they're made with, to keep for the run (a file of mode 600).
    pub fn kept(&self) -> &link_secretive::OpaqueSecret {
        &self.0
    }

    /// The placeholder for `of`: `prefix` (what it stands for keeps,
    /// which a client may read), [`PLACEHOLDER`] and a tag of 32
    /// hexadecimal characters, none of the secret.
    pub fn placeholder(&self, prefix: &str, of: &str) -> link_secretive::Secret {
        let tag: String = self
            .0
            .sign(format!("lnk wire placeholder\n{of}").as_bytes())[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("{prefix}{PLACEHOLDER}{tag}").into()
    }
}

/// An error unless `placeholder` has the whole shape of one
/// a holder made: `prefix` (what it stands for keeps, the
/// model key's or secret's own), [`PLACEHOLDER`] and a tag of 32
/// lowercase hexadecimal characters, and nothing else. The proxy adds a
/// secret only for a caller showing one, never an empty, guessable or
/// padded one.
pub fn check_placeholder(placeholder: &link_secretive::Secret, prefix: &str) -> anyhow::Result<()> {
    let made = placeholder
        .expose()
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix(PLACEHOLDER))
        .is_some_and(|tag| {
            tag.len() == 32 && tag.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
    anyhow::ensure!(made, "a placeholder that isn't one Link made");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placeholder_is_its_holders_for_one_run() {
        // Random for each run and holder: never a hash of the secret, so
        // one read in a home, or before a start, is no use after it.
        let (one, other) = (Placeholders::new().unwrap(), Placeholders::new().unwrap());
        assert_ne!(
            one.placeholder("sk-", "model"),
            other.placeholder("sk-", "model")
        );
        // The same for one holder's run, whatever reads them: its
        // program's settings and its proxy agree.
        let kept = one.kept().keep().unwrap();
        let again = Placeholders::from_kept(link_secretive::OpaqueSecret::from_text(kept));
        assert_eq!(
            one.placeholder("sk-", "model"),
            again.placeholder("sk-", "model")
        );
        // Each of a run its own.
        assert_ne!(
            one.placeholder("", "secret/chat"),
            one.placeholder("", "secret/mail")
        );
        let made = one.placeholder("42:", "secret/chat");
        assert!(made.expose().starts_with("42:link-wire-"));
        check_placeholder(&made, "42:").unwrap();
        assert!(check_placeholder(&made, "43:").is_err());
        let secret = crate::url_secret().unwrap();
        assert!(
            crate::is_url_secret(&secret)
                && !crate::is_url_secret("short")
                && !crate::is_url_secret(&"a/".repeat(10))
        );
    }
}
