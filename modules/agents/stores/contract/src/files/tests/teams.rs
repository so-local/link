//! Teams: a call naming a team reaches what the bridge is told at that call.

use super::*;

/// Teams' folders, each its own folder of the stand-in bucket by the
/// team's ID, and which team each name is now: asked at every call,
/// as a box asks the store.
type NamedTeams = BTreeMap<(Option<String>, String), (String, crate::harness::TeamFolders)>;

struct Teams {
    dir: PathBuf,
    /// `(owner, name)` to the team's ID and what the member reaches.
    named: Mutex<NamedTeams>,
    asked: AtomicUsize,
    /// Each write it was told of: the member, the team as named, the path.
    changed: Mutex<Vec<(String, String, String)>>,
}

impl Homes for Teams {
    fn home(&self, who: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: Arc::new(DirStore::new(&self.dir.join("homes").join(&who.agent))?),
            cap: None,
            write: vec!["work".into()],
        })
    }
    fn team(&self, who: &Identity, team: &TeamRef) -> anyhow::Result<Option<TeamFolder>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let named = self.named.lock().unwrap();
        let Some((id, folders)) = named.get(&(team.owner.clone(), team.name.clone())) else {
            return Ok(None);
        };
        Ok(Some(TeamFolder {
            store: Arc::new(DirStore::new(&self.dir.join("teams").join(id))?),
            id: id.clone(),
            owner: team.owner.clone().unwrap_or_else(|| who.owner.clone()),
            folders: folders.clone(),
            // Another account's member never keeps the index.
            keeps_index: team.owner.is_none(),
        }))
    }
    fn team_changed(&self, who: &Identity, team: &TeamRef, path: &str) {
        self.changed
            .lock()
            .unwrap()
            .push((who.agent.clone(), team.to_string(), path.into()));
    }
}

fn folders(write: &[&str], read: &[&str]) -> crate::harness::TeamFolders {
    crate::harness::TeamFolders {
        write: write.iter().map(|s| s.to_string()).collect(),
        read: read.iter().map(|s| s.to_string()).collect(),
    }
}

