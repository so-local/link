//! The environment file: what every box made from it has, read strictly
//! as every spec is (`link_spec`), each path from the file's
//! folder, and the hash its image is found by
//! (`docs/architecture/specs/env.md`).
//!
//! It names no harness, no cloud and no box: its OS, where the work
//! starts, the values every run is given, the stages that set a machine
//! up and the services it runs, each in the sandbox spec it names.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail, ensure};
use link_digest as digest;
use link_format::WithinExt;
use link_machine::machine::{Arch, Os};
use serde::{Deserialize, Serialize};

mod checks;
use checks::check;
pub use checks::{at, check_path, check_values, check_where, word};
pub(crate) use checks::{check_input, check_input_at};

/// The file's version this `lnk` reads.
pub const VERSION: u64 = 1;

pub use crate::declared::{Env, Group, Ready, Secrets, Service, Stage, StageWhen as When, Step};

/// No secrets: what a file naming none has.
static NO_SECRETS: Secrets = Secrets {
    names: None,
    sealed: None,
};

impl Step {
    /// Its commands: the stage itself, or the group's members.
    pub fn members(&self) -> &[Stage] {
        match self {
            Step::Run(stage) => std::slice::from_ref(stage),
            Step::Parallel(group) => &group.parallel,
        }
    }

    pub fn members_mut(&mut self) -> &mut [Stage] {
        match self {
            Step::Run(stage) => std::slice::from_mut(stage),
            Step::Parallel(group) => &mut group.parallel,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Step::Run(stage) => &stage.name,
            Step::Parallel(group) => &group.name,
        }
    }

    pub fn root(&self) -> bool {
        self.members().iter().any(Stage::root)
    }

    pub fn saved(&self) -> bool {
        self.members().iter().all(Stage::saved)
    }

    pub fn timeout(&self) -> Option<Timeout> {
        match self {
            Step::Run(stage) => stage.timeout,
            Step::Parallel(group) => group.timeout,
        }
    }

    /// The step as every side that runs one has it, each member mapped
    /// by `f`.
    pub fn try_map<U>(
        &self,
        mut f: impl FnMut(&Stage) -> anyhow::Result<U>,
    ) -> anyhow::Result<crate::stage::Stage<U>> {
        crate::stage::Stage::<&Stage>::from(self).try_map(|stage| f(stage))
    }
}

impl<'a> From<&'a Step> for crate::stage::Stage<&'a Stage> {
    fn from(step: &'a Step) -> Self {
        match step {
            Step::Run(stage) => crate::stage::Stage::Command(stage),
            Step::Parallel(group) => crate::stage::Stage::Parallel(crate::stage::Parallel {
                name: group.name.clone(),
                timeout: group.timeout,
                parallel: group.parallel.iter().collect(),
            }),
        }
    }
}

impl crate::stage::Staged for Step {
    type Member = Stage;
    fn members(&self) -> &[Stage] {
        self.members()
    }
    fn group(&self) -> Option<(&str, Option<Timeout>)> {
        match self {
            Step::Run(_) => None,
            Step::Parallel(group) => Some((&group.name, group.timeout)),
        }
    }
}

