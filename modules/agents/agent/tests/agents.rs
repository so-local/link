//! Two agents on one machine, with a fake harness program: each with its
//! own settings, folder, ports (`LNK_PORT`) and bot, `-a` picking one,
//! `lnk agent list`, and the sandbox keeping each out of the other's
//! folder.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use link_test_kit::TempDir;

use crate::common;

/// Records the base port it was given, and, as the harness, tries to read
/// main's files. HOME is its home, ~/.lnk/data/agents/<agent>/harnesses/fake:
/// the real home is six up. It serves none of its ports, which nothing
/// here asks for: a sandbox's proxy listening on them, as each test's
/// agents start side by side, could take another test's (each test's
/// agents are another user's, allocating from 20000 too).
const ADAPTER: &str = r#"#!/bin/sh
case "$1" in
  command)
    base="$LNK_PORT"
    printf '{"argv":["/bin/sh","-c","echo ${LNK_PORT:-none} > \\"$LNK_FILES/port\\"; cat %s/Link/Agents/main/home/secret > \\"$LNK_FILES/stolen\\" 2>/dev/null; true"],"ports":[%s],"serve":[]}' "$HOME/../../../../../.." "$base"
    ;;
  *) exit 0 ;;
esac
"#;

/// An adapter that ignores `LNK_PORT`: its fixed port, whatever it's given.
const FIXED: &str = r#"#!/bin/sh
case "$1" in
  command) printf '{"argv":["/bin/true"],"ports":[18789]}' ;;
  *) exit 0 ;;
esac
"#;

/// A Telegram module, as `lnk-telegram` answers `describe` and `rule`,
/// and its `connect` refusing the bot agent main has.
const TELEGRAM: &str = r#"#!/bin/sh
case "$1" in
  describe) echo '{"protocol": 1, "title":"Telegram","secret_fields":["bot_token"],"bridged":true,"hosts":["api.telegram.org"]}' ;;
  rule) echo '{"field":"bot_token","root":"api_root","host":"api.telegram.org","paths":[{"prefix":"bot{secret}/","rest":"name"},{"prefix":"file/bot{secret}/","rest":"path"}],"refuse":["bot{secret}/setwebhook","bot{secret}/logout","bot{secret}/close"],"methods":["GET","POST"],"public":":"}' ;;
  connect)
    case "$*" in
      *'"id":"123456","agent":"main"'*) echo "Error: that bot is agent main's already" >&2 ;;
      *) echo "Error: no bot here" >&2 ;;
    esac
    exit 1 ;;
  *) exit 1 ;;
esac
"#;

/// A Slack module, as `lnk-slack describe` answers.
const SLACK: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"protocol": 1, "title":"Slack","secret_fields":["bot_token","app_token"],"bridged":true}'; exit 0; }
exit 1
"#;

struct World {
    _root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    core: PathBuf,
    sandboxed: bool,
    proxy_ports: std::sync::Mutex<Vec<link_test_kit::HeldNumber>>,
}

