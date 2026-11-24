//! Link's key and its known hosts: the key made once, each box's host
//! keys read, adopted under its alias, pinned and forgotten.

use super::*;

/// Link's SSH key for boxes, made the first time (ed25519, no
/// passphrase: it lives in Link's settings folder, 700). Its comment is
/// `lnk <installation id> <machine name>`, so a lost computer's key can be
/// found on a box and removed.
pub fn key() -> anyhow::Result<PathBuf> {
    let dir = dir()?;
    let key = dir.join("id_ed25519");
    // Another `lnk box create` may be making it right now.
    let _lock = crate::state::lock(&dir.join("id_ed25519.lock"))?;
    if !key.exists() {
        let keygen = find("ssh-keygen").ok_or_else(|| missing("ssh-keygen"))?;
        let comment = match link_machine::installation::installation() {
            Ok(me) => format!("lnk {} {}", me.id, me.name),
            Err(_) => "lnk-box".into(),
        };
        let status = link_secretive::process::command(keygen)
            .args(["-q", "-t", "ed25519", "-N", "", "-C", &comment, "-f"])
            .arg(&key)
            .stdin(Stdio::null())
            .status()
            .within("running ssh-keygen")?;
        ensure!(status.success(), "ssh-keygen couldn't make Link's key");
    }
    Ok(key)
}

pub fn public_key() -> anyhow::Result<String> {
    let path = key()?.with_extension("pub");
    Ok(std::fs::read_to_string(&path)
        .within(format!("reading {}", path.display()))?
        .trim()
        .to_string())
}

pub(super) fn known_hosts() -> anyhow::Result<PathBuf> {
    Ok(dir()?.join("known_hosts"))
}

/// The name box `machine`'s host keys are kept under, whatever its
/// address (ssh's `HostKeyAlias`): its cloud's kind and its id there. So
/// a box at a new address (restarted, or its address learned from the
/// cloud) is still checked against the keys it had.
pub fn alias(machine: &crate::state::Machine) -> Option<String> {
    let name = format!("lnk-{}-{}", machine.kind, machine.id);
    let plain = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b));
    (!machine.id.is_empty() && plain).then_some(name)
}

/// The box Link last saw at `ip`, if any.
pub(super) fn at(ip: &str) -> Option<crate::state::Machine> {
    crate::state::Boxes::load()
        .ok()?
        .boxes
        .into_iter()
        .find(|m| m.ip.as_deref() == Some(ip))
}

/// The keys `file` holds for `host`, each `<type> <key>`: read here, as
/// `ssh-keygen -F` reads them, so no `ssh` started pays a program for it.
fn keys_for(file: &std::path::Path, host: &str) -> Vec<String> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
        .filter_map(|l| l.split_once([' ', '\t']))
        .filter(|(hosts, _)| names(hosts, host))
        .map(|(_, key)| key.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect()
}

/// Whether a known hosts line's `hosts` names `host`: one of its
/// comma-separated names, or, hashed (`|1|<salt>|<hash>`, ssh's
/// `HashKnownHosts`), its HMAC-SHA1 under the salt.
fn names(hosts: &str, host: &str) -> bool {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    if let Some(hashed) = hosts.strip_prefix("|1|") {
        let Some((salt, hash)) = hashed.split_once('|') else {
            return false;
        };
        let (Ok(salt), Ok(hash)) = (b64.decode(salt), b64.decode(hash)) else {
            return false;
        };
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, &salt);
        return ring::hmac::verify(&key, host.as_bytes(), &hash).is_ok();
    }
    hosts.split(',').any(|name| name.eq_ignore_ascii_case(host))
}

/// A box's keys kept under its address before, kept under `alias` too:
/// what it trusted stays trusted, never asked again.
pub(super) fn adopt(ip: &str, alias: &str) -> anyhow::Result<()> {
    adopt_in(&known_hosts()?, ip, alias)
}

/// [`adopt`], in the known hosts file `file`.
fn adopt_in(file: &std::path::Path, ip: &str, alias: &str) -> anyhow::Result<()> {
    let _lock = crate::state::lock(&file.with_extension("lock"))?;
    if !file.exists() || !keys_for(file, alias).is_empty() {
        return Ok(());
    }
    let keys = keys_for(file, ip);
    if keys.is_empty() {
        return Ok(());
    }
    let mut text = std::fs::read_to_string(file).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for key in keys {
        text.push_str(&format!("{alias} {key}\n"));
    }
    link_files::write_private(file, text.as_bytes()).within(format!("writing {}", file.display()))
}

