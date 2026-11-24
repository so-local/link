//! What a harness may do here, shown, and what it needs beyond
//! Link's defaults, asked for and granted.

use std::path::Path;

use link_agent::{
    adapter::Adapter,
    grants::{self, Grants},
    sandbox_file::{self, Sandbox},
    settings::{self, Settings},
};
use link_format::{
    Done, Exit, Fields, Name, Note, Out, Question, Refuser, Report, Sentence, Table, Text, Value,
    WithinExt, cmd, prose, sentence, states,
};
use link_sandbox_client::Spec;
use link_sandbox_contract::spec::Os;

use crate::and_list;

states! {
    /// A thing an agent may do, as the permissions list says it.
    pub(crate) enum Permitted {
        Allowed("allowed") => Quiet,
        Beyond("allowed, beyond the defaults") => Warn,
    }
}

states! {
    /// Whether an agent's sandbox is on, in the permissions' fields.
    pub(crate) enum Sandboxed {
        On("on") => Quiet,
        Off("off") => Warn,
    }
}

/// `lnk agent show --permissions`: what agent `settings` may do here, both
/// layers: what you grant it, whichever harness runs it, and what harness
/// `name` said it needs, each thing it opens marked where it came from.
pub(crate) fn permissions(out: &mut Out, settings: &Settings, name: &str) -> Result<Done, Report> {
    let agent = &settings.name;
    let shown = Name::new("agent", agent);
    let harness = Name::new("harness", name);
    let sandbox = sandbox_file::load_or_warn(agent);
    let grants = Grants::load_or_warn(name);
    let link = Sandbox::link().spec;
    let here = Os::here();
    out.block(Text(prose!(
        "What {} may do beyond the home and the own localhost ports of {}, while it runs it, \
         each as {} and {} take it back.",
        &shown,
        &harness,
        cmd!("lnk agent permission remove"),
        cmd!("lnk agent harness-permission remove")
    )));
    let yours = sandbox.spec.on(here).unwrap_or_default();
    let needed = grants.spec.on(here).unwrap_or_default();
    let mut table = Table::new(
        ["", "Grant", "From", "About"],
        sentence!("Nothing: no network, no folder beyond its own, no program."),
    );
    let mut items = yours.items();
    for item in needed.items() {
        if !items.contains(&item) {
            items.push(item);
        }
    }
    for item in &items {
        let (mine, its) = (
            yours.covers(item, here).unwrap_or(false),
            needed.covers(item, here).unwrap_or(false),
        );
        let permitted = match link.covers(item, here).unwrap_or(false) {
            true => Permitted::Allowed,
            false => Permitted::Beyond,
        };
        let from = match (mine, its) {
            (true, true) => format!("yours, {name}'s"),
            (true, false) => "yours".into(),
            (false, _) => format!("{name}'s"),
        };
        let flag = link_sandbox_client::flags::shown(item);
        table.row([
            Value::from(permitted),
            Value::from(flag.clone()),
            Value::from(from),
            Value::from(grants::about(&flag).to_string()),
        ]);
    }
    out.block(table);
    // Whether it runs behind its proxy as its start decides: its route
    // (`lnk agent route`), else its spec.
    let proxied = settings.sandbox && settings.proxy_of(name).is_some();
    let mut fields = Fields::new().row(
        "Its proxy",
        match proxied {
            true => match &settings.upstream {
                Some(upstream) => format!(
                    "it reaches the internet through it, routed through {upstream}, as the \
                     hosts above say: none, if none"
                ),
                None => "it reaches the internet through it, as the hosts above say".to_string(),
            },
            false => format!(
                "{name} runs it without one (no --internet.hosts, --network.mode host, or no \
                 sandbox)"
            ),
        },
    );
    fields = fields.row(
        "Sandbox",
        match settings.sandbox {
            true => Sandboxed::On,
            false => Sandboxed::Off,
        },
    );
    // What it reaches of its teams' folders (`lnk agent team add`).
    if !sandbox.spec.teams.is_empty() {
        let summary = sandbox_file::summary(&sandbox);
        let teams: Vec<Value> = summary
            .split("; ")
            .filter(|part| part.starts_with("team "))
            .map(Value::from)
            .collect();
        fields = fields.row("Teams' folders", Value::List(teams));
    }
    if !grants.risks.is_empty() {
        let risks: Vec<Value> = grants.risks.iter().map(Value::from).collect();
        fields = fields.row("Its harness's risks", Value::List(risks));
    }
    out.block(fields);
    let mut done = Done::shown();
    if !settings.sandbox {
        done = done.next(
            cmd!("lnk agent start --sandbox"),
            "turn the sandbox back on",
        );
    }
    Ok(done
        .next(
            cmd!("lnk agent permission add"),
            "grant a spec's flag, kept for any harness (--internet.hosts api.openai.com, --internet.tcp github.com:22, --files.read ~)",
        )
        .next(cmd!("lnk agent permission remove"), "take one back")
        .next(
            cmd!("lnk agent harness-permission list --harness {}", &harness),
            "what the harness said it needs",
        ))
}