impl Secrets {
    /// Adds `other`'s names after its own.
    pub fn extend(&mut self, other: &Secrets) {
        for (mine, theirs) in [
            (&mut self.names, other.names()),
            (&mut self.sealed, other.sealed()),
        ] {
            if !theirs.is_empty() {
                mine.get_or_insert_default().extend_from_slice(theirs);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.names().is_empty() && self.sealed().is_empty()
    }

    /// Secrets given to runs as plain variables, by name.
    pub fn names(&self) -> &[String] {
        self.names.as_deref().unwrap_or_default()
    }

    /// Secrets the sandbox's proxy adds to requests, by name.
    pub fn sealed(&self) -> &[String] {
        self.sealed.as_deref().unwrap_or_default()
    }
}

impl Stage {
    /// Whether it's saved in the image (it isn't run at every start).
    pub fn saved(&self) -> bool {
        self.when == When::Image
    }

    pub fn root(&self) -> bool {
        self.user.as_ref().is_some_and(User::is_root)
    }

    /// The files whose contents count in the image's hash.
    pub fn inputs(&self) -> &[PathBuf] {
        self.inputs.as_deref().unwrap_or_default()
    }
}

impl Service {
    pub fn root(&self) -> bool {
        self.user.as_ref().is_some_and(User::is_root)
    }
}

/// An environment file read and checked: the file, its text and folder.
#[derive(Clone, Debug)]
pub struct Read {
    pub file: Env,
    /// The file, its real path: what its secrets' grants name.
    pub path: PathBuf,
    /// Its folder, the real file's, which its paths are from.
    pub dir: PathBuf,
    pub text: String,
}

/// How a job is kept apart, read from what it may do and who started
/// it, never chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Isolation {
    /// In its stage's sandbox, on the box.
    Sandbox,
    /// On a new box, removed at the end.
    OwnBox,
}

impl Isolation {
    pub fn word(self) -> &'static str {
        match self {
            Isolation::Sandbox => "a sandbox",
            Isolation::OwnBox => "a box of its own",
        }
    }
}

/// What an image is built for: its OS and processor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

/// The OS and processor a machine is asked for, each where something
/// names it: none, a box's default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Named {
    pub os: Option<Os>,
    pub arch: Option<Arch>,
}

impl Named {
    /// The platform a box made as it says has.
    pub fn platform(self) -> Platform {
        Platform {
            os: self.os.unwrap_or_default(),
            arch: self.arch.unwrap_or_default(),
        }
    }
}

impl Env {
    /// A file of `name` naming nothing else: what a run of stages alone
    /// has.
    pub fn bare(name: String) -> Env {
        Env {
            name,
            version: VERSION,
            os: None,
            workdir: None,
            variables: BTreeMap::new(),
            secrets: None,
            stage: None,
            service: None,
        }
    }

    /// Its stages, in order.
    pub fn stages(&self) -> &[Step] {
        self.stage.as_deref().unwrap_or_default()
    }

    /// Its services.
    pub fn services(&self) -> &[Service] {
        self.service.as_deref().unwrap_or_default()
    }

    /// The secrets its runs are given, by name.
    pub fn secrets(&self) -> &Secrets {
        self.secrets.as_ref().unwrap_or(&NO_SECRETS)
    }

    /// Where work starts, from the home folder, if it names one.
    pub fn workdir(&self) -> Option<&Path> {
        self.workdir.as_deref().map(Path::new)
    }

    /// The platform of a box made for it, as Link makes one: its OS, or
    /// a box's by default when it names none, on a box's processor.
    pub fn platform(&self) -> Platform {
        self.named().platform()
    }

    /// The OS and processor it names a box for: its OS, if it names one.
    pub fn named(&self) -> Named {
        Named {
            os: self.os,
            arch: None,
        }
    }

    /// Its first service that runs as root, outside any sandbox.
    pub fn root_service(&self) -> Option<&Service> {
        self.services().iter().find(|s| s.root())
    }

    /// Why a job in it needs a box of its own, if it does: `others`, it
    /// was started by someone other than you; `root`, its command runs as
    /// root.
    pub fn own_box_because(&self, others: bool, root: bool) -> Option<String> {
        if others {
            return Some("it was started by someone other than you".into());
        }
        if root {
            return Some("it runs as root".into());
        }
        self.root_service()
            .map(|s| format!("its service {} runs as root", s.name))
    }

    /// A job's isolation, as [`Env::own_box_because`] reads it.
    pub fn isolation(&self, others: bool, root: bool) -> Isolation {
        match self.own_box_because(others, root) {
            Some(_) => Isolation::OwnBox,
            None => Isolation::Sandbox,
        }
    }

    /// Its secrets' names, given as variables.
    pub fn secret_names(&self) -> &[String] {
        self.secrets().names()
    }

    /// Its sealed secrets' names.
    pub fn sealed_names(&self) -> &[String] {
        self.secrets().sealed()
    }