/// A member reaches a team's folder at each call as it's told then:
/// the folders it writes, those it only reads, nothing else; writes
/// from the version read; and a team taken from it, or renamed, is
/// refused at its next call, its old name reaching no other team.
#[test]
fn a_team_is_reached_as_the_bridge_is_told_at_each_call() {
    const OTHER: &str = "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e";
    let dir = TempDir::short("files-teams");
    let ours = crate::testing::bound(&dir.join("keys"), "ours00000001");
    let teams = Arc::new(Teams {
        dir: dir.join("bucket"),
        named: Mutex::new(BTreeMap::from([
            (
                (None, "research".to_string()),
                (
                    "t4m9q2x7k1ab".to_string(),
                    folders(&["notes"], &["sources"]),
                ),
            ),
            (
                (Some(OTHER.to_string()), "design".to_string()),
                ("z9y8x7w6v5u4".to_string(), folders(&[], &["."])),
            ),
        ])),
        asked: AtomicUsize::new(0),
        changed: Mutex::default(),
    });
    let (socket, _) = served(&dir, teams.clone());
    let client = Client {
        socket,
        binding: ours,
    };
    let research = TeamRef::parse("research").unwrap();
    let design = TeamRef::parse(&format!("{OTHER}/design")).unwrap();
    // Written where it writes; read where it only reads; nowhere else.
    let v = client
        .write_in(Some(&research), "notes/a.md", b"one", Expect::Absent)
        .unwrap();
    assert!(
        client
            .write_in(Some(&research), "sources/b.md", b"x", Expect::Absent)
            .is_err()
    );
    assert!(
        client
            .write_in(Some(&research), "drafts/c.md", b"x", Expect::Absent)
            .is_err()
    );
    // What the others wrote, beside it.
    let shared = DirStore::new(&dir.join("bucket/teams/t4m9q2x7k1ab")).unwrap();
    shared
        .put("home/sources/s.md", b"source", &Expect::Absent)
        .unwrap();
    shared
        .put("home/drafts/d.md", b"draft", &Expect::Absent)
        .unwrap();
    assert_eq!(
        client
            .read_in(Some(&research), "sources/s.md")
            .unwrap()
            .unwrap()
            .0,
        b"source"
    );
    assert!(client.read_in(Some(&research), "drafts/d.md").is_err());
    let listed: Vec<String> = client
        .list_in(Some(&research), "")
        .unwrap()
        .into_iter()
        .map(|o| o.path)
        .collect();
    assert_eq!(listed, ["notes/a.md", "sources/s.md"]);
    // From the version read alone.
    let stale = client.write_in(Some(&research), "notes/a.md", b"two", Expect::Absent);
    assert!(stale.is_err_and(|e| is_conflict(&e)));
    client
        .write_in(Some(&research), "notes/a.md", b"two", Expect::Version(v))
        .unwrap();
    // Another account's team, read whole and written nowhere.
    assert!(client.list_in(Some(&design), "").unwrap().is_empty());
    assert!(
        client
            .write_in(Some(&design), "notes/a.md", b"x", Expect::Absent)
            .is_err()
    );
    // Home is Home: a folder of it named teams is its own.
    client
        .write("work/teams/research.md", b"mine", Expect::Absent)
        .unwrap();
    assert!(
        client
            .read_in(Some(&research), "work/teams/research.md")
            .is_err()
    );
    // Renamed: the old name reaches nothing, though another team now
    // takes it; taken off: refused at the next call.
    let asked = teams.asked.load(Ordering::SeqCst);
    {
        let mut named = teams.named.lock().unwrap();
        let research = named.remove(&(None, "research".to_string())).unwrap();
        named.insert((None, "researchers".to_string()), research);
        named.insert(
            (None, "research".to_string()),
            ("p3q4r5s6t7u8".to_string(), folders(&[], &[])),
        );
    }
    assert!(client.read_in(Some(&research), "notes/a.md").is_err());
    let renamed = TeamRef::parse("researchers").unwrap();
    assert_eq!(
        client
            .read_in(Some(&renamed), "notes/a.md")
            .unwrap()
            .unwrap()
            .0,
        b"two"
    );
    teams.named.lock().unwrap().clear();
    let off = client.read_in(Some(&renamed), "notes/a.md").unwrap_err();
    assert!(format!("{off:#}").contains("aren't on a team"), "{off:#}");
    assert!(
        teams.asked.load(Ordering::SeqCst) >= asked + 3,
        "asked at each call"
    );
}

/// A team is named `<name>` or `<account>/<name>`: a name holds no
/// `/` and an owner is an account's, so the two never meet.
#[test]
fn a_teams_two_forms_never_meet() {
    const OTHER: &str = "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e";
    let own = TeamRef::parse("research").unwrap();
    assert_eq!((own.owner, own.name.as_str()), (None, "research"));
    let theirs = TeamRef::parse(&format!("{OTHER}/design")).unwrap();
    assert_eq!(theirs.owner.as_deref(), Some(OTHER));
    assert_eq!(theirs.to_string(), format!("{OTHER}/design"));
    // A name shaped like an account is a name of the owner's own team,
    // never another account: the forms differ by their `/`.
    let named = TeamRef::parse("acct-0b7c").unwrap();
    assert_eq!(named.owner, None);
    // No account ID is a name (too long), so none reads as the other.
    assert!(TeamRef::parse(OTHER).is_err());
    for bad in [
        "",
        "Research",
        "research/",
        "/research",
        "me@box/research",
        "me/research",
        &format!("{OTHER}/design/x"),
        &format!("{OTHER}/Design"),
    ] {
        assert!(TeamRef::parse(bad).is_err(), "{bad}");
    }
    // As a call carries it.
    let action: Action = serde_json::from_str(&format!(
        r#"{{"action": "read", "path": "notes/a.md", "team": "{OTHER}/design"}}"#
    ))
    .unwrap();
    assert!(matches!(action, Action::Read { team: Some(t), .. } if t.owner.is_some()));
    // A field it doesn't know is refused, never ignored: a call naming a
    // team is never served from Home by a bridge that skipped it.
    assert!(
        serde_json::from_str::<Action>(
            r#"{"action": "index-get", "name": "link-memory/manifest.json", "teem": "research"}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<Action>(
            r#"{"action": "read", "path": "a", "team": "me@box/design"}"#
        )
        .is_err()
    );
}

