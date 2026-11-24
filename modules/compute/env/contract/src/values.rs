//! What a run is given of the secrets you keep here (`lnk secret`): an
//! environment you run acts as you, so it's given each secret its spec
//! names that you hold, plain or sealed as the spec names it, once you've
//! said yes for that spec ([`crate::approvals`]). A run no one is there
//! to answer is refused a secret it has no yes for, naming the command
//! that gives it ahead. A job, which runs with no one there, is given no
//! secret. And the blank every printed value gets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::bail;
pub use link_secret_contract::Sealing;
use link_secretive::Duplicate;
use link_secretive::Secret;
use serde::{Deserialize, Serialize};

use crate::approvals::{self, Approval};
use crate::file::Read;

/// What stands for a secret in what `lnk` prints and a job leaves.
pub const BLANK: &str = "***";

/// A spec's ask for a secret it has no yes for: put to you.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    /// The name the spec asks it by.
    pub asks: String,
    /// The secret of yours of that name, `secret:<id>`.
    pub resource: String,
    /// The spec's real path, and its `name`.
    pub spec: PathBuf,
    pub name: String,
}

/// Who answers an ask: you, at a terminal (yes or no), or no one.
pub enum Asker<'a> {
    You(&'a mut dyn FnMut(&Ask) -> anyhow::Result<bool>),
    NoOne,
}

/// What a run of the file is given of the secrets it names: each you've
/// said yes to for it, as its file names it; one you refuse, or one with
/// no yes that `asker` can't ask, refused, naming it.
pub fn of(read: &Read, asker: Asker) -> anyhow::Result<Given> {
    given(read, For::Run, asker)
}

/// [`of`], asking you on this process's terminal where there is one,
/// else no one: what a command you run (`lnk env create`, `lnk box
/// create`, `lnk box start`) is given for the environment it starts, which
/// a box it makes is handed then, never reading your Safe itself.
pub fn of_yours(read: &Read) -> anyhow::Result<Given> {
    let mut out = link_format::Out::from_env();
    if !out.can_ask() {
        return of(read, Asker::NoOne);
    }
    let mut ask = |ask: &Ask| -> anyhow::Result<bool> {
        let question = link_format::Question::confirm(
            link_format::sentence!(
                "Give environment {} ({}) your secret {} ({})?",
                link_format::Name::new("environment", &ask.name),
                link_format::Path::new(&ask.spec),
                link_format::Name::new("secret", &ask.asks),
                link_format::Value::from(ask.resource.clone())
            ),
            false,
        )
        .why(link_format::prose!(
            "It's kept as your yes for this file, its name and this secret: lnk env approval remove takes it back."
        ));
        out.ask(question)
            .map_err(|_| anyhow::anyhow!("{}: whether to give it wasn't answered", ask.asks))
    };
    of(read, Asker::You(&mut ask))
}

/// What an image's build of the file is given: nothing, since an image
/// keeps what its stages wrote, so a file naming a secret is refused.
pub fn sealed(read: &Read) -> anyhow::Result<Given> {
    given(read, For::Build, Asker::NoOne)
}

/// A look at the file (`lnk env check`): a secret it names is its run's
/// to refuse.
pub fn check(read: &Read) -> anyhow::Result<()> {
    given(read, For::Check, Asker::NoOne).map(|_| ())
}

/// What a job's own secrets, `secrets`, are given: none, since a job runs
/// with no one there to say yes, so one it names is refused, naming it.
pub fn of_job(job: &str, secrets: &crate::file::Secrets) -> anyhow::Result<Given> {
    unattended(job, secrets.names().iter().chain(secrets.sealed()))
}

/// What a run with no one there (a job's, a schedule's) is given of the
/// secrets its environment names: none; one it names is refused.
pub fn of_unattended(read: &Read) -> anyhow::Result<Given> {
    unattended(
        &read.file.name,
        read.file
            .secret_names()
            .iter()
            .chain(read.file.sealed_names()),
    )
}

fn unattended<'a>(who: &str, mut names: impl Iterator<Item = &'a String>) -> anyhow::Result<Given> {
    match names.next() {
        None => Ok(Given::default()),
        Some(name) => bail!(
            "{name}: {} runs with no one there, and is given no secret",
            link_format::clean(who)
        ),
    }
}

