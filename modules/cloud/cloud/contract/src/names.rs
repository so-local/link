//! The names and tags Link gives what it makes in a cloud, how each is
//! checked, and a box's host keys read from its console.

use super::*;

/// The label naming the folder a bucket backs up, as `lnk storage`
/// names it where a name can't hold a `:`.
pub const BUCKET_FOLDER: &str = "lnk-folder";

/// The label holding the name of whose folder a bucket backs up, to show.
pub const BUCKET_FOLDER_NAME: &str = "lnk-folder-name";

/// The label naming the Link account a bucket is for, `acct-<id>`.
pub const BUCKET_OWNER: &str = "lnk-owner";

/// The labels of a bucket made for one folder (by `lnk storage`): the
/// folder, the name of whose it is, and the Link account it's for
/// (`acct-<id>`), which `buckets` finds it by.
pub fn bucket_labels(folder: &str, name: &str, owner: Option<&str>) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::from([
        (BUCKET_FOLDER.to_string(), folder.to_string()),
        (BUCKET_FOLDER_NAME.to_string(), name.to_string()),
    ]);
    if let Some(owner) = owner {
        labels.insert(BUCKET_OWNER.into(), owner.to_string());
    }
    labels
}

/// Whether `line`, an `authorized_keys` line (`<type> <key> <comment>`,
/// optionally after Google Cloud's `<user>:`), is one of Link's own plain
/// SSH keys, the ones a box trusting certificates no longer needs: its comment's first word `lnk` (a
/// computer's, `lnk <installation id> <name>`), `lnk-box`, or `lnk-box-...`
/// (an AWS key pair's name). A box's own script drops the same lines.
pub fn links_ssh_key(line: &str) -> bool {
    let line = line.trim();
    let line = match line.split_once(':') {
        Some((user, rest)) if !user.contains(char::is_whitespace) => rest,
        _ => line,
    };
    let comment = line.split_whitespace().nth(2).unwrap_or_default();
    comment == "lnk" || comment == "lnk-box" || comment.starts_with("lnk-box-")
}

/// A label's key or value as every adapter takes it: lowercase letters,
/// digits and dashes, at most 63 (Google Cloud's limits, the tightest).
pub fn check_label(text: &str) -> anyhow::Result<()> {
    let plain = !text.is_empty()
        && text.len() <= 63
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !plain {
        bail!("{:?} can't be a label", link_format::clean(text));
    }
    Ok(())
}

/// A bucket's name as every adapter takes it: 3 to 63 lowercase letters,
/// digits, dashes and dots.
pub fn check_bucket(name: &str) -> anyhow::Result<()> {
    let plain = (3..=63).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
    if !plain {
        bail!("{:?} can't name a bucket", link_format::clean(name));
    }
    Ok(())
}

/// The tags a box's machine gets: its name, its installation ID and whose it
/// is (`acct-<id>`).
pub fn box_tags(name: &str, id: &str, owner: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("lnk-box".to_string(), name.to_string()),
        ("lnk-machine".into(), id.to_string()),
        ("lnk-owner".into(), owner.to_string()),
    ])
}

/// An image's name as every adapter takes it, after `lnk-`: lowercase
/// letters, digits and dashes, starting with a letter and not ending
/// with a dash, at most 40.
pub fn check_image(name: &str) -> anyhow::Result<()> {
    let plain = (1..=40).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !plain {
        bail!(
            "{:?} can't name an image: lowercase letters, digits and dashes, starting with a \
             letter, at most 40",
            link_format::clean(name)
        );
    }
    Ok(())
}

/// The tag holding an image's hash ([`FoundImage::hash`]).
pub const IMAGE_HASH: &str = "lnk-hash";

/// An image's hash as every adapter takes it, as a tag's value: lowercase
/// letters, digits and dashes, at most 63, so a hash in hex, cut to fit.
pub fn check_image_hash(hash: &str) -> anyhow::Result<()> {
    check_label(hash).map_err(|_| {
        anyhow::anyhow!(
            "{:?} can't be an image's hash: lowercase letters, digits and dashes, at most 63 \
             (a hash in hex, cut to fit, as shasum -a 256 ... | cut -c1-32 does)",
            link_format::clean(hash)
        )
    })
}

/// The name an image has in the cloud: `lnk-<name>`.
pub fn image_name(name: &str) -> String {
    format!("lnk-{name}")
}

/// The tags an image (and on AWS its snapshots) gets: its name, and
/// whose it is (its owner, `acct-<id>`), which `image-list` finds it by.
pub fn image_tags(name: &str, owner: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("lnk-image".to_string(), name.to_string()),
        ("lnk-owner".into(), owner.to_string()),
    ])
}

/// The name a box's machine has in the cloud: `lnk-<id>`, or, from an
/// older `lnk box` (no id), `lnk-<name>`.
pub fn machine_name(spec: &Spec) -> String {
    match spec.id().is_empty() {
        true => format!("lnk-{}", spec.name),
        false => format!("lnk-{}", spec.id()),
    }
}