/// `lnk agent harness-permission list`: what harness `name` said it needs
/// here and you allowed, for every agent that runs it.
pub(crate) fn harness_permissions(out: &mut Out, name: &str) -> Result<Done, Report> {
    let harness = Name::new("harness", name);
    let grants = Grants::load_or_warn(name);
    out.block(Text(prose!(
        "What {} said it needs on this machine, for every agent here that runs it, beside \
         what you grant each agent.",
        &harness
    )));
    let mut fields = Fields::new().row(
        "Allowed",
        match grants.summary().as_str() {
            "nothing" => "nothing: it runs with each agent's grants alone".to_string(),
            summary => summary.to_string(),
        },
    );
    let mut answered: Vec<String> = grants
        .asked
        .on(Os::here())
        .unwrap_or_default()
        .items()
        .iter()
        .map(link_sandbox_client::flags::shown)
        .collect();
    answered.extend(grants.answered.iter().cloned());
    if !answered.is_empty() {
        fields = fields.row(
            "It asked for",
            Value::List(answered.into_iter().map(Value::from).collect()),
        );
        out.block(Note(sentence!(
            "It is not asked again for what you answered, also once taken back, until {}.",
            cmd!("lnk agent harness reset --harness {}", &harness)
        )));
    }
    out.block(fields);
    Ok(Done::shown()
        .next(
            cmd!("lnk agent harness-permission add --harness {}", &harness),
            "allow a need or a risk",
        )
        .next(
            cmd!("lnk agent harness-permission remove --harness {}", &harness),
            "take one back",
        )
        .next(
            cmd!("lnk agent show --permissions"),
            "what each agent may do",
        ))
}

/// The lines a start prints when agent `settings` runs on harness `name`
/// with more than Link's defaults: what you granted it, and what `name`
/// needs; each with how to take it back.
pub(crate) fn beyond_lines(settings: &Settings, name: &str) -> anyhow::Result<Vec<String>> {
    let sandbox = sandbox_file::load(&settings.name)?;
    let grants = Grants::load(name)?;
    let mut lines = Vec::new();
    let agent = &settings.name;
    match sandbox_file::beyond_defaults(&sandbox).as_slice() {
        [] => {}
        [one] => lines.push(format!(
            "{agent} runs with {one}, beyond Link's defaults (lnk agent permission remove {one})"
        )),
        many => lines.push(format!(
            "{agent} runs with {}, beyond Link's defaults (lnk agent permission remove <flags> takes them \
             back)",
            and_list(many)
        )),
    }
    let had = sandbox.spec.join(&Sandbox::link().spec);
    match grants.beyond(&had).as_slice() {
        [] => {}
        [one] => lines.push(format!(
            "{name} runs with {one}, beyond Link's defaults (lnk agent harness-permission remove {one} \
             --harness {name})"
        )),
        many => lines.push(format!(
            "{name} runs with {}, beyond Link's defaults (lnk agent harness-permission remove <flags or \
             risk> --harness {name} takes one back)",
            and_list(many)
        )),
    }
    Ok(lines)
}

