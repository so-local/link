//! `lnk agent permission add` and `remove`, and their harness forms: grants
//! changed, and the agents they hold for restarted.

use link_agent::{
    adapter::Adapter,
    grants::{self, Grants},
    sandbox_file, service,
    settings::{self, Settings},
};
use link_agent_contract::agent_sandbox::Route;
use link_format::{
    Done, Exit, Name, Note, Out, Path, Question, Report, Sentence, Value, WithinExt, cmd, prose,
    sentence,
};
use link_sandbox_client::Spec;
use link_sandbox_contract::spec::Os;

use crate::confine::defined_tools;
use crate::launching::{launch, up};

/// Restarts every agent here running harness `name`, with what it may do
/// now.
///
/// Each one, whatever another's restart does: set up and started one
/// after another, then waited for together, each until it answers; then
/// says which still run with what the harness had before (one that
/// failed to restart, or one running in a terminal, which only its
/// terminal restarts), and fails if any didn't restart.
pub(crate) async fn restart_running(out: &mut Out, name: &str) -> Result<(), Report> {
    let harness = Name::new("harness", name);
    let mut failed = Vec::new();
    let mut in_terminal = Vec::new();
    let mut restarted = Vec::new();
    // Its harness first, without its keys: only an agent restarted loads
    // them (the Keychain, on a Mac); and only the agents of this harness
    // are asked whether they run, all at once.
    let ours: Vec<String> = settings::names()?
        .into_iter()
        .filter(|agent| !Settings::load_without_keys(agent).is_ok_and(|s| s.harness() != name))
        .collect();
    let running = service::running_each(&ours);
    let mut started = Vec::new();
    for (agent, running) in ours.into_iter().zip(running) {
        let foreground = service::foreground_running(&agent);
        if !foreground && !running {
            continue;
        }
        let shown = Name::new("agent", &agent);
        let settings = match Settings::load(&agent) {
            Ok(settings) => settings,
            Err(err) => {
                out.block(
                    Report::warning(sentence!("{} didn't restart.", &shown))
                        .why(Sentence::adopt(&format!("{err:#}"))),
                );
                failed.push(agent);
                continue;
            }
        };
        if settings.harness() != name {
            continue;
        }
        if foreground {
            in_terminal.push(agent);
            continue;
        }
        out.block(Note(sentence!("Restarting {} with it.", &shown)));
        match launch(out, &settings, name, false).await {
            Ok(Some(each)) => started.push((agent, settings, each)),
            Ok(None) => restarted.push(agent),
            Err(err) => {
                out.block(
                    Report::warning(sentence!("{} didn't restart.", &shown))
                        .why(Sentence::adopt(&err.headline.to_string())),
                );
                failed.push(agent);
            }
        }
    }
    // Each started, waited for at once: a harness takes seconds to
    // answer, which one after another would add up.
    let agents: Vec<String> = started.iter().map(|(agent, _, _)| agent.clone()).collect();
    let answered: Vec<bool> = std::thread::scope(|scope| {
        let waits: Vec<_> = started
            .into_iter()
            .map(|(_, settings, each)| scope.spawn(move || each.answered(&settings)))
            .collect();
        waits
            .into_iter()
            .map(|wait| wait.join().unwrap_or(false))
            .collect()
    });
    for (agent, answered) in agents.into_iter().zip(answered) {
        match answered {
            true => restarted.push(agent),
            false => {
                let shown = Name::new("agent", &agent);
                out.block(
                    Report::warning(sentence!("{} didn't restart.", &shown))
                        .why(prose!("{} started but isn't answering yet.", &harness))
                        .next(cmd!("lnk agent log show --agent {}", &shown), "see why"),
                );
                failed.push(agent);
            }
        }
    }
    if !restarted.is_empty() {
        out.block(Note(sentence!(
            "Running with it: {}.",
            names("agent", &restarted)
        )));
    }
    let old: Vec<String> = failed.iter().chain(&in_terminal).cloned().collect();
    if !old.is_empty() {
        let mut still = Report::warning(sentence!(
            "Still running with what {} had before: {}.",
            &harness,
            names("agent", &old)
        ))
        .next(
            cmd!("lnk agent start --agent <agent>"),
            "restart one with it",
        );
        if !in_terminal.is_empty() {
            still = still.why(prose!("One in a terminal restarts from that terminal."));
        }
        out.block(still);
    }
    if !failed.is_empty() {
        return Err(Report::error(sentence!(
            "{} didn't restart with the grants of {}.",
            names("agent", &failed),
            &harness
        )));
    }
    Ok(())
}

