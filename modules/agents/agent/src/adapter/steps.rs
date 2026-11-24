//! An adapter's steps: what each reads and reaches, the needs it answers,
//! and the input handed to it.

use super::*;
use link_format::WithinExt;

/// What `lnk-<name> needs` said, exiting `ok` with `stdout`: nothing for
/// an adapter without the command (an error exit, or no answer); else
/// its answer, which must be [`link_harness_contract::harness::Needs`].
pub(super) fn needs_of(
    name: &str,
    ok: bool,
    stdout: &[u8],
) -> anyhow::Result<link_harness_contract::harness::Needs> {
    if !ok || stdout.trim_ascii().is_empty() {
        return Ok(Default::default());
    }
    serde_json::from_slice(stdout).within(format!("reading what lnk-{name} needs said"))
}

/// The spec of setup step `step` behind its proxy: the agent's, reaching
/// the hosts and endpoints [`step_reach`] gives it, a page in your
/// browser only on those hosts, and no limit or tool: a step is no call
/// of the harness's.
pub(super) fn step_spec(boxed: &Boxed, step: &str, adapter: impl FnOnce() -> Vec<String>) -> Spec {
    let mut spec = boxed.spec(true);
    let internet = spec.internet.get_or_insert_default();
    let (hosts, tcp) = step_reach(step, adapter, internet);
    internet.hosts = Some(match hosts.is_empty() {
        true => vec!["*".into()],
        false => hosts,
    });
    internet.tcp = (!tcp.is_empty()).then_some(tcp);
    internet.per_minute = None;
    if let Some(browser) = &mut spec.browser {
        browser.hosts = internet.hosts.clone();
    }
    spec.tools = None;
    spec
}

/// The spec of a step that reads the agent's files with no network
/// (`state export`, `instructions` and its conversations): of the
/// agent's spec only `[commands]`, the programs its harness may run.
/// An adapter may read its harness's state with what the harness
/// installed, as Hermes's runs its launcher, a shell script, to start
/// its Python. The step then runs those programs as the harness itself
/// does, and never reaches more than the harness.
pub(super) fn offline_spec(boxed: &Boxed) -> Spec {
    Spec {
        commands: boxed.spec(false).commands,
        ..Spec::default()
    }
}

/// What a setup step `step` may reach behind its proxy: the adapter's
/// hosts for its installer and configuration (`adapter`), else what the
/// user lets the harness reach running (none: any host). So a step run
/// from the harness's home, as `link` runs what it installed there, never
/// reaches more than the harness itself.
fn step_reach(
    step: &str,
    adapter: impl FnOnce() -> Vec<String>,
    running: &link_sandbox_contract::spec::Internet,
) -> (Vec<String>, Vec<String>) {
    match step {
        "install" | "configure" => (adapter(), Vec::new()),
        _ => (
            running
                .hosts
                .clone()
                .unwrap_or_default()
                .into_iter()
                .filter(|h| h != "*")
                .collect(),
            running.tcp.clone().unwrap_or_default(),
        ),
    }
}

/// Refuses a `Launch` that would widen the sandbox beyond what an adapter
/// may ask for. It is printed by a step running in the home, so a harness
/// could shape it: no folders the harness can already write (it could
/// swap them for links to the user's), nor the home folder, Link's
/// settings, the agents' folders, `/tmp` or other programs' sockets
/// (`/run`, `$XDG_RUNTIME_DIR`), nor anything holding them; no write to
/// the system's folders or a folder on your `PATH`, as a spec's paths are
/// checked; service names that are
/// only names, and none of Link's own (any agent's, the exit's); no
/// environment Link sets itself, or that loads code; with a base port,
/// no port outside its agent's block (an adapter that ignores `LNK_PORT`
/// would take another agent's).
/// Whether `program` (canonical) is a module of Link's installed here,
/// `lnk-<name>` as `lnk` finds it: a harness may read one to run it (a
/// store driver), wherever it's installed, even in the home folder
/// (`~/.lnk/exec/bin`), as it reads its own program.
pub(super) fn installed_module(program: &Path) -> bool {
    program
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("lnk-"))
        .and_then(link_core::find_module)
        .is_some_and(|found| found.canonicalize().unwrap_or(found) == program)
}

/// Writes `input` to `child`'s stdin and closes it. An adapter that doesn't
/// read it may have closed it, or exited, first: not an error, its exit
/// status says how it went.
pub(super) fn hand(child: &mut std::process::Child, input: &[u8]) -> std::io::Result<()> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("no stdin"))?;
    match stdin.write_all(input) {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        written => written,
    }
}

/// How this machine reaches the internet, passed to unsandboxed setup
/// commands. Sandboxed setup gets Link's own proxy environment instead.
const MACHINE_NETWORK_ENV: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "PIP_CERT",
    "npm_config_https_proxy",
    "npm_config_noproxy",
];