/// What agent `settings` may do while harness `name` runs it, both
/// layers, as one spec on this machine: its own and what `name` needs,
/// its `[files]` as the agent sees them.
pub(crate) fn granted_spec(settings: &Settings, name: &str) -> Spec {
    sandbox_file::of(&settings.name)
        .spec
        .join(&Grants::of(name).spec)
        .on(Os::here())
        .unwrap_or_default()
}

/// [`granted_spec`], each thing it opens as its flag.
pub(crate) fn granted(settings: &Settings, name: &str) -> Vec<String> {
    granted_spec(settings, name)
        .items()
        .iter()
        .map(link_sandbox_client::flags::shown)
        .collect()
}

/// What agent `settings` runs with beyond Link's defaults on harness
/// `name`, each as `deny` names it: its own, then what `name` needs.
pub(crate) fn beyond_defaults(settings: &Settings, name: &str) -> Vec<String> {
    let sandbox = sandbox_file::of(&settings.name);
    let had = sandbox.spec.join(&Sandbox::link().spec);
    let mut beyond = sandbox_file::beyond_defaults(&sandbox);
    for more in Grants::of(name).beyond(&had) {
        if !beyond.contains(&more) {
            beyond.push(more);
        }
    }
    beyond
}

/// Whether to answer yes for the user (`--yes`) when a harness asks for
/// more than Link's defaults ([`answer_needs`]).
pub(crate) static YES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Asks for what harness `name` needs beyond Link's defaults (`lnk-<name>
/// needs`, given `settings`) and hasn't been answered here
/// ([`pending_needs`], [`ask_needs`]); allowed, it's granted to the
/// harness and remembered with the needs it answered. Then the risks it
/// runs without, if they aren't allowed ([`offer_risks`]). What it said,
/// for its install ([`Adapter::install`]).
pub(crate) fn answer_needs(
    out: &mut Out,
    settings: &Settings,
    name: &str,
    adapter: &Adapter,
) -> Result<link_harness_contract::harness::Needs, Report> {
    // What it said with the settings it was asked with, before what's
    // granted now changes them (the network as it is takes its proxy away).
    let needs = adapter.needs(settings)?;
    let (ask, offers, mut grants) = pending_needs(settings, name, adapter)?;
    if !ask.is_empty() {
        ask_needs(out, name, &keyed(&ask), None)?;
    }
    if !offers.is_empty() {
        offer_risks(out, name, &offers, &mut grants)?;
    }
    grant_needs(name, grants, &ask)?;
    Ok(needs)
}

/// Whether anyone can be asked a question: a terminal in and out.
fn may_ask() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && link_format::streams::stderr_is_terminal()
}

