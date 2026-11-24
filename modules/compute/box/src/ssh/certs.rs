//! Link's SSH certificates: this computer's certificate authority, its
//! secret kept as Link keeps secrets, signing a short-lived certificate
//! for one box before a connection there needs one. A box trusts the
//! authority from its first boot ([`trust_files`]), so this computer's
//! key, copied, reaches nothing once its certificates have expired.

use super::*;

/// How long a certificate lasts, in minutes.
const LIFETIME_MINUTES: u64 = 60;

/// A certificate is signed again once it's this old, so one always has
/// half its life left when a connection starts.
const RENEW_AFTER: Duration = Duration::from_secs(30 * 60);

/// Where a box keeps the authorities it trusts, one per line: each
/// computer's that has reached it.
pub const TRUSTED: &str = "/etc/ssh/lnk/user-cas.pub";

/// Where a box keeps the principal a certificate must name for `ubuntu`.
pub const PRINCIPALS: &str = "/etc/ssh/lnk/principals/ubuntu";

/// sshd's settings that trust them, ahead of Ubuntu's own.
pub const SSHD: &str = "/etc/ssh/sshd_config.d/10-lnk.conf";

/// What [`SSHD`] holds.
pub const SSHD_LINES: [&str; 2] = [
    "TrustedUserCAKeys /etc/ssh/lnk/user-cas.pub",
    "AuthorizedPrincipalsFile /etc/ssh/lnk/principals/%u",
];

/// The principal a box's certificates name, from its installation ID: a
/// certificate for one box reaches no other. None for an ID that isn't
/// a plain word (a box made before IDs has none until Link reads it).
pub fn principal(machine_id: &str) -> Option<String> {
    let plain = !machine_id.is_empty()
        && machine_id.len() <= 32
        && machine_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    plain.then(|| format!("lnk-box-{machine_id}"))
}

/// The authority's secret: in the Keychain where there is one, else in
/// `~/.lnk/safe/boxes/ssh-ca`, mode 600.
fn kept(dir: &std::path::Path) -> link_keychain::Kept {
    link_keychain::Kept {
        account: "box/ssh-ca".into(),
        path: dir.join("ssh-ca"),
    }
}

/// Where the authority's secret is kept, as `lnk uninstall` lists it;
/// none before it's made.
pub fn kept_at() -> Option<String> {
    let dir = link_files::layout::Hidden::Boxes.try_path().ok()?;
    dir.join("ssh-ca.pub").exists().then(|| kept(&dir).shown())
}

/// A folder of the box folder's own (700) holding the authority's secret
/// as a file while `ssh-keygen` reads it, removed when it goes.
struct Signing(PathBuf);

impl Signing {
    fn new(dir: &std::path::Path) -> anyhow::Result<Signing> {
        let path = dir.join(format!("signing-{}", link_files::random_token()?));
        link_files::create_private_dir(&path)?;
        Ok(Signing(path))
    }
}

impl Drop for Signing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn keygen() -> anyhow::Result<PathBuf> {
    find("ssh-keygen").within("boxes need OpenSSH's ssh-keygen, which isn't installed")
}

