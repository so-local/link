//! The module protocol and lifecycle.
//!
//! The core discovers programs through typed descriptors and dispatches their
//! declared commands. This crate owns discovery, signed releases, installation,
//! updates and removal. Shared libraries are independent dependencies.

pub mod answers;
pub mod descriptor;
pub mod help;
pub mod modules;
mod named;
pub mod owner;
pub mod release;
pub mod saved;
pub mod source;
pub mod uninstall;
pub mod update;
pub mod upgrades;

pub use modules::ensure;

#[doc(hidden)]
pub use link_spec as __spec;

use link_format as format;
use link_secretive::Secret;
use link_version::Version;
use std::path::PathBuf;

/// How this program was built, read once from `LNK_BUILD` as it's
/// compiled, which `lnk bundle create --build` sets: its stage. Unset, a
/// build from source, which is never released and trusts Link's signer;
/// `dev`, a development build, which trusts the signer a contributor
/// names in `LNK_DEV_SIGNER` beside Link's; `staging`, a staging build,
/// which trusts the staging signer beside Link's; `prod`, a production
/// build, which trusts exactly the signer it was compiled with
/// (`LNK_PROD_SIGNER`): Link's public release is the prod build compiled
/// with Link's signer (`release::TRUSTED_SIGNERS`). Any other value
/// doesn't compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    Source,
    Dev,
    Staging,
    Prod,
}

impl Build {
    pub(crate) const fn from(value: Option<&str>) -> Build {
        const fn is(value: &str, word: &str) -> bool {
            let (value, word) = (value.as_bytes(), word.as_bytes());
            if value.len() != word.len() {
                return false;
            }
            let mut i = 0;
            while i < value.len() {
                if value[i] != word[i] {
                    return false;
                }
                i += 1;
            }
            true
        }
        match value {
            None => Build::Source,
            Some(value) if is(value, "dev") => Build::Dev,
            Some(value) if is(value, "staging") => Build::Staging,
            Some(value) if is(value, "prod") => Build::Prod,
            Some(_) => {
                panic!("LNK_BUILD is dev, staging or prod, or unset for a build from source")
            }
        }
    }

    /// What `lnk --version` says of it on its second line: what each
    /// stage trusts. Nothing for a build from source, which is never
    /// released.
    pub const fn trusts(self) -> Option<&'static str> {
        match self {
            Build::Staging => {
                Some("a staging build: it trusts the staging signer as well as Link's")
            }
            Build::Dev => {
                Some("a dev build: it trusts the signer it was built with as well as Link's")
            }
            Build::Prod => Some("a prod build: it trusts only the signer it was built with"),
            Build::Source => None,
        }
    }
}

/// A page of Link's documentation, docs.local.link, by its path, for a
/// message that points a person there: `docs!("agents/tools/web")`. The
/// one place Link's code names a website; nothing needs it to run.
#[macro_export]
macro_rules! docs {
    ($path:literal) => {
        concat!("https://docs.local.link/", $path)
    };
}

/// This program's [`Build`].
pub const BUILD: Build = Build::from(option_env!("LNK_BUILD"));

/// Whether this program was built for release, of any stage: one built
/// from source never fetches a release's programs of
/// the same version, which may be another's code.
pub const RELEASED: bool = !matches!(BUILD, Build::Source);

/// The checkout a program built from source was built from,
/// recorded as it's compiled: where `lnk module add` installs modules from
/// on such a build, as `lnk up --source` does. A release build has none.
pub const CHECKOUT: Option<Checkout> = match BUILD {
    Build::Source => Some(Checkout(env!("CARGO_MANIFEST_DIR"))),
    Build::Dev | Build::Staging | Build::Prod => None,
};

/// A checkout's place: this crate's folder in it, as Cargo gave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkout(&'static str);

impl Checkout {
    /// The checkout's root, its workspace: this crate is `core/framework`
    /// in it.
    pub fn root(self) -> PathBuf {
        std::path::Path::new(self.0)
            .ancestors()
            .nth(2)
            .unwrap_or(std::path::Path::new(self.0))
            .to_path_buf()
    }
}

/// `lnk-<module>` next to this program, else on the PATH, when it's
/// built from this program's sources ([`source::check`]): one that isn't
/// is said aside, once, and isn't there. [`locate`] is the same, the
/// mismatch its error.
pub fn find_module(module: &str) -> Option<PathBuf> {
    locate(module).unwrap_or_else(|skew| {
        source::say(&skew);
        None
    })
}

