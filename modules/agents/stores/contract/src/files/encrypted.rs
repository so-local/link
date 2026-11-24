//! Objects kept encrypted in another store, in rclone's crypt format
//! ([`Crypt`]).

use super::*;

/// Objects kept encrypted in another store, in rclone's crypt format,
/// names and contents ([`crypt`]): what's put is encrypted before the
/// store sees it, what's got opened after; listed sizes are the
/// plaintext's. Versions are the store's.
#[cfg(feature = "files-bridge")]
pub struct Crypt<S> {
    store: S,
    cipher: crypt::Cipher,
}

#[cfg(feature = "files-bridge")]
impl<S> std::fmt::Debug for Crypt<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Crypt { .. }")
    }
}

#[cfg(feature = "files-bridge")]
impl<S: Store> Crypt<S> {
    /// `store`, its objects encrypted with the keys of `password`.
    pub fn new(store: S, password: &str) -> anyhow::Result<Self> {
        Ok(Self {
            store,
            cipher: crypt::Cipher::new(password)?,
        })
    }

    /// The store beneath.
    pub fn inner(&self) -> &S {
        &self.store
    }
}

#[cfg(feature = "files-bridge")]
impl<S: Store> Store for Crypt<S> {
    fn get_to(&self, name: &str, out: &mut dyn std::io::Write) -> anyhow::Result<Option<Version>> {
        use std::io::Seek;
        check_name(name)?;
        let mut sealed = spool()?;
        let Some(version) = self
            .store
            .get_to(&self.cipher.encrypt_name(name), &mut sealed)?
        else {
            return Ok(None);
        };
        sealed.rewind()?;
        self.cipher.decrypt_to(&mut sealed, out)?;
        Ok(Some(version))
    }
    fn put_file(
        &self,
        name: &str,
        file: &std::fs::File,
        expect: &Expect,
    ) -> anyhow::Result<Version> {
        use std::io::Seek;
        check_name(name)?;
        let mut sealed = spool()?;
        let mut plain = file.try_clone()?;
        plain.rewind()?;
        self.cipher.encrypt_to(&mut plain, &mut sealed)?;
        sealed.rewind()?;
        self.store
            .put_file(&self.cipher.encrypt_name(name), &sealed, expect)
            .map_err(|err| {
                if is_conflict(&err) {
                    Conflict { name: name.into() }.into()
                } else {
                    err
                }
            })
    }
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        check_name(name)?;
        let Some((sealed, version)) = self.store.get(&self.cipher.encrypt_name(name))? else {
            return Ok(None);
        };
        let plain = self
            .cipher
            .decrypt(&sealed)
            .with_context(|| format!("opening {name}"))?;
        Ok(Some((plain, version)))
    }

    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        check_name(name)?;
        let sealed = self.cipher.encrypt(data)?;
        self.store
            .put(&self.cipher.encrypt_name(name), &sealed, expect)
            .map_err(|err| match is_conflict(&err) {
                true => Conflict { name: name.into() }.into(),
                false => err,
            })
    }

    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        check_name(name)?;
        self.store
            .delete(&self.cipher.encrypt_name(name), expect)
            .map_err(|err| match is_conflict(&err) {
                true => Conflict { name: name.into() }.into(),
                false => err,
            })
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        // A name's parts are encrypted each alone: a prefix of whole
        // folders encrypts to the prefix of theirs.
        let (folders, rest) = match prefix.rfind('/') {
            Some(at) => (&prefix[..=at], &prefix[at + 1..]),
            None => ("", prefix),
        };
        let sealed_prefix = match folders {
            "" => String::new(),
            folders => format!(
                "{}/",
                self.cipher.encrypt_name(folders.trim_end_matches('/'))
            ),
        };
        let mut found = Vec::new();
        for object in self.store.list(&sealed_prefix)? {
            // An object that isn't one of this key's isn't Home's.
            let Ok(path) = self.cipher.decrypt_name(&object.path) else {
                continue;
            };
            let Some(size) = crypt::decrypted_size(object.size) else {
                continue;
            };
            if path.starts_with(folders) && path[folders.len()..].starts_with(rest) {
                found.push(Object {
                    path,
                    size,
                    version: object.version,
                    modified: object.modified,
                });
            }
        }
        found.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(found)
    }

    /// Its sealed name's size, opened: one object listed, never its
    /// folder.
    fn size(&self, name: &str) -> anyhow::Result<Option<u64>> {
        check_name(name)?;
        Ok(self
            .store
            .size(&self.cipher.encrypt_name(name))?
            .and_then(crypt::decrypted_size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_test_kit::TempDir;

    #[test]
    fn crypt_keeps_names_and_contents_encrypted() {
        let dir = TempDir::new("files-crypt");
        let plain = DirStore::new(&dir.join("s")).unwrap();
        let store = Crypt::new(plain.clone(), "a key").unwrap();
        store
            .put("home/memory/tax report.txt", b"refund: 42", &Expect::Absent)
            .unwrap();
        store.put("home/work/x", b"x", &Expect::Absent).unwrap();
        let raw = plain.list("").unwrap();
        assert_eq!(raw.len(), 2);
        for object in &raw {
            assert!(!object.path.contains("home") && !object.path.contains("tax"));
            let bytes = plain.get(&object.path).unwrap().unwrap().0;
            assert!(!bytes.windows(6).any(|w| w == b"refund"));
        }
        let listed = store.list("home/memory/").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, "home/memory/tax report.txt");
        assert_eq!(listed[0].size, 10);
        assert_eq!(store.list("home/").unwrap().len(), 2);
        assert_eq!(
            store.get("home/memory/tax report.txt").unwrap().unwrap().0,
            b"refund: 42"
        );
        // Another key reads nothing of it.
        let other = Crypt::new(plain, "another key").unwrap();
        assert!(other.list("").unwrap().is_empty());
    }
}
