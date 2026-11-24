//! Running an adapter: finding its program, calling a command, reading
//! its request and printing its reply, and a box's tunnel.

use super::*;
use link_format::WithinExt;

/// The adapter for `kind`, installing it first (`lnk module add <kind>`)
/// if it's one Link knows and it's missing, as `lnk agent harness set` does.
pub fn adapter(kind: &str) -> anyhow::Result<PathBuf> {
    check_kind(kind)?;
    let known = KNOWN.iter().any(|(k, _)| *k == kind);
    if known {
        return link_core::modules::add(kind, &format!("A {kind} account"));
    }
    let known: Vec<_> = KNOWN.iter().map(|(k, _)| *k).collect();
    link_core::find_module(kind).within(format!(
        "no adapter for {kind} (lnk-{kind}) next to lnk or on your PATH; Link's are {}",
        known.join(", ")
    ))
}

/// Runs adapter command `command` with `request`, its errors on this
/// terminal, and reads its reply.
pub fn call(kind: &str, command: &str, request: &Request) -> anyhow::Result<Reply> {
    let reply: Reply = if command == "launch" {
        crate::ownership::around(kind, request, || call_as(kind, command, request))?
    } else {
        call_as(kind, command, request)?
    };
    link_actions::value::Validate::validate(&reply, "")
        .map_err(|refusal| anyhow::anyhow!("what lnk-{kind} {command} printed: {refusal}"))?;
    Ok(reply)
}

/// [`call`], with a request and a reply of another contract's own: the
/// commands of a cloud that hosts agents are the agents' contract's.
pub fn call_as<Q: Serialize, A: DeserializeOwned>(
    kind: &str,
    command: &str,
    request: &Q,
) -> anyhow::Result<A> {
    let output = run(kind, command, request, Stdio::inherit())?;
    if !output.status.success() {
        bail!("lnk-{kind} {command} failed (above)");
    }
    serde_json::from_slice(&output.stdout)
        .within(format!("reading what lnk-{kind} {command} printed"))
}

/// What an adapter says when it has no such command: a hand-written
/// adapter's own words, or a generated command line's.
const NO_COMMAND: &[&str] = &["has no command", "unrecognized subcommand"];

/// Whether what a failed adapter said is that it has no such command.
fn lacks_command(said: &str) -> bool {
    NO_COMMAND.iter().any(|words| said.contains(words))
}

/// A failed adapter command's error when the adapter has no such
/// command.
#[derive(Debug)]
pub struct NoCommand;

impl std::fmt::Display for NoCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("the adapter has no such command")
    }
}

impl std::error::Error for NoCommand {}

/// [`call`], what the adapter says on stderr kept for the caller to
/// show or not: a failure's error is what it said, [`NoCommand`] when
/// it has no `command`; on success it's shown here.
pub fn call_quietly(kind: &str, command: &str, request: &Request) -> anyhow::Result<Reply> {
    let reply: Reply = call_quietly_as(kind, command, request)?;
    link_actions::value::Validate::validate(&reply, "")
        .map_err(|refusal| anyhow::anyhow!("what lnk-{kind} {command} printed: {refusal}"))?;
    Ok(reply)
}

/// A command of another module's for an adapter of `kind`, with that
/// module's own `request` and reply, run as every adapter command is:
/// what it says on stderr shown here as it comes when `show` (each line
/// cleaned, for a command that takes minutes, as an image's build does),
/// else kept for an error; [`NoCommand`] when the adapter has no
/// `command`.
pub fn ask<A: DeserializeOwned>(
    kind: &str,
    command: &str,
    request: &impl Serialize,
    show: bool,
) -> anyhow::Result<A> {
    match show {
        true => shown(kind, command, request),
        false => call_quietly_as(kind, command, request),
    }
}

/// [`call_quietly`], with a request and a reply of another contract's own.
pub fn call_quietly_as<Q: Serialize, A: DeserializeOwned>(
    kind: &str,
    command: &str,
    request: &Q,
) -> anyhow::Result<A> {
    let output = run(kind, command, request, Stdio::piped())?;
    let said = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        if lacks_command(&said) {
            return Err(NoCommand.into());
        }
        // The adapter's own report: its headline opens with a mark.
        let said: Vec<String> = said
            .trim()
            .trim_start_matches("Error: ")
            .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_ascii_punctuation())
            .lines()
            .map(|line| link_format::clean(line.trim()))
            .collect();
        let said = said.join("\n");
        bail!("lnk-{kind} {command}: {said}");
    }
    relay(&said);
    serde_json::from_slice(&output.stdout)
        .within(format!("reading what lnk-{kind} {command} printed"))
}