/// `kind` names as a list value.
fn names(kind: &'static str, names: &[String]) -> Value {
    Value::List(names.iter().map(|n| Name::new(kind, n).into()).collect())
}

/// The note when a grant of agent `settings` applies only behind its
/// proxy, which its harness runs it without.
pub(crate) fn unproxied_note(out: &mut Out, settings: &Settings) {
    let name = settings.harness();
    if !settings.sandbox || settings.proxy_of(name).is_none() {
        out.block(Note(sentence!(
            "It applies behind its proxy, which {} runs it without (no --internet.hosts, \
             --network.mode host, or no sandbox).",
            Name::new("harness", name)
        )));
    }
}

/// `lnk agent permission add` / `deny`: the sandbox spec's field flags (`fields`),
/// as `lnk sandbox run` takes them, added to or taken from what you grant agent
/// `settings`, whichever harness runs it, kept in its sandbox file; then
/// it's restarted with them, if it runs.
pub(crate) async fn allow_or_deny(
    out: &mut Out,
    settings: &mut Settings,
    fields: link_spec::FieldFlags,
    allow: bool,
) -> Result<Done, Report> {
    let agent = settings.name.clone();
    let shown = Name::new("agent", &agent);
    let me = match allow {
        true => cmd!("lnk agent permission add"),
        false => cmd!("lnk agent permission remove"),
    };
    let given: Spec = link_spec::over(&Spec::default(), &fields)?;
    if given.name.is_some() || given.version.is_some() || given == Spec::default() {
        return Err(Report::error(sentence!(
            "{} takes a spec's flags, as {} does.",
            me.clone(),
            cmd!("lnk sandbox run")
        ))
        .why(prose!(
            "{} lists them.",
            cmd!("lnk sandbox permission list")
        ))
        .exit(Exit::Usage)
        .next(
            me.clone().and("--internet.hosts api.openai.com"),
            "a host the internet reaches",
        )
        .next(
            me.clone().and("--internet.tcp github.com:22"),
            "a TCP address",
        )
        .next(me.clone().and("--files.read ~"), "a folder to read"));
    }
    if allow {
        for tool in given.tools.iter().flat_map(|t| t.names.iter().flatten()) {
            sandbox_file::check_tool(tool)?;
            if !defined_tools()?.iter().any(|t| t == tool) {
                return Err(Report::error(sentence!(
                    "There is no tool {}.",
                    Name::new("tool", tool)
                ))
                .exit(Exit::Usage)
                .next(
                    cmd!(
                        "lnk sandbox tool add {} -- <command>",
                        Name::new("tool", tool)
                    ),
                    "add it",
                ));
            }
        }
    }
    let mut sandbox = sandbox_file::load_or_warn(&agent);
    let readable = sandbox_file::load(&agent).is_ok();
    if !allow && let Some(why) = sandbox.spec.widened_by_taking(&given)? {
        return Err(
            Report::error(sentence!("Nothing was denied, since a deny never widens."))
                .why(Sentence::adopt(&why.to_string()))
                .exit(Exit::Usage),
        );
    }
    let before = sandbox.clone();
    sandbox.spec = match allow {
        true => sandbox.spec.join(&given),
        false => sandbox.spec.without(&given),
    };
    sandbox
        .check()
        .within("its sandbox file can't hold that: nothing changed")?;
    let named = Value::List(
        given
            .items()
            .iter()
            .map(|item| Value::from(link_sandbox_client::flags::shown(item)))
            .collect(),
    );
    let kept = Path::new(sandbox_file::path(&agent));
    if sandbox == before {
        // A file that couldn't be read is written anew, as what it was
        // taken for.
        if readable {
            out.block(Note(match allow {
                true => sentence!("{} has {} already.", &shown, named),
                false => sentence!("{} doesn't have {}.", &shown, named),
            }));
            return Ok(Done::shown());
        }
        sandbox_file::save(&agent, &sandbox)?;
        restart_agent(out, settings).await?;
        return Ok(
            Done::success(sentence!("The grants of {} are written anew.", &shown))
                .changed("Its sandbox file", kept),
        );
    }
    sandbox_file::save(&agent, &sandbox)?;
    let done = Done::success(match allow {
        true => sentence!("The grants of {} now include {}.", &shown, named.clone()),
        false => sentence!(
            "The grants of {} no longer include {}.",
            &shown,
            named.clone()
        ),
    })
    .changed("Kept in its sandbox file", kept);
    // A deny holds only once nothing it runs with grants it: what its
    // harness asked for is its own layer, which this doesn't take back.
    let held =
        [Route::Direct, Route::Proxied].map(|route| settings.spec_of(settings.harness(), route));
    let still = still_granted(&held, &given, Os::here());
    if !allow && !still.is_empty() {
        restart_agent(out, settings).await?;
        return Err(Report::error(sentence!(
            "{} still runs with {} as the grant of {}.",
            &shown,
            Value::List(
                still
                    .iter()
                    .map(|item| Value::from(link_sandbox_client::flags::shown(item)))
                    .collect()
            ),
            Name::new("harness", settings.harness())
        ))
        .next(harness_remove(&still, settings.harness()), "take it back"));
    }
    // What only its proxy carries: without one, it opens nothing.
    let proxied = given.internet.is_some()
        || given.lan.is_some()
        || given.tools.is_some()
        || given.browser.is_some();
    if allow && proxied {
        unproxied_note(out, settings);
    }
    restart_agent(out, settings).await?;
    Ok(done)
}