/// What a file's secrets are read for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum For {
    /// A run: given what's approved.
    Run,
    /// An image's build: a secret named is refused.
    Build,
    /// A look at the file: nothing read, nothing refused.
    Check,
}

/// The file's secrets as `reading` takes them.
fn given(read: &Read, reading: For, asker: Asker) -> anyhow::Result<Given> {
    let names = read.file.secret_names();
    let sealed = read.file.sealed_names();
    let first = names.iter().chain(sealed).next();
    let Some(first) = first.filter(|_| reading != For::Check) else {
        return Ok(Given::default());
    };
    if reading == For::Build {
        bail!(
            "{first}: an image's build is given no secret, since the image keeps what its \
             stages write: give it to the stages that run at every start"
        );
    }
    // A yes is to the file as written: field flags over it make another
    // spec, which no kept yes covers, so each of its secrets is asked
    // for this run alone.
    let as_written = std::fs::read_to_string(&read.path).is_ok_and(|text| text == read.text);
    let asked = Asked {
        spec: approvals::spec_path(&read.path),
        name: read.file.name.clone(),
        names,
        sealed,
        as_written,
    };
    asked.run(
        &approvals::path()?,
        &approvals::yours()?,
        asker,
        &link_secret_contract::for_you,
    )
}

/// How your secrets are read, as their holder, by their IDs.
type Reader<'a> = &'a dyn Fn(&[String]) -> anyhow::Result<Vec<link_secret_contract::Yours>>;

/// The secrets a spec asks for: its real path, its `name`, those it
/// names plain and those it names sealed.
struct Asked<'a> {
    spec: PathBuf,
    name: String,
    names: &'a [String],
    sealed: &'a [String],
    /// Whether what runs is the file as written, which its kept yeses
    /// cover; false, field flags changed it.
    as_written: bool,
}

impl Asked<'_> {
    /// What it's given: each name approved at `file` (or approved now, as
    /// `asker` says) of `yours`, read with `read`.
    fn run(
        &self,
        file: &Path,
        yours: &[approvals::Yours],
        asker: Asker,
        read: Reader,
    ) -> anyhow::Result<Given> {
        let approved = self.approve(file, yours, asker)?;
        let ids: Vec<String> = approved.iter().map(|(_, id)| id.clone()).collect();
        self.given(&approved, &read(&ids)?)
    }

    fn all(&self) -> impl Iterator<Item = (&String, bool)> {
        self.names
            .iter()
            .map(|n| (n, false))
            .chain(self.sealed.iter().map(|n| (n, true)))
    }

    /// Each name it asks, with the ID of the secret of `yours` it's
    /// given, approved at `file` for it: one with no yes asked of `asker`
    /// now, and kept when it's yes.
    fn approve(
        &self,
        file: &Path,
        yours: &[approvals::Yours],
        mut asker: Asker,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let kept = |id: &str| yours.iter().any(|y| y.id == id);
        let approved = approvals::read_at(file, &kept)?;
        let mut ids = Vec::new();
        for (asks, _) in self.all() {
            let found = approved
                .iter()
                .filter(|_| self.as_written)
                .find(|a| a.is_for(&self.spec, &self.name) && a.asks == *asks);
            if let Some(found) = found {
                ids.push((asks.clone(), found.secret_id().to_string()));
                continue;
            }
            let Some(secret) = yours.iter().find(|y| y.name == *asks) else {
                bail!("{asks}: no secret of that name is kept here: lnk secret create {asks}");
            };
            let ask = Ask {
                asks: asks.clone(),
                resource: format!("secret:{}", secret.id),
                spec: self.spec.clone(),
                name: self.name.clone(),
            };
            let yes = match &mut asker {
                Asker::You(ask_you) => ask_you(&ask)?,
                Asker::NoOne if !self.as_written => bail!(
                    "{asks}: environment {} at {} runs with field flags over it, which no yes \
                     given ahead covers, and no one is here to say yes: give its file the \
                     change instead",
                    self.name,
                    self.spec.display(),
                ),
                Asker::NoOne => bail!(
                    "{asks}: environment {} at {} hasn't been given it, and no one is here to \
                     say yes: {} gives it ahead",
                    self.name,
                    self.spec.display(),
                    Approval::command(asks, &self.spec)
                ),
            };
            if !yes {
                bail!(
                    "{asks}: you didn't give it to environment {}, so it doesn't run",
                    self.name
                );
            }
            // A yes to a flagged spec is this run's: the file as written
            // is still not given it.
            if !self.as_written {
                ids.push((asks.clone(), secret.id.clone()));
                continue;
            }
            approvals::add_at(
                file,
                &kept,
                Approval {
                    spec: ask.spec,
                    name: ask.name,
                    resource: ask.resource,
                    asks: ask.asks,
                },
            )?;
            ids.push((asks.clone(), secret.id.clone()));
        }
        Ok(ids)
    }

    /// What it's given of `yours`, read, each name of `approved` by its
    /// secret's ID: as its file names it, plain or sealed.
    fn given(
        &self,
        approved: &[(String, String)],
        yours: &[link_secret_contract::Yours],
    ) -> anyhow::Result<Given> {
        let mut given = Given::default();
        for (name, sealed) in self.all() {
            let found = approved
                .iter()
                .find(|(asks, _)| asks == name)
                .and_then(|(_, id)| yours.iter().find(|y| y.id == *id));
            let Some(found) = found else {
                bail!("{name}: the secret approved for it is no longer kept: lnk secret list");
            };
            match (sealed, &found.sealed) {
                (false, None) => {
                    given.secrets.insert(name.clone(), found.value.duplicate());
                }
                (true, Some(sealing)) => {
                    given.sealed.insert(name.clone(), found.value.duplicate());
                    given.sealing.insert(name.clone(), sealing.clone());
                }
                (false, Some(_)) => {
                    bail!("{name} is kept sealed, for its host alone: name it in [secrets] sealed")
                }
                (true, None) => {
                    bail!("{name} is kept plain: [secrets] sealed takes one added with --host")
                }
            }
        }
        Ok(given)
    }
}

