use std::cell::RefCell;
use std::path::{Path, PathBuf};

use link_test_kit::TempDir;

use super::*;
use crate::names::Ask;

/// An owner of agents answering from files alone, as a module would, and
/// noting what it was asked.
#[derive(Default)]
struct Fake {
    running: Vec<String>,
    refuse: Vec<String>,
    /// Those whose memory notes it carries in place of their own, and
    /// where: inside the folder it's given, or not. It leaves out their
    /// home's notes.
    carries: Vec<(String, Carried)>,
    asked: RefCell<Vec<String>>,
}

/// Where [`Fake`] puts what it carries in place of a thing's memory notes.
#[derive(Clone, Copy)]
enum Carried {
    Given,
    Outside,
}

impl Owner for Fake {
    fn check(&self, thing: &ThingFolders) -> anyhow::Result<()> {
        self.asked
            .borrow_mut()
            .push(format!("check {}", thing.name));
        anyhow::ensure!(
            !self.refuse.contains(&thing.name),
            "its settings are refused"
        );
        for (_, folder) in &thing.folders {
            anyhow::ensure!(folder.is_dir(), "{} is missing", folder.display());
        }
        Ok(())
    }
    fn running(&self, thing: &ThingFolders) -> anyhow::Result<Option<Running>> {
        Ok(self.running.contains(&thing.name).then(|| Running {
            stop: format!("lnk agent stop --agent {}", thing.name),
        }))
    }
    fn carry(&self, thing: &ThingFolders, staging: &Path) -> anyhow::Result<Vec<Replaced>> {
        let Some((_, at)) = self.carries.iter().find(|(name, _)| *name == thing.name) else {
            return Ok(Vec::new());
        };
        let with = match at {
            Carried::Given => staging.join("notes.md"),
            Carried::Outside => staging.parent().unwrap().join("notes.md"),
        };
        write(&with, "neutral");
        let own = thing.folder(link_files::layout::Home::Agents).unwrap();
        Ok(vec![
            Replaced {
                file: own.join(NOTES),
                with: Some(with),
            },
            Replaced {
                file: own.join(HOME_NOTES),
                with: None,
            },
        ])
    }
    fn missing(&self, _: &ThingFolders) -> anyhow::Result<Vec<Missing>> {
        Ok(vec![Missing {
            what: "a credential".into(),
            setup: "lnk agent start".into(),
        }])
    }
    fn adopt(&self, thing: &ThingFolders, was: &str) -> anyhow::Result<Adopted> {
        self.asked
            .borrow_mut()
            .push(format!("adopt {} was {was}", thing.name));
        Ok(Adopted {
            notes: Vec::new(),
            next: format!("lnk agent start --agent {}", thing.name),
        })
    }
}

impl Owners for Fake {
    fn of(&self, kind: &str) -> Option<&dyn Owner> {
        (kind == "agent").then_some(self as &dyn Owner)
    }
}