/// Runs `ssh-keygen` with `args`, quietly; its error said.
fn run_keygen(args: &[&std::ffi::OsStr], what: &str) -> anyhow::Result<()> {
    let output = link_secretive::process::command(keygen()?)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .within("running ssh-keygen")?;
    ensure!(
        output.status.success(),
        "ssh-keygen couldn't {what}: {}",
        link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    Ok(())
}

/// This computer's authority's public half (`ssh-ed25519 <key> <comment>`),
/// the authority made the first time.
pub fn authority() -> anyhow::Result<String> {
    let dir = dir()?;
    let public = dir.join("ssh-ca.pub");
    // Another `lnk box` may be making it right now.
    let _lock = crate::state::lock(&dir.join("ssh-ca.lock"))?;
    let kept = kept(&dir);
    match kept.load()? {
        Some(_) if public.exists() => {}
        Some(secret) => {
            // Its public half lost: read again from the secret.
            let signing = Signing::new(&dir)?;
            let file = signing.0.join("ca");
            write_secret(&file, &secret)?;
            let output = link_secretive::process::command(keygen()?)
                .arg("-y")
                .arg("-f")
                .arg(&file)
                .stdin(Stdio::null())
                .output()
                .within("running ssh-keygen")?;
            ensure!(
                output.status.success(),
                "ssh-keygen couldn't read Link's SSH certificate authority: {}",
                link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
            );
            link_files::write_private(&public, &output.stdout)?;
        }
        None => {
            let signing = Signing::new(&dir)?;
            let file = signing.0.join("ca");
            let comment = match link_machine::installation::installation() {
                Ok(me) => format!("lnk-ca {} {}", me.id, me.name),
                Err(_) => "lnk-ca".into(),
            };
            run_keygen(
                &[
                    "-q".as_ref(),
                    "-t".as_ref(),
                    "ed25519".as_ref(),
                    "-N".as_ref(),
                    "".as_ref(),
                    "-C".as_ref(),
                    comment.as_ref(),
                    "-f".as_ref(),
                    file.as_os_str(),
                ],
                "make Link's SSH certificate authority",
            )?;
            let secret = link_secretive::read_secret(&file)?;
            kept.keep(&secret)?;
            let made = std::fs::read(file.with_extension("pub"))?;
            link_files::write_private(&public, &made)?;
        }
    }
    Ok(std::fs::read_to_string(&public)
        .within(format!("reading {}", public.display()))?
        .trim()
        .to_string())
}

/// The secret as `ssh-keygen` reads it: a file of this user's alone,
/// ending in a newline, which keeping it trimmed.
fn write_secret(file: &std::path::Path, secret: &link_secretive::Secret) -> anyhow::Result<()> {
    let text = secret.expose_map(|secret| format!("{}\n", secret.trim_end()));
    link_files::write_private(file, text.expose().as_bytes())
        .within(format!("writing {}", file.display()))
}

/// A certificate for this computer's key, reaching box `machine` alone,
/// for [`LIFETIME_MINUTES`]: the one signed before while it's younger than
/// [`RENEW_AFTER`], else a new one. None when the box has no principal
/// ([`principal`]).
pub fn certificate(machine: &crate::state::Machine) -> anyhow::Result<Option<PathBuf>> {
    let Some(principal) = principal(&machine.machine_id) else {
        return Ok(None);
    };
    let dir = dir()?;
    let certs = dir.join("certs");
    let cert = certs.join(format!("{}-cert.pub", machine.machine_id));
    let fresh = std::fs::metadata(&cert)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age < RENEW_AFTER);
    if fresh {
        return Ok(Some(cert));
    }
    authority()?;
    let secret = kept(&dir)
        .load()?
        .within("Link's SSH certificate authority isn't kept here any more")?;
    let signing = Signing::new(&dir)?;
    let ca = signing.0.join("ca");
    write_secret(&ca, &secret)?;
    let public = signing.0.join(format!("{}.pub", machine.machine_id));
    std::fs::write(&public, format!("{}\n", public_key()?))?;
    let id = match link_machine::installation::installation() {
        Ok(me) => format!("lnk {} {}", me.id, me.name),
        Err(_) => "lnk".into(),
    };
    let validity = format!("-5m:+{LIFETIME_MINUTES}m");
    // Only what Link's connections need: a terminal and forwarding a
    // port, never your agent or X11.
    run_keygen(
        &[
            "-q".as_ref(),
            "-s".as_ref(),
            ca.as_os_str(),
            "-I".as_ref(),
            id.as_ref(),
            "-n".as_ref(),
            principal.as_ref(),
            "-V".as_ref(),
            validity.as_ref(),
            "-O".as_ref(),
            "clear".as_ref(),
            "-O".as_ref(),
            "permit-pty".as_ref(),
            "-O".as_ref(),
            "permit-port-forwarding".as_ref(),
            public.as_os_str(),
        ],
        "sign a certificate for the box",
    )?;
    let signed = signing.0.join(format!("{}-cert.pub", machine.machine_id));
    link_files::create_private_dir(&certs)?;
    std::fs::rename(&signed, &cert).within(format!("keeping {}", cert.display()))?;
    Ok(Some(cert))
}