/// The values given at start: secrets, which what runs reads, and sealed
/// secrets, which the proxy holds.
#[derive(Duplicate, Default, PartialEq, Serialize, Deserialize)]
pub struct Given {
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub secrets: BTreeMap<String, Secret>,
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub sealed: BTreeMap<String, Secret>,
    /// How each sealed one is given.
    #[serde(default)]
    pub sealing: BTreeMap<String, Sealing>,
}

impl std::fmt::Debug for Given {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Given")
            .field("secrets", &self.secrets.keys().collect::<Vec<_>>())
            .field("sealed", &self.sealed.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Given {
    /// Itself with `more`'s values too: a name is in one only.
    pub fn with(mut self, more: Given) -> Given {
        self.secrets.extend(more.secrets);
        self.sealed.extend(more.sealed);
        self.sealing.extend(more.sealing);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty() && self.sealed.is_empty()
    }

    /// Every value, for [`Blank`].
    pub fn values(&self) -> Vec<Secret> {
        self.secrets
            .values()
            .chain(self.sealed.values())
            .map(Secret::duplicate)
            .collect()
    }
}

/// Every value of `values` in a text replaced with [`BLANK`], as GitHub
/// does in its logs: a backstop, not the wall, since a value encoded on
/// its way out isn't caught.
#[derive(Duplicate, Default)]
pub struct Blank {
    #[duplicate]
    values: Vec<Secret<Vec<u8>>>,
}

impl Blank {
    pub fn new(values: impl IntoIterator<Item = Secret>) -> Blank {
        let mut values: Vec<Secret<Vec<u8>>> = values
            .into_iter()
            .filter(|v| !v.is_empty())
            .map(|v| v.expose_map(|v| v.as_bytes().to_vec()))
            .collect();
        // The longest first, so a value holding another is blanked whole.
        values.sort_by_key(|v| std::cmp::Reverse(v.expose().len()));
        values.dedup();
        Blank { values }
    }

    /// Whether there's nothing to blank.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `bytes`, each value in it blanked.
    pub fn bytes(&self, bytes: &[u8]) -> Vec<u8> {
        self.some(bytes, true).0
    }

    pub fn text(&self, text: &str) -> String {
        String::from_utf8_lossy(&self.bytes(text.as_bytes())).into_owned()
    }

    /// `bytes` blanked as far as can be told without what follows them:
    /// to where the longest value could still start, or to their end when
    /// `end`. The blanked bytes, how many of `bytes` they stand for, and
    /// whether a value was blanked.
    pub fn some(&self, bytes: &[u8], end: bool) -> (Vec<u8>, usize, bool) {
        let longest = self.values.first().map_or(0, |v| v.expose().len());
        let upto = match end {
            true => bytes.len(),
            false => bytes.len().saturating_sub(longest.saturating_sub(1)),
        };
        let mut out = Vec::with_capacity(upto);
        let (mut i, mut blanked) = (0, false);
        while i < upto {
            match self
                .values
                .iter()
                .find(|v| bytes[i..].starts_with(v.expose()))
            {
                Some(value) => {
                    out.extend_from_slice(BLANK.as_bytes());
                    i += value.expose().len();
                    blanked = true;
                }
                None => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        }
        (out, i, blanked)
    }

    /// Reads `input` to its end a piece at a time, each value in it
    /// blanked, writing what it makes to `output` when there's one:
    /// whether a value was blanked. A value split between pieces is
    /// caught, since what could start one is kept for the next.
    pub fn stream(
        &self,
        mut input: impl std::io::Read,
        mut output: Option<&mut dyn std::io::Write>,
    ) -> std::io::Result<bool> {
        let mut held = Vec::new();
        let mut piece = vec![0u8; 64 * 1024];
        let mut blanked = false;
        loop {
            let read = input.read(&mut piece)?;
            held.extend_from_slice(&piece[..read]);
            let (out, took, some) = self.some(&held, read == 0);
            blanked |= some;
            if let Some(output) = output.as_deref_mut() {
                output.write_all(&out)?;
            }
            held.drain(..took);
            if read == 0 {
                return Ok(blanked);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec at `spec` named `name`, asking `names` plain and
    /// `sealed` sealed.
    fn asked<'a>(spec: &Path, name: &str, names: &'a [String], sealed: &'a [String]) -> Asked<'a> {
        Asked {
            spec: spec.to_path_buf(),
            name: name.into(),
            names,
            sealed,
            as_written: true,
        }
    }

    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// Your secrets as the secret module keeps them, each by its ID:
    /// `STRIPE_KEY` sealed, the rest plain.
    struct Kept(Vec<approvals::Yours>);

    impl Kept {
        fn new(names: &[&str]) -> Kept {
            Kept(
                names
                    .iter()
                    .map(|name| approvals::Yours {
                        id: link_id::candidate().unwrap(),
                        name: name.to_string(),
                    })
                    .collect(),
            )
        }

        fn read(&self, ids: &[String]) -> anyhow::Result<Vec<link_secret_contract::Yours>> {
            Ok(self
                .0
                .iter()
                .filter(|y| ids.contains(&y.id))
                .map(|y| link_secret_contract::Yours {
                    id: y.id.clone(),
                    name: y.name.clone(),
                    value: Secret::new(format!("{}-value", y.name)),
                    sealed: (y.name == "STRIPE_KEY")
                        .then(|| Sealing::new("api.example.com", None, None, None).unwrap()),
                })
                .collect())
        }
    }

    /// A spec is given a secret it asks for only after your yes, kept for
    /// that spec at that path with its name: asked once, never again; a
    /// no, or no one there, gives nothing, naming the command that
    /// approves it.
    #[test]
    fn a_secret_is_given_only_after_your_yes() {
        let dir = link_test_kit::TempDir::new("env-approve");
        let file = dir.join("approvals.toml");
        let spec = dir.join("app").join("env.toml");
        let kept = Kept::new(&["NPM_TOKEN", "STRIPE_KEY"]);
        let read = |ids: &[String]| kept.read(ids);
        let (names, sealed) = (owned(&["NPM_TOKEN"]), owned(&["STRIPE_KEY"]));
        let app = asked(&spec, "app", &names, &sealed);
        // No one there: refused, naming the command that gives it ahead.
        let refused = format!(
            "{:#}",
            app.run(&file, &kept.0, Asker::NoOne, &read).unwrap_err()
        );
        assert!(
            refused.contains("lnk env approval add NPM_TOKEN --spec"),
            "{refused}"
        );
        // You say no: refused, and nothing kept.
        let mut no = |_: &Ask| Ok(false);
        assert!(app.run(&file, &kept.0, Asker::You(&mut no), &read).is_err());
        assert!(approvals::read_at(&file, &|_| true).unwrap().is_empty());
        // You say yes to each, asked once.
        let mut asks = Vec::new();
        let mut yes = |ask: &Ask| {
            asks.push(ask.clone());
            Ok(true)
        };
        let given = app
            .run(&file, &kept.0, Asker::You(&mut yes), &read)
            .unwrap();
        assert_eq!(given.secrets["NPM_TOKEN"].expose(), "NPM_TOKEN-value");
        assert_eq!(given.sealed["STRIPE_KEY"].expose(), "STRIPE_KEY-value");
        assert_eq!(given.sealing["STRIPE_KEY"].host, "api.example.com");
        assert_eq!(asks.len(), 2);
        assert_eq!(asks[0].resource, format!("secret:{}", kept.0[0].id));
        // Kept: no one need be there the next time.
        let again = app.run(&file, &kept.0, Asker::NoOne, &read).unwrap();
        assert_eq!(again.secrets["NPM_TOKEN"].expose(), "NPM_TOKEN-value");
    }

    /// A yes is to the file as written: with field flags over it, a kept
    /// yes gives nothing, each secret is asked again for that run alone,
    /// and no one there is refused.
    #[test]
    fn field_flags_over_an_approved_file_ask_again() {
        let dir = link_test_kit::TempDir::new("env-approve-flagged");
        let file = dir.join("approvals.toml");
        let spec = dir.join("app").join("env.toml");
        let kept = Kept::new(&["NPM_TOKEN"]);
        let read = |ids: &[String]| kept.read(ids);
        let names = owned(&["NPM_TOKEN"]);
        let app = asked(&spec, "app", &names, &[]);
        let mut yes = |_: &Ask| Ok(true);
        app.run(&file, &kept.0, Asker::You(&mut yes), &read)
            .unwrap();
        let flagged = Asked {
            as_written: false,
            ..asked(&spec, "app", &names, &[])
        };
        let refused = format!(
            "{:#}",
            flagged
                .run(&file, &kept.0, Asker::NoOne, &read)
                .unwrap_err()
        );
        assert!(refused.contains("field flags"), "{refused}");
        let mut asked_again = 0;
        let mut yes = |_: &Ask| {
            asked_again += 1;
            Ok(true)
        };
        let given = flagged
            .run(&file, &kept.0, Asker::You(&mut yes), &read)
            .unwrap();
        assert_eq!(given.secrets["NPM_TOKEN"].expose(), "NPM_TOKEN-value");
        assert_eq!(asked_again, 1);
        // That yes was the run's: the file keeps its one approval.
        assert_eq!(approvals::read_at(&file, &|_| true).unwrap().len(), 1);
    }

    /// The yes is the spec's path, its `name` and the secret: a copy or a
    /// move of the folder, or a changed `name`, asks again; a secret made
    /// again with the name asks again; one asked the other way than it's
    /// kept is refused.
    #[test]
    fn a_yes_is_for_one_spec_name_and_secret() {
        let dir = link_test_kit::TempDir::new("env-approve-tied");
        let file = dir.join("approvals.toml");
        let spec = dir.join("app").join("env.toml");
        let kept = Kept::new(&["NPM_TOKEN", "STRIPE_KEY"]);
        let read = |ids: &[String]| kept.read(ids);
        let (names, none) = (owned(&["NPM_TOKEN"]), Vec::new());
        let mut yes = |_: &Ask| Ok(true);
        asked(&spec, "app", &names, &none)
            .run(&file, &kept.0, Asker::You(&mut yes), &read)
            .unwrap();
        let unasked = |spec: &Path, name: &str| {
            asked(spec, name, &names, &none)
                .run(&file, &kept.0, Asker::NoOne, &read)
                .is_err()
        };
        assert!(!unasked(&spec, "app"));
        assert!(unasked(&dir.join("copy").join("env.toml"), "app"));
        assert!(unasked(&spec, "web"));
        // Made again with its name: another ID, asked again.
        let again = Kept::new(&["NPM_TOKEN"]);
        let read_again = |ids: &[String]| again.read(ids);
        assert!(
            asked(&spec, "app", &names, &none)
                .run(&file, &again.0, Asker::NoOne, &read_again)
                .is_err()
        );
        // Asked the other way than it's kept.
        let (plain, sealed) = (owned(&["STRIPE_KEY"]), owned(&["NPM_TOKEN"]));
        let err = |names: &[String], sealed: &[String]| {
            let mut yes = |_: &Ask| Ok(true);
            format!(
                "{:#}",
                asked(&spec, "app", names, sealed)
                    .run(&file, &kept.0, Asker::You(&mut yes), &read)
                    .unwrap_err()
            )
        };
        assert!(err(&plain, &none).contains("kept sealed"));
        assert!(err(&none, &sealed).contains("kept plain"));
        // One you don't hold is refused, naming how to keep it.
        assert!(err(&owned(&["QA_TOKEN"]), &none).contains("lnk secret create QA_TOKEN"));
    }

    /// An image's build, a look and a job's run are given none.
    #[test]
    fn a_build_a_look_and_a_job_are_given_none() {
        let dir = link_test_kit::TempDir::new("env-approve-none");
        let path = dir.join("env.toml");
        std::fs::write(
            &path,
            "schema = 1\nname = \"app\"\nversion = 1\n[secrets]\nnames = [\"NPM_TOKEN\"]\n",
        )
        .unwrap();
        let read = crate::file::read(&path).unwrap();
        let build = format!("{:#}", sealed(&read).unwrap_err());
        assert!(build.contains("an image's build"), "{build}");
        check(&read).unwrap();
        let job = format!("{:#}", of_unattended(&read).unwrap_err());
        assert!(job.contains("given no secret"), "{job}");
        let secrets = crate::file::Secrets {
            names: Some(owned(&["NPM_TOKEN"])),
            ..Default::default()
        };
        assert!(of_job("nightly", &secrets).is_err());
        assert!(of_job("nightly", &Default::default()).unwrap().is_empty());
    }

    #[test]
    fn values_are_blanked_the_longest_first() {
        let blank = Blank::new([
            "hunter2".to_string().into(),
            "hunter2-extra".to_string().into(),
            Secret::default(),
        ]);
        assert_eq!(blank.text("a hunter2-extra b hunter2 c"), "a *** b *** c");
        assert_eq!(blank.bytes(b"none here"), b"none here");
    }

    #[test]
    fn a_stream_is_blanked_a_piece_at_a_time_and_a_value_across_pieces_too() {
        let blank = Blank::new([Secret::from("hunter2".to_string())]);
        // A value across the 64 KiB a piece is read in.
        let mut text = vec![b'x'; 64 * 1024 - 3];
        text.extend_from_slice(b"hunter2 and hunter2");
        let mut out = Vec::new();
        let read = std::io::Cursor::new(&text);
        assert!(blank.stream(read, Some(&mut out)).unwrap());
        assert_eq!(out.len(), text.len() - 2 * ("hunter2".len() - BLANK.len()));
        assert!(out.ends_with(b"*** and ***"));
        assert!(!blank.stream(&b"nothing"[..], None).unwrap());
        // A piece's end keeps what could start a value.
        let (out, took, _) = blank.some(b"abc hunt", false);
        assert_eq!((out.as_slice(), took), (&b"ab"[..], 2));
        assert!(Blank::new([Secret::default()]).is_empty());
    }

    #[test]
    fn a_given_value_is_never_debug_printed() {
        let mut given = Given::default();
        given
            .secrets
            .insert("NPM_TOKEN".into(), "hunter2".to_string().into());
        given
            .sealed
            .insert("STRIPE_KEY".into(), "sk_live".to_string().into());
        let shown = format!("{given:?}");
        assert!(
            shown.contains("NPM_TOKEN") && !shown.contains("hunter2") && !shown.contains("sk_live")
        );
    }
}