/// A machine of its own: its Link folder and `~/.lnk`.
fn machine(dir: &TempDir, name: &str) -> Roots {
    Roots {
        link: dir.join(name).join("Link"),
        hidden: dir.join(name).join(".lnk"),
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The folder of the thing named programmer, from its location's root.
const PROGRAMMER: &str = "Agents/programmer";
/// Its memory notes, its home's notes, and its skill, in its folder.
const NOTES: &str = "agent/memory/notes.md";
const HOME_NOTES: &str = "home/notes.txt";
const SKILL: &str = "agent/skills/review/SKILL.md";

/// An agent `name` on `roots`, with its settings, a skill and a file.
fn agent(roots: &Roots, name: &str) {
    let own = roots.link.join("Agents").join(name);
    write(&own.join(HOME_NOTES), "notes");
    write(&own.join(SKILL), "# Review");
    std::fs::create_dir_all(own.join("agent/memory")).unwrap();
    write(
        &roots.link.join("Safe/Agents").join(name).join("agent.toml"),
        "harness = \"link\"\n",
    );
}

fn sel(text: &str) -> Selection {
    Selection::parse(text).unwrap()
}

fn export_one(roots: &Roots, owner: &Fake, name: &str, out: &Path) -> anyhow::Result<Exported> {
    export(
        roots,
        &[sel(&format!("agent.{name}"))],
        owner,
        out,
        "An agent",
        None,
    )
}

/// Answers given in a terminal, in order, and what it was told.
struct Script {
    answers: Vec<String>,
    told: Vec<String>,
}

impl Script {
    fn new(answers: &[&str]) -> Self {
        Self {
            answers: answers.iter().rev().map(|a| a.to_string()).collect(),
            told: Vec::new(),
        }
    }
}

impl Ask for Script {
    fn confirm(&mut self, question: &str) -> anyhow::Result<bool> {
        self.told.push(question.into());
        Ok(self.answers.pop().is_some_and(|a| a == "y"))
    }
    fn line(&mut self, prompt: &str) -> anyhow::Result<String> {
        self.told.push(prompt.into());
        self.answers
            .pop()
            .ok_or_else(|| anyhow::anyhow!("no more answers"))
    }
    fn say(&mut self, text: &str) {
        self.told.push(text.into());
    }
}

#[test]
fn a_selection_is_a_kind_and_a_name_split_one_way() {
    let programmer = sel("agent.programmer");
    assert_eq!(
        (programmer.kind(), programmer.name()),
        ("agent", "programmer")
    );
    assert_eq!(programmer.to_string(), "agent.programmer");
    for bad in [
        "programmer",
        "agent.co.der",
        "agent.",
        ".programmer",
        "agent.Programmer",
        "agent.all",
        "Agent.programmer",
    ] {
        assert!(Selection::parse(bad).is_err(), "{bad}");
    }
    let picks = parse_picks("agent.programmer=agent.my-programmer,agent.work").unwrap();
    assert_eq!(picks[0].to.as_deref(), Some("my-programmer"));
    assert_eq!(picks[1].to, None);
    assert!(parse_picks("agent.programmer=box.programmer").is_err());
    assert!(parse_picks("agent.programmer,agent.programmer").is_err());
    assert!(parse_picks("agent.programmer=agent.work,agent.work").is_err());
}

#[test]
fn an_agent_exported_and_imported_is_a_copy_named_as_chosen() {
    let dir = TempDir::new("archive-round");
    let (here, there) = (machine(&dir, "here"), machine(&dir, "there"));
    agent(&here, "programmer");
    let owner = Fake::default();
    let out = default_name(&here, &[sel("agent.programmer")], "2026-11-10");
    assert_eq!(out, here.link.join("Archive/programmer.tar.gz"));
    let exported = export_one(&here, &owner, "programmer", &out).unwrap();
    // notes.txt, SKILL.md and agent.toml.
    assert_eq!(exported.things, [(sel("agent.programmer"), 30)]);
    let manifest = read_manifest(&out).unwrap();
    assert_eq!(manifest.map["link.agents"], "Agents");
    assert_eq!(manifest.map["link.agent-settings"], "Safe/Agents");
    assert!(manifest.things.contains_key("agent.programmer"));
    assert_eq!(manifest.deleted, None);

    let picks = parse_picks("agent.programmer=agent.my-programmer").unwrap();
    let imported = import(&out, &there, Some(&picks), &owner, None).unwrap();
    assert_eq!(imported.things[0].name, "my-programmer");
    assert_eq!(
        imported.things[0].next,
        "lnk agent start --agent my-programmer"
    );
    let own = there.link.join("Agents/my-programmer");
    assert_eq!(
        std::fs::read_to_string(own.join("home/notes.txt")).unwrap(),
        "notes"
    );
    assert!(own.join("agent/memory").is_dir());
    assert!(
        there
            .link
            .join("Safe/Agents/my-programmer/agent.toml")
            .is_file()
    );
    assert!(
        owner
            .asked
            .borrow()
            .contains(&"adopt my-programmer was programmer".to_string())
    );
    // Staged in ~/.lnk, and nothing left there.
    let staging = there.staging();
    assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 0);
    // The archive stays where it was.
    assert!(out.is_file());
}

#[test]
fn the_same_state_writes_the_same_archive_anywhere() {
    let dir = TempDir::new("archive-same");
    let (one, two) = (machine(&dir, "one"), machine(&dir, "two"));
    agent(&one, "programmer");
    agent(&two, "programmer");
    // Another time on one machine's file, which no archive carries.
    std::fs::File::options()
        .write(true)
        .open(two.link.join(PROGRAMMER).join(HOME_NOTES))
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000))
        .unwrap();
    let owner = Fake::default();
    let (a, b) = (dir.join("a.tar.gz"), dir.join("b.tar.gz"));
    export_one(&one, &owner, "programmer", &a).unwrap();
    export_one(&two, &owner, "programmer", &b).unwrap();
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
}

