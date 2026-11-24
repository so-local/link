//! What a spec may hold: every value checked ([`check`]), table by table,
//! and the size of what it names ([`size`]).

use super::*;
use link_format::WithinExt;

/// A typed duration's milliseconds, saturated.
pub fn millis(duration: &link_actions::value::Duration) -> u64 {
    u64::try_from(duration.get().as_millis()).unwrap_or(u64::MAX)
}

/// An agent spec's `[access]` as the identity contract checks and reads
/// it: its chats, each parsed.
pub fn message_access(
    access: &crate::declared::AgentMessageAccess,
) -> anyhow::Result<crate::access::MessageAccess> {
    serde_json::from_value(serde_json::to_value(access)?).within("[access]")
}

/// Checks `spec`'s values: names, words, sizes and durations a person
/// can read, memory's keys only in memory's table.
pub fn check(spec: &Spec) -> anyhow::Result<()> {
    // Running in no sandbox, or under another's grants, is always the
    // person's explicit choice at a start, never a shared file's: no key of
    // [isolation] says it.
    if let Some(isolation) = &spec.isolation {
        isolation.check_vm()?;
    }
    if let Some(opinions) = &spec.approvals {
        for opinion in opinions.domains.values() {
            if let Some(wait) = opinion.wait_secs {
                ensure!(
                    (1..=crate::agent_spec::keys::INTERACTION_WAIT_MAX).contains(&wait),
                    "interaction wait must be a second to a day"
                );
            }
        }
    }
    let mut names = std::collections::BTreeSet::new();
    for schedule in spec.schedules.iter().flatten() {
        schedule.check()?;
        ensure!(
            names.insert(&schedule.name),
            "two schedules named {:?}: each name is one schedule",
            schedule.name
        );
    }
    if let Some(name) = &spec.name {
        crate::machine::check_name(name)?;
    }
    if let Some(access) = &spec.access {
        crate::access::Access::from(message_access(access)?).only(ACCESS)?;
    }
    if let Some(harness) = &spec.harness {
        ensure!(
            !harness.name.is_empty()
                && harness
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{:?} isn't a harness's name",
            link_format::clean(&harness.name)
        );
    }
    if let Some(model) = &spec.model {
        check_model(model)?;
    }
    if let Some(window) = &spec.window {
        for (key, tokens) in [("max", window.max), ("unknown", window.unknown)] {
            if let Some(tokens) = tokens {
                ensure!(
                    tokens >= 1024,
                    "[window] {key} is {tokens}: a window is at least 1024 tokens"
                );
            }
        }
        if let Some(margin) = window.margin {
            ensure!(
                (0.0..0.5).contains(&margin),
                "[window] margin is a share of the window, 0 to under 0.5, not {margin}"
            );
        }
        if let Some(answer) = window.answer {
            ensure!(
                answer > 0.0 && answer < 1.0,
                "[window] answer is a share of the window, over 0 and under 1, not {answer}"
            );
        }
        if let Some(CompactAt::Number(share)) = window.compact_at {
            ensure!(
                share > 0.0 && share < 1.0,
                "[window] compact_at is a share of the window, over 0 and under 1, or \"never\", not {share}"
            );
        }
        if let (Some(margin), Some(answer)) = (window.margin, window.answer) {
            ensure!(
                margin + answer < 1.0,
                "[window] margin and answer leave nothing of the window for the conversation"
            );
        }
    }
    if let Some(goals) = &spec.goals {
        if let Some(turns) = goals.turns {
            ensure!(
                (1..=COUNT_MAX).contains(&turns),
                "[goals] turns is 1 to {COUNT_MAX}, not {turns}"
            );
        }
        if let Some(tokens) = goals.tokens {
            ensure!(
                tokens >= 1000,
                "[goals] tokens is at least 1000, not {tokens}"
            );
        }
    }
    if let Some(turns) = &spec.turns {
        if let Some(rounds) = turns.rounds {
            ensure!(
                (1..=COUNT_MAX).contains(&rounds),
                "[turns] rounds is 1 to {COUNT_MAX}, not {rounds}"
            );
        }
        if let Some(calls) = turns.calls {
            ensure!(
                (1..=COUNT_MAX).contains(&calls),
                "[turns] calls is 1 to {COUNT_MAX}, not {calls}"
            );
        }
        let renew = match &turns.renew {
            Some(renew) => {
                let ms = millis(renew);
                ensure!(ms >= 1000, "[turns] renew is at least 1s");
                Some(ms)
            }
            None => None,
        };
        if let Some(lapse) = &turns.lapse {
            let ms = millis(lapse);
            // A lease renewed no more often than it lapses lapses while
            // its turn runs.
            if let Some(renew) = renew {
                ensure!(
                    ms > renew * 2,
                    "[turns] lapse is more than twice renew, or a running turn's lease lapses"
                );
            }
        }
    }
    if let Some(instructions) = &spec.instructions {
        check_instructions(instructions)?;
    }
    if let Some(tools) = &spec.tools {
        check_tool("tools", tools, "")?;
    }
    for (name, tool) in &spec.tool {
        ensure!(
            plain_word(name),
            "[tool.{}] isn't a tool's name",
            link_format::clean(name)
        );
        check_tool(&format!("tool.{name}"), tool, name)?;
    }
    if let Some(connections) = &spec.connections {
        check_connection("connections", connections)?;
    }
    for (name, connection) in &spec.connection {
        ensure!(
            plain_word(name),
            "[connection.{}] isn't a connection's name",
            link_format::clean(name)
        );
        check_connection(&format!("connection.{name}"), connection)?;
    }
    if let Some(after) = spec.run.as_ref().and_then(|r| r.idle_after.as_deref())
        && after != NEVER
    {
        let ms = duration_ms(after).within("[run] idle_after")?;
        ensure!(ms >= 1000, "[run] idle_after is at least 1s, or \"never\"");
    }
    if let Some(conversations) = spec
        .storage
        .as_ref()
        .and_then(|s| s.conversations.as_deref())
    {
        ensure!(
            [LOG, COMPACT].contains(&conversations)
                || crate::store::check_name(conversations).is_ok(),
            "[storage] conversations is \"log\", \"compact\" or a store driver's name (such as \"sqlite\"), not {:?}",
            link_format::clean(conversations)
        );
    }
    if let Some(storage) = &spec.storage {
        for (name, place) in [
            ("home", &storage.home),
            ("instructions", &storage.instructions),
            ("skills", &storage.skills),
            ("memory", &storage.memory),
            ("schedule", &storage.schedule),
        ] {
            if let Some(place) = place {
                crate::storage::check_place(place).within(format!("[storage] {name}"))?;
            }
        }
    }
    Ok(())
}