impl World {
    fn new(test: &str) -> Self {
        let root = TempDir::new(&format!("agents-{test}"));
        let (home, bin) = (root.join("home"), root.join("bin"));
        std::fs::create_dir_all(home.join(".lnk")).unwrap();
        common::own_machine(&home);
        std::fs::create_dir_all(&bin).unwrap();
        for (name, script) in [("lnk-fake", ADAPTER), ("lnk-fixed", FIXED)] {
            common::write_adapter(&bin.join(name), script).unwrap();
        }
        // `connect telegram` runs the Telegram module, and `connect slack`
        // asks Slack's: each a fake here, answering as its module does.
        for (name, script) in [("lnk-telegram", TELEGRAM), ("lnk-slack", SLACK)] {
            link_test_kit::write_program(&bin.join(name), script).unwrap();
        }
        let core = common::core_with_sandbox(&bin);
        let sandboxed = link_test_kit::gate::sandbox_with_landlock();
        if !sandboxed {
            eprintln!("the wrapped sandbox can't run here; testing without it");
        }
        World {
            _root: root,
            home,
            bin,
            core,
            sandboxed,
            proxy_ports: Default::default(),
        }
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn lnk(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-agent"));
        command
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.home),
            )
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", &self.core)
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
            .env("LNK_TELEGRAM_API", "http://127.0.0.1:9")
            .stdin(Stdio::null());
        command.output().unwrap()
    }

    /// `lnk agent <args>` for starting agent `name`, in the foreground,
    /// in its default files folder, sandboxed if the sandbox can run here.
    fn start(&self, name: &str, args: &[&str]) -> Output {
        let files = self.agent(name).join("home").display().to_string();
        let mut args = args.to_vec();
        args.extend(["--foreground", "--run.files", &files]);
        if !self.sandboxed {
            args.push("--no-sandbox");
        }
        self.lnk(&args)
    }

    fn agent(&self, name: &str) -> PathBuf {
        self.home.join("Link/Agents").join(name)
    }

    /// Creation writes the configuration before asking the adapter for its
    /// contract. Stop at that boundary to give the fixture its owned proxy
    /// port before the launcher rereads and fits its block. The fake serves
    /// no harness port; only its proxy binds on the host.
    fn start_on_owned_port(&self, name: &str, args: &[&str]) -> Output {
        let port = link_test_kit::HeldNumber::port();
        let ready = self.bin.join("contract-ready");
        let proceed = self.bin.join("contract-proceed");
        let _ = std::fs::remove_file(&ready);
        let _ = std::fs::remove_file(&proceed);
        let contract = "  contract) here=$(dirname \"$0\"); touch \"$here/contract-ready\"; while [ ! -f \"$here/contract-proceed\" ]; do sleep 0.01; done; printf '{\"version\":1,\"ports\":1}' ;;\n";
        link_test_kit::write_program(
            &self.bin.join("lnk-fake"),
            ADAPTER.replacen(
                "case \"$1\" in\n",
                &format!("case \"$1\" in\n{contract}"),
                1,
            ),
        )
        .unwrap();
        // Release the child also when fixture preparation panics, before
        // the scoped thread joins it.
        struct Release<'a>(&'a Path);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                let _ = std::fs::write(self.0, "");
            }
        }
        std::thread::scope(|scope| {
            let running = scope.spawn(|| self.start(name, args));
            let release = Release(&proceed);
            link_test_kit::wait::appears(
                "the adapter contract request",
                &ready,
                std::time::Duration::from_secs(30),
            );
            let path = link_files::layout::Home::AgentSettings
                .at(&self.home.join("Link"))
                .join(name)
                .join("agent.toml");
            assert!(path.is_file(), "{}", path.display());
            let base = i64::from(port.port_number()) - 1;
            crate::common::set_record(&self.home, name, "here.toml", "port", base.into());
            crate::common::set_record(&self.home, name, "here.toml", "block", 2.into());
            self.proxy_ports.lock().unwrap().push(port);
            drop(release);
            running.join().unwrap()
        })
    }
}

fn ok(output: &Output) -> String {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    text
}

