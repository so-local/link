//! A box's SSH sessions: the connection, shared while it is used and
//! closed with the box, scripts and commands run, ports forwarded.

use super::*;
use link_format::Address;

/// `ssh` to `ip` as [`USER`], with Link's key and known hosts, and
/// `options` (ssh's own, before the host: after it, they'd be the remote
/// command). A new box's host key is accepted the first time and pinned
/// after, unless `lnk box create` pinned it first from the box's console
/// ([`pin`]); a known box's are kept under its [`alias`], wherever it is.
/// The remote command, if any, goes after this.
///
/// Nothing of this machine reaches the box through it, whatever the
/// user's `~/.ssh/config` says: no agent, no X11, no port forwarded, and
/// no command run here.
pub fn command(ip: &str, options: &[&str]) -> anyhow::Result<Command> {
    connection(ip, options, false)
}

/// [`command`], but with `forwards`, ports forwarded: `options`', and
/// the user's config's, which ssh can't clear without clearing both.
fn connection(ip: &str, options: &[&str], forwards: bool) -> anyhow::Result<Command> {
    if let Some(name) = ip.strip_prefix(crate::machine::PREFIX) {
        ensure!(
            !forwards,
            "VM guest forwarding requires an authorized VM service grant"
        );
        return crate::machine::ssh(name, options);
    }
    ensure!(
        ip.parse::<std::net::IpAddr>().is_ok(),
        "{ip:?} isn't an address"
    );
    let machine = at(ip);
    let alias = machine.as_ref().and_then(alias);
    let proxy = match Reach::here()? {
        Reach::Direct => None,
        Reach::Tunnel => {
            let machine = machine.as_ref().within(format!(
                "no box of yours is at {ip}, so there's no tunnel to it (LNK_BOX_SSH=tunnel)"
            ))?;
            // A tunnel goes to a box by its id, not its address: two boxes
            // last seen there (one stopped, its address since given to
            // the other) would make it the wrong one.
            let seen = crate::state::Boxes::load()?
                .boxes
                .iter()
                .filter(|m| m.ip.as_deref() == Some(ip))
                .count();
            ensure!(
                seen == 1,
                "{seen} of your boxes were last seen at {ip}, so which one a tunnel goes to \
                 isn't known: lnk box list --refresh learns their addresses again"
            );
            let adapter = link_cloud_contract::adapter(&machine.kind)?;
            Some(proxy_command(&adapter, machine)?)
        }
    };
    let mut command = link_secretive::process::command(ssh()?);
    command
        .arg("-i")
        .arg(key()?)
        .args(["-o", "IdentitiesOnly=yes"])
        .arg("-o")
        .arg(format!(
            "UserKnownHostsFile={}",
            option_path(&known_hosts()?)?
        ));
    // The box's certificate, signed here when it's due: a box made with
    // certificates takes nothing else.
    if let Some(cert) = machine
        .as_ref()
        .map(certs::certificate)
        .transpose()?
        .flatten()
    {
        command
            .arg("-o")
            .arg(format!("CertificateFile={}", option_path(&cert)?));
    }
    if let Some(alias) = &alias {
        adopt(ip, alias)?;
        command.arg("-o").arg(format!("HostKeyAlias={alias}"));
    }
    if let Some(proxy) = &proxy {
        command.arg("-o").arg(format!("ProxyCommand={proxy}"));
    }
    // On the command line, these come before the config file's.
    command
        .args(["-o", "ForwardAgent=no"])
        .args(["-o", "ForwardX11=no"])
        .args(["-o", "PermitLocalCommand=no"]);
    if !forwards {
        // Clears the command line's too.
        command.args(["-o", "ClearAllForwardings=yes"]);
    }
    command
        .args(["-o", "StrictHostKeyChecking=accept-new"])
        .args(["-o", "ConnectTimeout=10"])
        .args(["-o", "ServerAliveInterval=30"])
        .args(options)
        .arg(format!("{USER}@{ip}"));
    Ok(command)
}

/// Waits (up to `timeout`) until the box at `ip` takes SSH.
#[expect(clippy::disallowed_methods, reason = "polls a box until it answers")]
pub fn wait(ip: &str, timeout: Duration) -> anyhow::Result<()> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let ok = command(ip, &["-o", "BatchMode=yes"])?
            .arg("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    Err(said(
        Report::error(sentence!(
            "Couldn't reach the box at {}, which didn't take SSH.",
            Address::new(ip)
        ))
        .why(prose!("It was tried for {} seconds.", timeout.as_secs()))
        .next(
            cmd!("lnk box list --refresh"),
            "ask its cloud for its state",
        ),
    ))
}

/// How long a box's shared connection stays open after its last use, in
/// seconds.
const PERSIST: u32 = 60;

/// Whether one connection to a box is shared by the commands run there.
pub(super) enum Shared {
    /// It's open: these options use it.
    Open(Vec<String>),
    /// Opening it failed: SSH doesn't get in.
    Failed,
    /// It can't be shared here: each command connects on its own.
    Off,
}