/// Each file of the archive at `path`, by its path in it, and what it
/// holds.
fn files_in(path: &Path) -> std::collections::BTreeMap<String, String> {
    use std::io::Read;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(
        std::fs::File::open(path).unwrap(),
    ));
    let mut files = std::collections::BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.header().entry_type() != tar::EntryType::Regular {
            continue;
        }
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        files.insert(name, text);
    }
    files
}

/// A file its owner carries in place of its own goes into the archive
/// instead, and one it leaves out stays out, the thing's own left as
/// they are and nothing left in staging; one it writes outside the
/// folder it was given is refused.
#[test]
fn an_owner_carries_folders_in_place_of_its_own() {
    let dir = TempDir::new("archive-carry");
    let here = machine(&dir, "here");
    agent(&here, "programmer");
    let own = here.link.join(PROGRAMMER);
    write(&own.join(NOTES), "here's");
    let out = dir.join("programmer.tar.gz");
    let owner = Fake {
        carries: vec![("programmer".into(), Carried::Given)],
        ..Fake::default()
    };
    export_one(&here, &owner, "programmer", &out).unwrap();
    let files = files_in(&out);
    let held = |file: &str| format!("{PROGRAMMER}/{file}");
    assert_eq!(files[&held(NOTES)], "neutral");
    assert!(!files.contains_key(&held(HOME_NOTES)), "{files:?}");
    assert_eq!(files[&held(SKILL)], "# Review");
    assert!(own.join(HOME_NOTES).is_file());
    assert_eq!(std::fs::read_to_string(own.join(NOTES)).unwrap(), "here's");
    assert_eq!(std::fs::read_dir(here.staging()).unwrap().count(), 0);
    let outside = Fake {
        carries: vec![("programmer".into(), Carried::Outside)],
        ..Fake::default()
    };
    let refused = dir.join("refused.tar.gz");
    let err = export_one(&here, &outside, "programmer", &refused).unwrap_err();
    assert!(
        format!("{err:#}").contains("isn't in the folder it was given"),
        "{err:#}"
    );
    assert!(!refused.exists());
    assert_eq!(std::fs::read_dir(here.staging()).unwrap().count(), 0);
}

#[test]
fn export_refuses_what_isnt_whole_here() {
    let dir = TempDir::new("archive-refused");
    let here = machine(&dir, "here");
    agent(&here, "programmer");
    let out = dir.join("out.tar.gz");
    let running = Fake {
        running: vec!["programmer".into()],
        ..Fake::default()
    };
    let err = export_one(&here, &running, "programmer", &out).unwrap_err();
    assert!(
        format!("{err:#}").contains("lnk agent stop --agent programmer"),
        "{err:#}"
    );
    let refused = Fake {
        refuse: vec!["programmer".into()],
        ..Fake::default()
    };
    assert!(export_one(&here, &refused, "programmer", &out).is_err());
    assert!(export_one(&here, &Fake::default(), "missing", &out).is_err());
    let boxed = export(
        &here,
        &[sel("box.programmer")],
        &Fake::default(),
        &out,
        "",
        None,
    );
    assert!(format!("{:#}", boxed.unwrap_err()).contains("no module here exports boxs"));
    assert!(!out.exists());
    // A link in its folder, never followed.
    std::os::unix::fs::symlink(
        "/etc/passwd",
        here.link.join(PROGRAMMER).join("home/passwd"),
    )
    .unwrap();
    let err = export_one(&here, &Fake::default(), "programmer", &out).unwrap_err();
    assert!(format!("{err:#}").contains("link"), "{err:#}");
    assert!(!out.exists());
    std::fs::remove_file(here.link.join(PROGRAMMER).join("home/passwd")).unwrap();
    // A file already there is never overwritten.
    write(&out, "mine");
    assert!(export_one(&here, &Fake::default(), "programmer", &out).is_err());
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "mine");
}