/// What harness `name` needs (`lnk-<name> needs`, given `settings`) and
/// hasn't been answered here, each as `deny` names it with why; the risks
/// it runs without (`without`), unanswered; and its grants, with a need
/// it has already remembered as answered, unasked, so taking it back
/// later isn't asked about again. What the agent has of its own
/// (`lnk agent permission add`) is had, and asks nothing. No sandbox at all is
/// never asked: a harness that needs it runs only for an agent started
/// `--no-sandbox`, and is refused for any other.
pub(crate) fn pending_needs(
    settings: &Settings,
    name: &str,
    adapter: &Adapter,
) -> Result<(Asks, Vec<link_harness_contract::harness::Risk>, Grants), Report> {
    let mut grants = Grants::load(name)?;
    let own = sandbox_file::load(&settings.name)?.spec;
    let needs = adapter.needs(settings)?;
    let checked = needs.checked().within(format!("what lnk-{name} needs"))?;
    let here = Os::here();
    let mut ask: Asks = Vec::new();
    for (spec, why) in checked {
        let covered = |by: &Spec| by.covers(&spec, here).unwrap_or(false);
        if covered(&grants.asked) || covered(&own) {
            continue;
        }
        match covered(&grants.spec) {
            true => grants.asked = grants.asked.join(&spec),
            false => ask.push(Ask {
                key: link_sandbox_client::flags::shown(&spec),
                why,
                spec: Some(spec),
            }),
        }
    }
    if let Some(unsandboxed) = &needs.unsandboxed
        && settings.sandbox
    {
        let agent = Name::new("agent", &settings.name);
        return Err(Report::error(sentence!(
            "{} runs only with no sandbox.",
            Name::new("harness", name)
        ))
        .why(Sentence::adopt(&format!(
            "It says: {}",
            link_format::clean(&unsandboxed.why)
        )))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk agent start --agent {} --no-sandbox", &agent),
            "run the agent without one, until --sandbox",
        ));
    }
    let mut offers = Vec::new();
    for risk in needs.risks {
        if grants.answered.contains(&risk.name) {
            continue;
        }
        if grants.risks.contains(&risk.name) {
            grants.answered.insert(risk.name);
            continue;
        }
        match risk.without {
            Some(_) => offers.push(risk),
            None => ask.push(Ask {
                key: risk.name,
                why: risk.why,
                spec: None,
            }),
        }
    }
    Ok((ask, offers, grants))
}

/// The warning that harness `name`, its home `dir`, runs with no sandbox.
pub(crate) fn no_sandbox_warning(out: &mut Out, name: &str, dir: &Path) {
    out.block(
        Report::warning(sentence!("There is no sandbox."))
            .why(prose!(
                "{} can read your whole home folder (documents, SSH keys, browser data) and \
                 write outside {}.",
                Name::new("harness", name),
                link_format::Path::new(dir)
            ))
            .next(
                cmd!("lnk agent start --sandbox"),
                "turn the sandbox back on",
            ),
    );
}

/// A need to ask for: as `deny` names it (a spec's flags, or a risk's
/// name), with why, and its spec (none: a risk).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ask {
    pub key: String,
    pub why: String,
    pub spec: Option<Spec>,
}

/// Needs to ask for.
pub(crate) type Asks = Vec<Ask>;

/// Each need of `asks` as `deny` names it, with why: what a question
/// shows, and a box answers with.
pub(crate) fn keyed(asks: &[Ask]) -> Vec<(String, String)> {
    asks.iter()
        .map(|a| (a.key.clone(), a.why.clone()))
        .collect()
}

/// Shows the risks harness `name` runs without unless they're allowed
/// (`offers`, from [`pending_needs`]), and in a terminal asks; the answer,
/// either way, is kept in `grants`. Without a terminal, or with `--yes`,
/// they stay off, unanswered, and it says how to allow one.
pub(crate) fn offer_risks(
    out: &mut Out,
    name: &str,
    offers: &[link_harness_contract::harness::Risk],
    grants: &mut Grants,
) -> Result<(), Report> {
    let harness = Name::new("harness", name);
    let (it, these) = match offers {
        [_] => ("it", "this"),
        _ => ("them", "these"),
    };
    let mut table = Table::new(["Risk", "What it is", "Without it"], sentence!("No risks."));
    for risk in offers {
        table.row([
            Value::from(risk.name.clone()),
            Value::from(format!("{name} says: {}", risk.why)),
            Value::from(risk.without.clone().unwrap_or_default()),
        ]);
    }
    out.now(Text(prose!(
        "{} can also run with {}, off unless you allow {}.",
        &harness,
        these.to_string(),
        it.to_string()
    )));
    out.now(table);
    let names: Vec<&str> = offers.iter().map(|r| r.name.as_str()).collect();
    let allow = match names.as_slice() {
        [one] => cmd!(
            "lnk agent harness-permission add {} --harness {}",
            one.to_string(),
            &harness
        ),
        _ => cmd!(
            "lnk agent harness-permission add <one> --harness {}",
            &harness
        ),
    };
    if YES.load(std::sync::atomic::Ordering::Relaxed) || !may_ask() {
        out.block(Note(sentence!("Left off: {} allows it.", allow)));
        return Ok(());
    }
    let allowed = out.ask(
        Question::confirm(
            sentence!(
                "Allow {} for {}, for {}?",
                these.to_string(),
                &harness,
                whom(&agents_running(name))
            ),
            false,
        )
        .why(prose!("No runs {} without {}.", &harness, it.to_string())),
    )?;
    for risk in offers {
        if allowed {
            grants.risks.insert(risk.name.clone());
        }
        grants.answered.insert(risk.name.clone());
    }
    if !allowed {
        out.block(Note(sentence!("Left off: {} allows it later.", allow)));
    }
    Ok(())
}