/// Each item of `given` that one of `held`, the specs an agent runs with
/// on each route, still grants on `os`, as its flag.
fn still_granted(held: &[Spec], given: &Spec, os: Os) -> Vec<Spec> {
    given
        .items()
        .into_iter()
        .filter(|item| {
            held.iter()
                .any(|spec| spec.covers(item, os).unwrap_or(false))
        })
        .collect()
}

/// `lnk agent harness-permission remove` of `items` from harness
/// `harness`'s grants: each item's field flags, a word each.
fn harness_remove(items: &[Spec], harness: &str) -> link_format::Command {
    let flags = items
        .iter()
        .flat_map(|item| link_spec::flags_of(item).unwrap_or_default());
    link_format::Command::typed(
        ["lnk", "agent", "harness-permission", "remove"]
            .map(String::from)
            .into_iter()
            .chain(flags)
            .chain(["--harness".into(), harness.into()]),
    )
}

/// Restarts agent `settings` with what it may do now, if it runs and you
/// say so (or gave `--yes`): in a terminal, said, since only its terminal
/// restarts it, and otherwise, how to restart it later.
pub(crate) async fn restart_agent(out: &mut Out, settings: &mut Settings) -> Result<(), Report> {
    let agent = Name::new("agent", &settings.name);
    if service::foreground_running(&settings.name) {
        out.block(Report::warning(sentence!(
            "{} runs in a terminal, so it runs with what it had until you start it again there.",
            &agent
        )));
        return Ok(());
    }
    if !service::running(&settings.name) {
        return Ok(());
    }
    settings.save()?;
    // Asked first: a change shouldn't restart what you're talking to
    // unawares. `--yes` restarts it; with no terminal to ask, it waits.
    let yes = crate::permissions::YES.load(std::sync::atomic::Ordering::Relaxed);
    if !restart_now(out, &agent, yes)? {
        out.block(Note(sentence!(
            "{} runs as it was until it restarts: {} restarts it.",
            &agent,
            Value::Command(cmd!("lnk agent restart"))
        )));
        return Ok(());
    }
    out.block(Note(sentence!("Restarting {} with them.", &agent)));
    let name = settings.harness().to_string();
    up(out, settings, &name, false).await?;
    Ok(())
}

/// Whether to restart a running agent now: given `--yes`, or answered
/// yes in a terminal. With no terminal to ask in, it waits.
fn restart_now(out: &mut Out, agent: &Name, yes: bool) -> Result<bool, Report> {
    if yes {
        return Ok(true);
    }
    Ok(out.can_ask()
        && out.ask(Question::confirm(
            sentence!("Restart {} now, so it has the change?", agent),
            true,
        ))?)
}

