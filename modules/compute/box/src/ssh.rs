//! Reaching a box: OpenSSH, with Link's own key, the short-lived
//! certificates this computer's authority signs for it, and known hosts,
//! kept in `~/.lnk/safe/boxes`.

use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, ensure};
use link_format::{Name, Report, WithinExt, cmd, prose, sentence};

use crate::said::said;

pub mod certs;
mod copying;
mod hosts;
mod sessions;
pub use copying::{copy, open_unlinked, remote_folder};
use hosts::{adopt, at, known_hosts};
pub use hosts::{alias, forget, key, pin, public_key};
use sessions::{Shared, shared};
pub use sessions::{
    append_key, can_login, certified_login, close, command, forward, forward_socket, interactive,
    run_lnk, run_script, script_output, wait,
};

/// The user every box has (Ubuntu's cloud images).
pub const USER: &str = "ubuntu";

pub fn dir() -> anyhow::Result<PathBuf> {
    let dir = link_files::layout::Hidden::Boxes.try_path()?;
    link_files::create_private_dir(&dir)?;
    Ok(dir)
}

/// A program: `LNK_SSH` or `LNK_SSH_KEYGEN` (tests), else on the PATH
/// (absolute folders only).
fn find(program: &str) -> Option<PathBuf> {
    let var = format!("LNK_{}", program.to_uppercase().replace('-', "_"));
    if let Some(path) = std::env::var_os(var) {
        return Some(PathBuf::from(path));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

/// How SSH reaches a box: straight to its port 22, or, where that can't
/// be reached (a network that allows only HTTPS), through its cloud's own tunnel over
/// HTTPS, its adapter's (`lnk-<kind> tunnel`, AWS's Systems Manager or
/// Google Cloud's IAP) as ssh's ProxyCommand. `LNK_BOX_SSH`: `direct`
/// (or unset) or `tunnel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    Direct,
    Tunnel,
}

impl Reach {
    /// This machine's, from `LNK_BOX_SSH`.
    pub fn here() -> anyhow::Result<Reach> {
        Reach::from(std::env::var("LNK_BOX_SSH").ok().as_deref())
    }

    fn from(value: Option<&str>) -> anyhow::Result<Reach> {
        match value {
            None | Some("") | Some("direct") => Ok(Reach::Direct),
            Some("tunnel") => Ok(Reach::Tunnel),
            Some(other) => bail!(
                "LNK_BOX_SSH is {:?}: direct (port 22) or tunnel (the cloud's own, over HTTPS)",
                link_format::clean(other)
            ),
        }
    }
}

/// ssh's ProxyCommand reaching `machine` through its cloud's tunnel: its
/// adapter's `tunnel`, every word in single quotes for the shell ssh runs
/// it with, and `%` doubled, which ssh reads as its tokens.
fn proxy_command(
    adapter: &std::path::Path,
    machine: &crate::state::Machine,
) -> anyhow::Result<String> {
    let tunnel = link_cloud_contract::Tunnel::new(&machine.account, &machine.place, &machine.id)?;
    let program = adapter.display().to_string();
    ensure!(
        !program.contains(['\'', '\\']),
        "{program} has a quote or a backslash, which ssh's ProxyCommand can't take"
    );
    let words: Vec<String> = std::iter::once(program)
        .chain(tunnel.args())
        .map(|word| format!("'{word}'"))
        .collect();
    Ok(words.join(" ").replace('%', "%%"))
}

fn ssh() -> anyhow::Result<PathBuf> {
    find("ssh").ok_or_else(|| missing("ssh"))
}

/// The report for OpenSSH's `program` not being installed.
pub(crate) fn missing(program: &str) -> anyhow::Error {
    said(
        Report::error(sentence!(
            "Couldn't find OpenSSH's {}.",
            Name::new("program", program)
        ))
        .why(prose!("Boxes need it, and it isn't installed."))
        .next(cmd!("lnk box list"), "work with boxes that need no ssh"),
    )
}

/// Stops before a box is made that couldn't be set up: `ssh`, Link's
/// key and this computer's certificate authority (made with
/// `ssh-keygen`), are there.
pub fn check() -> anyhow::Result<()> {
    ssh()?;
    key()?;
    certs::authority()?;
    Ok(())
}

/// `path` as an ssh option's value: in quotes, so a space doesn't make it
/// two files (the second, ssh's own, then trusted), with `%` doubled,
/// which ssh reads as its tokens.
fn option_path(path: &std::path::Path) -> anyhow::Result<String> {
    option_path_then(path, "")
}

/// [`option_path`] with `tokens` after it, ssh's own (`/%C`), in the
/// same quotes and left as they are.
fn option_path_then(path: &std::path::Path, tokens: &str) -> anyhow::Result<String> {
    let text = path.display().to_string();
    ensure!(
        !text.contains('"'),
        "{text} has a quote, which ssh can't take in its options"
    );
    Ok(format!("\"{}{tokens}\"", text.replace('%', "%%")))
}

/// The option naming the shared connection's socket in `dir`, one per
/// box (`%C`).
fn control_path(dir: &std::path::Path) -> anyhow::Result<String> {
    Ok(format!("ControlPath={}", option_path_then(dir, "/%C")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_one_value_to_ssh() {
        let path = std::path::Path::new("/Users/Dana Q/100% me/.lnk/safe/boxes/known_hosts");
        assert_eq!(
            option_path(path).unwrap(),
            "\"/Users/Dana Q/100%% me/.lnk/safe/boxes/known_hosts\""
        );
        assert!(option_path(std::path::Path::new("/a\"b")).is_err());
        // The shared connection's socket: the same quotes, ssh's `%C` kept.
        let mux = std::path::Path::new("/Users/Dana Q/100% me/.lnk/safe/boxes/mux");
        assert_eq!(
            control_path(mux).unwrap(),
            "ControlPath=\"/Users/Dana Q/100%% me/.lnk/safe/boxes/mux/%C\""
        );
    }

    /// LNK_BOX_SSH is direct, unset, or tunnel; anything else is said.
    #[test]
    fn a_box_is_reached_directly_unless_told_to_tunnel() {
        assert_eq!(Reach::from(None).unwrap(), Reach::Direct);
        assert_eq!(Reach::from(Some("")).unwrap(), Reach::Direct);
        assert_eq!(Reach::from(Some("direct")).unwrap(), Reach::Direct);
        assert_eq!(Reach::from(Some("tunnel")).unwrap(), Reach::Tunnel);
        let err = Reach::from(Some("ssm")).unwrap_err().to_string();
        assert!(err.contains("direct (port 22) or tunnel"), "{err}");
    }

    /// The ProxyCommand is the box's adapter's tunnel, each word quoted
    /// for ssh's shell; a value that isn't a plain word is refused.
    #[test]
    fn a_tunnel_is_its_adapters_command_quoted() {
        let mut machine: crate::state::Machine = serde_json::from_value(serde_json::json!({
            "name": "aws-1", "account": "work", "kind": "aws", "place": "us-east-2",
            "id": "i-0abc123", "size": "t3.small", "state": "running", "created": 0
        }))
        .unwrap();
        let proxy = proxy_command(std::path::Path::new("/opt/my lnk/lnk-aws"), &machine).unwrap();
        assert_eq!(
            proxy,
            "'/opt/my lnk/lnk-aws' 'tunnel' '--cloud' 'work' '--zone' 'us-east-2' '--id' 'i-0abc123'"
        );
        let doubled = proxy_command(std::path::Path::new("/opt/100%/lnk-aws"), &machine).unwrap();
        assert!(doubled.starts_with("'/opt/100%%/lnk-aws'"), "{doubled}");
        assert!(proxy_command(std::path::Path::new("/o'k/lnk-aws"), &machine).is_err());
        assert!(proxy_command(std::path::Path::new("/o\\k/lnk-aws"), &machine).is_err());
        machine.place = "--project=theirs".into();
        assert!(proxy_command(std::path::Path::new("/bin/lnk-aws"), &machine).is_err());
        machine.place = "us-east-2".into();
        machine.id = "i-1; rm -rf ~".into();
        let err = proxy_command(std::path::Path::new("/bin/lnk-aws"), &machine)
            .unwrap_err()
            .to_string();
        assert!(err.contains("ssh can't be given a tunnel"), "{err}");
    }
}