/// One SSH connection to the box at `ip`, shared by the several `lnk box
/// run` a command makes (a move makes four or more, an agent list one per
/// box): the first opens it in the background, and it closes [`PERSIST`]
/// seconds after the last. Its socket is in `~/.lnk/safe/boxes/mux`, 700.
pub(super) fn shared(ip: &str) -> Shared {
    if ip.starts_with(crate::machine::PREFIX) {
        return Shared::Off;
    }
    let Ok(dir) = dir().map(|dir| dir.join("mux")) else {
        return Shared::Off;
    };
    // ssh's socket name (`%C`) is 40 characters, plus 17 while it's made,
    // and a socket's path is at most 103 bytes on a Mac: longer, and ssh
    // fails rather than connecting on its own.
    if dir.as_os_str().len() + 1 + 40 + 17 > 100 || link_files::create_private_dir(&dir).is_err() {
        return Shared::Off;
    }
    let Ok(path) = control_path(&dir) else {
        return Shared::Off;
    };
    // One opens it; any other waits for it and uses it.
    let Ok(_lock) = crate::state::lock(&dir.join(format!("{ip}.lock"))) else {
        return Shared::Off;
    };
    let quiet = |mut command: Command| {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let Ok(check) = command(ip, &["-o", &path, "-O", "check"]) else {
        return Shared::Off;
    };
    if !quiet(check) {
        // `-f`: back once it's in, with nothing of this one's open, so
        // whoever reads this command's output isn't kept waiting.
        let persist = format!("ControlPersist={PERSIST}");
        let Ok(open) = command(
            ip,
            &[
                "-o",
                "BatchMode=yes",
                "-o",
                "ControlMaster=yes",
                "-o",
                &path,
                "-o",
                &persist,
                "-N",
                "-f",
            ],
        ) else {
            return Shared::Off;
        };
        if !quiet(open) {
            return Shared::Failed;
        }
    }
    Shared::Open(vec![
        "-o".into(),
        "ControlMaster=no".into(),
        "-o".into(),
        path,
    ])
}

/// Closes the shared connection to `ip`, if one is open: the box is new
/// or gone, and its address may be another's.
pub fn close(ip: &str) {
    let Ok(dir) = dir().map(|dir| dir.join("mux")) else {
        return;
    };
    if !dir.is_dir() {
        return;
    }
    let Ok(path) = control_path(&dir) else {
        return;
    };
    if let Ok(mut exit) = command(ip, &["-o", &path, "-O", "exit"]) {
        let _ = exit
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Whether SSH to the box at `ip` gets in (once, without asking). It
/// opens the shared connection the commands after it use.
pub fn can_login(ip: &str) -> anyhow::Result<bool> {
    match shared(ip) {
        Shared::Open(_) => return Ok(true),
        Shared::Failed => return Ok(false),
        Shared::Off => {}
    }
    Ok(command(ip, &["-o", "BatchMode=yes"])?
        .arg("true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success()))
}

/// Whether the box at `ip` lets this computer in with its certificate
/// alone (once, without asking, on a connection of its own): it trusts
/// this computer's authority.
pub fn certified_login(ip: &str) -> anyhow::Result<bool> {
    Ok(command(
        ip,
        &[
            "-o",
            "BatchMode=yes",
            "-o",
            "ControlPath=none",
            "-o",
            "PubkeyAcceptedAlgorithms=ssh-ed25519-cert-v01@openssh.com",
        ],
    )?
    .arg("true")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .is_ok_and(|s| s.success()))
}

/// Appends `public_key` to `ubuntu`'s `authorized_keys` on the box at
/// `ip`, unless it's there: for a key the cloud let in for a minute only.
pub fn append_key(ip: &str, public_key: &str) -> anyhow::Result<()> {
    let key = public_key.trim();
    ensure!(
        !key.is_empty() && !key.contains(['\n', '\r']),
        "that isn't one public key"
    );
    let key = crate::setup::quote(key);
    run_script(
        ip,
        &format!(
            "umask 077\nmkdir -p \"$HOME/.ssh\"\n\
             grep -qxF {key} \"$HOME/.ssh/authorized_keys\" 2>/dev/null || \
             echo {key} >> \"$HOME/.ssh/authorized_keys\"\n"
        ),
    )
}

/// Runs `script` on the box with `sh`, and returns what it printed. It
/// goes over the box's shared connection, so several in turn connect
/// once.
pub fn script_output(ip: &str, script: &str) -> anyhow::Result<String> {
    use std::io::Write;
    let mut options: Vec<String> = match shared(ip) {
        Shared::Open(options) => options,
        Shared::Failed | Shared::Off => Vec::new(),
    };
    options.extend(["-o".into(), "BatchMode=yes".into()]);
    let options: Vec<&str> = options.iter().map(String::as_str).collect();
    let mut child = command(ip, &options)?
        .args(["sh", "-s"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .within("running ssh")?;
    child
        .stdin
        .take()
        .within("no input to ssh")?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(said(
            Report::error(sentence!("Couldn't run the script on the box.")).next(
                cmd!("lnk box list --refresh"),
                "ask its cloud for its state",
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Runs `script` on the box with `sh`, in this terminal.
pub fn run_script(ip: &str, script: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let mut child = command(ip, &["-o", "BatchMode=yes"])?
        .args(["sh", "-s"])
        .stdin(Stdio::piped())
        .spawn()
        .within("running ssh")?;
    child
        .stdin
        .take()
        .within("no input to ssh")?
        .write_all(script.as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        return Err(said(
            Report::error(sentence!("Couldn't finish setting up the box."))
                .why(prose!(
                    "The script on the box ended with {}.",
                    status.to_string()
                ))
                .next(
                    cmd!("lnk box list --refresh"),
                    "ask its cloud for its state",
                ),
        ));
    }
    Ok(())
}

/// Runs `remote` on the box ([`crate::setup::command_line`]), with a
/// terminal for its questions.
pub fn interactive(ip: &str, remote: &[String]) -> anyhow::Result<ExitStatus> {
    let terminal = remote.is_empty() || std::io::IsTerminal::is_terminal(&std::io::stdin());
    let mut command = command(ip, if terminal { &["-t"] } else { &[] })?;
    if !remote.is_empty() {
        command.arg(crate::setup::command_line(remote));
    }
    command.status().within("running ssh")
}

/// Runs `lnk` on the box with `args`, each quoted for its shell. A
/// terminal only when both ends here are one: otherwise bytes pass
/// through untouched.
pub fn run_lnk(ip: &str, staged: bool, args: &[String]) -> anyhow::Result<ExitStatus> {
    use std::io::IsTerminal;
    let terminal = std::io::stdin().is_terminal() && link_format::streams::stdout_is_terminal();
    let follows = if ip.starts_with(crate::machine::PREFIX) {
        String::new()
    } else {
        crate::setup::for_box(staged)?
    };
    let network = if let Some(name) = ip.strip_prefix(crate::machine::PREFIX) {
        crate::machine::inspect(name)?
            .bootstrap
            .as_ref()
            .and_then(|b| b.network.as_ref())
            .map(|_| {
                format!(
                    "HTTP_PROXY={0} HTTPS_PROXY={0} http_proxy={0} https_proxy={0} ",
                    link_box_contract::GUEST_PROXY
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let mut remote = vec![format!("{network}{follows}\"$HOME/.lnk/exec/bin/lnk\"")];
    remote.extend(args.iter().map(|a| crate::setup::quote(a)));
    let mut options: Vec<String> = match shared(ip) {
        Shared::Open(options) => options,
        Shared::Failed | Shared::Off => Vec::new(),
    };
    if terminal {
        // Errors only: no "Shared connection to ... closed." after.
        options.extend(["-t".into(), "-o".into(), "LogLevel=ERROR".into()]);
    } else {
        options.extend(["-o".into(), "BatchMode=yes".into()]);
    }
    let options: Vec<&str> = options.iter().map(String::as_str).collect();
    command(ip, &options)?
        .arg(remote.join(" "))
        .status()
        .within("running ssh")
}

/// `ssh -R`: the box gets a SOCKS proxy on its loopback, `port`, whose
/// connections leave from this machine: the one on this machine's
/// loopback at `to` (never ssh's own, which reaches anything here). Runs
/// until the connection drops; fails at once if the port is taken there.
pub fn forward(ip: &str, port: u16, to: u16) -> anyhow::Result<ExitStatus> {
    let remote = format!("127.0.0.1:{port}:127.0.0.1:{to}");
    connection(
        ip,
        &[
            "-N",
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveCountMax=3",
            "-R",
            &remote,
        ],
        true,
    )?
    .stdin(std::process::Stdio::null())
    .status()
    .within("running ssh")
}

/// [`forward`] to a Unix socket on the box, `socket` in its home folder,
/// made only its user's: its folder made and a socket a connection
/// before left removed first, over a connection of their own.
pub fn forward_socket(ip: &str, socket: &str, to: u16) -> anyhow::Result<ExitStatus> {
    let prepare = link_box_contract::forward::socket_forward_prepare(socket)?;
    let home = command(ip, &["-o", "BatchMode=yes"])?
        .arg(prepare)
        .stdin(std::process::Stdio::null())
        .output()
        .within("running ssh")?;
    ensure!(
        home.status.success(),
        "couldn't make the socket's folder on the box ({})",
        home.status
    );
    let remote = link_box_contract::forward::socket_forward(
        &String::from_utf8_lossy(&home.stdout),
        socket,
        &to.to_string(),
    )?;
    connection(
        ip,
        &[
            "-N",
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveCountMax=3",
            "-R",
            &remote,
        ],
        true,
    )?
    .stdin(std::process::Stdio::null())
    .status()
    .within("running ssh")
}