/// Pins `keys` (`<type> <key>`) as the host keys of `ip`, in place of
/// any it had: SSH there then refuses any other.
pub fn pin(ip: &str, keys: &[String]) -> anyhow::Result<()> {
    pin_in(&known_hosts()?, ip, keys)
}

/// [`pin`], in the known hosts file `file`, holding its lock: two `lnk
/// box create` at once each keep their box's keys.
fn pin_in(file: &std::path::Path, ip: &str, keys: &[String]) -> anyhow::Result<()> {
    ensure!(
        ip.parse::<std::net::IpAddr>().is_ok(),
        "{ip:?} isn't an address"
    );
    let keys: Vec<String> = keys
        .iter()
        .filter_map(|k| link_cloud_contract::host_key(k))
        .collect();
    ensure!(!keys.is_empty(), "no host keys to pin for {ip}");
    // Under the box's alias when Link knows the box there, so they hold
    // at any address it moves to.
    let host = at(ip).as_ref().and_then(alias).unwrap_or(ip.to_string());
    let _lock = crate::state::lock(&file.with_extension("lock"))?;
    forget_in(file, ip);
    let mut text = std::fs::read_to_string(file).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for key in keys {
        text.push_str(&format!("{host} {key}\n"));
    }
    link_files::write_private(file, text.as_bytes()).within(format!("writing {}", file.display()))
}

/// Forgets the host key of `ip` (a removed box's address can come back
/// as another's), and of the box Link knows there, under its alias.
pub fn forget(ip: &str) {
    let Ok(file) = known_hosts() else {
        return;
    };
    if let Ok(_lock) = crate::state::lock(&file.with_extension("lock")) {
        forget_in(&file, ip);
    }
}

/// [`forget`], in `file`, its lock already held.
fn forget_in(file: &std::path::Path, ip: &str) {
    if !file.exists() {
        return;
    }
    let alias = at(ip).as_ref().and_then(alias);
    if let Some(keygen) = find("ssh-keygen") {
        for host in std::iter::once(ip).chain(alias.as_deref()) {
            let _ = link_secretive::process::command(&keygen)
                .args(["-q", "-R", host, "-f"])
                .arg(file)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = std::fs::remove_file(file.with_extension("old"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box's keys are read from the file as `ssh-keygen -F` reads
    /// them, plain or hashed, and adopted under its alias, with no
    /// program run.
    #[test]
    fn keys_are_read_plain_or_hashed() {
        let dir = link_test_kit::TempDir::new("known");
        let file = dir.join("known_hosts");
        let ed = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
        let rsa = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC7";
        let salt = "|1|AAECAwQFBgcICQoLDA0ODxAREhM=";
        std::fs::write(
            &file,
            format!(
                "# a comment\n\
                 {salt}|yYp6vR4fZnkno874WwkS7kckR7s= {ed}\n\
                 192.0.2.7,other {rsa}\n\
                 @revoked 10.0.0.5 {rsa}\n\
                 {salt}|AAAAAAAAAAAAAAAAAAAAAAAAAAA= {rsa}\n\
                 \n"
            ),
        )
        .unwrap();
        // 10.0.0.5, hashed with that salt; never a revoked line's key.
        assert_eq!(keys_for(&file, "10.0.0.5"), [ed]);
        assert_eq!(keys_for(&file, "192.0.2.7"), [rsa]);
        assert_eq!(keys_for(&file, "OTHER"), [rsa]);
        assert!(keys_for(&file, "192.0.2.70").is_empty());
        assert!(keys_for(&dir.join("none"), "10.0.0.5").is_empty());
        // Under the alias ssh hashes it by, too.
        assert!(names(
            "|1|AAECAwQFBgcICQoLDA0ODxAREhM=|SyXrk41OCYDW59hpXx8YNaQImow=",
            "lnk-aws-i-123"
        ));

        adopt_in(&file, "10.0.0.5", "lnk-aws-i-123").unwrap();
        assert_eq!(keys_for(&file, "lnk-aws-i-123"), [ed]);
    }

    /// Boxes started at once each keep their pinned keys.
    #[test]
    fn pins_at_once_all_land() {
        let dir = link_test_kit::TempDir::new("pins");
        let file = dir.join("known_hosts");
        let key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGJBYzUtpWCyCaqTG3+K2UlSHvzBhzNjHfipRwL4XjMh";
        let threads: Vec<_> = (1..=12)
            .map(|n| {
                let file = file.clone();
                std::thread::spawn(move || {
                    pin_in(&file, &format!("192.0.2.{n}"), &[key.to_string()]).unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for n in 1..=12 {
            assert!(
                text.contains(&format!("192.0.2.{n} ssh-ed25519 ")),
                "{n}: {text}"
            );
        }
    }
}