#[test]
fn a_name_taken_is_settled_in_the_same_question() {
    let dir = TempDir::new("archive-names");
    let (here, there) = (machine(&dir, "here"), machine(&dir, "there"));
    agent(&here, "programmer");
    agent(&there, "programmer");
    let owner = Fake::default();
    let out = dir.join("programmer.tar.gz");
    export_one(&here, &owner, "programmer", &out).unwrap();
    // Without a terminal, a name taken refuses it, naming the way out.
    let err = import(&out, &there, None, &owner, None).unwrap_err();
    assert!(
        format!("{err:#}").contains("--select agent.programmer=agent."),
        "{err:#}"
    );
    assert_eq!(
        std::fs::read_to_string(there.link.join("Safe/Agents/programmer/agent.toml")).unwrap(),
        "harness = \"link\"\n"
    );
    // In a terminal: asked for another, a taken or bad one asked again.
    let mut script = Script::new(&["programmer", "Bad Name", "mine"]);
    let imported = import(&out, &there, None, &owner, Some(&mut script)).unwrap();
    assert_eq!(imported.things[0].name, "mine");
    assert_eq!(
        script.told[0],
        "This agent is named programmer, which one of yours already is."
    );
    // Not taken: kept, or renamed when you say so.
    let other = machine(&dir, "other");
    let mut keep = Script::new(&["n"]);
    let kept = import(&out, &other, None, &owner, Some(&mut keep)).unwrap();
    assert_eq!(kept.things[0].name, "programmer");
    assert_eq!(keep.told, ["This agent is named programmer. Rename it?"]);
    let fresh = machine(&dir, "fresh");
    let mut rename = Script::new(&["y", "my-programmer"]);
    let renamed = import(&out, &fresh, None, &owner, Some(&mut rename)).unwrap();
    assert_eq!(renamed.things[0].name, "my-programmer");
    // A name given that's taken refuses it.
    let picks = parse_picks("agent.programmer=agent.my-programmer").unwrap();
    assert!(import(&out, &fresh, Some(&picks), &owner, None).is_err());
}

#[test]
fn an_import_checked_by_its_owner_moves_nothing_when_refused() {
    let dir = TempDir::new("archive-checked");
    let (here, there) = (machine(&dir, "here"), machine(&dir, "there"));
    agent(&here, "programmer");
    let out = dir.join("programmer.tar.gz");
    export_one(&here, &Fake::default(), "programmer", &out).unwrap();
    let refused = Fake {
        refuse: vec!["programmer".into()],
        ..Fake::default()
    };
    assert!(import(&out, &there, None, &refused, None).is_err());
    assert!(!there.link.join(PROGRAMMER).exists());
    assert!(
        !refused
            .asked
            .borrow()
            .iter()
            .any(|a| a.starts_with("adopt"))
    );
}

/// An archive written entry by entry, as anyone could write one.
struct Raw {
    builder: tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
}

impl Raw {
    fn new() -> Self {
        Self {
            builder: tar::Builder::new(flate2::write::GzEncoder::new(
                Vec::new(),
                flate2::Compression::fast(),
            )),
        }
    }
    fn manifest(self, text: &str) -> Self {
        self.entry(
            "archive.toml",
            tar::EntryType::Regular,
            text.as_bytes(),
            None,
        )
    }
    fn file(self, path: &str, text: &str) -> Self {
        self.entry(path, tar::EntryType::Regular, text.as_bytes(), None)
    }
    /// An entry named by its raw bytes, whatever they are.
    fn entry(mut self, path: &str, kind: tar::EntryType, data: &[u8], link: Option<&str>) -> Self {
        let mut header = tar::Header::new_old();
        header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_entry_type(kind);
        header.set_mode(0o644);
        header.set_size(data.len() as u64);
        if let Some(link) = link {
            header.as_old_mut().linkname[..link.len()].copy_from_slice(link.as_bytes());
        }
        header.set_cksum();
        self.builder.append(&header, data).unwrap();
        self
    }
    fn write(self, at: &Path) -> PathBuf {
        let bytes = self.builder.into_inner().unwrap().finish().unwrap();
        std::fs::write(at, bytes).unwrap();
        at.into()
    }
}