/// One line of what a question about a harness's grants changes: a need
/// or grant as `deny` names it, what it opens, and the harness's why for
/// one it asks for.
pub(crate) struct Line {
    pub key: String,
    pub about: String,
    pub why: Option<String>,
}

impl Line {
    /// A need harness asks for (`key`, as [`pending_needs`] names it),
    /// with its why.
    fn need(key: &str, why: &str) -> Self {
        Line {
            key: key.to_string(),
            about: grants::about(key).to_string(),
            why: Some(why.to_string()),
        }
    }
}

/// Shows `lines` of harness `name`'s: what each opens, and what the
/// harness says. A change to the programs it may run is said again,
/// plainly, after them: it's what the sandbox holds it to.
pub(crate) fn print_lines(out: &mut Out, name: &str, lines: &[Line]) {
    let mut table = Table::new(["Grant", "It opens", "Why"], sentence!("Nothing."));
    for line in lines {
        table.row([
            Value::from(line.key.clone()),
            Value::from(line.about.clone()),
            Value::from(
                line.why
                    .as_ref()
                    .map(|why| format!("{name} says: {why}"))
                    .unwrap_or_default(),
            ),
        ]);
    }
    out.now(table);
    // What it would run, each named: a program, or a toolchain's.
    let programs: Vec<String> = lines
        .iter()
        .filter_map(|l| {
            l.key
                .strip_prefix("--commands.allow ")
                .map(str::to_string)
                .or_else(|| {
                    l.key
                        .strip_prefix("--commands.toolchains ")
                        .map(|t| format!("the {t} toolchain's programs"))
                })
        })
        .collect();
    let harness = Name::new("harness", name);
    if !programs.is_empty() {
        out.now(Text(prose!(
            "This changes which programs {} may run: {}, beside those [commands] lists.",
            &harness,
            Value::List(programs.iter().map(|p| Value::from(p.as_str())).collect())
        )));
    }
}

/// What a question about a harness's grants is for.
pub(crate) enum Asking<'a> {
    /// What the harness needs and hasn't been answered, before it runs.
    Needs,
    /// Agent `agent` switching to the harness from harness `from`.
    Switch { agent: &'a str, from: &'a str },
}

/// The report a question answered no gives, or that no one was there to
/// answer: nothing changed, and nothing to try again.
pub(crate) fn declined(headline: Sentence, why: Sentence) -> Report {
    Report::refusal(Refuser::Person, headline).why(why)
}

/// Whether `report` is a [`declined`] one.
pub(crate) fn is_declined(report: &Report) -> bool {
    matches!(report.kind, link_format::Kind::Refusal(Refuser::Person))
}

