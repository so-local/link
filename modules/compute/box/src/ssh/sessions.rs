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
/// user's `~/.ssh/config` says: no forwarded keys, no X11, no port, no
/// command run here, and no key or certificate but Link's: it reads no
/// SSH settings (`-F none`) but for their `ProxyJump`, kept as a
/// ProxyCommand that reaches the jump host as your settings say.
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
    // Your settings' jump host, the one thing of them a box connection
    // keeps: reached as they say, by its own ssh.
    let proxy = match proxy {
        Some(proxy) => Some(proxy),
        None => user_jump(ip)?
            .map(|jump| jump_proxy(&ssh()?, &jump))
            .transpose()?,
    };
    if let Some(alias) = &alias {
        adopt(ip, alias)?;
    }
    let (known, checks) = super::hosts::checking(alias.as_deref().unwrap_or(ip))?;
    let mut command = link_secretive::process::command(ssh()?);
    command
        .args(["-F", "none"])
        .arg("-i")
        .arg(key()?)
        .args(["-o", "IdentitiesOnly=yes"])
        .args(["-o", "IdentityAgent=none"])
        .arg("-o")
        .arg(format!("UserKnownHostsFile={}", option_path(&known)?));
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
        .arg("-o")
        .arg(format!("StrictHostKeyChecking={checks}"))
        .args(["-o", "ConnectTimeout=10"])
        .args(["-o", "ServerAliveInterval=30"])
        .args(options)
        // Never read as an option, whatever it holds.
        .arg("--")
        .arg(format!("{USER}@{ip}"));
    Ok(command)
}