fn failed(output: &Output) -> String {
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Without a terminal to ask in, a first start takes the folder it would
/// offer, and says which. Given Link MCP, it registers the tool with
/// its reads allowed.
#[test]
fn a_first_start_without_a_terminal_takes_the_default_folder() {
    let world = World::new("no-folder");
    let mut args = vec![
        "create",
        "main",
        "--harness",
        "fake",
        "--foreground",
        "--tool.mcp.enabled",
        "true",
    ];
    if !world.sandboxed {
        args.push("--no-sandbox");
    }
    let said = ok(&world.lnk(&args));
    assert!(
        said.contains(
            "Your agent works in ~/Link/Agents/main/home, and --run.files picks another."
        ),
        "{said}"
    );
    assert!(!said.contains("pass --files"), "{said}");
    // Its tools, not asked: what adds them said, once.
    assert!(
        said.contains(
            "Your agent can also have memory, web, browser and desktop, and lnk agent tool add <tool> adds one."
        ),
        "{said}"
    );
    // A block of its own, `main`'s too: never the adapter's own port.
    let port = read(&world.agent("main").join("home/port"));
    assert!(port.trim().parse::<u16>().is_ok(), "{port}");
    let definitions = link_test_kit::env::command(&world.core, &world.home)
        .args(["sandbox", "tool", "list", "--json"])
        .output()
        .unwrap();
    assert!(
        definitions.status.success(),
        "{}",
        String::from_utf8_lossy(&definitions.stderr)
    );
    let tools: Vec<serde_json::Value> = String::from_utf8(definitions.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mcp = tools
        .iter()
        .find(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("mcp-"))
        })
        .expect("an agent given Link MCP has it");
    assert_eq!(mcp["ask"], "all");
    assert!(
        mcp["argv"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("agent:")
    );
    let allow = mcp["allow"].as_array().unwrap();
    assert!(
        allow.contains(&serde_json::json!("manual__show")),
        "{allow:?}"
    );
    assert!(allow.contains(&serde_json::json!("agent__tool__list")));
    assert!(!allow.contains(&serde_json::json!("box__start")));
    assert!(!allow.contains(&serde_json::json!("box__stop")));
    let context = world.lnk(&["tool", "list", "--json"]);
    assert!(
        context.status.success(),
        "{}",
        String::from_utf8_lossy(&context.stderr)
    );
    let context: serde_json::Value = serde_json::from_slice(&context.stdout).unwrap();
    // Listed by the reads its grant opens, never as an empty grant.
    let listed = context["namespaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|namespace| namespace["name"] == "mcp")
        .unwrap_or_else(|| panic!("{context}"));
    assert_eq!(listed["actions"], mcp["allow"], "{context}");
}

/// A new agent saves Link's behavior, written out, beside its setup,
/// and an explicit standalone spec is resolved into that same file.
#[test]
fn a_new_agent_writes_out_links_behavior() {
    let world = World::new("behavior");
    let spec = |name: &str| {
        read(
            &world
                .home
                .join(format!("Link/Safe/Agents/{name}/agent.toml")),
        )
    };
    ok(&world.start_on_owned_port("fresh", &["create", "fresh", "--harness", "fake"]));
    assert!(!spec("fresh").contains("extends ="));
    let fresh: toml::Table = spec("fresh").parse().unwrap();
    assert_eq!(
        fresh["behavior"]["instructions"]["repository"].as_str(),
        Some("none")
    );
    let printed = ok(&world.lnk(&["spec", "show", "fresh"]));
    assert!(printed.contains("repository"), "{printed}");

    // A minimal current configuration gains concrete behavior defaults.
    crate::common::write_agent(&world.home, "old", "harness = \"fake\"\n");
    ok(&world.start_on_owned_port("old", &["start", "-a", "old"]));
    assert!(spec("old").contains("[behavior"));
    assert!(!spec("old").contains("extends ="));

    // An explicit spec is expanded into the same configuration.
    let given = world.home.join("given.toml");
    std::fs::write(&given, "schema = 1\n[turns]\nrounds = 3\n").unwrap();
    let given = given.display().to_string();
    ok(&world.start_on_owned_port(
        "programmer",
        &[
            "create",
            "programmer",
            "--harness",
            "fake",
            "--spec",
            &given,
        ],
    ));
    let given: toml::Table = spec("programmer").parse().unwrap();
    assert_eq!(given["behavior"]["turns"]["rounds"].as_integer(), Some(3));
    assert_eq!(
        given["behavior"]["instructions"]["repository"].as_str(),
        Some("none")
    );
    assert!(!spec("programmer").contains("extends ="));
}

/// `lnk agent show` reads the agent whole, formatted for a person, never
/// as JSON: its state, who it is, its skills and memory, and where its
/// things are, alike with `--agent` and with its name.
#[test]
fn show_reads_the_agent_whole() {
    let world = World::new("readout");
    ok(&world.start_on_owned_port("work", &["create", "work", "--harness", "fake"]));
    let id = format!("agent:{}", id_of(&world, "work"));
    // Shown whole whether it answers or not: a stopped one exits nonzero.
    for asked in [&["show", "-a", "work"][..], &["show", "work"][..]] {
        let output = world.lnk(asked);
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!printed.trim_start().starts_with('{'), "{printed}");
        for part in [
            "State", "Harness", "Who", &id, "Skills", "Memory", "Where", "Its own", "Log",
        ] {
            assert!(printed.contains(part), "{part} missing from:\n{printed}");
        }
    }
    let json = ok(&world.lnk(&["show", "--json", "-a", "work"]));
    let shown: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(shown["name"], "work");
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn two_agents_on_one_machine_stay_apart() {
    let world = World::new("two");
    // The first agent is main, on a block of its own.
    ok(&world.start("main", &["create", "main", "--harness", "fake"]));
    std::fs::write(world.agent("main").join("home/secret"), "MAIN'S").unwrap();
    let main_port: u16 = read(&world.agent("main").join("home/port"))
        .trim()
        .parse()
        .unwrap();

    // A second: its own files, settings and a block of ports.
    ok(&world.start("moltbot-2", &["create", "moltbot-2", "--harness", "fake"]));
    let second = world.agent("moltbot-2");
    // A block from 20000, apart from main's.
    let port: u16 = read(&second.join("home/port")).trim().parse().unwrap();
    assert!(port >= 20000, "{port}");
    assert_ne!(port, main_port);
    assert!(
        link_files::layout::Hidden::AgentData
            .in_home(&world.home)
            .join("moltbot-2/harnesses/fake")
            .is_dir()
    );
    let settings = read(&world.home.join("Link/Safe/Agents/moltbot-2/agent.toml"));
    let table: toml::Table = settings.parse().unwrap();
    assert!(!table.contains_key("name"), "{settings}");
    let here = crate::common::record(&world.home, "moltbot-2", "here.toml");
    assert_eq!(here["port"], port, "{settings}");
    // Sandboxed, it couldn't read main's files; without the sandbox it
    // can, so the try is a real one.
    if world.sandboxed {
        assert_eq!(
            read(&second.join("home/stolen")),
            "",
            "it read main's folder"
        );
    } else {
        assert_eq!(read(&second.join("home/stolen")), "MAIN'S");
    }
    // The same name twice is refused.
    assert!(failed(&world.lnk(&["create", "moltbot-2"])).contains("already"));
    for bad in ["here", "all", "Moltbot", "2bot"] {
        assert!(!world.lnk(&["create", bad]).status.success(), "{bad}");
    }

    // With two, a command needs --agent.
    assert!(failed(&world.lnk(&["show"])).contains("--agent <name>"));
    let status = world.lnk(&["-a", "moltbot-2", "show", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    link_test_kit::ids::assert_named(&status);
    assert_eq!(status["name"], "moltbot-2");
    assert_eq!(status["port"], port);
    assert!(failed(&world.lnk(&["show", "-a", "nobody"])).contains("no agent named nobody here"));

    // Both, listed.
    let list = world.lnk(&["list", "--json"]);
    let list: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    link_test_kit::ids::assert_named(&list);
    let agents = list["agents"].as_array().unwrap();
    let names: Vec<_> = agents.iter().map(|a| a["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["main", "moltbot-2"]);
    assert_eq!(agents[0]["port"], main_port);
    assert_eq!(agents[1]["harness"], "fake");
    assert_ne!(agents[0]["workload"], agents[1]["workload"]);
    assert!(
        agents[1]["workload"]
            .as_str()
            .unwrap()
            .starts_with("agent:")
    );
    assert_eq!(agents[0]["machine"], agents[1]["machine"]);
    let machine = agents[0]["machine"]["resource"].as_str().unwrap();
    assert!(
        machine
            .strip_prefix("installation:")
            .is_some_and(|id| id.len() == 12),
        "{machine}"
    );
    assert_eq!(list["unreached"], serde_json::json!([]));
    // Where the harness module times its starts and stops, and no teams.
    assert!(
        agents[1]["times"]
            .as_str()
            .unwrap()
            .ends_with(".lnk/safe/agents/moltbot-2/times.jsonl"),
        "{}",
        agents[1]["times"]
    );
    assert_eq!(agents[1]["teams"], serde_json::json!([]));
    let text = ok(&world.lnk(&["list"]));
    assert!(
        text.lines()
            .any(|l| l.starts_with("  moltbot-2  fake") && l.contains("stopped")),
        "{text}"
    );

    // Its sandbox check tries the other agents' folders.
    if world.sandboxed {
        let check = ok(&world.lnk(&["-a", "moltbot-2", "harness", "check"]));
        assert!(
            check.contains("blocked  read what's hidden: ~/Link/Agents"),
            "{check}"
        );
    }

    // An adapter that ignores LNK_PORT runs no agent: its port isn't
    // the agent's own.
    let fixed = failed(&world.start("third", &["create", "third", "--harness", "fixed"]));
    assert!(fixed.contains("LNK_PORT"), "{fixed}");

    // Each agent needs its own bot.
    let settings = world.home.join("Link/Safe/Agents/main/agent.toml");
    let mut text = read(&settings);
    text.push_str(&crate::common::connection(
        "telegram",
        &["bot_token"],
        concat!(
            "bot_token = \"1234",
            "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw\"\nallowed_users = [42]"
        ),
        "id = \"123456\"",
    ));
    std::fs::write(&settings, text).unwrap();
    let mut connect = Command::new(env!("CARGO_BIN_EXE_lnk-agent"));
    let refused = connect
        .args(["-a", "moltbot-2", "connect", "telegram"])
        .env_clear()
        .env("HOME", &world.home)
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&world.home),
        )
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env(
            "LNK_TELEGRAM_TOKEN",
            concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw"),
        )
        .env("LNK_TELEGRAM_API", "http://127.0.0.1:9")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        failed(&refused).contains("agent main's already"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    // A connection its harness doesn't speak is refused (an adapter without
    // `connections` speaks Telegram only).
    let slack = failed(&world.lnk(&["-a", "moltbot-2", "connect", "slack"]));
    assert!(slack.contains("fake doesn't speak Slack"), "{slack}");
}

/// Keeps what `configure` is handed in its home, and serves nothing.
const TOLD: &str = r#"#!/bin/sh
case "$1" in
  configure) cat > "$HOME/told.json" ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  *) exit 0 ;;
esac
"#;

/// A harness that writes down its environment.
const TELLER: &str = r#"#!/bin/sh
case "$1" in
  command) printf '{"argv":["/bin/sh","-c","env > \\"$HOME/env.txt\\""],"ports":[]}' ;;
  *) exit 0 ;;
esac
"#;

/// A secret granted to an agent (`lnk secret add --to`) is sealed:
/// its harness is told only a placeholder and where its service is on its
/// proxy, which adds the value for its host alone. Without its own proxy
/// (no sandbox here) the start is refused, naming it.
#[test]
fn a_sealed_secret_granted_to_an_agent_reaches_its_harness_as_a_placeholder() {
    let world = World::new("sealed");
    common::write_adapter(&world.bin.join("lnk-teller"), TELLER).unwrap();
    ok(&world.start("main", &["create", "main", "--harness", "teller"]));
    let identity = crate::common::record(&world.home, "main", "identity.toml");
    let (service, id) = (
        identity["service"].as_str().unwrap(),
        identity["id"].as_str().unwrap(),
    );
    let store = world.home.join(".lnk/safe/secrets/secrets.toml");
    let kept = format!(
        "{}\n[entry.str1pek3y000]\nkind = \"named\"\nname = \"STRIPE_KEY\"\nvalue = \"sk_live_granted\"\n\
         sealed = {{ host = \"api.stripe.com\" }}\n\
         to = [{{ principal = \"service:{service}\", action = \"secret.secret.pass\", resource = \"secret:str1pek3y000\", workload = \"agent:{id}\", name = \"main\", as = {{ kind = \"variable\" }} }}]\n",
        read(&store)
    );
    link_files::write_private(&store, kept.as_bytes()).unwrap();
    // Sandboxed or not, the harness gets the placeholder: the proxy
    // holds the value.
    ok(&world.start("main", &["start"]));
    let env = read(
        &link_files::layout::Hidden::AgentData
            .in_home(&world.home)
            .join("main/harnesses/teller/env.txt"),
    );
    assert!(!env.contains("sk_live_granted"), "{env}");
    let placeholder = env
        .lines()
        .find_map(|l| l.strip_prefix("STRIPE_KEY="))
        .unwrap_or_else(|| panic!("no STRIPE_KEY in {env}"));
    assert!(
        placeholder.contains(link_sandbox_route::PLACEHOLDER),
        "{env}"
    );
    assert!(
        env.lines().any(
            |l| l.starts_with("LNK_SEALED_STRIPE_KEY_URL=http://127.0.0.1:")
                && l.ends_with("/_link/secret/stripe-key")
        ),
        "{env}"
    );
    // A plain one, which its grant reads, is never given to an agent.
    let plain = kept
        .replace("sealed = { host = \"api.stripe.com\" }\n", "")
        .replace("secret.secret.pass", "secret.secret.read");
    link_files::write_private(&store, plain.as_bytes()).unwrap();
    let out = world.start("main", &["start"]);
    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains(
            "STRIPE_KEY is granted to main plain, and an agent is given a secret sealed only"
        ),
        "{said}"
    );
}

