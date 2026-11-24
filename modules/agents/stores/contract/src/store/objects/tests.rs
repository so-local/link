use super::jobs::{CLAIM_LAPSE, Moving, placed};
use super::*;
use crate::files::DirStore;

/// Now, in seconds since 1970: a claim's time, as the tests set it.
fn secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A store's driver in a process of its own is what the contract
/// checks; here the objects themselves answer.
struct Here(Objects<DirStore>);

impl super::super::Store for Here {
    fn name(&self) -> &str {
        "objects"
    }

    fn call(&self, request: &Request) -> anyhow::Result<Reply> {
        match self.0.answer(request)? {
            Reply::Error { error } => bail!("{error}"),
            reply => Ok(reply),
        }
    }
}

fn agent() -> Agent {
    Agent {
        id: "aaaaaaaaaaaa".into(),
        owner: "me@box".into(),
    }
}

fn here(name: &str) -> (link_test_kit::TempDir, Here) {
    let dir = link_test_kit::TempDir::new(name);
    let store = DirStore::new(&dir.join("bucket")).unwrap();
    (dir, Here(Objects::new(store, agent())))
}

/// A lease taken a moment before a second turns isn't lapsed a
/// moment after it: its lapse is judged to the millisecond.
#[test]
fn a_lease_lapses_by_its_milliseconds_not_its_second() {
    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let set = |ms: u64| CLOCK.store(ms, std::sync::atomic::Ordering::SeqCst);
    let dir = link_test_kit::TempDir::new("objects-lapse");
    let store = DirStore::new(&dir.join("bucket")).unwrap();
    let objects =
        Objects::new(store, agent()).with_clock(|| CLOCK.load(std::sync::atomic::Ordering::SeqCst));
    set(100_999);
    assert_eq!(objects.take("t", "a", 1000).unwrap(), Took::Took);
    set(101_004);
    assert_eq!(objects.take("t", "b", 1000).unwrap(), Took::Held);
    set(101_999);
    assert_eq!(objects.take("t", "b", 1000).unwrap(), Took::Lapsed);
}

#[test]
fn the_contract_holds_over_objects() {
    let (_dir, here) = here("objects-contract");
    super::super::check(&here, &agent());
}

#[test]
fn the_jobs_contract_holds_over_objects() {
    let (_dir, here) = here("objects-jobs");
    super::super::check_jobs(&here, &agent());
}

#[test]
fn the_files_contract_holds_over_objects() {
    let (_dir, here) = here("objects-files");
    super::super::check_files(&here, &agent());
}

#[test]
fn a_conversations_tools_narrow_and_never_widen() {
    let (_dir, here) = here("objects-tools");
    super::super::check_narrowing(&here, &agent());
}

#[test]
fn names_escape_and_come_back() {
    for name in ["telegram:42", "a/b", "..", "%", "ünï"] {
        let escaped = escape(name);
        assert!(
            escaped
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_%".contains(&b))
        );
        assert_eq!(unescape(&escaped).as_deref(), Some(name));
    }
    assert_eq!(unescape("%zz"), None);
    // One spelling only: two objects never pass for one conversation.
    assert_eq!(unescape("telegram%3a42"), None);
    assert_eq!(unescape("a%2Db"), None);
}

/// A reader that read a shift's claim before the shift finished never
/// writes the job back: the shift moved it, its runner then removed
/// it, and the reader's late settle leaves it gone, not fired twice.
/// A claim whose shift stopped is finished by a reader once it's
/// [`CLAIM_LAPSE`] old, the job where it was going, once.
#[test]
fn a_late_reader_never_brings_a_shifted_job_back() {
    let (_dir, here) = here("objects-late");
    let objects = &here.0;
    let job = Job {
        id: "j1".into(),
        spec: serde_json::json!({"id": "j1"}),
        due: 1,
    };
    let waiting = objects.job(Place::Waiting, "j1");
    let firing = objects.job(Place::Firing, "j1");
    // A shift's claim, as a reader reads it mid-shift.
    let claim = serde_json::to_vec(&Moving {
        moving_to: Place::Firing,
        job: job.clone(),
        claimed: secs(),
    })
    .unwrap();
    let version = objects
        .store
        .put(&waiting, &claim, &Expect::Absent)
        .unwrap();
    // Read while young: in neither place yet, never finished by it.
    assert!(objects.job_at(Place::Waiting, "j1").unwrap().is_none());
    assert!(objects.store.get(&firing).unwrap().is_none());
    // The shift finishes; its runner fires the job and removes it.
    objects.settle(&waiting, &claim, version.clone()).unwrap();
    assert!(objects.store.get(&firing).unwrap().is_some());
    objects.store.delete(&firing, &Expect::Any).unwrap();
    // The reader, late, with what it read: nothing comes back.
    objects.settle_stopped(&waiting, &claim, &version).unwrap();
    assert!(objects.store.get(&firing).unwrap().is_none());
    assert!(objects.store.get(&waiting).unwrap().is_none());

    // A shift that stopped after claiming: finished once it's old.
    let stopped = serde_json::to_vec(&Moving {
        moving_to: Place::Firing,
        job: job.clone(),
        claimed: secs() - CLAIM_LAPSE,
    })
    .unwrap();
    objects
        .store
        .put(&waiting, &stopped, &Expect::Absent)
        .unwrap();
    let listed: Vec<String> = objects
        .jobs(Place::Firing)
        .unwrap()
        .into_iter()
        .map(|j| j.id)
        .collect();
    assert!(listed.is_empty());
    assert!(objects.jobs(Place::Waiting).unwrap().is_empty());
    assert_eq!(
        objects
            .jobs(Place::Firing)
            .unwrap()
            .into_iter()
            .map(|j| j.id)
            .collect::<Vec<_>>(),
        ["j1"]
    );
    assert!(objects.store.get(&waiting).unwrap().is_none());
}

/// A shift that died after placing its job, before removing its
/// claim: the job's runner removes it, which removes the claim too,
/// so a reader meeting the claim later never brings the job back.
#[test]
fn a_shift_that_died_after_placing_its_job_never_brings_it_back() {
    let (_dir, here) = here("objects-died");
    let objects = &here.0;
    let job = Job {
        id: "j2".into(),
        spec: serde_json::json!({"id": "j2"}),
        due: 1,
    };
    let waiting = objects.job(Place::Waiting, "j2");
    let firing = objects.job(Place::Firing, "j2");
    // Claimed long ago, the job placed, and the shift gone.
    let claim = serde_json::to_vec(&Moving {
        moving_to: Place::Firing,
        job: job.clone(),
        claimed: secs() - CLAIM_LAPSE,
    })
    .unwrap();
    let version = objects
        .store
        .put(&waiting, &claim, &Expect::Absent)
        .unwrap();
    objects
        .store
        .put(
            &firing,
            &placed(&job, &waiting, &version).unwrap(),
            &Expect::Absent,
        )
        .unwrap();
    // Its runner, done with it, removes it: the claim goes with it.
    let removed = super::super::Store::call(
        &here,
        &Request::JobRemove {
            agent: agent(),
            place: Place::Firing,
            id: "j2".into(),
        },
    )
    .unwrap();
    assert_eq!(removed, Reply::Found { found: true });
    assert!(objects.store.get(&waiting).unwrap().is_none());
    // A reader after: nothing to finish, nothing back.
    assert!(objects.jobs(Place::Waiting).unwrap().is_empty());
    assert!(objects.jobs(Place::Firing).unwrap().is_empty());
    assert!(objects.store.get(&firing).unwrap().is_none());
}
