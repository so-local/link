//! What an arriving agent keeps of this machine's: the model each
//! machine can reach, and the sandbox granted here.

use super::*;

/// The model to run with here: this machine's own when it's local and
/// answers (each machine keeps the model it can reach), else `given` (a
/// key for this move), else the one that came. Fails when that is a
/// local model this machine can't reach, or one that came from another
/// machine than the one running the move (`vouched` false): a box could
/// name any port of this machine's loopback that answers, which the
/// sandbox would then open to the harness as its model's.
pub fn settle_model(
    here: Option<&Model>,
    came: Option<&Model>,
    given: Option<Model>,
    vouched: bool,
    answers: impl Fn(&Model) -> bool,
) -> anyhow::Result<Model> {
    if let Some(here) = here
        && here.is_local()
        && answers(here)
    {
        return Ok(here.duplicate());
    }
    let model = match (given, came) {
        (Some(given), _) => given,
        (None, Some(came)) => came.duplicate(),
        (None, None) => bail!("the agent that came has no model"),
    };
    if model.is_local() && !(vouched && answers(&model)) {
        bail!(
            "your agent used a model on the machine it came from ({}), which isn't \
             here: move it again with an API key (set {})",
            model.name.as_deref().unwrap_or("local"),
            crate::settings::KEY_VARS_TEXT
        );
    }
    Ok(model)
}

/// Which of an agent's models came in a move ([`arrived`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Its model, settled first ([`settle_model`]).
    Model,
    /// Its fallback (`lnk agent start --model.fallback_model`).
    Fallback,
}

/// A model that came in a move, as it's kept here: a hosted one reached
/// at its provider's own address, never one the stream names; a local
/// one its model as [`settle_model`] vouched for it, but never its
/// fallback, which `lnk agent start` refuses on any machine, so one
/// that came from no lnk and could name any port here: none.
pub fn arrived(mut model: Model, role: Role) -> Option<Model> {
    match (model.is_local(), role) {
        (true, Role::Model) => Some(model),
        (true, Role::Fallback) => None,
        (false, _) => {
            model.base_url = None;
            Some(model)
        }
    }
}

/// What the agent may do here: what the machine running the move gave
/// (`given`: the agent's own, when it leaves that machine), else what it
/// has here already (`here`, coming back), else what `came`, as it is on
/// this machine, but what a stream alone never grants: what opens this
/// machine itself (your home, `[files] read = ["~"]`; its loopback,
/// `[localhost]`; its network past the proxy, `[network]` and `[lan]`;
/// your browser with your logins, `[browser]`), its endpoints (`[internet]
/// tcp`), its tools that run outside (`[tools]`) and the programs of its
/// `[commands]` named by a path or `*`, since the machine it came from
/// wrote it; none, Link's. Its teams' shared folders (`[teams]`) are
/// derived again here, given or come, by `teams` ([`TeamGrant`]): only a
/// team this machine keeps the member of. With what's left out, each as
/// `lnk agent permission add` takes it (`--internet.tcp github.com:22`), or `team <name>`.
pub fn settle_sandbox(
    given: Option<&Sandbox>,
    here: Option<&Sandbox>,
    came: Option<&Sandbox>,
    teams: TeamGrant<'_>,
) -> (Option<Sandbox>, Vec<String>) {
    if let Some(given) = given {
        let mut kept = given.clone();
        kept.spec.teams = derive_teams(&given.spec.teams, true, teams).0;
        return (Some(kept), Vec::new());
    }
    if let Some(here) = here {
        return (Some(here.clone()), Vec::new());
    }
    let Some(came) = came else {
        return (None, Vec::new());
    };
    let mut spec = came
        .spec
        .on(link_sandbox_contract::spec::Os::here())
        .unwrap_or_default();
    let mut lost = link_sandbox_client::Spec {
        network: spec.network.take(),
        lan: spec.lan.take(),
        localhost: spec.localhost.take(),
        tools: spec.tools.take(),
        browser: spec.browser.take(),
        ..Default::default()
    };
    if let Some(tcp) = spec.internet.as_mut().and_then(|i| i.tcp.take()) {
        lost.internet = Some(link_sandbox_contract::spec::Internet {
            tcp: Some(tcp),
            ..Default::default()
        });
    }
    if let Some(files) = &mut spec.files
        && let Some(read) = &mut files.read
        && read.iter().any(|r| r == "~")
    {
        read.retain(|r| r != "~");
        if read.is_empty() {
            files.read = None;
        }
        lost.files = Some(link_sandbox_contract::spec::Files {
            read: Some(vec!["~".into()]),
            ..Default::default()
        });
    }
    if spec
        .files
        .as_ref()
        .is_some_and(|f| f.write.is_none() && f.read.is_none())
    {
        spec.files = None;
    }
    // A program by its name, found on this machine's own PATH; never one
    // by a path the other machine chose, or any (`*`).
    let named = |c: &String| !c.starts_with('/') && c != "*";
    if let Some(allow) = spec.commands.as_mut().and_then(|c| c.allow.as_mut()) {
        let unnamed: Vec<String> = allow.iter().filter(|c| !named(c)).cloned().collect();
        allow.retain(named);
        if !unnamed.is_empty() {
            lost.commands = Some(link_sandbox_contract::spec::Commands {
                allow: Some(unnamed),
                ..Default::default()
            });
        }
    }
    let (teams, lost_teams) = derive_teams(&came.spec.teams, false, teams);
    let left_out = lost
        .items()
        .iter()
        .map(link_sandbox_client::flags::shown)
        .chain(lost_teams.iter().map(|t| format!("team {t}")))
        .collect();
    let kept = Sandbox {
        spec: link_sandbox_contract::spec::Spec { teams, ..spec },
    };
    (Some(kept), left_out)
}

/// How the machine an agent arrives on derives its grant of team `team`'s
/// folder again from that team's current roster, given the grant the
/// agent arrived with and whether the machine running the move vouches
/// for it (`true`) or only the stream says it (`false`): the grant it
/// keeps here, or none when this machine keeps no such member of it.
pub type TeamGrant<'a> = &'a dyn Fn(
    &str,
    &link_harness_contract::harness::TeamFolders,
    bool,
) -> Option<link_harness_contract::harness::TeamFolders>;

/// `grants` derived again by `teams`: those kept, and the names of those
/// that weren't but reached some of their team's folder.
fn derive_teams(
    grants: &BTreeMap<String, link_harness_contract::harness::TeamFolders>,
    vouched: bool,
    teams: TeamGrant<'_>,
) -> (
    BTreeMap<String, link_harness_contract::harness::TeamFolders>,
    Vec<String>,
) {
    let mut kept = BTreeMap::new();
    let mut lost = Vec::new();
    for (team, grant) in grants {
        match teams(team, grant, vouched) {
            Some(derived) if !derived.is_empty() => {
                kept.insert(team.clone(), derived);
            }
            _ if !grant.is_empty() => lost.push(team.clone()),
            _ => {}
        }
    }
    (kept, lost)
}