fn check_instructions(instructions: &Instructions) -> anyhow::Result<()> {
    let sources = [
        ("source", &instructions.source),
        ("base", &instructions.base),
        ("environment", &instructions.environment),
        ("skills", &instructions.skills),
        ("persona", &instructions.persona),
        ("owner", &instructions.owner),
        ("standing", &instructions.standing),
        ("now", &instructions.now),
        ("notes", &instructions.notes),
        ("repository", &instructions.repository),
    ];
    for (key, source) in sources {
        let Some(source) = source else { continue };
        let source = Source::parse(source).within(format!("[instructions] {key}"))?;
        match (key, &source) {
            ("persona" | "owner" | "standing", Source::Link) => {
                bail!("[instructions] {key} is a file of yours or none: Link has none")
            }
            ("notes", Source::Link) => {
                bail!("[instructions] notes is the agent's file or none: Link has none")
            }
            ("now", Source::File(_)) => {
                bail!("[instructions] now is link or none: it's written for each message")
            }
            ("repository", Source::File(_)) => {
                bail!("[instructions] repository is link or none: the repository's own files")
            }
            _ => {}
        }
    }
    let max = instructions.max.clone().unwrap_or_default();
    for (key, most) in [
        ("base", &max.base),
        ("model", &max.model),
        ("tools", &max.tools),
        ("environment", &max.environment),
        ("repository", &max.repository),
        ("skills", &max.skills),
        ("persona", &max.persona),
        ("owner", &max.owner),
        ("standing", &max.standing),
        ("now", &max.now),
        ("notes", &max.notes),
    ] {
        let Some(most) = most else { continue };
        let bytes = size(most).within(format!("[instructions.max] {key}"))?;
        ensure!(
            (64..=SECTION_MAX).contains(&bytes),
            "[instructions.max] {key} is 64 B to 64 KB"
        );
    }
    Ok(())
}