/// A harness this machine shares between an owner's agents
/// (`shared_harness`), which serves each through its own proxy.
const SHARER: &str = r#"#!/bin/sh
case "$1" in
  contract) printf '{"version":1,"proxy":true,"folders":true,"shared":true}' ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  *) exit 0 ;;
esac
"#;

/// An agent with a sealed secret granted doesn't start in a harness it
/// shares with your other agents: the placeholder would be in a process
/// they share. It says so, naming the secret.
#[test]
fn a_sealed_secret_is_refused_in_a_shared_harness() {
    let world = World::new("shared");
    // No sandbox here (its gate said so), so no harness is shared.
    if !world.sandboxed {
        return;
    }
    link_test_kit::write_program(&world.bin.join("lnk-sharer"), SHARER).unwrap();
    ok(&world.start("main", &["create", "main", "--harness", "sharer"]));
    crate::common::add_policy(&world.home, "shared_harness = true\n");
    let identity = crate::common::record(&world.home, "main", "identity.toml");
    let (service, id) = (
        identity["service"].as_str().unwrap(),
        identity["id"].as_str().unwrap(),
    );
    let store = world.home.join(".lnk/safe/secrets/secrets.toml");
    let kept = format!(
        "{}\n[entry.str1pek3y000]\nkind = \"named\"\nname = \"STRIPE_KEY\"\nvalue = \"sk_live_granted\"\n\
         sealed = {{ host = \"api.stripe.com\" }}\n\
         to = [{{ principal = \"service:{service}\", action = \"secret.secret.pass\", resource = \"secret:str1pek3y000\", workload = \"agent:{id}\", name = \"main\", as = {{ kind = \"variable\" }} }}]\n",
        read(&store)
    );
    link_files::write_private(&store, kept.as_bytes()).unwrap();
    let out = world.start("main", &["start"]);
    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("STRIPE_KEY is granted to main sealed, which its own proxy adds, and its harness is shared with your other agents here"),
        "{said}"
    );
    assert!(!said.contains("sk_live_granted"), "{said}");
}

