//! `lnk agent show`, `use` and `log show`: the agent's state, and
//! whether its harness and model answer.

use std::time::Duration;

use link_agent::{
    adapter::{Adapter, Boxed},
    service,
    settings::{self, Settings},
};
use link_format::{
    Address, Count, Done, Elapsed, Fields, Foreign, Name, Out, Path, Product, Report, Table, Text,
    Value, WithinExt, cmd, prose, sentence, states,
};

use crate::confine::team_tools;
use crate::model::{provider_name, small_model};
use crate::permissions::{beyond_defaults, granted};
use crate::{run_file, scheduler, screens};

states! {
    /// An agent's state, as `show` and `list` say it.
    pub(crate) enum AgentState {
        Asleep("asleep") => Good,
        Running("running") => Good,
        RunningInTerminal("running in a terminal") => Good,
        Stopped("stopped") => Warn,
        NotAnswering("not answering") => Bad,
    }
}

/// The agent's state, one block: what `lnk agent show` shows.
pub(crate) fn fields(
    out: &mut Out,
    settings: &Settings,
    machine: Option<&str>,
    state: AgentState,
) -> Result<Fields, Report> {
    let mut fields = Fields::new()
        .key("Agent", Name::new("agent", &settings.name))
        .row("State", state);
    fields = model_rows(out, fields, settings)?;
    fields = tools_row(fields, settings);
    fields = phone_rows(fields, settings);
    fields = fields
        .row("Harness", Name::new("harness", settings.harness()))
        .row("Kept in", Path::new(settings.box_of(settings.harness())))
        .row("Files", Path::new(settings.files()));
    fields = machine_row(fields, machine);
    Ok(sandbox_rows(fields, settings, |fields| {
        fields
            .row(
                "Sandbox file",
                Path::new(link_agent::sandbox_file::path(&settings.name)),
            )
            .row(
                "Saved grants",
                Foreign::new(&link_agent::sandbox_file::summary(
                    &link_agent::sandbox_file::of(&settings.name),
                )),
            )
    }))
}

/// What the agent's service says of it: its state, how long it's been
/// up and how often it restarted.
pub(crate) struct Seen {
    pub(crate) state: AgentState,
    up: Option<Duration>,
    restarts: Option<u64>,
}

/// One look at the agent's service: running, its process, its restarts.
pub(crate) fn seen(settings: &Settings) -> Result<Seen, Report> {
    if settings.model.is_none() && settings.id.is_none() {
        return Err(Report::error(sentence!("There is no agent yet."))
            .next(cmd!("lnk agent start"), "set one up"));
    }
    let service = service::status(&settings.name);
    let terminal = !service.running && service::foreground_running(&settings.name);
    let running = service.running || terminal;
    let answering = running && answers(settings, settings.harness());
    let asleep = link_agent::idle::asleep(&settings.key());
    let state = match (running, answering) {
        (false, _) if asleep => AgentState::Asleep,
        (false, _) => AgentState::Stopped,
        (true, true) if terminal => AgentState::RunningInTerminal,
        (true, true) => AgentState::Running,
        (true, false) => AgentState::NotAnswering,
    };
    let up = match state {
        AgentState::Running => service.pid.and_then(service::uptime),
        _ => None,
    };
    let restarts = service
        .runs
        .filter(|runs| running && *runs > 1)
        .map(|runs| u64::from(runs) - 1);
    Ok(Seen {
        state,
        up,
        restarts,
    })
}