/// A team's index is kept beside its folder, in the folder's storage,
/// and reached only by a member granted all of the folder: one granted
/// part of it is refused the index, though it reads and writes its part.
/// A member's write is told, so the team's memory lists it again.
#[test]
fn a_teams_index_is_beside_its_folder_for_whole_members_alone() {
    let dir = TempDir::short("files-team-index");
    let whole = crate::testing::bound(&dir.join("keys"), "whole0000001");
    let part = crate::testing::bound(&dir.join("keys"), "part00000001");
    let teams = Arc::new(Teams {
        dir: dir.join("bucket"),
        named: Mutex::new(BTreeMap::from([(
            (None, "research".to_string()),
            ("t4m9q2x7k1ab".to_string(), folders(&["."], &[])),
        )])),
        asked: AtomicUsize::new(0),
        changed: Mutex::default(),
    });
    let (socket, _) = served(&dir, teams.clone());
    let research = TeamRef::parse("research").unwrap();
    let ours = InTeam {
        client: Client {
            socket: socket.clone(),
            binding: whole,
        },
        team: research.clone(),
    };
    // Through the store's names, as memory writes its index's copy.
    let v = ours
        .put("index/link-memory/manifest.json", b"{}", &Expect::Absent)
        .unwrap();
    ours.put("home/notes/a.md", b"one", &Expect::Absent)
        .unwrap();
    assert_eq!(
        ours.get("index/link-memory/manifest.json")
            .unwrap()
            .unwrap(),
        (b"{}".to_vec(), v.clone())
    );
    let listed: Vec<String> = ours
        .list("index/link-memory/")
        .unwrap()
        .into_iter()
        .map(|o| o.path)
        .collect();
    assert_eq!(listed, ["index/link-memory/manifest.json"]);
    // Kept beside the folder, in its storage; not in the member's Home.
    let shared = DirStore::new(&dir.join("bucket/teams/t4m9q2x7k1ab")).unwrap();
    assert!(
        shared
            .get("index/link-memory/manifest.json")
            .unwrap()
            .is_some()
    );
    assert!(
        ours.client
            .index_get("link-memory/manifest.json")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        teams.changed.lock().unwrap().clone(),
        [(
            "whole0000001".to_string(),
            "research".to_string(),
            "notes/a.md".to_string()
        )]
    );
    // Granted part of the folder: its part, never the index.
    teams.named.lock().unwrap().insert(
        (None, "research".to_string()),
        ("t4m9q2x7k1ab".to_string(), folders(&["notes"], &[])),
    );
    let theirs = InTeam {
        client: Client {
            socket,
            binding: part,
        },
        team: research,
    };
    theirs
        .put("home/notes/b.md", b"two", &Expect::Absent)
        .unwrap();
    let refused = theirs.get("index/link-memory/manifest.json").unwrap_err();
    assert!(
        format!("{refused:#}").contains("granted all"),
        "{refused:#}"
    );
    assert!(
        theirs
            .put("index/link-memory/x", b"x", &Expect::Absent)
            .is_err()
    );
    assert!(
        theirs
            .delete("index/link-memory/manifest.json", &Expect::Version(v))
            .is_err()
    );
}

