//! What the environment file is checked against: each path, place and
//! value it names, and the whole file at once, each refusal naming the
//! line it's on.

use super::*;

/// The line, from 1, where `text` first has `needle` (a key, a value
/// quoted), for an error: `line 7: `, or nothing.
pub fn at(text: &str, needles: &[&str]) -> String {
    text.lines()
        .position(|line| needles.iter().all(|n| line.contains(n)))
        .map(|i| format!("line {}: ", i + 1))
        .unwrap_or_default()
}

/// `text` as a shell word: as it is when it's plain, else quoted.
pub fn word(text: &str) -> String {
    match !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/:@-+=,".contains(&b))
    {
        true => text.to_string(),
        false => crate::shell_quote(text),
    }
}

/// A path the file names: relative, from its folder, never `..`.
pub fn check_path(text: &str, path: &Path, what: &str) -> anyhow::Result<()> {
    let plain = !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        && !path.to_string_lossy().starts_with('~');
    let shown = path.display().to_string();
    ensure!(
        plain,
        "{}{what} {shown:?} isn't a path from the file's folder: write it relative, without ..",
        at(text, &[&shown])
    );
    Ok(())
}

/// Each part of `path`, from `dir`, that's a link: none may be, so a
/// file names only what's in its folder, never what a link there points
/// at.
fn check_unlinked(text: &str, dir: &Path, path: &Path, what: &str) -> anyhow::Result<()> {
    let mut on_the_way = dir.to_path_buf();
    for part in path.components() {
        on_the_way.push(part);
        let link = std::fs::symlink_metadata(&on_the_way).is_ok_and(|m| m.file_type().is_symlink());
        let shown = path.display().to_string();
        ensure!(
            !link,
            "{}{what} {shown:?} goes through a link ({}): a file names what's in its folder, \
             never what a link points at",
            at(text, &[&shown]),
            on_the_way.display()
        );
    }
    Ok(())
}

/// Where a run is: in the sandbox it names, or `root`'s, outside any; a
/// user of the box by name runs in their own sandbox (`lnk box user add`),
/// and names none.
pub fn check_where(
    line: &str,
    what: &str,
    sandbox: Option<&PathBuf>,
    user: Option<&User>,
) -> anyhow::Result<()> {
    match (sandbox, user) {
        (Some(_), Some(user)) if user.is_root() => bail!(
            "{line}{what} has a sandbox and user = \"root\": as root it runs outside any sandbox"
        ),
        (Some(_), Some(name)) => bail!(
            "{line}{what} has a sandbox and user = \"{name}\": a user of the box runs in their \
             own sandbox (lnk box user add)"
        ),
        (None, Some(_)) | (Some(_), None) => Ok(()),
        (None, None) => {
            bail!("{line}{what} has no sandbox: give it the sandbox spec it runs in, or a user")
        }
    }
}

/// Variables and secrets, as a file names them (`text`, for their
/// lines): each a variable's name, none named like a secret written as a
/// variable, each in one place only. Their names.
pub fn check_values(
    text: &str,
    variables: &BTreeMap<String, String>,
    secrets: &Secrets,
) -> anyhow::Result<BTreeSet<String>> {
    let mut values = BTreeSet::new();
    for (name, value) in variables {
        let line = at(text, &[name]);
        check_variable(name).within(line.clone())?;
        ensure!(
            !link_spec::named_like_a_secret(&name.to_ascii_lowercase()),
            "{line}{name} is named like a secret, and a variable is written in the file for \
             anyone to read: a private value is a secret, named under [secrets] and granted to \
             the file with lnk secret create {name} and lnk env approval add {name} --spec <file>"
        );
        ensure!(!value.contains('\0'), "{line}{name} has a NUL in it");
        values.insert(name.clone());
    }
    for (name, sealed) in secrets
        .names()
        .iter()
        .map(|n| (n, false))
        .chain(secrets.sealed().iter().map(|n| (n, true)))
    {
        let line = at(text, &[&format!("\"{name}\"")]);
        // Never the name itself: one mistyped may be the value pasted.
        check_variable(name).map_err(|_| {
            anyhow::anyhow!(
                "{line}a secret's name in [secrets] (or --secret, --sealed) is not a variable \
                 name: capital letters, digits and _, and none Link sets"
            )
        })?;
        ensure!(
            values.insert(name.clone()),
            "{line}{name} is given twice: once is a variable, a secret or a sealed secret"
        );
        if sealed {
            link_secret_contract::route::check_secret_name(&proxy_name(name))
                .within(line.clone())?;
        }
    }
    Ok(values)
}