/// With keys on the wire, a harness speaking Telegram is handed a
/// placeholder and its proxy's address, by the rule the Telegram module
/// gives (`lnk-telegram rule --json`); the token stays outside.
#[test]
fn a_wired_harness_gets_its_connections_placeholder() {
    let world = World::new("wired");
    // No sandbox here (its gate said so), so no proxy to add the token.
    if !world.sandboxed {
        return;
    }
    common::write_adapter(&world.bin.join("lnk-told"), TOLD).unwrap();
    ok(&world.start("main", &["create", "main", "--harness", "told"]));
    let settings = world.home.join("Link/Safe/Agents/main/agent.toml");
    let token = concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw");
    crate::common::write_agent(
        &world.home,
        "main",
        &format!(
            "wire_keys = true\n{}{}",
            read(&settings),
            crate::common::connection(
                "telegram",
                &["bot_token"],
                &format!("bot_token = \"{token}\"\nallowed_users = [42]"),
                "id = \"123456\""
            )
        ),
    );
    ok(&world.start("main", &["start"]));
    let told = read(
        &link_files::layout::Hidden::AgentData
            .in_home(&world.home)
            .join("main/harnesses/told/told.json"),
    );
    let told: serde_json::Value = serde_json::from_str(&told).unwrap();
    let telegram = &told["telegram"];
    // A placeholder of this start's own, never one a hash of the token
    // would make.
    let placeholder = telegram["bot_token"].as_str().unwrap();
    assert!(placeholder.starts_with("123456:link-wire-"), "{told}");
    link_sandbox_route::check_placeholder(&placeholder.to_string().into(), "123456:").unwrap();
    assert_eq!(
        telegram["api_root"],
        link_secret_contract::route::url(link_sandbox_client::PROXY_PORT, "telegram")
    );
    assert!(!told.to_string().contains("AAHdqTcv"), "{told}");
    let check = ok(&world.lnk(&["harness", "check", "--harness", "told"]));
    assert!(
        check.contains("blocked  its telegram secret, which its proxy adds")
            && check.contains("added by its proxy, not in its home"),
        "{check}"
    );
    // Nor what its placeholders are made with, nor where it's kept.
    assert!(
        check.contains("blocked  what its placeholders are made with"),
        "{check}"
    );
}