fn shown<A: DeserializeOwned>(
    kind: &str,
    command: &str,
    request: &impl Serialize,
) -> anyhow::Result<A> {
    let output = run_adapter(kind, command, request, Stdio::piped(), true)?;
    let said = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        if lacks_command(&said) {
            return Err(NoCommand.into());
        }
        bail!("lnk-{kind} {command} failed (above)");
    }
    serde_json::from_slice(&output.stdout)
        .within(format!("reading what lnk-{kind} {command} printed"))
}

/// Runs adapter command `command` with `request` on its stdin, its
/// stderr to `stderr`.
fn run(
    kind: &str,
    command: &str,
    request: &impl Serialize,
    stderr: Stdio,
) -> anyhow::Result<std::process::Output> {
    run_adapter(kind, command, request, stderr, false)
}

/// Runs adapter command `command` with `request` on its stdin, its
/// stderr to `stderr`, or, when `show`, read here: shown as it comes,
/// but for saying it has no command, and kept.
fn run_adapter(
    kind: &str,
    command: &str,
    request: &impl Serialize,
    stderr: Stdio,
    show: bool,
) -> anyhow::Result<std::process::Output> {
    use std::io::{BufRead, Write};
    let program = adapter(kind)?;
    let mut child = link_secretive::process::ours(&program)
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .within(format!("running {}", program.display()))?;
    let reader = match show {
        true => {
            let said = child.stderr.take().within("no errors from the adapter")?;
            Some(std::thread::spawn(move || {
                let mut kept = Vec::new();
                // Saying it has no such command ends what an adapter says
                // (a generated command line adds a tip and its usage), and
                // the caller does without it, so none of it is shown.
                let mut lacking = false;
                for line in std::io::BufReader::new(said)
                    .split(b'\n')
                    .map_while(Result::ok)
                {
                    let text = String::from_utf8_lossy(&line);
                    lacking = lacking || lacks_command(&text);
                    if !lacking {
                        relay(&text);
                    }
                    kept.extend_from_slice(&line);
                    kept.push(b'\n');
                }
                kept
            }))
        }
        false => None,
    };
    let input = serde_json::to_string(request)?;
    let mut stdin = child.stdin.take().within("no input to the adapter")?;
    // Written beside the reading of its output, so a request bigger than
    // a pipe holds never waits on it; one it ended without reading (a
    // command it doesn't have) is no error here: what it said is.
    let writer = std::thread::spawn(move || match stdin.write_all(input.as_bytes()) {
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    });
    let mut output = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("writing to the adapter"))??;
    if let Some(reader) = reader {
        output.stderr = reader
            .join()
            .map_err(|_| anyhow::anyhow!("reading the adapter's errors"))?;
    }
    Ok(output)
}

/// Starts adapter command `command` with `request` as one line on its
/// stdin, left open for what follows (`agent-receive`'s stream), its stdout
/// piped (`agent-send`'s stream, or its reply), what it says on stderr
/// shown here as it comes, cleaned, as for every adapter command.
pub fn spawn(
    kind: &str,
    command: &str,
    request: &impl Serialize,
) -> anyhow::Result<std::process::Child> {
    use std::io::{BufRead, Write};
    let program = adapter(kind)?;
    let mut child = link_secretive::process::ours(&program)
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .within(format!("running {}", program.display()))?;
    let said = child.stderr.take().within("no errors from the adapter")?;
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(said)
            .split(b'\n')
            .map_while(Result::ok)
        {
            relay(&String::from_utf8_lossy(&line));
        }
    });
    let stdin = child.stdin.as_mut().within("no input to the adapter")?;
    writeln!(stdin, "{}", serde_json::to_string(request)?)?;
    Ok(child)
}

/// In an adapter: reads the [`Request`] on the first line of `input`,
/// leaving what follows it (a stream) to read.
pub fn read_request_line<Q: DeserializeOwned>(
    input: &mut impl std::io::BufRead,
) -> anyhow::Result<Q> {
    use std::io::{BufRead, Read};
    let mut line = String::new();
    input.take(1 << 20).read_line(&mut line)?;
    serde_json::from_str(&line).within("reading the request on stdin")
}

/// Reads all of stdin as one JSON request of the caller's own wire.
pub fn read_stdin<Q: DeserializeOwned>() -> anyhow::Result<Q> {
    serde_json::from_reader(std::io::stdin().lock()).within("reading the request on stdin")
}

