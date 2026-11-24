use super::*;

#[test]
fn an_action_id_has_three_names_and_a_command() {
    let id: ActionId = "env.approval.add".parse().unwrap();
    assert_eq!(
        (id.namespace(), id.resource(), id.verb()),
        ("env", "approval", "add")
    );
    assert_eq!(id.command(), "lnk env approval add");
    let own: ActionId = "env.env.create".parse().unwrap();
    assert_eq!(own.command(), "lnk env create");
    // Its program never reads its own namespace.
    assert_eq!(
        (id.program(), id.words()),
        ("lnk-env".into(), vec!["approval", "add"])
    );
    assert_eq!(own.words(), ["create"]);
    let served: ActionId = "web-tool.web-tool.serve".parse().unwrap();
    assert_eq!(
        (served.program(), served.command()),
        ("lnk-web-tool".into(), "lnk web-tool serve".into())
    );
    // Core's program is lnk itself.
    let doctor: ActionId = "core.core.doctor".parse().unwrap();
    assert_eq!(
        (doctor.program(), doctor.command()),
        ("lnk".into(), "lnk doctor".into())
    );
    let add: ActionId = "core.module.add".parse().unwrap();
    assert_eq!(
        (add.words(), add.command()),
        (vec!["module", "add"], "lnk module add".into())
    );
    for bad in ["env.create", "env.a.b.c", "Env.env.create", "a..b", ""] {
        assert!(bad.parse::<ActionId>().is_err(), "{bad:?} read");
    }
}

#[test]
fn a_written_name_is_qualified_by_its_module() {
    let own = TypeName::written("stage", "env").unwrap();
    assert_eq!(own.to_string(), "env.stage");
    let other = TypeName::written("box.id", "env").unwrap();
    assert_eq!((other.module(), other.name()), ("box", "id"));
    for bad in ["", "Stage", "a.b.c", "box.", ".id", "files/x"] {
        assert!(TypeName::written(bad, "env").is_err(), "{bad:?} read");
    }
}

#[test]
fn a_reference_names_an_action_or_pins_one_resource() {
    let plain: ActionRef = "social.post.show".parse().unwrap();
    assert_eq!(plain.resource(), None);
    let pinned: ActionRef = "social.post:k7m2p9q4r8s3.show".parse().unwrap();
    assert_eq!(pinned.action().as_str(), "social.post.show");
    assert_eq!(pinned.resource(), Some("post:k7m2p9q4r8s3"));
    for bad in ["social.post", "social.post:.show", "social.post:a b.show"] {
        assert!(bad.parse::<ActionRef>().is_err(), "{bad:?} read");
    }
}