/// What follows the agent's fields: why it's asleep, a small model, and
/// the report its state would give, with what to do next.
pub(crate) fn said(
    out: &mut Out,
    settings: &Settings,
    state: AgentState,
) -> (Done, Option<Report>) {
    let agent = Name::new("agent", &settings.name);
    if state == AgentState::Asleep {
        out.block(Text(prose!(
            "It stopped for being idle, and its next message starts it."
        )));
    }
    if let Some(small) = small_model_warning(settings) {
        out.block(small);
    }
    let mut report = match state {
        AgentState::Asleep | AgentState::Running | AgentState::RunningInTerminal => None,
        AgentState::Stopped => Some(
            Report::error(sentence!("{} is stopped.", agent))
                .next(cmd!("lnk agent start"), "start it"),
        ),
        AgentState::NotAnswering => Some(
            Report::error(sentence!("{} is running, but not answering.", agent))
                .next(cmd!("lnk agent log show"), "see why"),
        ),
    };
    let mut done = Done::shown();
    if !settings.sandbox {
        let back = cmd!("lnk agent start --sandbox");
        done = done.next(back.clone(), "turn the sandbox back on");
        report = report.map(|r| r.next(back, "turn the sandbox back on"));
    }
    (
        match report {
            Some(_) => done,
            None => done.next(cmd!("lnk agent log show"), "see what it's doing"),
        },
        report,
    )
}

/// Its name, state, and while it runs how long and how often restarted.
pub(crate) fn state_rows(fields: Fields, settings: &Settings, seen: &Seen) -> Fields {
    let mut fields = fields
        .key("Agent", Name::new("agent", &settings.name))
        .row("State", seen.state);
    if let Some(up) = seen.up {
        fields = fields.row("Up", Elapsed(up));
    }
    if let Some(restarts) = seen.restarts {
        fields = fields.row("Restarts", Count::new(restarts, "time", "times"));
    }
    fields
}

/// The machine it runs on: `machine`, as the computer asking says, or
/// this one.
pub(crate) fn machine_row(fields: Fields, machine: Option<&str>) -> Fields {
    fields.row(
        "Machine",
        match machine {
            Some(machine) => Value::from(machine),
            None => Value::from("this computer"),
        },
    )
}

/// Its model, where it runs or who serves it, and its fallback; a local
/// runtime that isn't answering is warned of.
pub(crate) fn model_rows(
    out: &mut Out,
    mut fields: Fields,
    settings: &Settings,
) -> Result<Fields, Report> {
    let needs_default = [settings.model.as_ref(), settings.fallback.as_ref()]
        .into_iter()
        .flatten()
        .any(|model| model.name.is_none());
    let spec = needs_default.then(|| settings.spec()).transpose()?;
    let model = |model: &link_harness_contract::harness::Model| match &model.name {
        Some(id) => Value::from(id.as_str()),
        None => Value::from(
            spec.as_ref()
                .and_then(|spec| Settings::model_name(model, spec))
                .unwrap_or_else(|| "its default model".into()),
        ),
    };
    match &settings.model {
        None => fields = fields.row("Model", "none"),
        Some(m) if m.is_local() => {
            let runtime = if m.provider == "ollama" {
                "Ollama"
            } else {
                "a runtime"
            };
            fields = fields.row("Model", model(m));
            fields = fields.row(
                "Runs on",
                Value::List(vec![
                    Value::from(format!("{runtime} here")),
                    Address::new(m.base_url.as_deref().unwrap_or_default()).into(),
                ]),
            );
            if let Some(port) = settings.model_port()
                && !port_answers(port)
            {
                let mut down =
                    Report::warning(sentence!("{} isn't answering.", Product::new(runtime)));
                if m.provider == "ollama" {
                    down = down.next(cmd!("ollama serve"), "start Ollama");
                }
                out.block(down);
            }
        }
        Some(m) => {
            fields = fields.row("Model", model(m));
            fields = fields.row("From", Product::new(provider_name(&m.provider)));
        }
    }
    // Its fallback, under its model.
    if let Some(fallback) = &settings.fallback {
        fields = fields.row(
            "Fallback",
            Value::List(vec![
                model(fallback),
                Product::new(provider_name(&fallback.provider)).into(),
            ]),
        );
    }
    Ok(fields)
}

/// The tools it has.
pub(crate) fn tools_row(fields: Fields, settings: &Settings) -> Fields {
    let tools: Vec<Value> = tool_names(settings, settings.harness())
        .into_iter()
        .map(Value::from)
        .collect();
    fields.row(
        "Tools",
        match tools.is_empty() {
            true => Value::from("none yet"),
            false => Value::List(tools),
        },
    )
}

