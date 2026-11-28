//! `~/.lnk/safe/boxes/boxes.toml`: the boxes you made, and where.

use std::path::{Path, PathBuf};

use link_format::{Exit, Name, Report, Value, WithinExt, cmd, prose, sentence};
use serde::{Deserialize, Serialize};

use crate::said::said;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<link_cloud_contract::prices::Price>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<u64>,
    /// Its name, as you type it (`aws-1`) and as its `lnk-box` tag says.
    pub name: String,
    /// Its installation ID (its `installation.toml` there, its `lnk-machine` tag):
    /// its machine is `lnk-<id>` in the cloud. Empty for a box made before
    /// IDs, until Link reads it from the box.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub machine_id: String,
    /// The cloud account it's in (`lnk cloud list`).
    pub account: String,
    /// `aws` or `gcp`.
    pub kind: String,
    /// Its region (aws) or zone (gcp).
    pub place: String,
    /// Its id in the cloud.
    pub id: String,
    pub size: String,
    /// Its public address, as last seen (it changes when it restarts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// As last seen: `running`, `stopped`, or `starting` until set up.
    pub state: String,
    /// When it was made, in seconds since 1970.
    pub created: u64,
    /// Its SSH host keys, from its console at the first boot, pinned at
    /// each address it has. None when the console didn't say: then the
    /// first contact's are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_keys: Vec<String>,
    /// Whether its machine has its tags (name, ID, owner): a box made
    /// before them gets them when Link next reaches it.
    #[serde(default)]
    pub tagged: bool,
    /// Found in the cloud, not made here: this computer's key may not let
    /// it in yet, nor its host keys be pinned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_cloud: bool,
    /// The spec it was made from, its flags laid over, for `lnk box spec`:
    /// none for a box made before specs, or found in a cloud. Kept here,
    /// never on the box, which could write it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<toml::Table>,
    /// A spot machine, which its cloud may stop.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spot: bool,
    /// Stopped by its cloud, not by `lnk box stop`, as last seen: a spot
    /// box taken back, its disk (its login, an agent's state) kept.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped_by_cloud: bool,
    /// Whether `stopped_by_cloud` is Link's guess, its adapter unable to
    /// say: a spot box found stopped that ran when last seen here, as one
    /// another computer stopped is too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stop_guessed: bool,
    /// Whether it trusts this computer's SSH certificates, and Link's
    /// plain keys are gone from it: a box made here with certificates
    /// does from its first boot; one made before them, or found in the
    /// cloud, once Link next reaches it ([`crate::reach`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub certified: bool,
    /// The environment it was made with, as `lnk box create` captured it:
    /// started again from these files at each `lnk box start`, never
    /// from its source, which may have changed since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<link_box_contract::EnvironmentInput>,
    /// A box on this machine with no VM, which Box placed itself: its
    /// home and what runs its commands. None for a cloud box. Box tells
    /// the two apart by this, never by a provider's files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub here: Option<Here>,
    /// Whether it follows the copy of releases this machine staged on it
    /// when it was made ([`crate::setup::staged`]): a box made from a
    /// bundle, or releases in a folder here. Every `lnk` run there installs
    /// from that copy, whatever this machine follows by then.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub staged: bool,
}

/// A box Box placed on this machine with no VM ([`Machine::here`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Here {
    /// Its home: `~/Link/Boxes/<name>/home`, a data folder.
    pub home: PathBuf,
    /// What runs its commands.
    pub confinement: Confinement,
}

/// What runs a box's commands on this machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Confinement {
    /// Its sandbox: the spec captured, absolute, when it was made, its
    /// home among what it writes. Never read from its source again.
    Sandbox { spec: String },
    /// This machine directly, with the user's own access.
    Direct,
}

/// Whether a cloud's `state` is a machine that's stopped (Google Cloud
/// says `terminated`).
pub fn stopped(state: &str) -> bool {
    matches!(state, "stopped" | "stopping" | "terminated" | "suspended")
}