fn check_model(model: &Model) -> anyhow::Result<()> {
    let providers = [
        &model.anthropic,
        &model.openai,
        &model.openrouter,
        &model.ollama,
        &model.local,
    ];
    let names = providers
        .iter()
        .copied()
        .flatten()
        .flat_map(|p| p.default.iter().chain(p.allow.iter().flatten()));
    for name in model
        .default
        .iter()
        .chain(model.allow.iter().flatten())
        .chain(names)
    {
        ensure!(
            !name.is_empty() && name.len() <= 128 && !name.chars().any(char::is_control),
            "{:?} isn't a model's name",
            link_format::clean(name)
        );
    }
    // `[model] default` says who serves it; a provider's table names its
    // own model, as that provider knows it.
    if let Some(name) = &model.default {
        link_model_contract::selection::Route::parse(name)
            .map_err(|why| {
                anyhow::anyhow!(
                    "[model] default: {why}; a spec written before names said this reads as it was meant with schema = 1"
                )
            })?;
    }
    let answers = providers.iter().copied().flatten().map(|p| p.answer);
    for answer in std::iter::once(model.answer).chain(answers).flatten() {
        ensure!(answer >= 256, "[model] answer is at least 256 tokens");
    }
    let budgets = providers
        .iter()
        .copied()
        .flatten()
        .map(|p| p.thinking_budget);
    for budget in std::iter::once(model.thinking_budget)
        .chain(budgets)
        .flatten()
    {
        ensure!(
            budget >= 1024,
            "[model] thinking_budget is at least 1024 tokens"
        );
    }
    if let Some(name) = &model.fallback_model {
        ensure!(
            !name.trim().is_empty(),
            "[model] fallback_model is not empty"
        );
        // A fallback answers when this machine's model can't, so one named
        // on this machine is refused, switched on or not.
        let named = model.aliases.get(name).unwrap_or(name);
        let here = link_model_contract::selection::Route::parse(named)
            .is_ok_and(|route| route.provider == link_model_contract::selection::Provider::Local);
        ensure!(
            !here,
            "[model] fallback_model {:?} runs on this machine: a fallback is a hosted model",
            link_format::clean(name)
        );
    }
    for (alias, name) in &model.aliases {
        ensure!(
            !alias.trim().is_empty()
                && !name.trim().is_empty()
                && !alias.contains(char::is_whitespace),
            "[model.aliases] {alias:?}: a name of one word, for a model that isn't empty"
        );
    }
    Ok(())
}

/// Calls that lose a stranger's standing: schedules, subagents and
/// messages run as this agent, as do commands and language servers that
/// may run project build scripts. Canonical wrappers retain the same
/// boundary as their providers; ordinary file operations remain delegable.
const ACTING: &[&str] = &["scheduler", "run", "code", "task", "cron"];
const ACTING_TOOLS: &[&str] = &[
    "agent__send",
    "file__definition",
    "file__references",
    "file__diagnostics",
];

fn acts_as_the_agent(tool: &str) -> bool {
    let server = tool
        .split_once(link_spec::follow::TOOL_SEPARATOR)
        .map_or(tool, |(server, _)| server);
    ACTING.contains(&server) || ACTING_TOOLS.contains(&tool)
}