/// The ID in agent `name`'s settings, without its kind.
fn id_of(world: &World, name: &str) -> String {
    field_of(world, name, "id", "agent:")
}

/// The service agent `name`'s settings say it acts as, without its kind.
fn service_of(world: &World, name: &str) -> String {
    field_of(world, name, "service", "service:")
}

fn field_of(world: &World, name: &str, field: &str, kind: &str) -> String {
    let identity = crate::common::record(&world.home, name, "identity.toml");
    let id = identity[field].as_str().unwrap();
    assert!(!id.starts_with(kind), "kept bare: {id}");
    id.to_string()
}

/// A delete whose interaction store can't be written stops before the
/// agent's settings or files go, so the same delete runs again once the
/// store can be.
#[test]
fn a_delete_that_cant_forget_its_reader_leaves_the_agent_to_delete_again() {
    let world = World::new("delete-again");
    ok(&world.start_on_owned_port("work", &["create", "work", "--harness", "fake"]));
    common::owner_reader(&world.home, "work");
    let state = link_files::layout::Hidden::Approvals
        .in_home(&world.home)
        .join("interactions.json");
    let saved = std::fs::read(&state).unwrap();
    std::fs::write(&state, b"{").unwrap();
    let said = failed(&world.lnk(&["delete", "work", "--yes"]));
    assert!(said.contains("interaction store"), "{said}");
    let config = world.home.join("Link/Safe/Agents");
    assert!(config.join("work/agent.toml").is_file());
    assert!(world.agent("work").exists());
    std::fs::write(&state, saved).unwrap();
    ok(&world.lnk(&["delete", "work", "--yes"]));
    assert!(!config.join("work").exists());
    assert!(!common::owner_reads(&world.home, "work"));
    let left = link_test_kit::left_under(&world._root);
    assert!(left.is_empty(), "{left:?}");
}