impl Machine {
    /// Takes the state its cloud says it has, and whether the cloud
    /// stopped it (`by_cloud`, from its adapter). When the adapter can't
    /// say, a spot box that ran when last seen and is found stopped is
    /// guessed stopped by its cloud: another computer's `lnk box stop`
    /// looks the same.
    pub fn seen(&mut self, state: String, by_cloud: Option<bool>) {
        match (stopped(&state), by_cloud) {
            (false, _) => (self.stopped_by_cloud, self.stop_guessed) = (false, false),
            (true, Some(said)) => (self.stopped_by_cloud, self.stop_guessed) = (said, false),
            (true, None) => {
                if self.spot && self.state == "running" {
                    (self.stopped_by_cloud, self.stop_guessed) = (true, true);
                }
            }
        }
        self.state = state;
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Boxes {
    #[serde(default, rename = "box")]
    pub boxes: Vec<Machine>,
}

link_files::specification!(Boxes, include_str!("boxes.schema.toml"));

/// The boxes, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Boxes>()
}

impl Boxes {
    pub fn path() -> anyhow::Result<PathBuf> {
        Ok(crate::ssh::dir()?.join("boxes.toml"))
    }

    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&Self::path()?)
    }

    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                link_files::saved::parse(&text).within(format!("reading {}", path.display()))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err).within(format!("reading {}", path.display())),
        }
    }

    /// Changes the saved boxes: read again and written back under a lock,
    /// so two `lnk box` commands at once (a `create` takes minutes) keep
    /// each other's changes. The lock is held only for `change`.
    pub fn update<T>(change: impl FnOnce(&mut Boxes) -> T) -> anyhow::Result<T> {
        Self::update_at(&Self::path()?, change)
    }

    pub fn update_at<T>(path: &Path, change: impl FnOnce(&mut Boxes) -> T) -> anyhow::Result<T> {
        let _lock = lock(&path.with_extension("lock"))?;
        let mut boxes = Self::load_from(path)?;
        let changed = change(&mut boxes);
        link_files::saved::write(path, &boxes).within(format!("writing {}", path.display()))?;
        Ok(changed)
    }

    /// Saves what one command learned from the clouds about the boxes it
    /// loaded as `before`: a box another command changed or removed
    /// meanwhile keeps that change, a box new to both is added.
    pub fn merge(&mut self, before: &Boxes, learned: &Boxes) {
        for machine in &learned.boxes {
            let was = before.get(&machine.name);
            let now = self.get(&machine.name);
            if was == now && was != Some(machine) {
                self.put(machine.clone());
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&Machine> {
        self.boxes.iter().find(|m| m.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Machine> {
        self.boxes.iter_mut().find(|m| m.name == name)
    }

    pub fn put(&mut self, machine: Machine) {
        self.boxes.retain(|m| m.name != machine.name);
        let name = machine.name.clone();
        self.boxes.push(machine);
        self.boxes.sort_by(|a, b| a.name.cmp(&b.name));
        self.claim_address(&name);
    }

    /// Box `name`'s address is its alone: another box last seen there (one
    /// stopped since, its address given to this one) no longer has one.
    /// So the box at an address, which its host keys, certificate and
    /// tunnel are found by, is never another's.
    pub fn claim_address(&mut self, name: &str) {
        let Some(ip) = self.get(name).and_then(|m| m.ip.clone()) else {
            return;
        };
        for other in self.boxes.iter_mut().filter(|m| m.name != name) {
            if other.ip.as_deref() == Some(ip.as_str()) {
                other.ip = None;
            }
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<Machine> {
        let index = self.boxes.iter().position(|m| m.name == name)?;
        Some(self.boxes.remove(index))
    }

    /// The box `name`, or the only one when no name is given.
    pub fn pick(&self, name: Option<&str>) -> anyhow::Result<&Machine> {
        match name {
            Some(name) => self.get(name).ok_or_else(|| {
                said(
                    Report::error(sentence!(
                        "Couldn't find a box named {}.",
                        Name::new("box", name)
                    ))
                    .next(cmd!("lnk box list"), "list your boxes")
                    .exit(Exit::Usage),
                )
            }),
            None => match self.boxes.as_slice() {
                [] => Err(said(
                    Report::error(sentence!("Couldn't pick a box, since you have none."))
                        .next(cmd!("lnk box create <cloud>"), "make one")
                        .exit(Exit::Usage),
                )),
                [only] => Ok(only),
                several => {
                    let names: Vec<Value> = several
                        .iter()
                        .map(|m| Name::new("box", &m.name).into())
                        .collect();
                    Err(said(
                        Report::error(sentence!("Couldn't tell which box you mean."))
                            .why(prose!("You have several: {}.", Value::List(names)))
                            .next(cmd!("lnk box list"), "list your boxes")
                            .exit(Exit::Usage),
                    ))
                }
            },
        }
    }
}

impl Boxes {
    /// The first free `<account>-<n>`, counting `taken` names too (boxes
    /// found in the cloud).
    pub fn next_name(&self, account: &str, taken: &[String]) -> String {
        (1..)
            .map(|n| format!("{account}-{n}"))
            .find(|name| self.get(name).is_none() && !taken.contains(name))
            .expect("a free name")
    }
}

/// Holds `path`, a lock file, until the returned file is dropped: the
/// next `lnk box` command to ask waits.
pub fn lock(path: &Path) -> anyhow::Result<std::fs::File> {
    link_files::lock_file(path)
}

/// A box's name: lowercase letters, digits and dashes, starting with a
/// letter, at most 30, and not one of Link's words (`link_machine::machine::machine`'s
/// rules for a name).
pub fn check_name(name: &str) -> anyhow::Result<()> {
    if name.ends_with('-') || link_machine::machine::check_name(name).is_err() {
        return Err(said(
            Report::error(sentence!(
                "Couldn't use {} as a box's name.",
                Value::from(name.to_string())
            ))
            .why(prose!(
                "A name is lowercase letters, digits and dashes, starting with a letter, and not {}.",
                link_machine::machine::WORDS.join(", ")
            ))
            .exit(Exit::Usage),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(name: &str, id: &str) -> Machine {
        Machine {
            price: None,
            deadline: None,
            name: name.into(),
            machine_id: String::new(),
            account: "aws".into(),
            kind: "aws".into(),
            place: "us-east-2".into(),
            id: id.into(),
            size: "t3.micro".into(),
            ip: None,
            state: "running".into(),
            created: 0,
            host_keys: Vec::new(),
            tagged: true,
            from_cloud: false,
            spec: None,
            spot: false,
            stopped_by_cloud: false,
            stop_guessed: false,
            certified: false,
            environment: None,
            here: None,
            staged: false,
        }
    }

    /// A folder kept while the test holds it, and `boxes.toml` in it.
    fn temp(name: &str) -> (link_test_kit::TempDir, PathBuf) {
        let dir = link_test_kit::TempDir::new(&format!("box-{name}"));
        let path = dir.join("boxes.toml");
        (dir, path)
    }

    /// Two `lnk box create` at once, each saving its box several times
    /// while the other runs: both boxes are kept.
    #[test]
    fn commands_at_once_keep_each_others_boxes() {
        let (_kept, path) = temp("race");
        let threads: Vec<_> = ["aws-1", "gcp-1"]
            .into_iter()
            .map(|name| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..50 {
                        let mut m = machine(name, &format!("i-{i}"));
                        m.state = if i == 49 { "running" } else { "starting" }.into();
                        Boxes::update_at(&path, |b| b.put(m)).unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let saved = Boxes::load_from(&path).unwrap();
        assert_eq!(saved.boxes.len(), 2);
        assert!(saved.boxes.iter().all(|m| m.id == "i-49"));
    }

    #[test]
    fn a_spot_box_stopped_by_its_cloud_as_its_adapter_says() {
        let mut m = machine("a", "i-1");
        m.spot = true;
        // Said by the adapter: a person stopped it, here or elsewhere.
        m.seen("stopped".into(), Some(false));
        assert!(!m.stopped_by_cloud);
        m.seen("running".into(), None);
        m.seen("stopped".into(), Some(true));
        assert!(m.stopped_by_cloud && !m.stop_guessed);
        m.seen("running".into(), None);
        assert!(!m.stopped_by_cloud);
        // Not said: guessed, and known as a guess.
        m.seen("terminated".into(), None);
        assert!(m.stopped_by_cloud && m.stop_guessed);
        // Not spot: never the cloud's.
        let mut m = machine("b", "i-2");
        m.seen("stopped".into(), None);
        assert!(!m.stopped_by_cloud);
    }

    #[test]
    fn an_address_is_one_boxs() {
        let mut boxes = Boxes {
            boxes: vec![machine("a", "i-1"), machine("b", "i-2")],
        };
        boxes.get_mut("a").unwrap().ip = Some("10.0.0.5".into());
        boxes.claim_address("a");
        // b is given the address a was last seen at, a stopped since.
        let mut b = boxes.get("b").unwrap().clone();
        b.ip = Some("10.0.0.5".into());
        boxes.put(b);
        assert_eq!(boxes.get("a").unwrap().ip, None);
        assert_eq!(boxes.get("b").unwrap().ip.as_deref(), Some("10.0.0.5"));
        let at: Vec<_> = boxes
            .boxes
            .iter()
            .filter(|m| m.ip.as_deref() == Some("10.0.0.5"))
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(at, ["b"]);
    }

    #[test]
    fn a_merge_keeps_what_others_changed() {
        let before = Boxes {
            boxes: vec![
                machine("a", "i-1"),
                machine("b", "i-2"),
                machine("c", "i-3"),
            ],
        };
        let mut learned = before.clone();
        for m in learned.boxes.iter_mut() {
            m.state = "stopped".into();
        }
        learned.put(machine("found", "i-9"));
        // Meanwhile: b removed, c stopped and started with a new address.
        let mut saved = before.clone();
        saved.remove("b");
        saved.get_mut("c").unwrap().ip = Some("1.2.3.4".into());
        saved.merge(&before, &learned);
        let names: Vec<_> = saved.boxes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a", "c", "found"]);
        assert_eq!(saved.get("a").unwrap().state, "stopped");
        assert_eq!(saved.get("c").unwrap().state, "running");
        assert_eq!(saved.get("c").unwrap().ip.as_deref(), Some("1.2.3.4"));
    }
}