pub(crate) fn check_input(file: &Env) -> anyhow::Result<()> {
    check_input_at(file, "")
}

/// [`check_input`] of `file` as written in `text`, each refusal naming
/// its line: what a file's reader runs before the spec's own hook, which
/// sees the file but not its text.
pub(crate) fn check_input_at(file: &Env, text: &str) -> anyhow::Result<()> {
    check_with(file, text, &|path, what| {
        // Text read without a source folder retains absolute host paths.
        // A consuming reader resolves and confines them before this check.
        if path.is_absolute() {
            Ok(())
        } else {
            check_path(text, path, what)
        }
    })
}

pub(super) fn check(read: &Read) -> anyhow::Result<()> {
    let text = &read.text;
    let exists = |path: &PathBuf, what: &str| -> anyhow::Result<()> {
        check_path(text, path, what)?;
        check_unlinked(text, &read.dir, path, what)?;
        let shown = path.display().to_string();
        ensure!(
            read.dir.join(path).is_file(),
            "{}{what} {shown} isn't a file in {}",
            at(text, &[&shown]),
            read.dir.display()
        );
        Ok(())
    };
    check_with(&read.file, text, &exists)
}

fn check_with(
    file: &Env,
    text: &str,
    exists: &dyn Fn(&PathBuf, &str) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    check_name(&file.name).within(at(text, &["name", &file.name]))?;
    ensure!(
        file.version == VERSION,
        "{}version {} isn't one this lnk reads ({VERSION}): lnk upgrade",
        at(text, &["version"]),
        file.version
    );
    if let Some(workdir) = file.workdir() {
        check_path(text, workdir, "workdir").map_err(|_| {
            anyhow::anyhow!(
                "{}workdir {:?} isn't a folder from the home folder: write it relative, without \
                 ..",
                at(text, &["workdir"]),
                workdir.display().to_string()
            )
        })?;
    }
    check_values(text, &file.variables, file.secrets())?;
    let mut names = BTreeSet::new();
    check_groups(file.stages())?;
    for stage in file.stages().iter().flat_map(|s| s.members()) {
        let line = at(text, &["name", &format!("\"{}\"", stage.name)]);
        let what = format!("stage {}", stage.name);
        check_name(&stage.name).within(line.clone())?;
        ensure!(
            names.insert(stage.name.clone()),
            "{line}two stages named {}",
            stage.name
        );
        ensure!(!stage.run.trim().is_empty(), "{line}{what} runs nothing");
        check_where(&line, &what, stage.sandbox.as_ref(), stage.user.as_ref())?;
        if let Some(spec) = &stage.sandbox {
            exists(spec, &format!("{what}'s sandbox"))?;
        }
        ensure!(
            !(stage.root() && stage.when == When::Start),
            "{line}{what} runs as root and at every start: a root stage runs only while an \
             image is built"
        );
        for input in stage.inputs() {
            exists(input, &format!("{what}'s input"))?;
        }
    }
    let mut services = BTreeSet::new();
    let mut ports = BTreeSet::new();
    for service in file.services() {
        let line = at(text, &["name", &format!("\"{}\"", service.name)]);
        let what = format!("service {}", service.name);
        check_name(&service.name).within(line.clone())?;
        ensure!(
            services.insert(service.name.clone()),
            "{line}two services named {}",
            service.name
        );
        ensure!(!service.run.trim().is_empty(), "{line}{what} runs nothing");
        check_where(
            &line,
            &what,
            service.sandbox.as_ref(),
            service.user.as_ref(),
        )?;
        if let Some(spec) = &service.sandbox {
            exists(spec, &format!("{what}'s sandbox"))?;
        }
        let ready = &service.ready;
        crate::stage::check_timeout(&format!("{line}{what}'s ready"), Some(ready.timeout))?;
        match (ready.port, &ready.run) {
            (Some(_), Some(_)) | (None, None) => {
                bail!("{line}{what}'s ready is a port or a run, one of them")
            }
            (Some(port), None) => ensure!(
                port != 0 && ports.insert(port),
                "{line}{what}'s port {port} is another's, or none"
            ),
            (None, Some(run)) => {
                ensure!(!run.trim().is_empty(), "{line}{what}'s ready runs nothing")
            }
        }
        if let Some(reset) = &service.reset {
            ensure!(
                !reset.trim().is_empty(),
                "{line}{what}'s reset runs nothing"
            );
        }
    }
    Ok(())
}