fn check_tool(table: &str, tool: &Tool, name: &str) -> anyhow::Result<()> {
    if let Some(strangers) = &tool.strangers {
        ensure!(
            name.is_empty(),
            "[{table}] strangers: only [tools] takes it"
        );
        if let Some(bad) = strangers
            .iter()
            .find(|t| !crate::connection::is_tool_name(t))
        {
            bail!(
                "[{table}] strangers: {:?} isn't a tool's name",
                link_format::clean(bad)
            );
        }
        if let Some(acting) = strangers.iter().find(|t| acts_as_the_agent(t)) {
            bail!(
                "[{table}] strangers: {} acts as the agent itself (a schedule, a subagent or a \
                 message it sends is its own; a command runs as it), so a stranger is never given \
                 it",
                link_format::clean(acting)
            );
        }
    }
    if let Some(key) = tool.others.keys().next()
        && (name.is_empty() || LINK_TOOLS.contains(&name))
    {
        bail!(
            "[{table}] {}: not a key Link's tools take",
            link_format::clean(key)
        );
    }
    for key in tool.others.keys() {
        ensure!(
            plain_word(key),
            "[{table}] {:?} isn't a key's name",
            link_format::clean(key)
        );
    }
    let memory = name == "memory";
    let scheduler_keys = [
        tool.per_day.is_some(),
        tool.subagents.is_some(),
        tool.hops.is_some(),
        tool.late.is_some(),
        tool.ask.is_some(),
        tool.recurring.is_some(),
        tool.zone.is_some(),
        tool.interval.is_some(),
        tool.firing.is_some(),
        tool.driver.is_some(),
    ];
    if name != "scheduler"
        && let Some(n) = scheduler_keys.iter().position(|set| *set)
    {
        bail!(
            "[{table}] {}: only [tool.scheduler] takes it",
            SCHEDULER_ONLY[n]
        );
    }
    ensure!(
        tool.enabled.is_none() || SWITCHED.contains(&name) || SCREENS.contains(&name),
        "[{table}] enabled: only [tool.<name>] of {} takes it",
        [SWITCHED, SCREENS].concat().join(", ")
    );
    for (key, set) in [("find", tool.find), ("diffs", tool.diffs)] {
        ensure!(
            set.is_none() || name == "file",
            "[{table}] {key}: only [tool.file] takes it"
        );
    }
    for (key, value) in [
        ("per_day", tool.per_day),
        ("subagents", tool.subagents),
        ("hops", tool.hops),
    ] {
        if let Some(value) = value {
            ensure!(
                (1..=COUNT_MAX).contains(&value),
                "[{table}] {key} is 1 to {COUNT_MAX}, not {value}"
            );
        }
    }
    if let Some(zone) = &tool.zone {
        ensure!(
            zone == "local" || is_zone(zone),
            "[{table}] zone {:?} is local or a time zone's name, such as \"Europe/Paris\"",
            link_format::clean(zone)
        );
    }
    if let Some(interval) = &tool.interval {
        let ms = millis(interval);
        ensure!(
            (60_000..=86_400_000).contains(&ms),
            "[{table}] interval is 1m to 1d"
        );
    }
    if let Some(firing) = &tool.firing {
        let ms = millis(firing);
        ensure!(
            (1000..=86_400_000).contains(&ms),
            "[{table}] firing is 1s to 1d"
        );
    }
    if let Some(answer) = &tool.answer {
        let bytes = size(answer).within(format!("[{table}] answer"))?;
        ensure!(
            (crate::ANSWER_LEAST..=16 << 20).contains(&bytes),
            "[{table}] answer is 1 KB to 16 MB"
        );
    }
    if let Some(timeout) = &tool.timeout {
        let ms = millis(timeout);
        ensure!(ms >= 1000, "[{table}] timeout is at least 1s");
    }
    if let Some(fetch) = &tool.fetch {
        ensure!(name == "web", "[{table}] fetch: only [tool.web] takes it");
        let bytes = size(fetch).within(format!("[{table}] fetch"))?;
        ensure!(
            (1024..=1 << 20).contains(&bytes),
            "[{table}] fetch is 1 KB to 1 MB"
        );
    }
    if let Some(hosts) = &tool.hosts {
        ensure!(name == "web", "[{table}] hosts: only [tool.web] takes it");
        for host in hosts {
            link_sandbox_contract::net::check_host(host).within(format!("[{table}] hosts"))?;
        }
    }
    if let Some(program) = &tool.program {
        ensure!(
            name == "browser",
            "[{table}] program: only [tool.browser] takes it"
        );
        ensure!(
            std::path::Path::new(program).is_absolute(),
            "[{table}] program {:?} isn't an absolute path: name the browser's program from /",
            link_format::clean(program)
        );
    }
    if let Some(output) = &tool.output {
        ensure!(name == "run", "[{table}] output: only [tool.run] takes it");
        let bytes = size(output).within(format!("[{table}] output"))?;
        ensure!(
            (1024..=16 << 20).contains(&bytes),
            "[{table}] output is 1 KB to 16 MB"
        );
    }
    let run_keys = [
        tool.background.is_some(),
        tool.background_timeout.is_some(),
        tool.background_output.is_some(),
        tool.background_keep.is_some(),
        tool.background_per_day.is_some(),
    ];
    if name != "run"
        && let Some(n) = run_keys.iter().position(|set| *set)
    {
        bail!("[{table}] {}: only [tool.run] takes it", RUN_ONLY[n]);
    }
    if let Some(background) = tool.background {
        ensure!(
            background <= BACKGROUND_MAX,
            "[{table}] background is 0 to {BACKGROUND_MAX}, not {background}"
        );
    }
    if let Some(timeout) = &tool.background_timeout {
        let ms = millis(timeout);
        ensure!(
            (1000..=7 * 86_400_000).contains(&ms),
            "[{table}] background_timeout is 1s to 7d"
        );
    }
    if let Some(output) = &tool.background_output {
        let bytes = size(output).within(format!("[{table}] background_output"))?;
        ensure!(
            (1024..=1 << 30).contains(&bytes),
            "[{table}] background_output is 1 KB to 1 GB"
        );
    }
    if let Some(keep) = &tool.background_keep {
        let ms = millis(keep);
        ensure!(
            (60_000..=365 * 86_400_000).contains(&ms),
            "[{table}] background_keep is 1m to 365d"
        );
    }
    if let Some(per_day) = tool.background_per_day {
        ensure!(
            (1..=COUNT_MAX).contains(&per_day),
            "[{table}] background_per_day is 1 to {COUNT_MAX}, not {per_day}"
        );
    }
    if !tool.servers.is_empty() {
        let servers = &tool.servers;
        ensure!(
            name == "code",
            "[{table}] servers: only [tool.code] takes it"
        );
        check_servers(table, servers)?;
    }
    let memory_keys = [
        tool.keep.is_some(),
        tool.max_age.is_some(),
        tool.tokens.is_some(),
        tool.model.is_some(),
        tool.notes.is_some(),
        tool.dimensions.is_some(),
        tool.memory.is_some(),
        tool.reserve.is_some(),
        tool.file.is_some(),
        tool.threads.is_some(),
        tool.deadline.is_some(),
        tool.folders.is_some(),
        tool.kept_off.is_some(),
    ];
    if !memory {
        if let Some(n) = memory_keys.iter().position(|set| *set) {
            bail!("[{table}] {}: only [tool.memory] takes it", MEMORY_ONLY[n]);
        }
        return Ok(());
    }
    if let Some(tokens) = tool.tokens {
        ensure!(tokens >= 100, "[{table}] tokens is at least 100");
    }
    if let Some(model) = &tool.model {
        ensure!(
            !model.is_empty()
                && model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b)),
            "[{table}] model {:?} isn't a model's name",
            link_format::clean(model)
        );
    }
    if let Some(notes) = &tool.notes {
        // At most a section's most: the notes are one of the prompt's.
        let bytes = size(notes).within(format!("[{table}] notes"))?;
        ensure!(
            (64..=SECTION_MAX).contains(&bytes),
            "[{table}] notes is 64 B to 64 KB"
        );
    }
    // Memory's sizes: each a size, the index's own at least a megabyte.
    for (key, given) in [
        ("memory", &tool.memory),
        ("reserve", &tool.reserve),
        ("file", &tool.file),
        ("kept_off", &tool.kept_off),
    ] {
        if let Some(given) = given {
            let bytes = size(given).within(format!("[{table}] {key}"))?;
            ensure!(
                key == "reserve" || bytes >= 1 << 20,
                "[{table}] {key} is at least 1 MB"
            );
        }
    }
    if let Some(folders) = &tool.folders {
        ensure!(!folders.is_empty(), "[{table}] folders names at least one");
    }
    Ok(())
}