const MANIFEST: &str = "schema = 1\ndescription = \"x\"\n[map]\n\"link.agents\" = \"Agents\"\n\"link.agent-settings\" = \"Safe/Agents\"\n[\"agent.programmer\"]\n";

fn whole(raw: Raw) -> Raw {
    raw.file("Agents/programmer/home/a", "a")
        .file("Safe/Agents/programmer/agent.toml", "harness = \"link\"\n")
}

#[test]
fn an_archive_is_read_as_untrusted_data() {
    let dir = TempDir::new("archive-hostile");
    let there = machine(&dir, "there");
    let owner = Fake::default();
    let at = dir.join("x.tar.gz");
    let refused = |raw: Raw, says: &str| {
        let path = raw.write(&at);
        let err = import(&path, &there, None, &owner, None).unwrap_err();
        assert!(format!("{err:#}").contains(says), "{says}: {err:#}");
        assert!(!there.link.join("Agents/programmer").exists(), "{says}");
        assert_eq!(
            std::fs::read_dir(there.staging())
                .map(|d| d.count())
                .unwrap_or(0),
            0,
            "{says}: staging left"
        );
    };
    // A whole one is admitted, to show each refusal below is its own.
    import(
        &whole(Raw::new().manifest(MANIFEST)).write(&at),
        &there,
        None,
        &owner,
        None,
    )
    .unwrap();
    std::fs::remove_dir_all(there.link.join("Agents/programmer")).unwrap();
    std::fs::remove_dir_all(there.link.join("Safe/Agents/programmer")).unwrap();

    refused(whole(Raw::new()).manifest(MANIFEST), "isn't an archive");
    refused(
        whole(Raw::new().manifest(MANIFEST)).entry(
            "Agents/programmer/home/link",
            tar::EntryType::Symlink,
            b"",
            Some("/etc/passwd"),
        ),
        "symbolic link",
    );
    refused(
        whole(Raw::new().manifest(MANIFEST)).entry(
            "Agents/programmer/home/hard",
            tar::EntryType::Link,
            b"",
            Some("Agents/programmer/home/a"),
        ),
        "hard link",
    );
    refused(
        whole(Raw::new().manifest(MANIFEST)).entry(
            "Agents/programmer/dev",
            tar::EntryType::Char,
            b"",
            None,
        ),
        "device",
    );
    refused(
        whole(Raw::new().manifest(MANIFEST)).file("/etc/evil", "x"),
        "isn't a plain path",
    );
    refused(
        whole(Raw::new().manifest(MANIFEST)).file("Agents/programmer/../../evil", "x"),
        "isn't a plain path",
    );
    refused(
        whole(Raw::new().manifest(MANIFEST)).file("Agents/programmer/home/a", "again"),
        "twice",
    );
    refused(
        whole(Raw::new().manifest(MANIFEST)).file("Agents/other/x", "x"),
        "isn't in a thing",
    );
    refused(
        Raw::new()
            .manifest(MANIFEST)
            .file("Agents/programmer/home/a", "a"),
        "doesn't hold its folder Safe/Agents/programmer",
    );
    refused(
        whole(Raw::new().manifest(&MANIFEST.replace("link.agents", "link.nowhere"))),
        "this Link doesn't know",
    );
}