/// An unsandboxed install or configure step runs on the machine's network
/// as it is: its ordinary proxy and CA settings are kept, as a command run
/// directly in the user's terminal would have them. Model keys and other
/// process secrets stay absent from the cleared environment `env`.
pub(super) fn machine_network(env: &mut BTreeMap<String, String>) {
    for name in MACHINE_NETWORK_ENV {
        if let Some(value) = std::env::var_os(name) {
            env.insert((*name).into(), value.to_string_lossy().into_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_adapter_that_doesnt_answer_needs_needs_nothing() {
        let nothing = link_harness_contract::harness::Needs::default();
        // Its error for a command it doesn't know, or no answer.
        assert_eq!(needs_of("x", false, b"").unwrap(), nothing);
        assert_eq!(needs_of("x", false, b"{\"oops\"").unwrap(), nothing);
        assert_eq!(needs_of("x", true, b"  \n").unwrap(), nothing);
        // An answer is read strictly.
        assert!(needs_of("x", true, b"lan").is_err());
        assert!(needs_of("x", true, br#"{"permissions": "lan"}"#).is_err());
        let host = needs_of(
            "x",
            true,
            br#"{"needs": [{"spec": {"network": {"mode": "host"}}, "why": "sockets"}]}"#,
        )
        .unwrap();
        assert_eq!(host.needs[0].why, "sockets");
    }

    /// Setting up reaches the adapter's hosts; every other step, `link`
    /// (which runs code from the harness's home) among them, only what
    /// the harness may reach running.
    /// A step reads its programs, by their real paths, and nothing else
    /// in the folder they're in.
    #[test]
    fn a_step_reads_its_programs_not_their_folder() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("lnk-step-reads-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        let real = root.join("real");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        let root = root.canonicalize().unwrap();
        let (bin, real) = (root.join("bin"), root.join("real"));
        std::fs::write(real.join("lnk-h"), "").unwrap();
        symlink(real.join("lnk-h"), bin.join("lnk-h")).unwrap();
        std::fs::write(bin.join("lnk-telegram"), "").unwrap();
        std::fs::write(bin.join("secrets"), "").unwrap();
        let adapter = Adapter {
            name: "h".into(),
            program: bin.join("lnk-h"),
        };
        assert_eq!(
            adapter.step_reads(&adapter.program),
            (real.join("lnk-h"), vec![real.join("lnk-h")])
        );
        assert_eq!(
            adapter.step_reads(&bin.join("lnk-telegram")),
            (
                bin.join("lnk-telegram"),
                vec![real.join("lnk-h"), bin.join("lnk-telegram")]
            )
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A step that reads the agent's files runs the programs its harness
    /// may run, and gets nothing else of the agent's spec.
    #[test]
    fn an_offline_step_runs_what_its_harness_runs_and_nothing_more() {
        let boxed = Boxed::for_tests().granted(
            "[internet]\nhosts = [\"api.anthropic.com\"]\n",
            "[commands]\nallow = [\"/nonexistent/programs/\"]\ntoolchains = [\"shell\"]\n",
        );
        let running = boxed.spec(false);
        assert!(running.internet.is_some(), "the fixture grants a network");
        let offline = offline_spec(&boxed);
        assert!(offline.commands.is_some(), "the harness's programs");
        assert_eq!(offline.commands, running.commands);
        assert_eq!(
            offline,
            Spec {
                commands: running.commands,
                ..Spec::default()
            }
        );
    }

    #[test]
    fn a_step_reaches_no_more_than_its_harness() {
        let boxed = Boxed {
            name: "h".into(),
            agent: "main".into(),
            identity: None,
            service: None,
            programs: None,
            profile: "main-h".into(),
            folders: None,
            teams: Default::default(),
            port: None,
            block: settings::PORT_BLOCK,
            dir: "/nonexistent/h".into(),
            folder: link_agent_contract::folder::Folder {
                home: "/nonexistent/files".into(),
                agent: "/nonexistent/agent".into(),
            },
            sandbox: true,
            ports: BTreeSet::new(),
            models: BTreeSet::new(),
            path: String::new(),
            net: "/nonexistent/net".into(),
            wires: None,
            spec: Spec::default(),
            proxied: Spec::default(),
        };
        let boxed = boxed.granted(
            "[internet]\nhosts = [\"api.anthropic.com\"]\ntcp = [\"db.example.com:5432\"]\n",
            "",
        );
        let running = boxed.spec(true).internet.unwrap();
        let adapter = vec!["registry.npmjs.org".to_string()];
        for step in ["install", "configure"] {
            assert_eq!(
                step_reach(step, || adapter.clone(), &running),
                (adapter.clone(), vec![])
            );
        }
        for step in ["link", "health"] {
            assert_eq!(
                step_reach(step, || adapter.clone(), &running),
                (
                    vec!["api.anthropic.com".to_string()],
                    vec!["db.example.com:5432".to_string()]
                )
            );
        }
        // A page in your browser on the step's hosts alone, and no limit
        // or tool.
        let open = Boxed::for_tests().granted(
            "[internet]\nhosts = [\"api.anthropic.com\"]\nper_minute = 60\n[tools]\nnames = \
             [\"gh\"]\n[browser]\nhosts = [\"api.anthropic.com\"]\n",
            "",
        );
        let install = step_spec(&open, "install", || adapter.clone());
        assert_eq!(install.browser.and_then(|b| b.hosts), Some(adapter.clone()));
        assert!(install.tools.is_none());
        assert_eq!(install.internet.and_then(|i| i.per_minute), None);
        let link = step_spec(&open, "link", || adapter.clone());
        assert_eq!(
            link.browser.and_then(|b| b.hosts),
            Some(vec!["api.anthropic.com".to_string()])
        );
        // Any host, running: any, for its steps too.
        let any = Boxed::for_tests().granted("[internet]\nhosts = [\"*\"]\n", "");
        let running = any.spec(true).internet.unwrap();
        assert_eq!(step_reach("link", Vec::new, &running), (vec![], vec![]));
    }

    /// An adapter that leaves its input unread has nothing to say by it:
    /// here one closes its stdin before the write, which then breaks.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn input_an_adapter_leaves_unread_is_no_error() {
        use std::io::BufRead;
        let mut child = Command::new("sh")
            .args(["-c", "exec 0<&-; echo closed; sleep 5"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line, "closed\n");
        hand(&mut child, b"{}").unwrap();
        child.kill().unwrap();
        child.wait().unwrap();

        // One that reads it gets all of it.
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        hand(&mut child, b"{\"name\":\"main\"}").unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.stdout, b"{\"name\":\"main\"}");
    }
}