/// Shows what harness `name` asks for (`ask`, from [`pending_needs`]) and
/// the agents here it holds for, and asks; an error, which changes
/// nothing, unless it's allowed, in a terminal or by `--yes`.
/// `running`: the agents its answer holds for, where they aren't this
/// machine's (a move's question for a box); none, this machine's.
pub(crate) fn ask_needs(
    out: &mut Out,
    name: &str,
    ask: &[(String, String)],
    running: Option<&[String]>,
) -> Result<(), Report> {
    let lines: Vec<Line> = ask.iter().map(|(k, why)| Line::need(k, why)).collect();
    confirm(out, name, &Asking::Needs, &lines, running)
}

/// Shows what harness `name` would get (`lines`) and the agents here it
/// holds for, and asks, as `asking` says; an error, which changes
/// nothing, unless it's allowed, in a terminal or by `--yes`. `running`
/// as for [`ask_needs`].
pub(crate) fn confirm(
    out: &mut Out,
    name: &str,
    asking: &Asking,
    lines: &[Line],
    running: Option<&[String]>,
) -> Result<(), Report> {
    let running = match running {
        Some(running) => running.to_vec(),
        None => agents_running(name),
    };
    let harness = Name::new("harness", name);
    let names: Vec<&str> = lines.iter().map(|l| l.key.as_str()).collect();
    out.block(Report::warning(match asking {
        Asking::Needs => sentence!("{} asks to do more than Link's defaults allow.", &harness),
        Asking::Switch { from, .. } => sentence!(
            "{} may do more on this machine than {}.",
            &harness,
            Name::new("harness", *from)
        ),
    }));
    print_lines(out, name, lines);
    out.now(Text(prose!(
        "This holds for {} {}.",
        whom_runs(&running),
        &harness
    )));
    if YES.load(std::sync::atomic::Ordering::Relaxed) {
        out.block(Note(sentence!("Allowed by --yes.")));
        return Ok(());
    }
    let what = match asking {
        Asking::Needs => format!("{name} asks for {}", and_list(&names)),
        Asking::Switch { agent, .. } => {
            format!("switching {agent} to {name} gives it {}", and_list(&names))
        }
    };
    if !may_ask() {
        return Err(declined(
            Sentence::adopt(&format!(
                "{what}: answer in a terminal, or add --yes to allow it"
            )),
            sentence!("Nothing changed."),
        )
        .next(out.typed().and("--yes"), "allow it"));
    }
    let question = match asking {
        Asking::Needs => Question::confirm(sentence!("Allow and run {}?", &harness), false)
            .why(prose!("No stops here, and nothing changes.")),
        Asking::Switch { agent, from } => Question::confirm(
            sentence!(
                "Switch {} to {} with these?",
                Name::new("agent", *agent),
                &harness
            ),
            false,
        )
        .why(prose!(
            "No keeps it on {}, and nothing changes.",
            Name::new("harness", *from)
        )),
    };
    if out.ask(question)? {
        return Ok(());
    }
    Err(match asking {
        Asking::Needs => declined(
            sentence!("Not allowed: {} doesn't run without them.", &harness),
            Sentence::adopt(&format!(
                "Nothing changed, and it asked for {}",
                and_list(&names)
            )),
        )
        .next(
            cmd!(
                "lnk agent harness-permission add <need> --harness {}",
                &harness
            ),
            "allow one; its next start asks again",
        )
        .next(
            cmd!("lnk agent harness set <harness>"),
            "run another harness",
        ),
        Asking::Switch { agent, from } => declined(
            sentence!(
                "Not switched: {} stays on {}.",
                Name::new("agent", *agent),
                Name::new("harness", *from)
            ),
            sentence!("Nothing changed."),
        ),
    })
}