    /// Every file it names, from its folder: each sandbox spec, then
    /// each stage's inputs, each once, in the file's order.
    pub fn named_files(&self) -> Vec<PathBuf> {
        let mut files = self.sandbox_specs();
        for input in self
            .stages()
            .iter()
            .flat_map(|s| s.members())
            .flat_map(|s| s.inputs())
        {
            if !files.contains(input) {
                files.push(input.clone());
            }
        }
        files
    }

    /// The sandbox specs it names: a stage's and a service's.
    pub fn sandbox_specs(&self) -> Vec<PathBuf> {
        let mut specs: Vec<PathBuf> = Vec::new();
        let named = self
            .stages()
            .iter()
            .flat_map(|s| s.members())
            .filter_map(|s| s.sandbox.clone())
            .chain(self.services().iter().filter_map(|s| s.sandbox.clone()));
        for spec in named {
            if !specs.contains(&spec) {
                specs.push(spec);
            }
        }
        specs
    }

    pub fn service(&self, name: &str) -> anyhow::Result<&Service> {
        self.services().iter().find(|s| s.name == name).within({
            let names: Vec<&str> = self.services().iter().map(|s| s.name.as_str()).collect();
            match names.is_empty() {
                true => format!("{} has no services", self.name),
                false => format!(
                    "{} has no service {}: it has {}",
                    self.name,
                    link_format::clean(name),
                    names.join(", ")
                ),
            }
        })
    }
}

/// Reads the environment file at `path`, strictly: an unknown key, a spec
/// or input that's missing, a value that can't be, each refused naming
/// its line.
pub fn read(path: &Path) -> anyhow::Result<Read> {
    read_with(path, &link_spec::FieldFlags::default())
}

/// The environment file and its field flags, resolved from their own
/// source folders. Sidecar files remain child-relative within the
/// environment's bundle after resolution, and every input is checked
/// before it is used or included in the environment's hash.
pub fn read_with(path: &Path, flags: &link_spec::FieldFlags) -> anyhow::Result<Read> {
    let read = link_spec::read::<toml::Table>(path)?;
    link_spec::named_schema::<Env>(&read.text)
        .with_context(|| format!("in {}", read.path.display()))?;
    // The spec's hook checks what the file names without its text, so a
    // file read as written is checked first, each refusal naming its
    // line. One its type refuses is refused, by its line, as it's read
    // just after; with field flags over it, the effective spec is what's
    // checked.
    if flags.is_empty() {
        let mut written = read.spec.clone();
        written.retain(|key, _| !link_spec::RESERVED.contains(&key));
        if let Ok(written) = toml::Value::Table(written).try_into::<Env>() {
            check_input_at(&written, &read.text)
                .with_context(|| format!("in {}", read.path.display()))?;
        }
    }
    let file = link_spec::read_text_at::<Env>(&read.text, &read.dir, flags).with_context(|| {
        let guidance = if read.spec.contains_key("box") {
            ": [box] belongs to a box spec, used with lnk box create --spec <file>"
        } else if read.spec.contains_key("repository") {
            ": replace [repository] with a [[stage]], name = \"clone\", and its git command"
        } else {
            ""
        };
        format!("in {}{guidance}", read.path.display())
    })?;
    let text = if flags.is_empty() {
        read.text
    } else {
        link_spec::print_spec(&file)?
    };
    let checked = Read {
        file,
        path: read.path,
        dir: read.dir,
        text,
    };
    check(&checked).within(format!("in {}", checked.path.display()))?;
    Ok(checked)
}

/// `text` as an environment file, by its type alone: what [`read`] checks
/// against the files it names isn't.
pub fn parse(text: &str) -> anyhow::Result<Env> {
    link_spec::parse_spec(text)
}

pub use crate::stage::{Timeout, User, check_groups, duration};
pub use link_secret_contract::proxy_name;
pub use link_spec::{check_name, check_variable};