/// `lnk-<module>` next to this program, else on the PATH, refused when
/// it was built from other sources than this program ([`source::Skew`]).
pub fn locate(module: &str) -> Result<Option<PathBuf>, source::Skew> {
    match found(module) {
        Some(path) => source::check(module, &path).map(|()| Some(path)),
        None => Ok(None),
    }
}

/// Whether `lnk-<module>` is next to this program or on the PATH. Its
/// sources aren't asked, so nothing runs: listing every module starts
/// none of them.
pub fn module_present(module: &str) -> bool {
    found(module).is_some()
}

/// `lnk-<module>` in this program's own folder, where `lnk module add`
/// installs, when it's built from this program's sources: what is
/// installed. One of the same name only elsewhere on the PATH, such as a
/// build from source in a checkout's `target/debug`, isn't, so `lnk module
/// add` installs this program's own beside it.
pub fn installed_module(module: &str) -> Option<PathBuf> {
    let path = beside(module).filter(|p| p.is_file())?;
    match source::check(module, &path) {
        Ok(()) => Some(path),
        Err(skew) => {
            source::say(&skew);
            None
        }
    }
}

/// `lnk-<module>` next to this program, else on the PATH, as it is.
fn found(module: &str) -> Option<PathBuf> {
    let name = program_file(module);
    beside(module)
        .into_iter()
        .chain(on_path(
            &name,
            &std::env::var_os("PATH").unwrap_or_default(),
        ))
        .find(|p| p.is_file())
}

/// Where `lnk-<module>` goes in this program's own folder, there or not.
fn beside(module: &str) -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.canonicalize().unwrap_or(exe))
        .and_then(|exe| exe.parent().map(|dir| dir.join(program_file(module))))
}

/// `lnk-<module>`'s file name on this system.
fn program_file(module: &str) -> String {
    format!("lnk-{module}{}", std::env::consts::EXE_SUFFIX)
}

/// The environment variable `lnk` sets to its own path when it runs a
/// module, so the module can run other `lnk` commands with the same core.
pub const LNK_EXE: &str = "LNK_EXE";

/// A command that runs `lnk`: the one that started this module, else the
/// `lnk` next to this binary, else `lnk` in an absolute folder on the PATH
/// (never the current folder, where anyone could have put one).
pub fn lnk() -> std::process::Command {
    lnk_with([])
}

/// [`lnk`], with `more` secrets handed to it on its pipe, each by the
/// variable it's read from ([`link_secretive::process::ours_with`]).
pub fn lnk_with<'a>(
    more: impl IntoIterator<Item = (&'a str, &'a Secret)>,
) -> std::process::Command {
    ours(lnk_program(), more)
}

/// A command that runs module `module`'s program directly, as `lnk` would
/// run it: `lnk-<module>` beside the `lnk` that [`lnk`] runs, else on the
/// PATH, where `lnk` finds one too ([`find_module`]), told that `lnk`
/// ([`LNK_EXE`]), so what it runs of `lnk` is the same core. How a module
/// asks another a question ([`answers`]).
pub fn module_program(module: &str) -> std::process::Command {
    let lnk = lnk_program();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let program = module_beside(&lnk, module, &path);
    let mut command = ours(program, []);
    command.env(LNK_EXE, lnk);
    command
}

/// `lnk-<module>` beside `lnk`, else in an absolute folder on `path` (a
/// PATH), else where it would be beside `lnk`, so running it fails there.
fn module_beside(lnk: &std::path::Path, module: &str, path: &std::ffi::OsStr) -> PathBuf {
    let name = program_file(module);
    let beside = lnk
        .parent()
        .map_or_else(|| PathBuf::from(&name), |dir| dir.join(&name));
    std::iter::once(beside.clone())
        .chain(on_path(&name, path))
        .find(|path| path.is_file())
        .unwrap_or(beside)
}

/// `name` in each absolute folder on `path` (a PATH), in order.
fn on_path(name: &str, path: &std::ffi::OsStr) -> impl Iterator<Item = PathBuf> {
    let name = name.to_string();
    link_files::path_dirs(path)
        .into_iter()
        .map(move |dir| dir.join(&name))
}