/// Its connections its harness speaks, and those kept while it doesn't.
pub(crate) fn phone_rows(mut fields: Fields, settings: &Settings) -> Fields {
    let name = settings.harness();
    // Without its adapter, nothing is known to be off. Its adapter is
    // asked only with a connection to place, and for an endpoint only when
    // its harness doesn't speak one itself.
    let connections = settings.connections();
    let adapter = match connections.is_empty() {
        true => None,
        false => Adapter::find(name).ok(),
    };
    let (on, off): (Vec<_>, Vec<_>) = connections.into_iter().partition(|c| {
        adapter.as_ref().is_none_or(|adapter| {
            let own = adapter.connections();
            own.contains(c)
                || link_agent::adapter::Speaking {
                    own,
                    endpoint: adapter.endpoint(settings.port).is_some(),
                }
                .speaks(settings, c)
        })
    });
    let titled = |connections: &[String]| {
        Value::List(
            connections
                .iter()
                .map(|c| Value::from(settings.title(c)))
                .collect(),
        )
    };
    fields = fields.row(
        "Phone",
        match on.is_empty() {
            true => Value::from("not connected"),
            false => titled(&on),
        },
    );
    if !off.is_empty() {
        fields = fields.row("Kept", titled(&off));
    }
    fields
}

/// Whether its sandbox is on, `grants`' rows, what's beyond Link's
/// defaults, its network and, on a Mac, whether it keeps it awake.
pub(crate) fn sandbox_rows(
    mut fields: Fields,
    settings: &Settings,
    grants: impl FnOnce(Fields) -> Fields,
) -> Fields {
    let name = settings.harness();
    let granted = granted(settings, name);
    let beyond = beyond_defaults(settings, name);
    fields = fields.row(
        "Sandbox",
        match settings.sandbox {
            true => SandboxState::On,
            false => SandboxState::Off,
        },
    );
    fields = grants(fields);
    if settings.sandbox && !beyond.is_empty() {
        fields = fields.row(
            "Beyond defaults",
            Value::List(beyond.into_iter().map(Value::from).collect()),
        );
    }
    // Sandboxed with no `[internet]`: behind its proxy all the same, which
    // reaches no host.
    let offline = settings.sandbox
        && settings
            .spec_of(name, link_agent_contract::agent_sandbox::Route::Direct)
            .internet
            .is_none();
    let hosts = settings.hosts_of(name);
    let only = match hosts.is_empty() {
        true => String::new(),
        false => format!(", to {} only", hosts.join(", ")),
    };
    let route = match settings.proxy_of(name) {
        Some(_) if offline => "none (no --internet.hosts)".to_string(),
        Some(_) if !settings.sandbox => {
            "this machine's, all of it: its proxy beside it is advice no sandbox holds it to"
                .to_string()
        }
        Some(upstream) if upstream == "direct" => {
            format!("through its proxy, from this machine{only}")
        }
        Some(upstream) => {
            format!("only through its proxy, to {upstream}{only}")
        }
        None if !settings.sandbox => "this machine's, all of it".to_string(),
        None if granted.iter().any(|g| g == "--network.mode host") => {
            "this machine's as it is, its local network too (--network.mode host)".to_string()
        }
        None => "none (no --internet.hosts)".to_string(),
    };
    fields = fields.row("Network", route);
    // Where no notification can say so (a box): its wire closed.
    if settings.proxy_of(name).is_some()
        && let Some(why) =
            link_sandbox_client::wires().and_then(|w| w.closed(&settings.profile(name)))
    {
        fields = fields.row("Wire closed", Foreign::new(&why));
    }
    if cfg!(target_os = "macos") {
        fields = fields.row(
            "Sleep",
            match settings.awake {
                true => "keeps this Mac awake while it runs",
                false => "lets this Mac sleep",
            },
        );
    }
    fields
}

states! {
    /// Whether an agent's sandbox is on.
    pub(crate) enum SandboxState {
        On("on") => Quiet,
        Off("off") => Warn,
    }
}

