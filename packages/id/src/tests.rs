use super::*;

const ID: &str = "k3m4n5p6q7r8";

#[test]
fn an_id_is_a_kind_and_a_minted_id() {
    // A workload's kinds are its owners', none of them this package's.
    for kind in ["agent", "job", "env-2"] {
        let workload: Workload = format!("{kind}:{ID}").parse().unwrap();
        assert_eq!(workload.kind().as_str(), kind);
        assert_eq!(workload.id(), ID);
        let named = WorkloadKind::new(kind).unwrap();
        assert_eq!(Workload::new(&named, ID).unwrap(), workload);
    }
    let longest = format!("{}:{ID}", "k".repeat(KIND_MAX));
    assert!(longest.parse::<Workload>().is_ok());
    let minted = candidate().unwrap();
    assert!(format!("job:{minted}").parse::<Workload>().is_ok());
    // Only an id Link minted: never a name, an outside ID, or one of
    // another length or alphabet.
    for not in [
        ID,
        "job:",
        "job:x1",
        "job:main",
        "job:K3M4N5P6Q7R8",
        "job:k3m4n5p6q7r",
        "job:k3m4n5p6q7r8s",
        "job:k3m4-5p6q7r8",
        "job:a/b:c4d5e6f7",
        "job:0aa1b2c3-0000-4000-8000-0000000000aa",
        "user:slack:T0ACME/U0ALICE",
        &format!("Job:{ID}"),
        &format!("2job:{ID}"),
        &format!("my_job:{ID}"),
        &format!("{}:{ID}", "k".repeat(KIND_MAX + 1)),
        "",
    ] {
        assert!(not.parse::<Workload>().is_err(), "{not}");
    }
    assert!(Workload::of("job", "main").is_err());
    assert_eq!(
        format!("Job:{ID}")
            .parse::<Workload>()
            .unwrap_err()
            .to_string(),
        "not a workload's kind"
    );
}

#[test]
fn an_execution_is_a_workload_and_its_run() {
    let run: Execution = "agent:a1b2c3d4e5f6@4242".parse().unwrap();
    assert_eq!(run.workload().as_str(), "agent:a1b2c3d4e5f6");
    assert_eq!(run.run(), "4242");
    assert_eq!(run.to_string(), "agent:a1b2c3d4e5f6@4242");
    let run: Execution = format!("job:{ID}@run-1.2_3").parse().unwrap();
    assert_eq!(run.run(), "run-1.2_3");
    for not in [
        "agent:a1b2c3d4e5f6",
        "agent:a1b2c3d4e5f6@",
        "agent:a1b2c3d4e5f6@x/y",
        "agent:a1b2c3d4e5f6@x@y",
        "agent:a1@4242",
        "@4242",
        &format!("agent:a1b2c3d4e5f6@{}", "9".repeat(RUN_MAX + 1)),
    ] {
        assert!(not.parse::<Execution>().is_err(), "{not}");
    }
}

#[test]
fn each_is_written_as_a_string() {
    let job: Workload = format!("job:{ID}").parse().unwrap();
    assert_eq!(
        serde_json::to_string(&job).unwrap(),
        format!("\"job:{ID}\"")
    );
    assert_eq!(
        serde_json::from_str::<Workload>(&format!("\"job:{ID}\"")).unwrap(),
        job
    );
    assert!(serde_json::from_str::<Workload>(&format!("\"{ID}\"")).is_err());
    assert!(serde_json::from_str::<Workload>("\"job:k3m4\"").is_err());
    let run = Execution::new(job, "7").unwrap();
    assert_eq!(
        serde_json::to_string(&run).unwrap(),
        format!("\"job:{ID}@7\"")
    );
    assert_eq!(
        serde_json::from_str::<Execution>(&format!("\"job:{ID}@7\"")).unwrap(),
        run
    );
}

#[test]
fn a_resource_is_written_of_its_owners_kind() {
    let r#box = Resource::of("box", "a1b2c3d4e5f6").unwrap();
    assert_eq!(r#box.as_str(), "box:a1b2c3d4e5f6");
    assert_eq!(r#box.kind().as_str(), "box");
    assert!(Resource::of("Box", "a1b2c3d4e5f6").is_err());
    assert_eq!(
        candidate_of("box", "box:a1b2c3d4e5f6"),
        Some("a1b2c3d4e5f6")
    );
    assert_eq!(candidate_of("box", "team:a1b2c3d4e5f6"), None);
    assert_eq!(candidate_of("box", "a1b2c3d4e5f6"), None);
    assert_eq!(candidate_of("box", "box:A1"), None);
}

#[test]
fn its_axes_read() {
    for axis in ["principal", "workload", "execution", "parent", "resource"] {
        assert!(is_axis(axis), "{axis}");
    }
    for not in ["id", "host", "group"] {
        assert!(!is_axis(not), "{not}");
    }
    assert_eq!(axes().len(), 5);
}

#[test]
fn its_limits_read() {
    let limits = limits();
    assert!(limits.candidate_length >= 8);
    assert!(limits.reserve_attempts > 0 && limits.parent_depth > 0);
}

#[test]
fn a_candidate_is_its_alphabet_at_its_length() {
    let drawn: std::collections::BTreeSet<String> = (0..64).map(|_| candidate().unwrap()).collect();
    assert_eq!(drawn.len(), 64, "candidates repeat");
    for id in &drawn {
        assert!(is_candidate(id), "{id}");
        assert!(is_id(id));
    }
    for not in ["", "short", "ABCDEFGHIJKL", "abcdefghijk-", "abcdefghijklm"] {
        assert!(!is_candidate(not), "{not}");
    }
}

/// An owner of IDs, as the agent and secret modules are: a table under
/// its own lock.
#[derive(Default)]
struct Owner(std::sync::Mutex<Table>);

impl Owner {
    fn create(&self) -> String {
        reserve(&mut *self.0.lock().unwrap()).unwrap()
    }
    fn delete(&self, id: &str) {
        self.0.lock().unwrap().retire(id);
    }
}

#[test]
fn concurrent_reservations_never_share_an_id() {
    let owner = std::sync::Arc::new(Owner::default());
    let made: Vec<String> = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| (0..32).map(|_| owner.create()).collect::<Vec<_>>()))
            .collect();
        threads
            .into_iter()
            .flat_map(|t| t.join().unwrap())
            .collect()
    });
    let unique: std::collections::BTreeSet<_> = made.iter().cloned().collect();
    assert_eq!(unique.len(), made.len());
    assert_eq!(owner.0.lock().unwrap().live, unique);
}