/// Whether harness `name` declares what `given` names (`lnk-<name>
/// needs`) for an agent here that runs it, or did when you answered it: a
/// risk by its name, or a spec its needs' cover.
pub(crate) fn declares(name: &str, given: &Given, grants: &Grants) -> bool {
    let here = Os::here();
    match given {
        Given::Risk(risk) if grants.answered.contains(risk) => return true,
        Given::Spec(spec) if grants.asked.covers(spec, here).unwrap_or(false) => return true,
        _ => {}
    }
    let Ok(adapter) = Adapter::find(name) else {
        return false;
    };
    let asks = |s: &Settings| {
        adapter.needs(s).is_ok_and(|needs| match given {
            Given::Risk(risk) => needs.risks.iter().any(|r| r.name == *risk),
            Given::Spec(spec) => needs.checked().is_ok_and(|checked| {
                checked
                    .iter()
                    .fold(Spec::default(), |all, (need, _)| all.join(need))
                    .covers(spec, here)
                    .unwrap_or(false)
            }),
        })
    };
    // Each agent's settings make its own answer, so each is asked, and a
    // refusal asks them all: a few at a time, side by side.
    let agents: Vec<Settings> = settings::names()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|n| Settings::load_without_keys(&n).ok())
        .filter(|s| s.harness() == name)
        .collect();
    let each = agents.len().div_ceil(NEEDS_AT_ONCE).max(1);
    std::thread::scope(|scope| {
        let asked: Vec<_> = agents
            .chunks(each)
            .map(|some| scope.spawn(|| some.iter().any(asks)))
            .collect();
        asked.into_iter().any(|one| one.join().unwrap_or(false))
    })
}

/// How many agents' `needs` [`declares`] asks at once.
pub(crate) const NEEDS_AT_ONCE: usize = 8;

/// What `lnk agent harness-permission add` and `remove` are given: a risk of the
/// harness's own, or a spec's flags.
pub(crate) enum Given {
    Risk(String),
    Spec(Box<Spec>),
}

impl Given {
    /// The risk or the flags, not both, nor neither.
    pub(crate) fn of(
        risk: Option<String>,
        fields: link_spec::FieldFlags,
        verb: &str,
    ) -> Result<Self, Report> {
        let spec: Spec = link_spec::over(&Spec::default(), &fields)?;
        if !spec.teams.is_empty() {
            return Err(Report::error(sentence!(
                "{} takes no team's folders.",
                cmd!("lnk agent harness {}", verb.to_string())
            ))
            .why(prose!(
                "A team's shared folder is an agent's grant, given with {}.",
                cmd!("lnk agent team add")
            ))
            .exit(Exit::Usage));
        }
        match (risk, spec == Spec::default()) {
            (Some(risk), true) => Ok(Given::Risk(risk)),
            (None, false) if spec.name.is_none() && spec.version.is_none() => {
                Ok(Given::Spec(Box::new(spec.on(Os::here())?)))
            }
            _ => Err(Report::error(sentence!(
                "{} takes a risk the harness names, or a spec's flags.",
                cmd!("lnk agent harness {}", verb.to_string())
            ))
            .why(prose!(
                "A spec's flags are given as {} takes them, such as --network.mode host.",
                cmd!("lnk sandbox run")
            ))
            .exit(Exit::Usage)),
        }
    }

    /// As `deny` names it.
    fn key(&self) -> String {
        match self {
            Given::Risk(risk) => risk.clone(),
            Given::Spec(spec) => link_sandbox_client::flags::shown(spec),
        }
    }
}