/// Every call on a team's folder passes one check of the member's grant:
/// a member reading all of the folder reads the team's index, never
/// writes or deletes it (nor writes a file, as before); a member writing
/// all of it writes only the index's own objects, within the team's cap.
#[test]
fn a_teams_index_is_written_only_by_a_member_writing_all_of_it() {
    let dir = TempDir::short("files-team-index-write");
    let reader = crate::testing::bound(&dir.join("keys"), "reader000001");
    let teams = Arc::new(Teams {
        dir: dir.join("bucket"),
        named: Mutex::new(BTreeMap::from([(
            (None, "research".to_string()),
            ("t4m9q2x7k1ab".to_string(), folders(&[], &["."])),
        )])),
        asked: AtomicUsize::new(0),
        changed: Mutex::default(),
    });
    let (socket, _) = served(&dir, teams.clone());
    let team = TeamRef::parse("research").unwrap();
    let ours = InTeam {
        client: Client {
            socket,
            binding: reader,
        },
        team: team.clone(),
    };
    // The team's writer's index, beside the folder.
    let shared = DirStore::new(&dir.join("bucket/teams/t4m9q2x7k1ab")).unwrap();
    let v = shared
        .put("index/link-memory/manifest.json", b"{}", &Expect::Absent)
        .unwrap();
    shared
        .put("home/memory/plan.md", b"plan", &Expect::Absent)
        .unwrap();
    // Read whole: the index read and listed, the folder read.
    assert!(
        ours.get("index/link-memory/manifest.json")
            .unwrap()
            .is_some()
    );
    assert_eq!(ours.list("index/link-memory/").unwrap().len(), 1);
    assert!(ours.get("home/memory/plan.md").unwrap().is_some());
    // Never a write: the index's manifest, a segment, a delete, a file.
    let segment = format!("index/link-memory/seg-{}", "a".repeat(64));
    for refused in [
        ours.put(
            "index/link-memory/manifest.json",
            b"{\"evil\":1}",
            &Expect::Version(v.clone()),
        ),
        ours.put(&segment, b"x", &Expect::Absent),
        ours.put("home/memory/plan.md", b"x", &Expect::Version(v.clone())),
    ] {
        let err = refused.unwrap_err();
        assert!(format!("{err:#}").contains("write"), "{err:#}");
    }
    assert!(
        ours.delete(
            "index/link-memory/manifest.json",
            &Expect::Version(v.clone())
        )
        .is_err()
    );
    assert_eq!(
        shared
            .get("index/link-memory/manifest.json")
            .unwrap()
            .unwrap()
            .0,
        b"{}"
    );
    // Writing all of it: the index's own objects alone, within the cap.
    teams.named.lock().unwrap().insert(
        (None, "research".to_string()),
        ("t4m9q2x7k1ab".to_string(), folders(&["."], &[])),
    );
    ours.put(&segment, b"segment", &Expect::Absent).unwrap();
    for name in ["index/link-memory/notes.txt", "index/link-memory/seg-xyz"] {
        let err = ours.put(name, b"x", &Expect::Absent).unwrap_err();
        assert!(format!("{err:#}").contains("isn't an object"), "{err:#}");
    }
    // Another account's member writing all of it: it reads the index,
    // and never keeps it, as the team owner pays for it.
    const OTHER: &str = "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e";
    teams.named.lock().unwrap().insert(
        (Some(OTHER.to_string()), "research".to_string()),
        ("t4m9q2x7k1ab".to_string(), folders(&["."], &[])),
    );
    let outsider = InTeam {
        client: ours.client.duplicate(),
        team: TeamRef::parse(&format!("{OTHER}/research")).unwrap(),
    };
    assert!(
        outsider
            .get("index/link-memory/manifest.json")
            .unwrap()
            .is_some()
    );
    let err = outsider
        .put(&segment, b"theirs", &Expect::Absent)
        .unwrap_err();
    assert!(format!("{err:#}").contains("pays for"), "{err:#}");
    // The cap counts what's beside the folder: here, a store that holds
    // its most already.
    let full = Arc::new(Full(Arc::new(shared)));
    let refused = index_of(
        full.as_ref(),
        Action::IndexPut {
            name: "link-memory/manifest.json".into(),
            data: base64::engine::general_purpose::STANDARD.encode(b"{}"),
            expect: Expect::Absent,
            team: Some(team),
        },
    )
    .unwrap_err();
    assert!(format!("{refused:#}").contains("at most"), "{refused:#}");
}

