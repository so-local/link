//! Connecting as this machine's service account, with a stand-in for
//! its metadata server, and what each reader makes of such an account.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::*;

/// A metadata server that answers from `values`, or isn't there, and
/// keeps the paths it was asked.
struct Fake {
    values: Option<BTreeMap<&'static str, &'static str>>,
    asked: RefCell<Vec<String>>,
}

impl Fake {
    fn new(values: &[(&'static str, &'static str)]) -> Self {
        Self {
            values: Some(values.iter().copied().collect()),
            asked: RefCell::default(),
        }
    }
}

impl Metadata for Fake {
    fn read(&self, path: &str) -> anyhow::Result<Reading> {
        self.asked.borrow_mut().push(path.to_string());
        Ok(match &self.values {
            None => Reading::NoServer,
            Some(values) => match values.get(path) {
                Some(value) => Reading::Found((*value).to_string()),
                None => Reading::Missing,
            },
        })
    }
}

const EMAIL: &str = "box@my-project.iam.gserviceaccount.com";

fn compute_engine() -> Fake {
    Fake::new(&[
        ("instance/service-accounts/default/email", EMAIL),
        ("project/project-id", "my-project"),
        ("instance/zone", "projects/123456789/zones/europe-west2-b"),
    ])
}

fn options(project: Option<&str>, zone: Option<&str>) -> Connect {
    Connect {
        credential_file: None,
        machine: true,
        name: "services".into(),
        project: project.map(str::to_string),
        zone: zone.map(str::to_string),
    }
}

fn headline(report: &Report) -> String {
    serde_json::to_string(&report.headline).unwrap()
}

#[test]
fn a_machine_names_its_account_project_and_zone() {
    let metadata = compute_engine();
    let Ok(Who::Machine {
        email,
        project,
        zone,
    }) = this_machine(&metadata, &options(None, None))
    else {
        panic!("not this machine's account");
    };
    assert_eq!(email, EMAIL);
    assert_eq!(project.as_deref(), Some("my-project"));
    assert_eq!(zone.as_deref(), Some("europe-west2-b"));
}

#[test]
fn flags_name_the_project_and_zone_instead() {
    let metadata = compute_engine();
    let Ok(Who::Machine { project, zone, .. }) =
        this_machine(&metadata, &options(Some("other"), Some("us-east1-b")))
    else {
        panic!("not this machine's account");
    };
    // Connect takes the flags' own; the machine's aren't read.
    assert_eq!((project, zone), (None, None));
    assert_eq!(
        *metadata.asked.borrow(),
        ["instance/service-accounts/default/email"]
    );
}

#[test]
fn a_machine_with_no_metadata_server_is_refused() {
    let metadata = Fake {
        values: None,
        asked: RefCell::default(),
    };
    let Err(report) = this_machine(&metadata, &options(None, None)) else {
        panic!("connected with no metadata server");
    };
    assert!(headline(&report).contains("no Google metadata server"));
    assert_eq!(report.exit, Exit::Usage);
}

#[test]
fn a_machine_with_no_service_account_is_refused() {
    let metadata = Fake::new(&[("project/project-id", "my-project")]);
    let Err(report) = this_machine(&metadata, &options(None, None)) else {
        panic!("connected with no service account");
    };
    assert!(headline(&report).contains("no service account attached"));
}

#[test]
fn settings_name_one_kind_of_credential() {
    let named = |names: &[&str]| credential(|name| names.contains(&name));
    assert_eq!(named(&[CREDENTIALS, OVERRIDE]), Some(Credential::File));
    assert_eq!(named(&["GCE_METADATA_HOST"]), Some(Credential::Machine));
    assert_eq!(named(&[CREDENTIALS, "GCE_METADATA_HOST"]), None);
    assert_eq!(named(&["CLOUDSDK_CORE_PROJECT"]), None);
}

#[test]
fn the_metadata_settings_read() {
    let settings = metadata::settings();
    assert_eq!(settings.host_variable, "GCE_METADATA_HOST");
    assert!(!settings.host.is_empty());
}