/// A model too small to call its tools, said as a warning.
pub(crate) fn small_model_warning(settings: &Settings) -> Option<Report> {
    let model = settings.model.as_ref().filter(|m| small_model(m))?;
    let id = model.name.as_deref().unwrap_or_default();
    Some(
        Report::warning(sentence!("{} is a small model.", Name::new("model", id)))
            .why(prose!(
                "It may not call its tools, or not use what it's told."
            ))
            .next(
                cmd!("lnk agent start --model.default claude-sonnet-5-5"),
                "use a hosted one, which thinks better",
            ),
    )
}

/// The tools agent `settings` has on harness `name`, as `lnk agent tool`
/// names them: Link's own (memory, scheduler, run, file, code), each tool its
/// sandbox file lets it call, and its teams' memories.
pub(crate) fn tool_names(settings: &Settings, name: &str) -> Vec<String> {
    let mut names: Vec<String> = [
        settings.memory_tool().map(|_| "memory"),
        scheduler::tool(settings).map(|_| "scheduler"),
        run_file::tool(settings, "run").map(|_| "run"),
        run_file::tool(settings, "file").map(|_| "file"),
        run_file::tool(settings, "code").map(|_| "code"),
    ]
    .into_iter()
    .flatten()
    .map(String::from)
    .collect();
    for tool in settings.tools_of(name) {
        if !names.contains(&tool) {
            names.push(tool);
        }
    }
    names.extend(
        screens::handed(settings)
            .into_iter()
            .map(|(handed, _)| handed),
    );
    names.extend(team_tools(settings).into_iter().map(|(handed, _)| handed));
    names
}

/// Whether harness `name` answers its health check, in its sandbox.
pub(crate) fn answers(settings: &Settings, name: &str) -> bool {
    let Ok(adapter) = Adapter::find(name) else {
        return false;
    };
    let Ok(mut boxed) = Boxed::new(settings, name) else {
        return false;
    };
    boxed.ports.extend(settings.model_port());
    // Its ports as its start saved them; else as its adapter says.
    let ports = match settings::ports_of(&settings.name, name) {
        Some(ports) => ports,
        None => adapter
            .command(&boxed)
            .map(|launch| launch.ports)
            .unwrap_or_default(),
    };
    // Nothing listening on any: not running, known without starting a
    // sandbox (and a proxy) for its health check. Something listening
    // may be its proxy alone, which closes without an answer, or a
    // harness that answers but isn't well (a 503 without its key): one
    // that answers a request with success is the harness, up, known
    // without the check too. Any other answer is its health check's.
    if !ports.is_empty() && !ports.iter().any(|p| port_answers(*p)) {
        return false;
    }
    if ports_succeed(&ports) {
        return true;
    }
    boxed.ports.extend(ports);
    adapter.healthy(&boxed)
}

/// Whether something answers a request on one of `ports` of this
/// machine's loopback: a sign the harness may be up, which only its
/// health check confirms. A port its proxy holds while the harness isn't
/// listening closes without an answer.
pub(crate) fn ports_speak(ports: &[u16]) -> bool {
    ports.iter().any(|port| !reply(*port, 1).is_empty())
}

/// Whether one of `ports` of this machine's loopback answers a request
/// with success (a 2xx status line): the harness, up and well, known
/// without its health check.
pub(crate) fn ports_succeed(ports: &[u16]) -> bool {
    ports.iter().any(|port| succeeded(&reply(*port, 16)))
}

/// Whether `reply` starts with an HTTP status line saying success:
/// `HTTP/1.1 200`.
pub(crate) fn succeeded(reply: &[u8]) -> bool {
    let Some(code) = reply.strip_prefix(b"HTTP/").and_then(|rest| {
        rest.iter()
            .position(|b| *b == b' ')
            .map(|at| &rest[at + 1..])
    }) else {
        return false;
    };
    code.len() >= 3 && code[0] == b'2' && code[..3].iter().all(u8::is_ascii_digit)
}

