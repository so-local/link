//! What `lnk module list --json` prints: Link's modules, and those added
//! from outside Link (`lnk module add --url <url>`), each with where it comes
//! from. A module reads it to know the outside ones: the harness module
//! lists their harnesses, and adds one on a box from the same place.
//! And how a module gets another it needs ([`ensure`], [`add`]).

use serde::{Deserialize, Serialize};

pub use link_component::Kind;

/// One module, as `lnk module list --json` lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    /// Its name: the module is `lnk-<name>`.
    pub name: String,
    /// Whether its program is next to `lnk` or on the PATH.
    pub installed: bool,
    /// Its declared component kind, when this build supplies it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// What the component does, from its declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Where it comes from: none for Link's own (Link's releases, signed
    /// by a key built into `lnk`), else the repository it was added from
    /// (`https://github.com/<owner>/<repo>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// For one from outside Link, the key its releases must be signed
    /// with, as `lnk` pinned it on the first `add` (`ssh-ed25519 <base64>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    /// The questions it answers (`storage.folders`), as its manifest
    /// says: each through its program's `--link-answer <question>`
    /// ([`crate::answers`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<String>,
}

/// The modules added from outside Link, each a harness adapter (the one
/// kind `lnk module add --url <url>` adds). Empty when `lnk` can't say.
pub fn outside() -> Vec<Listed> {
    list().into_iter().filter(|p| p.source.is_some()).collect()
}

/// The variable `lnk` hands the module it runs its listing in
/// ([`Handed`]), so the module's questions find their answerers without
/// starting another `lnk`. Only the process it was handed to reads it:
/// one it starts inherits it, and asks `lnk` for itself.
pub const LNK_MODULES: &str = "LNK_MODULES";

/// What [`LNK_MODULES`] holds: `lnk module list --json` as it was when
/// `lnk` ran the module, for the process `to`, the module's own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handed {
    /// The process it's for: `lnk`'s own, which becomes the module's.
    pub to: u32,
    pub modules: Vec<Listed>,
}

/// This process's listing, once known: every question it asks shares it.
static LISTED: std::sync::Mutex<Listing> = std::sync::Mutex::new(Listing {
    known: None,
    handed: true,
});

struct Listing {
    known: Option<Vec<Listed>>,
    /// Whether [`LNK_MODULES`] may still be read: not once a module was
    /// installed after it was handed.
    handed: bool,
}

/// Every module, as `lnk module list --json` lists them, found once a
/// process: as `lnk` handed it ([`LNK_MODULES`]), else asked. Empty when
/// `lnk` can't say, and asked again next time.
pub fn list() -> Vec<Listed> {
    let mut listing = LISTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(known) = &listing.known {
        return known.clone();
    }
    let handed = listing
        .handed
        .then(|| {
            handed(
                std::env::var(LNK_MODULES).ok().as_deref(),
                std::process::id(),
            )
        })
        .flatten();
    listing.known = handed.or_else(asked);
    listing.known.clone().unwrap_or_default()
}

/// The listing `variable` ([`LNK_MODULES`]) hands process `this`: none
/// when it's for another, or isn't one.
fn handed(variable: Option<&str>, this: u32) -> Option<Vec<Listed>> {
    let handed: Handed = serde_json::from_str(variable?).ok()?;
    (handed.to == this).then_some(handed.modules)
}

/// What `lnk module list --json` says: none when it can't say.
fn asked() -> Option<Vec<Listed>> {
    crate::lnk()
        .args(["module", "list", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Vec<Listed>>(&o.stdout).ok())
}

/// Forgets the listing once a module is installed, so the next question
/// asks `lnk` again.
fn forget() {
    let mut listing = LISTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    listing.known = None;
    listing.handed = false;
}

/// Cached identity returned by the account provider: no credential.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountLogin {
    pub username: Option<String>,
    pub owner: Option<String>,
    pub accounts: Option<String>,
    pub relay: Option<String>,
}

impl AccountLogin {
    /// The account service only while its provider reports a login.
    pub fn account_service(&self) -> Option<&str> {
        match (&self.username, &self.owner, &self.accounts) {
            (Some(username), Some(owner), Some(accounts))
                if !username.is_empty() && !owner.is_empty() =>
            {
                Some(accounts)
            }
            _ => None,
        }
    }
}

/// Set up components this core's trusted registry associates with its
/// cached sign-in. The core chooses them; no service chooses a program.
pub fn setup_account() -> anyhow::Result<()> {
    use link_format::WithinExt;
    let mut setup = crate::lnk();
    setup
        .args(["module", "setup"])
        .stdin(std::process::Stdio::null());
    #[cfg(unix)]
    {
        if let Ok(stderr) = link_format::streams::child_stderr() {
            setup.stdout(stderr);
        }
    }
    let status = setup.status().within("running account setup")?;
    anyhow::ensure!(
        status.success(),
        "account setup failed (the sign-in is saved)"
    );
    Ok(())
}

