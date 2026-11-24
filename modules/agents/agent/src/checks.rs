//! The checks `actions.toml` names beside each argument's type: a rule a
//! type can't say, run on every read of the command's arguments.

use crate::cli::{
    AgentReceiveArgs, AgentShowArgs, HarnessCheckArgs, HarnessDeleteArgs, HarnessListArgs,
    HarnessPermissionAddArgs, HarnessPermissionListArgs, HarnessPermissionRemoveArgs,
    HarnessResetArgs, HarnessRunArgs, HarnessSetArgs, HarnessShowArgs, QuestionListArgs,
    SkillAddArgs,
};

/// A command that names a harness, by `--harness` or its argument.
pub trait Harnessed {
    fn harness(&self) -> Option<&str>;
}

macro_rules! harnessed {
    ($($args:ty),*) => {
        $(impl Harnessed for $args {
            fn harness(&self) -> Option<&str> {
                self.harness.as_deref()
            }
        })*
    };
}

harnessed!(
    AgentShowArgs,
    HarnessCheckArgs,
    HarnessDeleteArgs,
    HarnessListArgs,
    HarnessPermissionAddArgs,
    HarnessPermissionRemoveArgs,
    HarnessPermissionListArgs,
    HarnessResetArgs,
    HarnessRunArgs,
    HarnessShowArgs
);

impl Harnessed for HarnessSetArgs {
    fn harness(&self) -> Option<&str> {
        Some(&self.harness)
    }
}

/// A harness is named as one: lowercase letters, digits and dashes, never
/// its module's name nor one of `lnk agent`'s own commands.
pub fn harness_name(args: &impl Harnessed) -> Result<(), String> {
    match args.harness() {
        Some(name) => harness(name),
        None => Ok(()),
    }
}

/// Whether `name` names a harness.
pub fn harness(name: &str) -> Result<(), String> {
    if let Some(harness) = crate::grants::harness_of_module(name) {
        return Err(format!(
            "{name:?} is the module of harness {harness}: name the harness"
        ));
    }
    if crate::cli::agent_command(name) {
        return Err(format!(
            "{name:?} is an lnk agent command, which no harness can be named"
        ));
    }
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        let known: Vec<_> = crate::settings::KNOWN.iter().map(|(k, _, _)| *k).collect();
        return Err(format!(
            "{name:?} is not a harness name (Link's are {})",
            known.join(", ")
        ));
    }
    Ok(())
}

/// A command that names a harness's risk.
pub trait Risked {
    fn risk(&self) -> Option<&str>;
}

impl Risked for HarnessPermissionAddArgs {
    fn risk(&self) -> Option<&str> {
        self.risk.as_deref()
    }
}

impl Risked for HarnessPermissionRemoveArgs {
    fn risk(&self) -> Option<&str> {
        self.risk.as_deref()
    }
}

/// A risk is named as a harness names one; what a harness needs beside
/// its risks is a spec's flags.
pub fn risk_name(args: &impl Risked) -> Result<(), String> {
    let Some(name) = args.risk() else {
        return Ok(());
    };
    if name == "unsandboxed" {
        return Err(crate::grants::NOT_A_GRANT.to_string());
    }
    match link_harness_contract::harness::is_risk_name(name) {
        true => Ok(()),
        false => Err(format!(
            "{:?} isn't a risk's name; what a harness needs beside its risks is a spec's \
             flags (--network.mode host), as lnk sandbox run takes them",
            link_format::clean(name)
        )),
    }
}

/// `--have` asks what this machine holds of one agent, named by
/// `--agent-id`.
pub fn receive(args: &AgentReceiveArgs) -> Result<(), String> {
    match args.have && args.agent_id.is_none() {
        true => Err("--have needs --agent-id <agent id>".into()),
        false => Ok(()),
    }
}

/// A skill comes from a folder or a URL.
pub fn skill_add(args: &SkillAddArgs) -> Result<(), String> {
    match (&args.path, &args.url) {
        (None, None) => Err("give a skill's folder, or --url <url>".into()),
        _ => Ok(()),
    }
}

/// `--reply`, `--draft` and `--submit` answer the question its code names.
pub fn questions(args: &QuestionListArgs) -> Result<(), String> {
    let answers = args.reply.is_some() || args.draft.is_some() || args.submit.is_some();
    match answers && args.code.is_none() {
        true => Err("--reply, --draft and --submit answer a question: give its code".into()),
        false => Ok(()),
    }
}

/// A model is named as one: a provider and its model, or an alias.
pub fn model_route(args: &crate::cli::ModelSetArgs) -> Result<(), String> {
    link_model_contract::selection::routed(&args.model).map(|_| ())
}

/// A command on a team another account keeps, which it names.
pub trait Placed {
    fn account(&self) -> Option<&str>;
    fn cloud(&self) -> Option<&str>;
}

impl Placed for crate::cli::TeamJoinArgs {
    fn account(&self) -> Option<&str> {
        self.account.as_deref()
    }
    fn cloud(&self) -> Option<&str> {
        self.cloud.as_deref()
    }
}

impl Placed for crate::cli::TeamLeaveArgs {
    fn account(&self) -> Option<&str> {
        self.account.as_deref()
    }
    fn cloud(&self) -> Option<&str> {
        self.cloud.as_deref()
    }
}

/// Joining or leaving a team names the account that keeps it and the
/// cloud account that hosts it.
pub fn team_place(args: &impl Placed) -> Result<(), String> {
    match (args.account(), args.cloud()) {
        (Some(_), Some(_)) => Ok(()),
        _ => {
            Err("name the team's account and cloud: --account <account> and --cloud <cloud>".into())
        }
    }
}