#[test]
fn an_archives_map_is_refused_outside_or_overlapping() {
    let dir = TempDir::new("archive-map");
    let there = machine(&dir, "there");
    let owner = Fake::default();
    let at = dir.join("x.tar.gz");
    for (from, to, says) in [
        ("= \"Agents\"", "= \"../Agents\"", "isn't a plain path"),
        ("= \"Agents\"", "= \"Safe\"", "one is inside the other"),
        (
            "= \"Agents\"",
            "= \"archive.toml\"",
            "the archive's own file",
        ),
        (
            "\"link.agents\"",
            "\"lnk.agents\"",
            "isn't in the Link folder",
        ),
        (
            "[\"agent.programmer\"]",
            "[\"box.programmer\"]",
            "doesn't say where",
        ),
        (
            "[\"agent.programmer\"]",
            "[\"thing.programmer\"]",
            "which no kind here is",
        ),
        (
            "[\"agent.programmer\"]",
            "[\"agent.programmer\"]\nid = \"a1\"",
            "unknown field",
        ),
        ("description", "descripton", "descripton"),
    ] {
        let raw = whole(Raw::new().manifest(&MANIFEST.replacen(from, to, 1)));
        let err = import(&raw.write(&at), &there, None, &owner, None).unwrap_err();
        assert!(format!("{err:#}").contains(says), "{says}: {err:#}");
    }
}

#[test]
fn only_a_deletions_archive_expires() {
    let dir = TempDir::new("archive-expiry");
    let here = machine(&dir, "here");
    agent(&here, "programmer");
    let owner = Fake::default();
    let archive = here.archive();
    let things = [sel("agent.programmer")];
    export(
        &here,
        &things,
        &owner,
        &archive.join("programmer-2026-10-01.tar.gz"),
        "",
        Some("2026-10-01"),
    )
    .unwrap();
    export(
        &here,
        &things,
        &owner,
        &archive.join("programmer-2026-11-01.tar.gz"),
        "",
        Some("2026-11-01"),
    )
    .unwrap();
    export(
        &here,
        &things,
        &owner,
        &archive.join("mine.tar.gz"),
        "",
        None,
    )
    .unwrap();
    // One a deletion made, renamed to keep it.
    export(
        &here,
        &things,
        &owner,
        &archive.join("programmer-keep.tar.gz"),
        "",
        Some("2026-10-01"),
    )
    .unwrap();
    write(&archive.join("broken.tar.gz"), "not gzip");
    assert_eq!(
        expired(&archive, 30, "2026-11-10"),
        [archive.join("programmer-2026-10-01.tar.gz")]
    );
    assert_eq!(day_number("1970-01-01"), Some(0));
    assert_eq!(day_number("2026-13-01"), None);
}

#[test]
fn everything_here_is_each_owners_things() {
    let dir = TempDir::new("archive-all");
    let here = machine(&dir, "here");
    agent(&here, "work");
    agent(&here, "programmer");
    write(&here.link.join("Safe/Agents/defaults.toml"), "");
    assert_eq!(
        everything(&here, &Fake::default()).unwrap(),
        [sel("agent.programmer"), sel("agent.work")]
    );
    let several = default_name(
        &here,
        &[sel("agent.programmer"), sel("agent.work")],
        "2026-11-10",
    );
    assert_eq!(several, here.link.join("Archive/2026-11-10.tar.gz"));
}

#[test]
fn a_long_path_goes_and_comes_back_and_a_header_past_its_limit_is_refused() {
    let dir = TempDir::new("archive-long");
    let (here, there) = (machine(&dir, "here"), machine(&dir, "there"));
    agent(&here, "programmer");
    let deep = "a-folder-with-a-long-name/".repeat(5) + "notes.txt";
    write(
        &here.link.join("Agents/programmer/home").join(&deep),
        "deep",
    );
    let out = dir.join("programmer.tar.gz");
    export_one(&here, &Fake::default(), "programmer", &out).unwrap();
    import(&out, &there, None, &Fake::default(), None).unwrap();
    assert_eq!(
        std::fs::read_to_string(there.link.join("Agents/programmer/home").join(&deep)).unwrap(),
        "deep"
    );
    let at = dir.join("x.tar.gz");
    for (raw, says) in [
        (
            whole(Raw::new().manifest(MANIFEST)).entry(
                "././@LongLink",
                tar::EntryType::GNULongName,
                "x".repeat(2000).as_bytes(),
                None,
            ),
            "long name longer",
        ),
        (
            whole(Raw::new().manifest(MANIFEST)).entry(
                "PaxHeaders/a",
                tar::EntryType::XHeader,
                b"30 path=Agents/programmer/home/b\n",
                None,
            ),
            "special entry",
        ),
    ] {
        let err = import(&raw.write(&at), &there, None, &Fake::default(), None).unwrap_err();
        assert!(format!("{err:#}").contains(says), "{says}: {err:#}");
    }
}