fn drawing(ids: &[&str]) -> impl FnMut() -> Result<String, NoRandomness> {
    let mut ids: Vec<String> = ids.iter().rev().map(|s| s.to_string()).collect();
    move || Ok(ids.pop().expect("drew past the list"))
}

#[test]
fn a_reservation_refuses_a_live_or_retired_candidate() {
    let mut table = Table::default();
    assert_eq!(reserve_from(&mut table, drawing(&["a1"])), Ok("a1".into()));
    table.hold("b2");
    table.retire("b2");
    assert_eq!(
        reserve_from(&mut table, drawing(&["a1", "b2", "c3"])),
        Ok("c3".into())
    );
    assert_eq!(admit(&mut table, "a1"), Err(Refused::Live));
    assert_eq!(admit(&mut table, "b2"), Err(Refused::Retired));
    let always = vec!["a1"; limits().reserve_attempts];
    assert_eq!(
        reserve_from(&mut table, drawing(&always)),
        Err(Refused::Exhausted)
    );
}

#[test]
fn a_deleted_id_stays_retired() {
    let owner = Owner::default();
    let id = owner.create();
    owner.delete(&id);
    let mut table = owner.0.lock().unwrap();
    assert!(!table.is_live(&id) && table.is_retired(&id));
    assert_eq!(admit(&mut *table, &id), Err(Refused::Retired));
    // Written and read back, it's retired still.
    let text = toml::to_string(&*table).unwrap();
    let mut read: Table = toml::from_str(&text).unwrap();
    assert_eq!(admit(&mut read, &id), Err(Refused::Retired));
    // What it named, brought back whole, is the one way it's live again.
    read.revive(&id);
    assert!(read.is_live(&id) && !read.is_retired(&id));
    assert!(toml::from_str::<Table>("other = []").is_err());
}

#[test]
fn an_executions_parent_is_beside_it_and_bounded() {
    let start: Execution = "agent:a1b2c3d4e5f6@k1".parse().unwrap();
    let root = Lineage::root(start.clone());
    assert_eq!((root.parent(), root.depth()), (None, 0));
    let tool: Execution = "agent:a1b2c3d4e5f6@k2".parse().unwrap();
    let child = root.child(tool.clone()).unwrap();
    assert_eq!(child.parent(), Some(&start));
    assert_eq!(child.depth(), 1);
    // Its ID is its own: the parent is never part of it.
    assert_eq!(child.execution().to_string(), "agent:a1b2c3d4e5f6@k2");
    assert!(root.child(start.clone()).is_err());
    assert!(child.child(start.clone()).is_err());
    let mut deepest = root.clone();
    for n in 0..limits().parent_depth {
        let next: Execution = format!("job:j1o2b3j4o5b6@r{n}").parse().unwrap();
        deepest = deepest.child(next).unwrap();
    }
    assert!(
        deepest
            .child("job:j1o2b3j4o5b6@last".parse().unwrap())
            .is_err()
    );
}

#[test]
fn a_lineage_is_checked_as_it_is_read() {
    let child = Lineage::root("agent:a1b2c3d4e5f6@k1".parse().unwrap())
        .child("agent:a1b2c3d4e5f6@k2".parse().unwrap())
        .unwrap();
    let text = serde_json::to_string(&child).unwrap();
    assert_eq!(
        text,
        r#"{"execution":"agent:a1b2c3d4e5f6@k2","parent":"agent:a1b2c3d4e5f6@k1","depth":1}"#
    );
    assert_eq!(serde_json::from_str::<Lineage>(&text).unwrap(), child);
    for not in [
        r#"{"execution":"agent:a1b2c3d4e5f6@k2","depth":1}"#,
        r#"{"execution":"agent:a1b2c3d4e5f6@k2","parent":"agent:a1b2c3d4e5f6@k1"}"#,
        r#"{"execution":"agent:a1b2c3d4e5f6@k2","parent":"agent:a1b2c3d4e5f6@k2","depth":1}"#,
        r#"{"execution":"agent:a1b2c3d4e5f6@k2","parent":"agent:a1b2c3d4e5f6@k1","depth":99}"#,
    ] {
        assert!(serde_json::from_str::<Lineage>(not).is_err(), "{not}");
    }
}