/// `lnk agent harness-permission add`: grants what harness `name` declares
/// (`needs`): a risk of its own, or a spec its needs cover, remembered as
/// answered. Anything it doesn't declare is refused: what you grant an
/// agent is `lnk agent permission add`'s, and a harness only ever holds what it
/// asked for.
pub(crate) async fn allow_need(out: &mut Out, name: &str, given: Given) -> Result<Done, Report> {
    let harness = Name::new("harness", name);
    let mut grants = Grants::load_or_warn(name);
    let readable = Grants::load(name).is_ok();
    let key = given.key();
    if !declares(name, &given, &grants) {
        return Err(Report::error(sentence!(
            "{} doesn't ask for {} for the agents here.",
            &harness,
            Value::from(key.clone())
        ))
        .why(prose!("A harness holds only what it asks for."))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk agent permission add"),
            "grant your agent what you choose",
        ));
    }
    let before = grants.clone();
    match &given {
        Given::Risk(risk) => {
            grants.risks.insert(risk.clone());
            grants.answered.insert(risk.clone());
        }
        Given::Spec(spec) => {
            grants.spec = grants.spec.join(spec);
            grants.asked = grants.asked.join(spec);
        }
    }
    grants.check()?;
    if grants == before && readable {
        out.block(Note(sentence!(
            "{} has {} already.",
            &harness,
            Value::from(key)
        )));
        return Ok(Done::shown());
    }
    grants.save(name)?;
    restart_running(out, name).await?;
    Ok(Done::success(sentence!(
        "The grants of {} now include {}.",
        &harness,
        Value::from(key.clone())
    ))
    .changed("Its use", Value::from(grants::about(&key).to_string()))
    .changed("Kept in", Path::new(grants::path(name)?)))
}

/// `lnk agent harness-permission remove`: takes back what harness `name` asked for
/// and you allowed: a risk of its own, or what a spec's flags name. It
/// stays answered, so it isn't asked again (`lnk agent harness-permission add`
/// gives it back, `reset` asks again); a harness that can't run
/// without it refuses to start.
pub(crate) async fn deny_need(out: &mut Out, name: &str, given: Given) -> Result<Done, Report> {
    let harness = Name::new("harness", name);
    let mut grants = Grants::load_or_warn(name);
    let readable = Grants::load(name).is_ok();
    let key = given.key();
    // A risk it never named is no need of its own to take back: refused,
    // as `add` refuses one, rather than said to be already off.
    if let Given::Risk(risk) = &given
        && !grants.risks.contains(risk)
        && !declares(name, &given, &grants)
    {
        return Err(Report::error(sentence!(
            "{} never asks for {}, so there's nothing to take back.",
            &harness,
            Value::from(key.clone())
        ))
        .why(prose!("A harness's risk is one it names itself."))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk agent harness-permission list --harness {}", &harness),
            "see what it asks for",
        ));
    }
    let before = grants.clone();
    match &given {
        Given::Risk(risk) => {
            if grants.risks.remove(risk) {
                grants.answered.insert(risk.clone());
            }
        }
        Given::Spec(spec) => {
            if let Some(why) = grants.spec.widened_by_taking(spec)? {
                return Err(Report::error(sentence!(
                    "Nothing was denied, since a deny never widens."
                ))
                .why(Sentence::adopt(&why.to_string()))
                .exit(Exit::Usage));
            }
            grants.spec = grants.spec.without(spec);
            if grants.spec != before.spec {
                grants.asked = grants.asked.join(spec);
            }
        }
    }
    if grants == before && readable {
        out.block(Note(sentence!(
            "{} doesn't have {} as a need.",
            &harness,
            Value::from(key)
        )));
        return Ok(Done::shown().next(
            cmd!("lnk agent permission remove"),
            "take back what you grant an agent",
        ));
    }
    grants.save(name)?;
    restart_running(out, name).await?;
    Ok(Done::success(sentence!(
        "The grants of {} no longer include {}.",
        &harness,
        Value::from(key.clone())
    ))
    .next(
        cmd!("lnk agent harness-permission add")
            .and(&key)
            .and(&format!("--harness {name}")),
        "give it back; it isn't asked again",
    )
    .next(
        cmd!("lnk agent harness reset --harness {}", &harness),
        "ask on its next start",
    ))
}