/// The files a box's first boot writes so its sshd trusts `authority`'s
/// certificates naming `principal` for `ubuntu`, as cloud-config
/// `write_files:` items.
pub fn trust_files(authority: &str, principal: &str) -> String {
    use link_cloud_contract::first_boot::file;
    [
        file(TRUSTED, &[authority]),
        file(PRINCIPALS, &[principal]),
        file(SSHD, &SSHD_LINES),
    ]
    .concat()
}

/// The script, run as `ubuntu` over SSH, that has a box trust
/// `authority`'s certificates naming `principal` as its first boot would
/// have: the authority added to those it trusts, and sshd told.
pub fn trust_script(authority: &str, principal: &str) -> String {
    let q = crate::setup::quote;
    format!(
        "set -e\n\
         sudo -n mkdir -p /etc/ssh/lnk/principals\n\
         sudo -n touch {trusted}\n\
         sudo -n grep -qxF {authority} {trusted} || echo {authority} | sudo -n tee -a {trusted} >/dev/null\n\
         echo {principal} | sudo -n tee {principals} >/dev/null\n\
         printf '%s\\n' {first} {second} | sudo -n tee {sshd} >/dev/null\n\
         sudo -n chmod 644 {trusted} {principals} {sshd}\n\
         sudo -n systemctl reload ssh 2>/dev/null || sudo -n systemctl reload sshd\n",
        trusted = TRUSTED,
        principals = PRINCIPALS,
        sshd = SSHD,
        authority = q(authority),
        principal = q(principal),
        first = q(SSHD_LINES[0]),
        second = q(SSHD_LINES[1]),
    )
}

/// The script that drops Link's plain keys from `ubuntu`'s
/// `authorized_keys`, anyone else's kept: the lines whose comment
/// [`link_cloud_contract::links_ssh_key`] knows. Run by a box's first
/// boot as root (an image's keys already there go), and over SSH as `ubuntu`.
pub const DROP_KEYS: &str = r#"f=/home/ubuntu/.ssh/authorized_keys
if [ -f "$f" ]; then
  awk '{ c = $3 } !(c == "lnk" || c == "lnk-box" || c ~ /^lnk-box-/)' "$f" > "$f.lnk" && cat "$f.lnk" > "$f"
  rm -f "$f.lnk"
fi
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_principal_is_one_boxs() {
        assert_eq!(
            principal("k3v9q2x7m1ab").as_deref(),
            Some("lnk-box-k3v9q2x7m1ab")
        );
        for bad in ["", "K3V9", "a b", "x,y", &"a".repeat(33)] {
            assert_eq!(principal(bad), None, "{bad}");
        }
    }

    /// The box's script drops exactly the lines the contract calls
    /// Link's.
    #[test]
    fn a_boxs_script_drops_links_keys_alone() {
        let dir = link_test_kit::TempDir::new("drop-keys");
        let file = dir.join("authorized_keys");
        let lines = [
            "ssh-ed25519 AAAAa lnk k3v9q2x7m1ab danas-mac",
            "ssh-ed25519 AAAAb lnk-box",
            "ssh-ed25519 AAAAc lnk-box-3f2a9c",
            "ssh-ed25519 AAAAd dana@laptop",
            "ssh-ed25519 AAAAe my-lnk-box",
            "ssh-ed25519 AAAAf",
        ];
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        let script = DROP_KEYS.replace(
            "/home/ubuntu/.ssh/authorized_keys",
            &file.display().to_string(),
        );
        #[expect(clippy::disallowed_methods, reason = "runs the box's script with sh")]
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .status()
            .unwrap();
        assert!(status.success());
        let kept: Vec<String> = lines
            .iter()
            .filter(|l| !link_cloud_contract::links_ssh_key(l))
            .map(|l| l.to_string())
            .collect();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            kept.join("\n") + "\n"
        );
        assert_eq!(kept.len(), 3);
    }
}