#[test]
fn export_refuses_a_hard_link_and_leaves_no_part_written() {
    let dir = TempDir::new("archive-hard");
    let here = machine(&dir, "here");
    agent(&here, "programmer");
    write(&dir.join("secret"), "secret");
    std::fs::hard_link(
        dir.join("secret"),
        here.link.join("Agents/programmer/home/secret"),
    )
    .unwrap();
    let out = here.archive().join("programmer.tar.gz");
    let err = export_one(&here, &Fake::default(), "programmer", &out).unwrap_err();
    assert!(format!("{err:#}").contains("hard link"), "{err:#}");
    assert_eq!(std::fs::read_dir(here.archive()).unwrap().count(), 0);
}

#[test]
fn a_name_the_layout_keeps_is_never_taken() {
    let dir = TempDir::new("archive-reserved");
    let (here, there) = (machine(&dir, "here"), machine(&dir, "there"));
    agent(&here, "programmer");
    let out = dir.join("programmer.tar.gz");
    export_one(&here, &Fake::default(), "programmer", &out).unwrap();
    let picks = parse_picks("agent.programmer=agent.journal").unwrap();
    assert!(import(&out, &there, Some(&picks), &Fake::default(), None).is_err());
    assert!(!there.link.join("Agents/journal").exists());
}

#[test]
fn an_archive_folder_is_packed_whole_or_refused() {
    let dir = TempDir::new("archive-pack");
    let folder = dir.join("programmer");
    write(&folder.join("archive.toml"), MANIFEST);
    write(&folder.join("README.md"), "# programmer");
    write(&folder.join("Agents/programmer/home/a"), "a");
    write(
        &folder.join("Safe/Agents/programmer/agent.toml"),
        "harness = \"link\"\n",
    );
    let out = dir.join("out/programmer.tar.gz");
    pack(&folder, &out).unwrap();
    assert!(
        read_manifest(&out)
            .unwrap()
            .things
            .contains_key("agent.programmer")
    );
    let there = machine(&dir, "there");
    import(&out, &there, None, &Fake::default(), None).unwrap();
    assert!(there.link.join("Agents/programmer/home/a").is_file());
    // A file in no thing's folder is refused, never left out unsaid.
    write(&folder.join("Agents/other/x"), "x");
    let err = pack(&folder, &dir.join("again.tar.gz")).unwrap_err();
    assert!(format!("{err:#}").contains("in no thing"), "{err:#}");
}

/// A location renamed by a step of the archive's schema: an older
/// archive's `[map]` names it by its old name and reads as the current
/// one, which this Link knows. Without the step, the old name is refused.
#[test]
fn a_renamed_location_in_the_map_follows_by_the_archives_step() {
    let older = include_str!("../tests/fixtures/archive-1.toml");
    let renamed =
        link_files::schema::Format::parse(include_str!("../tests/fixtures/renamed.schema.toml"))
            .unwrap();
    let manifest = renamed.read_saved::<Manifest>(older).unwrap().spec;
    assert_eq!(manifest.map["link.boxes"], "Boxes");
    assert!(!manifest.map.contains_key("link.machines"));
    crate::reading::Plan::of(&manifest).unwrap();
    let unrenamed = link_files::saved::parse::<Manifest>(older).unwrap();
    let refused = crate::reading::Plan::of(&unrenamed)
        .err()
        .unwrap()
        .to_string();
    assert!(refused.contains("link.machines"), "{refused}");
}