/// The `lnk` [`lnk`] runs: the one that started this module, else the one
/// next to this binary, else one in an absolute folder on the PATH.
fn lnk_program() -> PathBuf {
    let name = format!("lnk{}", std::env::consts::EXE_SUFFIX);
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|path| path.is_file());
    std::env::var_os(LNK_EXE)
        .map(PathBuf::from)
        .or(sibling)
        .or_else(|| {
            on_path(&name, &std::env::var_os("PATH").unwrap_or_default())
                .find(|path| path.is_file())
        })
        // Not found: fails to start rather than searching the PATH again.
        .unwrap_or_else(|| PathBuf::from("/nonexistent/lnk"))
}

/// `program`, one of Link's own, with `more` secrets on its pipe.
fn ours<'a>(
    program: PathBuf,
    more: impl IntoIterator<Item = (&'a str, &'a Secret)>,
) -> std::process::Command {
    let mut command = link_secretive::process::ours_with(program, more);
    // Run for this command's own: its Next and report's footer are ours,
    // and its trace, if it fails, joins ours from the folder it's given.
    let inner = link_settings::output::records().map_or_else(
        || "1".into(),
        |records| link_format::inner::folder(&records).into_os_string(),
    );
    command.env(link_format::INNER, inner);
    command
}

/// What a child program, here or over SSH, said to seal its secrets to on
/// `stdout` ([`link_secretive::Recipient::announce`]), and what's left of
/// `stdout` to read. Each line before it (a login's banner) is printed
/// here, without control characters. An error when it says nothing in
/// two minutes, or ends first (an older `lnk` there, or none): never a
/// guess, and never a secret sent unsealed.
pub fn announced(
    stdout: std::process::ChildStdout,
) -> anyhow::Result<(
    link_secretive::SealTo,
    std::io::BufReader<std::process::ChildStdout>,
)> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        // A login's banner, another program's lines: about the run.
        let got = link_secretive::SealTo::read_announced(&mut reader, |line| {
            format::aside(format::Tail::last(line, 1));
        });
        let _ = tx.send(got.map(|to| (to, reader)));
    });
    match rx.recv_timeout(std::time::Duration::from_secs(120)) {
        Ok(got) => anyhow::Context::context(got, "it didn't say what to seal its secrets to"),
        Err(_) => anyhow::bail!("it didn't say what to seal its secrets to within two minutes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_skips_empty_and_relative_entries() {
        let path = std::env::join_paths(["/usr/bin", "", ".", "bin", "/opt/lnk"]).unwrap();
        assert_eq!(
            link_files::path_dirs(&path),
            vec![PathBuf::from("/usr/bin"), PathBuf::from("/opt/lnk")]
        );
        assert!(link_files::path_dirs(std::ffi::OsStr::new("::")).is_empty());
    }

    /// A module asks another beside its `lnk`, else on the PATH, as `lnk`
    /// finds modules: a build whose modules are in two folders on the
    /// PATH runs them all.
    #[test]
    fn a_module_is_beside_lnk_else_on_the_path() {
        let root = link_test_kit::TempDir::new("module-beside");
        let (core, other) = (root.path().join("core"), root.path().join("other"));
        for dir in [&core, &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let lnk = core.join(format!("lnk{}", std::env::consts::EXE_SUFFIX));
        let file = |dir: &std::path::Path, module: &str| {
            let path = dir.join(program_file(module));
            std::fs::write(&path, "").unwrap();
            path
        };
        let path = std::env::join_paths([&other, &core]).unwrap();
        let here = file(&other, "here");
        assert_eq!(module_beside(&lnk, "here", &path), here);
        let near = file(&core, "here");
        assert_eq!(module_beside(&lnk, "here", &path), near, "beside lnk first");
        let none = core.join(program_file("none"));
        assert_eq!(module_beside(&lnk, "none", &path), none);
    }

    /// A source build knows its checkout, the workspace holding this crate;
    /// a release build has none.
    #[test]
    fn a_source_build_records_the_checkout_it_was_built_from() {
        assert_eq!(CHECKOUT.is_some(), !RELEASED);
        let root = Checkout(env!("CARGO_MANIFEST_DIR")).root();
        assert!(root.join("core/framework/Cargo.toml").is_file(), "{root:?}");
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("[workspace]"), "{root:?}");
    }
}

#[cfg(test)]
link_test_kit::program!(system);