/// Before agent `settings` switches from harness `from` to `name`: what
/// `name` may do that `from` couldn't, its own grants and the needs it
/// asks for now, shown and asked as one question ([`confirm`]); then the
/// risks it runs without ([`offer_risks`]). What it may do less of is
/// said too, and asks nothing.
pub(crate) fn confirm_switch(
    out: &mut Out,
    settings: &Settings,
    from: &str,
    name: &str,
    adapter: &Adapter,
) -> Result<(), Report> {
    let (ask, offers, mut grants) = pending_needs(settings, name, adapter)?;
    let old = Grants::of(from);
    let own = sandbox_file::of(&settings.name).spec;
    // What it asks for now, then what its grants already open beyond the
    // other harness's: either way, more than the agent had.
    let mut lines: Vec<Line> = ask.iter().map(|a| Line::need(&a.key, &a.why)).collect();
    let mut after = grants.clone();
    for need in &ask {
        grant(&mut after, need);
    }
    for (key, about) in after.wider_than(&old, &own) {
        if !ask.iter().any(|a| a.key == key) {
            lines.push(Line {
                key,
                about,
                why: None,
            });
        }
    }
    let narrower = after.narrower_than(&old, &own);
    if !lines.is_empty() {
        // The agent switching among them, though it isn't saved on `name`
        // until this is agreed.
        let mut running = agents_running(name);
        if !running.contains(&settings.name) {
            running.insert(0, settings.name.clone());
        }
        confirm(
            out,
            name,
            &Asking::Switch {
                agent: &settings.name,
                from,
            },
            &lines,
            Some(&running),
        )?;
    }
    if !narrower.is_empty() {
        out.block(Note(sentence!(
            "{} may do less than {} here: not {}.",
            Name::new("harness", name),
            Name::new("harness", from),
            and_list(&narrower)
        )));
    }
    if !offers.is_empty() {
        offer_risks(out, name, &offers, &mut grants)?;
    }
    Ok(grant_needs(name, grants, &ask)?)
}

/// The agents here whose harness is `name`.
pub(crate) fn agents_running(name: &str) -> Vec<String> {
    settings::names()
        .unwrap_or_default()
        .into_iter()
        .filter(|n| Settings::load_without_keys(n).is_ok_and(|s| s.harness() == name))
        .collect()
}

/// Whom an answer about a harness's grants holds for: every agent here
/// that runs it, `running` by name.
pub(crate) fn whom(running: &[String]) -> String {
    match running {
        [] => "every agent here that runs it".to_string(),
        agents => {
            let agents: Vec<String> = agents.iter().map(|a| link_format::clean(a)).collect();
            format!("every agent here that runs it ({})", and_list(&agents))
        }
    }
}

/// [`whom`], before the harness's name: `every agent here that runs`, or
/// `main and every other agent here that runs`.
pub(crate) fn whom_runs(running: &[String]) -> String {
    match running {
        [] => "every agent here that runs".to_string(),
        agents => {
            let agents: Vec<String> = agents.iter().map(|a| link_format::clean(a)).collect();
            format!(
                "{}, and every other agent here that runs",
                and_list(&agents)
            )
        }
    }
}

/// Grants harness `name` what was allowed (`allowed`, as [`pending_needs`]
/// names them) and remembers them as answered, saving `grants` when that
/// or [`pending_needs`] changed them.
pub(crate) fn grant_needs(name: &str, mut grants: Grants, allowed: &[Ask]) -> anyhow::Result<()> {
    let before = Grants::load(name)?;
    for need in allowed {
        grant(&mut grants, need);
    }
    if grants != before {
        grants.save(name)?;
    }
    Ok(())
}

/// Grants `need` in `grants`, remembered as answered.
pub(crate) fn grant(grants: &mut Grants, need: &Ask) {
    match &need.spec {
        Some(spec) => {
            grants.spec = grants.spec.join(spec);
            grants.asked = grants.asked.join(spec);
        }
        None => {
            grants.risks.insert(need.key.clone());
            grants.answered.insert(need.key.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_names_every_agent_it_holds_for() {
        assert_eq!(
            whom(&["main".into(), "work".into()]),
            "every agent here that runs it (main and work)"
        );
        assert_eq!(whom(&[]), "every agent here that runs it");
        assert_eq!(
            whom_runs(&["main".into()]),
            "main, and every other agent here that runs"
        );
    }
}