/// The SSH host keys in a box's console output: the block cloud-init
/// prints at the first boot, between `-----BEGIN SSH HOST KEY KEYS-----`
/// and its END line, as `<type> <key>` lines (see [`host_key`]). The first
/// block counts: later ones come after the box ran what it was given.
/// Output still in base64 (as AWS's API gives it) is decoded first.
pub fn host_keys(console: &str) -> Vec<String> {
    const BEGIN: &str = "-----BEGIN SSH HOST KEY KEYS-----";
    const END: &str = "-----END SSH HOST KEY KEYS-----";
    let decoded;
    let console = if console.contains(BEGIN) {
        console
    } else {
        use base64::Engine;
        let compact: String = console.split_whitespace().collect();
        match base64::engine::general_purpose::STANDARD.decode(compact) {
            Ok(bytes) => {
                decoded = String::from_utf8_lossy(&bytes).into_owned();
                &decoded
            }
            Err(_) => return Vec::new(),
        }
    };
    let Some((_, rest)) = console.split_once(BEGIN) else {
        return Vec::new();
    };
    let Some((block, _)) = rest.split_once(END) else {
        return Vec::new();
    };
    block.lines().filter_map(host_key).collect()
}

/// A console or `known_hosts` line's host key, as `<type> <key>`: the
/// first word that is a key type, and the base64 after it. Anything
/// before (a serial console's timestamp) or after (a comment) is dropped.
pub fn host_key(line: &str) -> Option<String> {
    const TYPES: &[&str] = &[
        "ssh-ed25519",
        "ecdsa-sha2-nistp256",
        "ecdsa-sha2-nistp384",
        "ecdsa-sha2-nistp521",
        "ssh-rsa",
    ];
    let mut words = line.split_whitespace();
    let kind = words.by_ref().find(|w| TYPES.contains(w))?;
    let key = words.next()?;
    let base64 = key.len() >= 16
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=');
    base64.then(|| format!("{kind} {key}"))
}

/// An error unless `kind` can name a cloud's adapter (`lnk-<kind>`):
/// lowercase letters and dashes, starting with a letter, at most 63.
pub fn check_kind(kind: &str) -> anyhow::Result<()> {
    let plain = kind.len() <= 63
        && kind.starts_with(|c: char| c.is_ascii_lowercase())
        && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
    if !plain {
        bail!(
            "{:?} isn't a cloud: lowercase letters and dashes, as its adapter lnk-<cloud> is named",
            link_format::clean(kind)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c";

    #[test]
    fn host_keys_come_from_the_first_boots_block() {
        let console = format!(
            "[   10.1] cloud-init[800]: ssh-ed25519 {KEY}FAKE before the block\n\
             -----BEGIN SSH HOST KEY KEYS-----\n\
             ecdsa-sha2-nistp256 {KEY}= root@box\n\
             [   25.2] cloud-init[900]: ssh-ed25519 {KEY} root@box\n\
             ssh-ed25519 not;base64\n\
             -----END SSH HOST KEY KEYS-----\n\
             -----BEGIN SSH HOST KEY KEYS-----\n\
             ssh-rsa {KEY}LATER\n\
             -----END SSH HOST KEY KEYS-----\n"
        );
        let expected = [
            format!("ecdsa-sha2-nistp256 {KEY}="),
            format!("ssh-ed25519 {KEY}"),
        ];
        assert_eq!(host_keys(&console), expected);
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&console);
        assert_eq!(host_keys(&encoded), expected);
        assert!(host_keys("booting...\n").is_empty());
        assert!(host_keys("-----BEGIN SSH HOST KEY KEYS-----\nssh-ed25519 x").is_empty());
        assert_eq!(host_key("ssh-dss AAAAB3NzaC1kc3MAAACBAP"), None);
    }

    /// Link's plain keys are known by their comments, with or without
    /// Google Cloud's user; anyone else's are kept.
    #[test]
    fn links_own_keys_are_known_by_their_comment() {
        for link in [
            format!("ssh-ed25519 {KEY} lnk k3v9q2x7m1ab dana's mac"),
            format!("ubuntu:ssh-ed25519 {KEY} lnk-box"),
            format!("ssh-ed25519 {KEY} lnk-box-3f2a9c"),
        ] {
            assert!(links_ssh_key(&link), "{link}");
        }
        for other in [
            format!("ssh-ed25519 {KEY} dana@laptop"),
            format!("ubuntu:ssh-ed25519 {KEY}"),
            format!("ssh-ed25519 {KEY} lnkx"),
            format!("ssh-ed25519 {KEY} my-lnk-box"),
            String::new(),
        ] {
            assert!(!links_ssh_key(&other), "{other}");
        }
    }

    #[test]
    fn an_image_is_named_like_a_box() {
        for good in ["web-ready", "a", "deps-2026-10-01"] {
            assert!(check_image(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "Web",
            "1web",
            "web-",
            "web.ready",
            "--zone=x",
            &"a".repeat(41),
        ] {
            assert!(check_image(bad).is_err(), "{bad}");
        }
        assert_eq!(image_name("web-ready"), "lnk-web-ready");
        // An older adapter's reply, with no images, still reads.
        let old: Reply = serde_json::from_str(r#"{"id": "i-1"}"#).unwrap();
        assert!(old.images().is_empty());
        let request: Request =
            serde_json::from_str(r#"{"account": {"name": "a", "kind": "aws", "place": "x"}}"#)
                .unwrap();
        assert_eq!(request.image, None);
        // An older adapter's image, with no hash, still reads.
        let old: FoundImage =
            serde_json::from_str(r#"{"id": "i", "name": "n", "state": "ready"}"#).unwrap();
        assert_eq!(old.hash(), "");
        assert!(check_image_hash("3f2a9c").is_ok());
        for bad in ["", "3F2A", "a b", "--x=1", &"a".repeat(64)] {
            assert!(check_image_hash(bad).is_err(), "{bad}");
        }
    }
}