/// In an adapter: reads the [`Request`] on stdin, refused where its
/// declaration refuses it.
pub fn read_request() -> anyhow::Result<Request> {
    let request: Request = read_stdin()?;
    link_actions::value::Validate::validate(&request, "")
        .map_err(|refusal| anyhow::anyhow!("the request on stdin: {refusal}"))?;
    Ok(request)
}

/// In an adapter: prints `reply`.
pub fn reply(reply: &impl Serialize) -> anyhow::Result<()> {
    use std::io::Write;
    // A document for the module that ran the adapter, as it is.
    let mut stdout = link_format::streams::protocol();
    writeln!(stdout, "{}", serde_json::to_string(reply)?).within("writing the reply")?;
    stdout.flush().within("writing the reply")
}

/// What an adapter said on its stderr, passed on as notes, cleaned: this
/// library holds no `Out` of its caller's to hand them to.
fn relay(said: &str) {
    for line in said.lines().filter(|line| !line.trim().is_empty()) {
        link_format::aside(link_format::Note(link_format::Sentence::adopt(line)));
    }
}

/// Cloud account `name` (`lnk cloud env <name> --json`), with its
/// sign-in; none when there's no such account.
pub fn account(name: &str) -> Option<Account> {
    let output = link_core::lnk()
        .args(["cloud", "env", name, "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let account: Account = serde_json::from_slice(&output.stdout).ok()?;
    check_kind(&account.kind).ok()?;
    Some(account)
}

/// A box's tunnel (`lnk-<kind> tunnel`): the account it acts as, the
/// box's place and its id there, as `lnk-box` writes them into ssh's
/// ProxyCommand and an adapter reads them back. Each a word ssh's shell
/// reads as itself. Its arguments are the contract's `tunnel` action's,
/// declared in its `actions.toml`, which each adapter's generated parser
/// reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tunnel {
    pub cloud: String,
    pub zone: String,
    pub id: String,
}

impl Tunnel {
    /// It, checked.
    pub fn new(account: &str, place: &str, id: &str) -> anyhow::Result<Tunnel> {
        for value in [account, place, id] {
            tunnel_word(value).map_err(anyhow::Error::msg)?;
        }
        Ok(Tunnel {
            cloud: account.into(),
            zone: place.into(),
            id: id.into(),
        })
    }

    /// The adapter's arguments for it, `tunnel` first, as the contract
    /// declares them (proved by its test against `actions.toml`).
    pub fn args(&self) -> Vec<String> {
        [
            "tunnel",
            "--cloud",
            &self.cloud,
            "--zone",
            &self.zone,
            "--id",
            &self.id,
        ]
        .map(String::from)
        .to_vec()
    }
}

fn tunnel_word(value: &str) -> Result<String, String> {
    let plain = !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
    if plain {
        Ok(value.into())
    } else {
        Err(format!(
            "{:?} isn't letters, digits, dots, dashes and underscores (not first a dash), so ssh can't be given a tunnel to it",
            link_format::clean(value)
        ))
    }
}

#[cfg(test)]
mod tunnel_tests {
    use super::*;

    #[test]
    fn a_tunnels_line_is_the_contracts_declared_tunnel() {
        let tunnel = Tunnel::new("work", "eu-west-1", "box.1").unwrap();
        let args = tunnel.args();
        let contract = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalog = link_actions::Catalog::load(&[contract]).unwrap();
        let id = "cloud-contract.cloud-contract.tunnel".parse().unwrap();
        let declared = catalog.action(&id).unwrap();
        assert_eq!(args[0], declared.id.verb());
        let mut flags: Vec<&str> = args[1..]
            .iter()
            .filter_map(|word| word.strip_prefix("--"))
            .collect();
        flags.sort_unstable();
        let names: Vec<&str> = declared.args.keys().map(String::as_str).collect();
        assert_eq!(flags, names);
        for (flag, value) in [
            ("--cloud", "work"),
            ("--zone", "eu-west-1"),
            ("--id", "box.1"),
        ] {
            let at = args.iter().position(|word| word == flag).unwrap();
            assert_eq!(args[at + 1], value);
        }
    }

    #[test]
    fn a_tunnel_refuses_what_ssh_cant_take() {
        for invalid in ["", "-option", "two words", "$(command)", "a;b"] {
            assert!(Tunnel::new(invalid, "zone", "id").is_err());
            assert!(Tunnel::new("work", invalid, "id").is_err());
            assert!(Tunnel::new("work", "zone", invalid).is_err());
        }
    }
}