/// The jump host your SSH settings give the box at `ip` (`ProxyJump`),
/// as `ssh -G` resolves them; None when they give none.
fn user_jump(ip: &str) -> anyhow::Result<Option<String>> {
    let output = link_secretive::process::command(ssh()?)
        .arg("-G")
        .arg("--")
        .arg(format!("{USER}@{ip}"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .within("running ssh -G")?;
    Ok(jump_of(&String::from_utf8_lossy(&output.stdout)))
}

/// The `proxyjump` in `ssh -G`'s `resolved` settings, unless it's none.
fn jump_of(resolved: &str) -> Option<String> {
    resolved
        .lines()
        .filter_map(|line| line.trim().split_once(' '))
        .find(|(name, _)| name.eq_ignore_ascii_case("proxyjump"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
}

/// ssh's ProxyCommand for `jump` (`[user@]host[:port]`, comma-separated
/// hops): `ssh -W` through its last hop, the hops before it its `-J`,
/// each run with your SSH settings, as a jump host needs. Every word in
/// single quotes for the shell ssh runs it with, and `%` doubled but in
/// ssh's own `%h` and `%p`.
fn jump_proxy(ssh: &std::path::Path, jump: &str) -> anyhow::Result<String> {
    let program = ssh.display().to_string();
    ensure!(
        !program.contains(['\'', '\\']) && !jump.contains(['\'', '\\']),
        "a jump host or ssh's path has a quote or a backslash, which ssh's ProxyCommand can't take"
    );
    let quoted = |word: &str| format!("'{}'", word.replace('%', "%%"));
    let (before, last) = match jump.rsplit_once(',') {
        Some((before, last)) => (Some(before), last),
        None => (None, jump),
    };
    ensure!(
        !last.is_empty() && !last.starts_with('-'),
        "{jump:?} isn't a jump host"
    );
    let mut words = vec![quoted(&program)];
    if let Some(before) = before {
        words.extend(["-J".to_string(), quoted(before)]);
    }
    words.extend([
        "-W".to_string(),
        "'[%h]:%p'".to_string(),
        "--".to_string(),
        quoted(last),
    ]);
    Ok(words.join(" "))
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
    let read = capped(child.stdout.take().within("no output from ssh")?);
    let status = child.wait()?;
    let stdout = read.within("reading the script's output")?;
    if !status.success() {
        return Err(said(
            Report::error(sentence!("Couldn't run the script on the box.")).next(
                cmd!("lnk box list --refresh"),
                "ask its cloud for its state",
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// All of `from`, a box's answer, up to [`crate::constants::MOST_OUTPUT`]:
/// more is refused, never held.
fn capped(from: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    capped_at(from, crate::constants::MOST_OUTPUT)
}

/// [`capped`], at `most` bytes.
fn capped_at(from: impl std::io::Read, most: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut read = Vec::new();
    from.take(most + 1).read_to_end(&mut read)?;
    if read.len() as u64 > most {
        return Err(std::io::Error::other(format!(
            "the box answered more than {most} bytes"
        )));
    }
    Ok(read)
}

/// Runs `command`, a box's, its output reaching your terminal clean
/// ([`link_format::clean`]), line by line: a box's program never moves
/// your cursor, retitles your window or writes what it hides. Output to
/// anything but a terminal passes through untouched, as do the bytes of a
/// session with a terminal of its own (`-t`).
fn shown(mut command: Command, input: Option<&[u8]>) -> anyhow::Result<ExitStatus> {
    use link_format::streams::{Stream, passed};
    let out = link_format::streams::stdout_is_terminal();
    let err = link_format::streams::stderr_is_terminal();
    if out {
        command.stdout(Stdio::piped());
    }
    if err {
        command.stderr(Stdio::piped());
    }
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().within("running ssh")?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        std::io::Write::write_all(&mut stdin, input)?;
    }
    let errors = child
        .stderr
        .take()
        .map(|from| std::thread::spawn(move || cleaned(from, passed(Stream::Err))));
    if let Some(from) = child.stdout.take() {
        cleaned(from, passed(Stream::Out))?;
    }
    if let Some(errors) = errors {
        let _ = errors.join();
    }
    child.wait().within("running ssh")
}

/// Each line of `from` written to `to` clean ([`link_format::clean`]).
fn cleaned(from: impl std::io::Read, mut to: impl std::io::Write) -> std::io::Result<()> {
    use std::io::BufRead;
    for line in std::io::BufReader::new(from).split(b'\n') {
        let line = line?;
        writeln!(
            to,
            "{}",
            link_format::clean(&String::from_utf8_lossy(&line))
        )?;
        to.flush()?;
    }
    Ok(())
}

/// Runs `script` on the box with `sh`, in this terminal.
pub fn run_script(ip: &str, script: &str) -> anyhow::Result<()> {
    let mut command = command(ip, &["-o", "BatchMode=yes"])?;
    command.args(["sh", "-s"]);
    let status = shown(command, Some(script.as_bytes()))?;
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
    match terminal {
        true => command.status().within("running ssh"),
        false => shown(command, None),
    }
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
    let mut command = command(ip, &options)?;
    command.arg(remote.join(" "));
    match terminal {
        true => command.status().within("running ssh"),
        false => shown(command, None),
    }
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
    let mut made = command(ip, &["-o", "BatchMode=yes"])?
        .arg(prepare)
        .stdin(std::process::Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .within("running ssh")?;
    let home = capped(made.stdout.take().within("no output from ssh")?);
    let status = made.wait().within("running ssh")?;
    let home = home.within("reading the socket's folder")?;
    ensure!(
        status.success(),
        "couldn't make the socket's folder on the box ({status})"
    );
    let remote = link_box_contract::forward::socket_forward(
        &String::from_utf8_lossy(&home),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Your settings' jump host is read from `ssh -G`, and nothing else.
    #[test]
    fn a_jump_host_is_read_from_your_settings() {
        let resolved = "user ubuntu\nidentityfile ~/.ssh/id_rsa\nproxyjump admin@bastion:2222\n";
        assert_eq!(jump_of(resolved).as_deref(), Some("admin@bastion:2222"));
        assert_eq!(jump_of("user ubuntu\nproxyjump none\n"), None);
        assert_eq!(jump_of("user ubuntu\n"), None);
    }

    /// A jump host is reached by its own ssh, with your settings: its
    /// hops before the last are that ssh's `-J`, and every word is
    /// quoted for the shell, `%` doubled but in ssh's own tokens.
    #[test]
    fn a_box_is_offered_links_key_alone_through_its_jump_host() {
        let ssh = std::path::Path::new("/usr/bin/ssh");
        assert_eq!(
            jump_proxy(ssh, "bastion").unwrap(),
            "'/usr/bin/ssh' -W '[%h]:%p' -- 'bastion'"
        );
        assert_eq!(
            jump_proxy(ssh, "a%b,me@last:22").unwrap(),
            "'/usr/bin/ssh' -J 'a%%b' -W '[%h]:%p' -- 'me@last:22'"
        );
        assert!(jump_proxy(ssh, "it's").is_err());
        assert!(jump_proxy(ssh, "a,-oProxyCommand=x").is_err());
    }

    /// A box's answer read whole is bounded: past the cap, it's refused.
    #[test]
    fn a_boxs_output_is_bounded() {
        assert_eq!(
            capped_at(&b"twelve bytes"[..], 12).unwrap(),
            b"twelve bytes"
        );
        assert!(capped_at(&b"thirteen byte"[..], 12).is_err());
    }

    /// What a box prints reaches your terminal without its escapes, line
    /// by line.
    #[test]
    fn a_boxs_output_is_cleaned() {
        let mut shown = Vec::new();
        cleaned(&b"ok\x1b]0;owned\x07\r\nnext\x1b[2J\n"[..], &mut shown).unwrap();
        assert_eq!(String::from_utf8(shown).unwrap(), "ok]0;owned\nnext[2J\n");
    }
}