/// The first bytes, up to `most`, of what `port` on this machine's
/// loopback answers to `GET /`: none if nothing does in time.
pub(crate) fn reply(port: u16, most: usize) -> Vec<u8> {
    use std::io::{Read, Write};
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(100),
    ) else {
        return Vec::new();
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    let request = format!("GET / HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return Vec::new();
    }
    let mut got = vec![0u8; most];
    let mut len = 0;
    while len < most {
        match stream.read(&mut got[len..]) {
            Ok(n) if n > 0 => len += n,
            _ => break,
        }
    }
    got.truncate(len);
    got
}

pub(crate) fn port_answers(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(500),
    )
    .is_ok()
}

/// `lnk agent harness set` alone: the harnesses, and which this agent runs.
pub(crate) fn list(out: &mut Out, settings: &Settings) -> Result<Done, Report> {
    let state = harness_state(&settings.name);
    let mut table = Table::new(
        ["Now", "Harness", "About", "State"],
        sentence!("No harnesses."),
    );
    let mut harnesses = settings::harnesses();
    if !harnesses.iter().any(|(name, _)| name == settings.harness()) {
        harnesses.push((settings.harness().to_string(), String::new()));
    }
    for (name, about) in &harnesses {
        let current = name == settings.harness();
        table.row([
            match current {
                true => Value::Mark(link_format::look::Mark::Success),
                false => Value::from(""),
            },
            Name::new("harness", name).into(),
            Value::from(link_format::clean(about)),
            match current {
                true => state.into(),
                false => Value::from(""),
            },
        ]);
    }
    out.block(table);
    out.block(Text(prose!(
        "Known harnesses to add: {}.",
        settings::MORE_HARNESSES.to_string()
    )));
    Ok(harness_choice_navigation())
}

fn harness_choice_navigation() -> Done {
    Done::shown()
        .next(
            cmd!("lnk agent harness set <harness>"),
            "switch, and your settings and files come along",
        )
        .next(
            cmd!("lnk module add --url <its URL>"),
            "add another harness",
        )
}

/// The state of agent `agent`'s harness in `lnk agent harness set`'s list, as
/// `show` says it: its service's, or the terminal it runs in.
fn harness_state(agent: &str) -> AgentState {
    match service::running(agent) {
        true => AgentState::Running,
        false if service::foreground_running(agent) => AgentState::RunningInTerminal,
        false => AgentState::Stopped,
    }
}

/// `lnk agent log show`: the last start's lines (at most the last 200), each
/// with its local time, its level's mark and its IDs, the same line in a
/// row shown once with how many times; with `--json`, each record.
#[expect(clippy::disallowed_methods, reason = "follows a log as it grows")]
pub(crate) fn logs(out: &mut Out, settings: &Settings, follow: bool) -> Result<Done, Report> {
    use link_format::log::{Entry, Split};
    use std::io::{Read, Seek, SeekFrom};

    let path = settings.log_path(settings.harness());
    let Ok(mut file) = std::fs::File::open(&path) else {
        return Err(Report::error(sentence!("There is no log yet."))
            .next(cmd!("lnk agent start"), "start your agent"));
    };
    let own = settings.agent_id();
    let len = file.metadata().within("reading its log")?.len();
    let start = std::fs::read_to_string(service::start_path(&path))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|start| *start <= len)
        .unwrap_or(0);
    // At most the last 4 MiB, then the last 200 lines of those.
    let from = start.max(len.saturating_sub(4 << 20));
    file.seek(SeekFrom::Start(from)).within("reading its log")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(len - from)
        .read_to_end(&mut bytes)
        .within("reading its log")?;
    let mut split = Split::default();
    let mut lines = split.push(&bytes);
    lines.extend(split.rest());
    let entries: Vec<Entry> = lines[lines.len().saturating_sub(200)..]
        .iter()
        .map(|line| Entry::read(line))
        .collect();
    let empty = entries.is_empty();
    show(out, entries, own);
    // Nothing written since its last start: said, not an empty screen.
    if empty {
        out.block(Text(prose!(
            "Nothing in its log yet since its last start, in {}.",
            Path::new(&path)
        )));
    }
    if !follow {
        return Ok(Done::shown());
    }
    // New lines as they come; a partial line waits for its end. Each
    // read takes at most 4 MiB, and a burst is read on without waiting.
    let mut pos = len;
    let mut split = Split::default();
    let mut behind = false;
    loop {
        if !behind {
            std::thread::sleep(Duration::from_millis(500));
        }
        let now = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if now < pos {
            pos = 0; // started again, or cleared
            split = Split::default();
        }
        behind = false;
        if now == pos {
            continue;
        }
        let mut file = std::fs::File::open(&path).within("reading its log")?;
        file.seek(SeekFrom::Start(pos)).within("reading its log")?;
        let mut bytes = Vec::new();
        let read = (&mut file)
            .take((now - pos).min(4 << 20))
            .read_to_end(&mut bytes)
            .within("reading its log")?;
        pos += read as u64;
        behind = read > 0 && pos < now;
        let entries = split
            .push(&bytes)
            .iter()
            .map(|line| Entry::read(line))
            .collect();
        show(out, entries, own);
    }
}