/// Module `module`'s program, installed first if it's missing, for
/// `why`, the feature that needs it (`` `lnk example copy` ``): asked in
/// a terminal, said without one, and installed from Link's release with
/// what it needs (`lnk module add`). What every module calls in place of
/// "you don't have this module": a refusal is the only stop.
pub fn ensure(module: &str, why: &str) -> anyhow::Result<std::path::PathBuf> {
    get(module, why, Asked::IfTerminal)
}

/// [`ensure`] for a command that names the module itself (an explicitly selected module): installed unasked, as
/// typing it was the yes, and said.
pub fn add(module: &str, why: &str) -> anyhow::Result<std::path::PathBuf> {
    get(module, why, Asked::Never)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Asked {
    IfTerminal,
    Never,
}

fn get(module: &str, why: &str, asked: Asked) -> anyhow::Result<std::path::PathBuf> {
    use std::io::IsTerminal;
    let terminal = std::io::stdin().is_terminal() && link_format::streams::stderr_is_terminal();
    let ask = |question: &str| -> anyhow::Result<bool> {
        let asked = link_format::Question::confirm(link_format::Sentence::adopt(question), true);
        link_format::ask_aside(asked).map_err(|r| anyhow::anyhow!("{}", r.headline))
    };
    let say =
        |line: &str| link_format::aside(link_format::Note(link_format::Sentence::adopt(line)));
    getting(
        module,
        why,
        &|| offered(module).then(|| wanted(module)),
        (asked == Asked::IfTerminal && terminal).then_some(&ask),
        &say,
        &|| install(module),
        &|| crate::find_module(module),
    )
}

/// What `lnk module add <module>` would install, the module with what it
/// needs (`--dry`); the module alone when `lnk` can't say, which
/// only words what [`getting`] says.
fn wanted(module: &str) -> Vec<String> {
    crate::lnk()
        .args(["module", "add", "--dry", module])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(link_format::clean)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|all| !all.is_empty())
        .unwrap_or_else(|| vec![module.to_string()])
}

/// Whether this `lnk`'s release has `module` to install: a release
/// leaves out a module it holds back (`released = false`), and its `lnk`
/// lists only what it has. True when `lnk` can't say, so that install
/// says what's wrong.
pub fn offered(module: &str) -> bool {
    let listed = list();
    listed.is_empty() || listed.iter().any(|p| p.name == module)
}