/// Link's connections whose bridges mark no message with a reaction: a
/// `working` or `steered` of theirs would change nothing.
const NO_REACTIONS: &[&str] = &["telegram", "signal"];

fn check_connection(table: &str, connection: &Connection) -> anyhow::Result<()> {
    // Empty: none, so a set before can be turned off.
    for (key, reaction) in [
        ("steered", &connection.steered),
        ("working", &connection.working),
    ] {
        let Some(reaction) = reaction.as_deref().filter(|r| !r.is_empty()) else {
            continue;
        };
        ensure!(
            plain_word(reaction)
                || reaction
                    .bytes()
                    .all(|b| b == b'_' || b.is_ascii_lowercase()),
            "[{table}] {key} {:?} isn't a reaction's name",
            link_format::clean(reaction)
        );
        if let Some(name) = table.strip_prefix("connection.") {
            ensure!(
                !NO_REACTIONS.contains(&name),
                "[{table}] {key}: Link's {name} marks no message with a reaction (Slack and Discord do)"
            );
        }
    }
    for (key, mark) in [
        ("activity_mark", &connection.activity_mark),
        ("activity_separator", &connection.activity_separator),
    ] {
        if let Some(mark) = mark {
            ensure!(
                mark.chars().count() <= link_connection_policy::ACTIVITY_MARK_MAX
                    && !mark.chars().any(char::is_control),
                "[{table}] {key} is at most {} characters, on one line",
                link_connection_policy::ACTIVITY_MARK_MAX
            );
        }
    }
    if let Some(pace) = &connection.pace {
        ensure!(!pace.get().is_zero(), "[{table}] pace is more than nothing");
    }
    if let Some(wait) = &connection.ask_wait {
        ensure!(
            table == "connections",
            "[{table}] ask_wait: a question waits as long on every connection, so it's \
             [connections]'s alone"
        );
        let ms = u64::try_from(wait.get().as_millis()).unwrap_or(u64::MAX);
        ensure!(
            (1_000..=ASK_WAIT_MOST_MS).contains(&ms),
            "[{table}] ask_wait is a second at least, and a day at most"
        );
    }
    if let Some(after) = connection
        .compact_after
        .as_deref()
        .filter(|after| ![NEVER, AUTO].contains(after))
    {
        let ms = duration_ms(after).within(format!("[{table}] compact_after"))?;
        ensure!(
            ms >= COMPACT_AFTER_LEAST,
            "[{table}] compact_after is \"auto\", at least 1m, or \"never\""
        );
    }
    if let Some(cache) = connection.cache.as_deref().filter(|cache| *cache != AUTO) {
        let ms = duration_ms(cache).within(format!("[{table}] cache"))?;
        ensure!(
            ms > 0,
            "[{table}] cache is \"auto\", or a lifetime its provider offers"
        );
    }
    if let Some(pauses) = &connection.pauses {
        ensure!(
            (1..=PAUSES_MOST).contains(&pauses.len()),
            "[{table}] pauses are 1 to {PAUSES_MOST} durations"
        );
        for pause in pauses {
            ensure!(
                !pause.get().is_zero(),
                "[{table}] pauses are each more than nothing"
            );
        }
    }
    Ok(())
}