/// `lnk agent delete`: one agent goes, after asking, its files put aside
/// unless asked to go too, and the other agent stays as it was. Neither
/// is main.
#[test]
fn removing_an_agent_takes_only_it() {
    let world = World::new("delete");
    ok(&world.start_on_owned_port("keep", &["create", "keep", "--harness", "fake"]));
    ok(&world.start_on_owned_port("work", &["create", "work", "--harness", "fake"]));
    std::fs::write(world.agent("work").join("home/notes.md"), "WORK'S").unwrap();
    let config = world.home.join("Link/Safe/Agents");
    assert!(config.join("work/agent.toml").is_file());
    common::owner_reader(&world.home, "work");
    common::owner_reader(&world.home, "keep");

    // Without a terminal and without --yes: asked, so nothing goes.
    let asked = failed(&world.lnk(&["delete", "work"]));
    assert!(asked.contains("--yes"), "{asked}");
    assert!(config.join("work/agent.toml").is_file());
    assert!(common::owner_reads(&world.home, "work"));
    assert!(failed(&world.lnk(&["delete", "nobody", "--yes"])).contains("no agent named nobody"));

    // --yes: it goes, its files put aside.
    let said = ok(&world.lnk(&["delete", "work", "--yes"]));
    assert!(said.contains("Removed work."), "{said}");
    assert!(!config.join("work/agent.toml").exists());
    assert!(!config.join("work").exists());
    assert!(!world.agent("work").exists());
    // Its owner delivery never reads the outbox again: nothing waits for
    // it there. The other agent's still reads.
    assert!(!common::owner_reads(&world.home, "work"));
    assert!(common::owner_reads(&world.home, "keep"));
    let archives = || -> Vec<PathBuf> {
        std::fs::read_dir(world.home.join("Link/Archive"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|path| path.is_file())
            .collect()
    };
    let kept = archives();
    assert_eq!(kept.len(), 1, "{kept:?}");
    // One archive of its state, made by its deletion.
    let name = kept[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("work-20") && name.ends_with(".tar.gz"),
        "{name}"
    );
    let manifest = link_archive::read_manifest(&kept[0]).unwrap();
    assert!(manifest.deleted.is_some());
    assert!(manifest.things.contains_key("agent.work"));
    let mut inside = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::GzDecoder::new(std::fs::File::open(&kept[0]).unwrap()),
        &mut inside,
    )
    .unwrap();
    let inside = String::from_utf8_lossy(&inside);
    assert!(inside.contains("Agents/work/home/notes.md") && inside.contains("WORK'S"));
    assert!(inside.contains("Safe/Agents/work/agent.toml"));
    assert!(said.contains("~/Link/Archive/work-"), "{said}");

    // keep is untouched, and the only agent now: no -a needed.
    assert!(
        link_files::layout::Hidden::AgentData
            .in_home(&world.home)
            .join("keep/harnesses/fake")
            .is_dir()
    );
    let status = world.lnk(&["show", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["name"], "keep");

    // --files: its files go too.
    ok(&world.lnk(&["delete", "keep", "--yes", "--delete-files"]));
    assert!(!world.agent("keep").exists());
    assert!(!config.join("keep/agent.toml").exists());
    assert_eq!(archives().len(), 1);
    // Nothing of Link's started for either outlives its removal (K8).
    let left = link_test_kit::left_under(&world._root);
    assert!(left.is_empty(), "{left:?}");
}

/// A fake systemd where every unit runs: `is-active` says so for each,
/// `show` gives each its block, its main process numbered by its place,
/// past the most a process can be numbered (2^22), so no process of the
/// machine's is ever taken for one; each call kept in `systemctl.log`
/// beside it.
const SYSTEMCTL: &str = r#"#!/bin/sh
here=$(dirname "$0")
echo "$*" >> "$here/systemctl.log"
case "$*" in
  *is-active*) shift 2; for unit in "$@"; do echo active; done ;;
  *show*)
    shift 4; n=4194400
    for unit in "$@"; do
      [ $n = 4194400 ] || echo
      case "$unit" in lnk-bridge-*) pid=0 ;; *) pid=$n ;; esac
      printf 'Id=%s\nActiveState=active\nMainPID=%s\nNRestarts=0\n' "$unit" "$pid"
      n=$((n+1))
    done ;;
  *) exit 0 ;;