/// `entries` of agent `own`'s log shown: each run of the same line once,
/// with how many times, for a person; every one with `--json`.
fn show(out: &mut Out, entries: Vec<link_format::log::Entry>, own: Option<&str>) {
    let shown = match out.is_json() {
        true => entries
            .into_iter()
            .map(|entry| link_format::log::Shown { entry, times: 1 })
            .collect(),
        false => link_format::log::collapse(entries),
    };
    for line in &shown {
        out.live().log(line, own);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_and_context_name_the_effective_model_without_pinning_the_saved_default() {
        let _config = crate::tests::own_config();
        let mut agent = Settings::new("effective-model-status");
        assert!(
            !agent.path().exists(),
            "this fixture uses only its in-memory behavior"
        );
        agent.behavior = Some(link_agent_spec::parse(
            "schema = 1\n[model]\ndefault = 'openai/gpt-6.1-sol'\n[model.anthropic]\ndefault = 'claude-sonnet-5-5'\n"
        ).unwrap());
        agent.wire_keys = Some(false);
        for (saved, expected) in [
            (Some("claude-opus-5-5"), "claude-opus-5-5"),
            (None, "claude-sonnet-5-5"),
        ] {
            agent.model = Some(link_harness_contract::harness::Model {
                provider: "anthropic".into(),
                name: saved.map(str::to_owned),
                ..Default::default()
            });
            let before = serde_json::to_value(&agent).unwrap();
            let handed = agent.for_harness(&Default::default()).unwrap();
            assert_eq!(handed.model.name.as_deref(), Some(expected));
            assert!(handed.fallback.is_none());
            assert_eq!(
                agent.for_needs().unwrap().model.name.as_deref(),
                Some(expected)
            );
            let (mut out, _) = Out::capture(false, &["lnk", "agent", "show"]);
            let fields = model_rows(&mut out, Fields::new(), &agent).unwrap();
            assert_eq!(fields.rows[0].value, Value::from(expected));
            assert_eq!(
                crate::context_mcp::shown(&agent, false)
                    .unwrap()
                    .model
                    .as_deref(),
                Some(expected)
            );
            assert_eq!(serde_json::to_value(&agent).unwrap(), before);
            assert_eq!(agent.model.as_ref().unwrap().name.as_deref(), saved);
        }
        // Explicit IDs remain inspectable without resolving unrelated defaults.
        agent.model.as_mut().unwrap().name = Some("claude-opus-5-5".into());
        agent.behavior = Some(link_agent_spec::parse("schema = 1\nextends = 'none'\n").unwrap());
        assert!(
            agent.spec().is_err(),
            "this opinion set is deliberately incomplete"
        );
        let (mut out, _) = Out::capture(false, &["lnk", "agent", "show"]);
        assert_eq!(
            model_rows(&mut out, Fields::new(), &agent).unwrap().rows[0].value,
            Value::from("claude-opus-5-5")
        );
        assert_eq!(
            agent.for_needs().unwrap().model.name.as_deref(),
            Some("claude-opus-5-5")
        );
    }

    #[test]
    fn harness_choice_navigation_is_declared() {
        let actions: toml::Value = crate::navigation::navigation();
        let done = harness_choice_navigation();
        for action in ["agent harness list", "agent harness set"] {
            let declared: Vec<String> = actions[action]["next"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect();
            let typed = link_format::Command::fill(&format!("lnk {action}"), vec![]);
            for next in &done.next.0 {
                assert!(
                    link_format::nexts::declares(&declared, &next.command, &typed),
                    "{action} offers undeclared harness navigation: {}",
                    next.command.text()
                );
            }
        }
    }

    /// Every state `lnk agent show` reports offers only the Next its
    /// declaration holds: a debug build panics on any other.
    #[test]
    fn each_states_next_is_declared() {
        let _config = crate::tests::own_config();
        let actions: toml::Value = crate::navigation::navigation();
        let declared = |key: &str| -> Vec<String> {
            actions["agent show"][key]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect()
        };
        let typed = link_format::Command::fill("lnk agent show", vec![]);
        let mut agent = Settings::new("each-state-next");
        for sandbox in [true, false] {
            agent.sandbox = sandbox;
            for state in [
                AgentState::Asleep,
                AgentState::Running,
                AgentState::RunningInTerminal,
                AgentState::Stopped,
                AgentState::NotAnswering,
            ] {
                let (mut out, _) = Out::capture(false, &["lnk", "agent", "show"]);
                let (done, report) = said(&mut out, &agent, state);
                for next in &done.next.0 {
                    assert!(
                        link_format::nexts::declares(&declared("next"), &next.command, &typed),
                        "{state:?} offers undeclared {}",
                        next.command.text()
                    );
                }
                for next in report.iter().flat_map(|report| report.next.0.iter()) {
                    assert!(
                        link_format::nexts::declares(&declared("fail"), &next.command, &typed),
                        "{state:?} fails with undeclared {}",
                        next.command.text()
                    );
                }
            }
        }
    }

    /// One running in a terminal is listed so, not as stopped.
    #[test]
    fn a_harness_running_in_a_terminal_is_listed_running() {
        let _config = crate::tests::own_config();
        let agent = "listed";
        assert_eq!(harness_state(agent), AgentState::Stopped);
        let _running = service::foreground_lock(agent).unwrap();
        assert_eq!(harness_state(agent), AgentState::RunningInTerminal);
    }

    /// `show` takes a harness for up without its health check only when
    /// one of its ports answers with success: a 503 (Link Harness without
    /// its key), or a proxy closing without a word, is its check's to say.
    #[test]
    fn only_a_success_answer_skips_the_health_check() {
        use std::io::{Read, Write};
        let serve = |answer: &'static [u8]| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let mut stream = stream.unwrap();
                    let _ = stream.read(&mut [0u8; 256]);
                    // A byte at a time: the status line read whole.
                    for byte in answer {
                        let _ = stream.write_all(std::slice::from_ref(byte));
                        let _ = stream.flush();
                    }
                }
            });
            port
        };
        let ok = serve(b"HTTP/1.1 200 OK\r\n\r\n");
        let unavailable = serve(b"HTTP/1.1 503 Service Unavailable\r\n\r\n");
        let silent = serve(b"");
        assert!(ports_succeed(&[ok]));
        assert!(ports_succeed(&[silent, ok]));
        assert!(!ports_succeed(&[unavailable]));
        assert!(!ports_succeed(&[silent]));
        assert!(!ports_succeed(&[]));
        // Still a sign it may be up, for a start's waiting.
        assert!(ports_speak(&[unavailable]));
        assert!(!ports_speak(&[silent]));
        for (reply, success) in [
            (&b"HTTP/1.0 204 No Content"[..], true),
            (b"HTTP/1.1 299", true),
            (b"HTTP/1.1 301 Moved", false),
            (b"HTTP/1.1 2", false),
            (b"HTTP/1.1 2xx", false),
            (b"SSH-2.0-OpenSSH", false),
            (b"", false),
        ] {
            assert_eq!(
                succeeded(reply),
                success,
                "{}",
                String::from_utf8_lossy(reply)
            );
        }
    }
}