/// The hash its image is found by on `platform`: of what builds the
/// image alone, so a changed lockfile makes a new image and nothing else
/// does. That's the file's name, the OS and processor, `workdir`, the
/// variables (given while it's built), and each stage saved in the image,
/// its command, user, sandbox spec and inputs read; never a comment, a
/// secret, a service or a stage run at every start. 32 hex digits, an
/// image's hash (`lnk box image create --hash`).
pub fn hash(read: &Read, platform: Platform) -> anyhow::Result<String> {
    let file = &read.file;
    let contents = |path: &PathBuf| -> anyhow::Result<serde_json::Value> {
        let bytes =
            std::fs::read(read.dir.join(path)).within(format!("reading {}", path.display()))?;
        Ok(serde_json::json!([path, digest::sha256(&bytes)]))
    };
    let mut stages = Vec::new();
    let command_hash = |stage: &Stage| -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({"run": stage.run, "user": stage.user,
            "sandbox": stage.sandbox.as_ref().map(contents).transpose()?,
            "inputs": stage.inputs().iter().map(contents).collect::<anyhow::Result<Vec<_>>>()? }))
    };
    for stage in file.stages().iter().filter(|s| s.saved()) {
        stages.push(match stage {
            Step::Run(command) => command_hash(command)?,
            Step::Parallel(group) => serde_json::json!({"name":group.name,"timeout":group.timeout,
                "parallel":group.parallel.iter().map(command_hash).collect::<anyhow::Result<Vec<_>>>()?}),
        });
    }
    // Its keys sorted here, so the same file is the same text whatever
    // serde_json was built with (`preserve_order` keeps them as written).
    let builds = serde_json::json!({
        "name": file.name,
        "os": platform.os.word(),
        "arch": platform.arch.word(),
        "workdir": file.workdir,
        "variables": file.variables,
        "stages": stages,
    });
    let digest = digest::sha256_bytes(format!("lnk env image 3\n{}", sorted(&builds)).as_bytes());
    Ok(digest::hex(&digest[..16]))
}

/// `value` as compact JSON, each object's keys in order.
fn sorted(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::Value::from(k.as_str()),
                        sorted(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        serde_json::Value::Array(items) => {
            let items: Vec<String> = items.iter().map(sorted).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

impl crate::stage::Member for Stage {
    fn name(&self) -> &str {
        &self.name
    }
    fn root(&self) -> bool {
        self.root()
    }
    fn timeout(&self) -> Option<Timeout> {
        self.timeout
    }
    fn saved(&self) -> bool {
        self.saved()
    }
}

#[cfg(test)]
mod tests;

#[derive(Default, Deserialize)]
struct Machine {
    #[serde(default)]
    os: Option<Os>,
    #[serde(default)]
    arch: Option<Arch>,
    #[serde(flatten)]
    rest: toml::Table,
}
pub fn platform_named(env: Option<&Read>, machine: Option<&Path>) -> anyhow::Result<Named> {
    let env_os = env.and_then(|read| read.file.os);
    let Some(path) = machine else {
        return Ok(Named {
            os: env_os,
            arch: None,
        });
    };
    let read = link_spec::read::<toml::Table>(path)?;
    let at_machine = read.spec.get("machine").cloned();
    let machine: Machine = match at_machine {
        Some(value) => value
            .try_into()
            .map_err(|e| anyhow::anyhow!("{}: [machine]: {e}", path.display()))?,
        None => Machine::default(),
    };
    // A cloud's own table (`[machine.aws]`) naming either would make the
    // image's processor or OS another than the box's.
    for (cloud, table) in &machine.rest {
        if let Some(table) = table.as_table() {
            for key in ["os", "arch"] {
                ensure!(
                    !table.contains_key(key),
                    "{}: [machine.{cloud}] names its {key}, and an image is built for one: \
                     name it in [machine]",
                    path.display()
                );
            }
        }
    }
    if let (Some(env_os), Some(box_os)) = (env_os, machine.os)
        && env_os != box_os
    {
        bail!(
            "{} runs on {}, and the box spec {} names {}: a box of another OS is refused",
            env.map_or("the environment", |r| r.file.name.as_str()),
            env_os.word(),
            path.display(),
            box_os.word()
        );
    }
    Ok(Named {
        os: env_os.or(machine.os),
        arch: machine.arch,
    })
}