esac
"#;

/// Running agents are listed with the processes each runs as, every
/// agent's service and its connections' asked in one `systemctl show`, not
/// one each.
#[test]
fn running_agents_are_listed_from_one_show() {
    let world = World::new("show");
    link_test_kit::write_program(&world.bin.join("systemctl"), SYSTEMCTL).unwrap();
    for name in ["a1", "a2", "a3"] {
        let files = world.agent(name).join("home").display().to_string();
        ok(&world.lnk(&[
            "create",
            name,
            "--harness",
            "fake",
            "--no-sandbox",
            "--run.files",
            &files,
        ]));
    }
    let log = world.bin.join("systemctl.log");
    std::fs::write(&log, "").unwrap();
    let list = ok(&world.lnk(&["list", "--json", "--here"]));
    let list: serde_json::Value = serde_json::from_str(&list).unwrap();
    let pids: Vec<&serde_json::Value> = list["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| &a["processes"]["pids"])
        .collect();
    // Each its own, its connections' service running none.
    assert_eq!(
        pids,
        [
            &serde_json::json!([4194400]),
            &serde_json::json!([4194402]),
            &serde_json::json!([4194404])
        ]
    );
    let calls = read(&log);
    assert_eq!(
        calls.lines().filter(|l| l.contains("show")).count(),
        1,
        "{calls}"
    );
    assert_eq!(calls.lines().count(), 2, "{calls}");
}

/// A harness whose program is in `~/.lnk/exec/bin`, where cargo and the
/// install script put it, as `lnk-harness` is: it reads its own
/// program, by the path it was run from, as Link Harness does, and runs.
/// With the sandbox available, one asking for the folder its program is
/// in, which holds whatever else the user keeps there, is refused.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_harness_installed_in_the_home_folder_runs() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let world = World::new("home-bin");
    if !world.sandboxed {
        return;
    }
    let bin = world.home.join(".lnk/exec/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let adapter = |name: &str, read: &str| {
        crate::common::write_adapter(
            &bin.join(format!("lnk-{name}")),
            format!(
                "#!/bin/sh\ncase \"$1\" in\n  command) printf '{{\"argv\":[\"/bin/sh\",\"-c\",\"test -r %s && echo yes > \\\\\"$LNK_FILES/read\\\\\"\"],\"read\":[\"%s\"]}}' \"$0\" {read} ;;\n  *) exit 0 ;;\nesac\n"
            ),
        )
        .unwrap();
    };
    adapter("own", "\"$0\"");
    adapter("greedy", "\"$(dirname \"$0\")\"");
    let lnk = |name: &str, args: &[&str]| {
        let files = world.agent(name).join("home").display().to_string();
        let mut args = args.to_vec();
        args.extend(["--foreground", "--run.files", &files]);
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-agent"));
        command
            .args(&args)
            .env_clear()
            .env("HOME", &world.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&world.home),
            )
            .env(
                "PATH",
                format!("{}:{}:/usr/bin:/bin", bin.display(), world.bin.display()),
            )
            .env("LNK_EXE", &world.core)
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
            .stdin(Stdio::null());
        command.output().unwrap()
    };
    ok(&lnk("main", &["create", "main", "--harness", "own"]));
    assert_eq!(read(&world.agent("main").join("home/read")), "yes\n");

    // World reported the missing capability before choosing --no-sandbox.
    // The launch above remains covered; the native grant refusal needs it.
    if !world.sandboxed {
        return;
    }
    let err = failed(&lnk("other", &["create", "other", "--harness", "greedy"]));
    assert!(err.contains("is, holds or is in"), "{err}");
}

mod first_start;
mod flags;
mod ids;
mod ports;
mod services;