/// "it" for `module` alone, else the modules named: "helper, provider and
/// example".
fn them(module: &str, wanted: &[String]) -> String {
    match wanted {
        [] => "it".to_string(),
        [one] if one == module => "it".to_string(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// How [`getting`] asks a question: whether the answer was yes.
type Ask<'a> = &'a dyn Fn(&str) -> anyhow::Result<bool>;

/// What `lnk` exits with when `lnk <group>`'s module isn't installed and
/// it didn't add it ([`link_format::NotInstalled`]): how a program running `lnk`,
/// through another module too, tells that from any other failure.
pub const NOT_INSTALLED: u8 = 4;

/// Whether a program's exit `status` is [`NOT_INSTALLED`]'s.
pub fn not_installed(status: &std::process::ExitStatus) -> bool {
    status.code() == Some(i32::from(NOT_INSTALLED))
}

/// [`get`], with what it installs (none: this release doesn't have it),
/// how it asks (none: it doesn't), says, installs and finds given.
fn getting(
    module: &str,
    why: &str,
    wanted: &dyn Fn() -> Option<Vec<String>>,
    ask: Option<Ask>,
    say: &dyn Fn(&str),
    install: &dyn Fn() -> anyhow::Result<()>,
    find: &dyn Fn() -> Option<std::path::PathBuf>,
) -> anyhow::Result<std::path::PathBuf> {
    if let Some(program) = find() {
        return Ok(program);
    }
    let Some(wanted) = wanted() else {
        anyhow::bail!(
            "{why} needs the {module} module (lnk-{module}), which this release of Link doesn't \
             have"
        );
    };
    let them = them(module, &wanted);
    match ask {
        Some(ask) => {
            let question = format!(
                "{why} needs the {module} module (lnk-{module}), which isn't installed. \
                 Install {them}?"
            );
            anyhow::ensure!(
                ask(&question)?,
                "not installed: {why} needs the {module} module"
            );
        }
        None => say(&format!(
            "{why} needs the {module} module (lnk-{module}): installing {them} from Link's \
             release."
        )),
    }
    install()?;
    find().ok_or_else(|| {
        anyhow::anyhow!("the {module} module installed, but lnk-{module} isn't beside lnk")
    })
}

/// `lnk module add <module>`, what it prints on stderr: stdout is often
/// the answer the command asking prints (`--json`).
fn install(module: &str) -> anyhow::Result<()> {
    use link_format::WithinExt;
    let mut add = crate::lnk();
    add.args(["module", "add", module])
        .stdin(std::process::Stdio::null());
    #[cfg(unix)]
    {
        if let Ok(stderr) = link_format::streams::child_stderr() {
            add.stdout(stderr);
        }
    }
    let status = add.status().within("running lnk module add");
    forget();
    let status = status?;
    anyhow::ensure!(
        status.success(),
        "couldn't install the {module} module (lnk module add {module}, above)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;

    struct Fake {
        installed: Cell<bool>,
        offered: bool,
        said: RefCell<Vec<String>>,
        asked: RefCell<Vec<String>>,
        wanted: Vec<String>,
    }

    impl Fake {
        fn new(installed: bool) -> Self {
            Fake {
                installed: Cell::new(installed),
                offered: true,
                said: RefCell::new(Vec::new()),
                asked: RefCell::new(Vec::new()),
                wanted: vec!["example".to_string()],
            }
        }

        fn run(&self, answer: Option<bool>) -> anyhow::Result<PathBuf> {
            let ask = |q: &str| {
                self.asked.borrow_mut().push(q.to_string());
                Ok(answer.unwrap_or(false))
            };
            getting(
                "example",
                "`lnk example copy`",
                &|| self.offered.then(|| self.wanted.clone()),
                answer
                    .is_some()
                    .then_some(&ask as &dyn Fn(&str) -> anyhow::Result<bool>),
                &|line| self.said.borrow_mut().push(line.to_string()),
                &|| {
                    self.installed.set(true);
                    Ok(())
                },
                &|| {
                    self.installed
                        .get()
                        .then(|| PathBuf::from("/bin/lnk-example"))
                },
            )
        }
    }

    #[test]
    fn a_listing_is_read_only_by_the_process_it_was_handed_to() {
        let listed = vec![Listed {
            name: "example".into(),
            installed: true,
            answers: vec!["storage.folders".into()],
            ..Default::default()
        }];
        let variable = serde_json::to_string(&Handed {
            to: 7,
            modules: listed.clone(),
        })
        .unwrap();
        assert_eq!(handed(Some(&variable), 7), Some(listed));
        assert_eq!(handed(Some(&variable), 8), None);
        assert_eq!(handed(Some("{\"to\": 7}"), 7), None);
        assert_eq!(handed(None, 7), None);
    }

    #[test]
    fn an_installed_module_is_found_unasked_and_unsaid() {
        let fake = Fake::new(true);
        assert_eq!(
            fake.run(Some(false)).unwrap(),
            PathBuf::from("/bin/lnk-example")
        );
        assert!(fake.asked.borrow().is_empty() && fake.said.borrow().is_empty());
    }

    #[test]
    fn a_missing_module_is_installed_and_said_without_a_terminal() {
        let fake = Fake::new(false);
        assert_eq!(fake.run(None).unwrap(), PathBuf::from("/bin/lnk-example"));
        assert_eq!(
            fake.said.borrow()[..],
            [
                "`lnk example copy` needs the example module (lnk-example): installing it from \
              Link's release."
            ]
        );
    }

    #[test]
    fn a_module_this_release_holds_back_is_refused_plainly_unasked() {
        let mut fake = Fake::new(false);
        fake.offered = false;
        let error = fake.run(Some(true)).unwrap_err().to_string();
        assert_eq!(
            error,
            "`lnk example copy` needs the example module (lnk-example), which this release of \
             Link doesn't have"
        );
        assert!(fake.asked.borrow().is_empty() && fake.said.borrow().is_empty());
        assert!(!fake.installed.get());
    }

    #[test]
    fn a_missing_module_is_asked_for_in_a_terminal() {
        let fake = Fake::new(false);
        fake.run(Some(true)).unwrap();
        assert!(fake.installed.get());
        assert!(
            fake.asked.borrow()[0].contains("`lnk example copy` needs the example module"),
            "{:?}",
            fake.asked.borrow()
        );
        let refused = Fake::new(false);
        let err = refused.run(Some(false)).unwrap_err().to_string();
        assert!(!refused.installed.get());
        assert_eq!(
            err,
            "not installed: `lnk example copy` needs the example module"
        );
        assert!(!err.contains("lnk module add"), "{err}");
    }

    #[test]
    fn the_question_names_everything_it_installs() {
        let mut fake = Fake::new(false);
        fake.wanted = ["helper", "provider", "example"].map(String::from).to_vec();
        fake.run(Some(true)).unwrap();
        assert!(
            fake.asked.borrow()[0].ends_with("Install helper, provider and example?"),
            "{:?}",
            fake.asked.borrow()
        );
        let mut fake = Fake::new(false);
        fake.wanted = ["helper", "example"].map(String::from).to_vec();
        fake.run(None).unwrap();
        assert!(
            fake.said.borrow()[0].ends_with("installing helper and example from Link's release."),
            "{:?}",
            fake.said.borrow()
        );
    }
}