/// The longest a question for the owner waits: a day, as the sandbox's
/// own `ask_wait` at most.
const ASK_WAIT_MOST_MS: u64 = 24 * 60 * 60 * 1000;
/// The shortest `[connections] compact_after`, in milliseconds: a minute.
const COMPACT_AFTER_LEAST: u64 = 60 * 1000;
/// The most `[connections] pauses` a spec assumes.
const PAUSES_MOST: usize = 32;

/// A time zone's name as the zone database writes it (`Europe/Paris`,
/// `America/Argentina/Buenos_Aires`, `UTC`): words of letters, digits,
/// `_`, `-` and `+`, joined by `/`.
pub fn is_zone(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.split('/').all(|word| {
            !word.is_empty()
                && word.starts_with(|c: char| c.is_ascii_alphabetic())
                && word
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-+".contains(&b))
        })
}

/// Lowercase letters, digits, `-` and `_`, starting with a letter, at
/// most 64.
/// Code's servers: each language a plain word, a command whose program
/// is named, extensions and root files plain names, and no extension
/// served twice.
fn check_servers(table: &str, servers: &BTreeMap<String, CodeServer>) -> anyhow::Result<()> {
    let mut served: BTreeMap<&str, &str> = BTreeMap::new();
    for (language, server) in servers {
        ensure!(
            plain_word(language),
            "[{table}.servers] {:?} isn't a language's name",
            link_format::clean(language)
        );
        if let Some(program) = server.command().first() {
            ensure!(
                !program.trim().is_empty(),
                "[{table}.servers.{language}] command names no program"
            );
        }
        for extension in server.files() {
            ensure!(
                !extension.is_empty()
                    && extension.len() <= 16
                    && extension.bytes().all(|b| b.is_ascii_alphanumeric()),
                "[{table}.servers.{language}] files: {:?} isn't an extension (rs, no dot)",
                link_format::clean(extension)
            );
            if let Some(other) = served.insert(extension, language) {
                bail!("[{table}.servers] .{extension} files are both {other}'s and {language}'s");
            }
        }
        for root in server.roots() {
            ensure!(
                !root.is_empty() && !root.contains('/') && root != "." && root != "..",
                "[{table}.servers.{language}] roots: {:?} isn't a file's name",
                link_format::clean(root)
            );
        }
    }
    Ok(())
}

fn plain_word(word: &str) -> bool {
    (1..=64).contains(&word.len())
        && word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// `text` as bytes: a number and a unit, `B`, `KB`, `MB` or `GB`
/// (powers of 1024, as Link's sizes are).
pub fn size(text: &str) -> anyhow::Result<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && *n >= 0.0)
        .within(format!(
            "{:?} isn't a size, such as \"32 KB\"",
            link_format::clean(text)
        ))?;
    let times: f64 = match unit.trim() {
        "B" => 1.0,
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        _ => bail!(
            "{:?} isn't a size: B, KB, MB or GB after the number",
            link_format::clean(text)
        ),
    };
    Ok((number * times) as u64)
}
