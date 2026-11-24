//! Every Next the bundle module's commands give is one its
//! `actions.toml` declares, which a debug build checks as it runs.

use link_format::Command as Typed;

/// Its declared `next` or `fail` for `typed`, from the Next graph the
/// program itself reads.
fn declared(typed: &str, failed: bool) -> Vec<String> {
    let (_, next, fail) = link_bundle::cli::NEXTS
        .iter()
        .find(|(command, _, _)| *command == typed)
        .unwrap_or_else(|| panic!("{typed} declares no Nexts"));
    let list = if failed { fail } else { next };
    list.iter().map(|c| c.to_string()).collect()
}

fn assert_declared(next: &link_format::Next, templates: &[String], typed: &Typed) {
    assert!(!next.0.is_empty());
    for suggestion in &next.0 {
        assert!(
            link_format::nexts::declares(templates, &suggestion.command, typed),
            "{} gives {}, not among {templates:?}",
            typed.text(),
            suggestion.command.text()
        );
    }
}

fn made(bin: Option<std::path::PathBuf>) -> link_bundle::make::Made {
    link_bundle::make::Made {
        name: "link-0.1.0-dev".to_string(),
        out: std::path::PathBuf::from("bundle"),
        version: "0.1.0-dev".to_string(),
        assets: Vec::new(),
        signed: true,
        bin,
    }
}

#[test]
fn a_made_bundle_gives_only_nexts_create_declares() {
    let typed = link_format::cmd!("lnk bundle create");
    let templates = declared("lnk bundle create", false);
    // A bundle holding this computer's programs, and one without.
    for bin in [Some(std::path::PathBuf::from("bundle/bin")), None] {
        assert_declared(&super::made_done(&made(bin)).next, &templates, &typed);
    }
}

#[test]
fn a_bundle_named_twice_gives_only_nexts_create_declares() {
    let typed = link_format::cmd!("lnk bundle create");
    let templates = declared("lnk bundle create", true);
    assert_declared(&super::named_twice().next, &templates, &typed);
}

#[test]
fn the_shelf_gives_only_nexts_list_declares() {
    let typed = link_format::cmd!("lnk bundle list");
    let templates = declared("lnk bundle list", false);
    assert_declared(&super::listed_done().next, &templates, &typed);
}