/// A full capacity authority. Ordinary writes remain possible: this fixture
/// refuses only when the bridge asks for shared capacity before mutation.
struct Full(Arc<dyn Store>);

impl Store for Full {
    fn put_bounded(
        &self,
        _: &str,
        _: &[u8],
        _: &Expect,
        capacity: &Capacity,
    ) -> anyhow::Result<Version> {
        assert_eq!(*capacity, team_index_capacity());
        bail!("the shared object capacity holds at most its configured bytes")
    }
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.0.get(name)
    }
    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        self.0.put(name, data, expect)
    }
    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        self.0.delete(name, expect)
    }
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        self.0.list(prefix)
    }
}

/// A team's file's grant is a role: what it writes is read and written,
/// what it only reads is read, and `.` is the whole folder.
#[test]
fn a_teams_folders_are_a_role() {
    let role = team_role("t4m9q2x7k1ab", None, &folders(&["drafts"], &["notes"])).unwrap();
    assert_eq!(role.name, "team-t4m9q2x7k1ab-folder");
    let text = link_files::schema::print_spec(&role).unwrap();
    assert_eq!(link_policy::parse(&text).unwrap(), role);
    assert!(
        text.contains(r#"on = ["bucket:team-t4m9q2x7k1ab/drafts/*"]"#),
        "{text}"
    );
    assert!(text.contains(r#"actions = ["bucket.read"]"#), "{text}");
    let whole = team_role("t4m9q2x7k1ab", None, &folders(&["."], &[])).unwrap();
    assert!(
        toml::to_string(&whole)
            .unwrap()
            .contains(r#"on = ["bucket:team-t4m9q2x7k1ab/*"]"#)
    );
    assert!(
        team_role("t4m9q2x7k1ab", None, &folders(&[], &[]))
            .unwrap()
            .allow
            .is_empty()
    );
}

/// A folder service's calls ([`TeamCalls`]) are checked as the bridge's
/// own: for the member it proved, on the team its route names whatever
/// the call says, refused for a folder its grants don't reach, and
/// refused at the next call once it's taken off the team.
#[test]
fn a_folder_services_calls_are_checked_as_the_bridges() {
    let dir = TempDir::short("files-team-calls");
    let teams = Arc::new(Teams {
        dir: dir.join("bucket"),
        named: Mutex::new(BTreeMap::from([(
            (None, "research".to_string()),
            (
                "t4m9q2x7k1ab".to_string(),
                folders(&["notes"], &["sources"]),
            ),
        )])),
        asked: AtomicUsize::new(0),
        changed: Mutex::default(),
    });
    let calls = TeamCalls::new(teams.clone());
    let who = super::who("ours00000001");
    let research = TeamRef::parse("research").unwrap();
    let write = |path: &str, team: Option<TeamRef>| Action::Write {
        path: path.into(),
        data: base64::engine::general_purpose::STANDARD.encode(b"one"),
        expect: Expect::Absent,
        team,
    };
    // Another team named in the call is the route's team all the same.
    let forged = TeamRef::parse("elsewhere").unwrap();
    assert!(matches!(
        calls.act(&who, &research, write("notes/a.md", Some(forged))),
        Ok(Answer::Written { .. })
    ));
    let shared = DirStore::new(&dir.join("bucket/teams/t4m9q2x7k1ab")).unwrap();
    assert!(shared.get("home/notes/a.md").unwrap().is_some());
    let refused = calls.act(&who, &research, write("sources/a.md", None));
    assert_eq!(refused.unwrap_err().why, Why::Refused);
    let stale = calls.act(&who, &research, write("notes/a.md", None));
    assert_eq!(stale.unwrap_err().why, Why::Conflict);
    teams.named.lock().unwrap().clear();
    let read = Action::Read {
        path: "notes/a.md".into(),
        team: None,
    };
    let gone = calls.act(&who, &research, read).unwrap_err();
    assert_eq!(gone.why, Why::Refused);
}