/// `lnk agent harness reset`: forgets the needs harness `name` asked
/// for and you answered, so its next start asks again what it needs and
/// doesn't have. What it has stays.
pub(crate) fn reset(name: &str) -> Result<Done, Report> {
    let harness = Name::new("harness", name);
    let mut grants = Grants::load_or_warn(name);
    let readable = Grants::load(name).is_ok();
    if grants.answered.is_empty() && grants.asked == Spec::default() && readable {
        return Ok(
            Done::success(sentence!("{} has no answers to forget.", &harness))
                .next(cmd!("lnk agent start"), "its next start asks what it needs"),
        );
    }
    grants.answered.clear();
    grants.asked = Spec::default();
    grants.save(name)?;
    Ok(
        Done::success(sentence!("The answers of {} are forgotten.", &harness)).next(
            cmd!("lnk agent start"),
            "its next start asks again, in a terminal, for what it needs and doesn't have",
        ),
    )
}

/// Saves agent `agent`'s settings, and restarts it with them if it runs.
pub(crate) async fn restart_with(out: &mut Out, agent: &mut Settings) -> Result<(), Report> {
    let running = service::running(&agent.name);
    agent.save()?;
    if running {
        let name = agent.harness().to_string();
        up(out, agent, &name, false).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A change restarts a running agent only when you say so: `--yes`,
    /// or yes in a terminal. With no terminal, it isn't restarted.
    #[test]
    fn a_change_restarts_a_running_agent_only_when_asked() {
        let agent = Name::new("agent", "main");
        let (mut out, _) = Out::capture(false, &["lnk-agent", "tool", "add", "web"]);
        assert!(!restart_now(&mut out, &agent, false).unwrap());
        assert!(restart_now(&mut out, &agent, true).unwrap());
    }

    /// A harness holds only what it asked for, and a team's folders are
    /// an agent's grant: `harness allow --teams` is refused.
    #[test]
    fn a_harness_grant_takes_no_teams() {
        let binding = link_spec::Binding::of::<Spec>();
        let given = |flag: &str| {
            let args =
                ["lnk-agent", "harness-permission", "add", flag].map(std::ffi::OsString::from);
            link_spec::split(args, Some(&binding), None).unwrap().0
        };
        let teams = given("--teams={research={read=[\"sources\"]}}");
        assert!(!teams.is_empty());
        let Err(refused) = Given::of(None, teams, "allow") else {
            panic!("a harness grant took a team's folders");
        };
        assert!(format!("{refused:?}").contains("team"), "{refused:?}");
        // The loosening: another section's flag is a harness's grant.
        assert!(matches!(
            Given::of(None, given("--network.mode=host"), "allow"),
            Ok(Given::Spec(_))
        ));
    }

    fn spec(text: &str) -> Spec {
        link_spec::parse(text).unwrap()
    }

    /// A deny of yours that another layer still grants is said, kind by
    /// kind; one nothing grants any more isn't.
    #[test]
    fn a_deny_another_layer_still_grants_is_said() {
        let os = Os::here();
        let harness = spec(
            "[internet]\nhosts = [\"a.com\"]\n[files]\nread = [\"/srv\"]\n\
             [tools]\nnames = [\"gh\"]\n",
        );
        let given = spec(
            "[internet]\nhosts = [\"a.com\", \"b.com\"]\n[files]\nread = [\"/srv\"]\n\
             [tools]\nnames = [\"gh\"]\n",
        );
        let kept = still_granted(&[Spec::default(), harness], &given, os);
        assert_eq!(kept.len(), 3, "{kept:?}");
        let shown: Vec<_> = kept.iter().map(link_sandbox_client::flags::shown).collect();
        assert!(!shown.iter().any(|k| k.contains("b.com")), "{shown:?}");
        // What takes them back is that command, a word each, as typed.
        let back = harness_remove(&kept[..1], "link");
        let words: Vec<_> = serde_json::from_value(serde_json::to_value(&back).unwrap()).unwrap();
        let mut expected: Vec<String> = ["lnk", "agent", "harness-permission", "remove"]
            .map(String::from)
            .into();
        expected.extend(link_spec::flags_of(&kept[0]).unwrap());
        expected.extend(["--harness".into(), "link".into()]);
        assert_eq!(words, expected);
        assert!(words.iter().all(|w: &String| !w.contains(' ')), "{words:?}");
        // The loosening: with nothing else granting it, a deny holds.
        assert!(still_granted(&[Spec::default()], &given, os).is_empty());
    }
}
