//! What a command opens a sandbox from: its spec and flags, made one confinement, and what it asks first.

use std::path::{Path, PathBuf};

use anyhow::ensure;
use link_format::{
    Foreign, Note, Out, Path as Shown, Question, Refuser, Report, Table, WithinExt, cmd, prose,
    sentence,
};
use link_sandbox::opening::Opening;
use link_sandbox::spec::{self, Spec};
use link_sandbox_client::Wrap;
use link_sandbox_client::{At, Confinement, Programs, ProxyAt, Way};
use link_secretive::Duplicate;

/// A sandbox as its spec and flags make it.
pub(crate) struct Opened {
    /// What it runs in: its spec's, with what every sandbox hides and its
    /// programs found.
    pub(crate) sandbox: spec::Resolved,
    /// The spec's file, when it's from one.
    pub(crate) read: Option<link_spec::Read<Spec>>,
    /// What it opens, as its spec says: without what every sandbox hides.
    pub(crate) confinement: spec::Resolved,
}

/// What `lnk sandbox`'s flags open: the spec and confinement they make.
pub(crate) trait Makes {
    fn flags_spec(&self) -> anyhow::Result<Spec>;
    fn spec(&self) -> anyhow::Result<(Spec, Option<link_spec::Read<Spec>>)>;
    fn opened(&self, name: &str) -> anyhow::Result<Opened>;
}

/// Flag path `p` as a spec holds it: absolute (from `here`, but `~`)
/// with no `..`. Refused from `/`, where a tool's server runs: a tool
/// written for its own folder (`--write .`) would mean the whole machine.
pub(crate) fn flag_path(p: &Path, here: &Path) -> anyhow::Result<String> {
    let text = p.to_string_lossy();
    let climbs = p.components().any(|c| c == std::path::Component::ParentDir);
    if (text == "~" || text.starts_with("~/")) && !climbs {
        return Ok(text.into_owned());
    }
    let abs = match text.strip_prefix("~/") {
        Some(rest) => link_files::try_home_dir()?.join(rest),
        None => {
            ensure!(
                p.is_absolute() || here != Path::new("/"),
                "{}: a path from /, where a tool's server runs: give it in full",
                p.display()
            );
            here.join(p)
        }
    };
    Ok(link_sandbox_confine::lexical(&abs).display().to_string())
}

impl Makes for Opening {
    /// The spec the field flags write, on this OS: their paths absolute
    /// (from this folder, but `~`) with no `..`, which a spec's file
    /// can't have. Flags aren't checked as a spec's file is: a path from
    /// `/` is refused ([`flag_path`]), and `/` itself, as any folder
    /// holding Link's settings, by the wrap.
    fn flags_spec(&self) -> anyhow::Result<Spec> {
        let here = std::env::current_dir().within("finding this folder")?;
        let mut spec = link_spec::over(&Spec::default(), &self.fields)?.on(spec::Os::here())?;
        if let Some(files) = &mut spec.files {
            for list in [
                &mut files.write,
                &mut files.read,
                &mut files.hide,
                &mut files.write_dot_files,
            ]
            .into_iter()
            .flatten()
            {
                *list = list
                    .iter()
                    .map(|p| flag_path(Path::new(p), &here))
                    .collect::<anyhow::Result<_>>()?;
            }
        }
        Ok(spec)
    }

    /// The spec these make on this OS: the file's (if any) with the
    /// flags over it, and the file read.
    fn spec(&self) -> anyhow::Result<(Spec, Option<link_spec::Read<Spec>>)> {
        let os = spec::Os::here();
        let read = self
            .spec
            .as_deref()
            .map(link_spec::read_spec::<Spec>)
            .transpose()?;
        let file = match &read {
            Some(read) => read.spec.on(os)?,
            None => Spec::default(),
        };
        let flags = self.flags_spec()?;
        Ok((file.with(&flags), read))
    }

    /// The sandbox these make, named `name`, every refusal checked.
    fn opened(&self, name: &str) -> anyhow::Result<Opened> {
        let (spec, read) = self.spec()?;
        let home = link_files::try_home_dir()?;
        let dir = match &read {
            Some(read) => read.dir.clone(),
            None => std::env::current_dir()?,
        };
        // A file may be anyone's: what it opens is checked whole. Flags
        // alone are this user's, or Link's own (an agent's memory, in its
        // folder); Link's settings stay closed to both.
        let path = link_files::absolute_path(&std::env::var_os("PATH").unwrap_or_default());
        let confinement = spec::Resolved::new(Confinement::of(
            &spec,
            At {
                name,
                dir: &dir,
                home: &home,
                path: Some(&path),
                way: Way::Proxy(ProxyAt {
                    upstream: self.upstream.clone(),
                    dir: link_sandbox::run_proxy_dir()?,
                    ..Default::default()
                }),
            },
        )?)?;
        if let Some(read) = &read {
            spec::check(
                &confinement,
                &spec::Machine {
                    home: &home,
                    settings: &link_files::try_installation_dir()?,
                    hidden: &link_sandbox_contract::hidden_dirs(),
                    spec: Some(&read.path),
                    run_path: &path,
                },
            )?;
        }
        spec::network_here(&confinement, spec::on_cloud())?;
        let mut sandbox = confinement.duplicate();
        sandbox.set_programs(spec::programs(
            &confinement,
            &link_sandbox_confine::programs::temp_folders(),
        )?);
        sandbox.hide_more(link_sandbox_contract::hidden_dirs());
        Ok(Opened {
            sandbox,
            read,
            confinement,
        })
    }
}

/// A confinement as a caller sent it: none with no wall, which has
/// nothing to confine or check.
pub(crate) fn door(sandbox: Wrap) -> anyhow::Result<Confinement> {
    match sandbox {
        Wrap::Confinement(confinement) => Ok(confinement),
        Wrap::Unwalled(_) => anyhow::bail!("no wall: nothing confines its program"),
    }
}

/// What a run leaves behind: its rules, its proxy's sockets, and its
/// wire's pause, a name no sandbox has again.
pub(crate) fn clean_up(policy: &Confinement) {
    link_sandbox_client::forget(&policy.name);
    if let Ok(dir) = link_sandbox_proxy::wire::dir() {
        let _ = link_sandbox_proxy::wire::resume(&dir, &policy.name);
    }
    if let Some(proxy) = link_sandbox::proxy_of(policy) {
        let _ = std::fs::remove_dir_all(&proxy.dir);
        let _ = std::fs::remove_dir_all(link_sandbox::etc_dir(&proxy.dir));
    }
}

/// The environment `lnk sandbox run` keeps: nothing that could be a
/// secret (tokens, `SSH_AUTH_SOCK`). `SSL_CERT_FILE` among it, whose file
/// the sandbox lets it read, so a machine behind a proxy that re-signs
/// TLS reaches the internet from inside too.
pub(crate) const KEPT: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "TERM",
    "COLORTERM",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "USER",
    "LOGNAME",
    "SHELL",
    "SSL_CERT_FILE",
];

/// What `--with-fd` gives a run, read from that descriptor to its end,
/// then closed: a pipe its caller wrote.
pub(crate) fn read_with(fd: i32) -> anyhow::Result<link_sandbox_client::RunWith> {
    link_sandbox_client::RunWith::read_fd(fd)
}

/// A report if `command` starts with a flag, one `run` or `learn` doesn't
/// take (`--allow`, say), which would be run as the program.
pub(crate) fn flagless(command: &[String]) -> Result<(), Report> {
    match command.first() {
        Some(first) if first.starts_with('-') => Err(Report::error(sentence!(
            "{} isn't a flag of this command.",
            Foreign::new(first)
        ))
        .why(prose!("The command to run goes after --."))
        .exit(link_format::Exit::Usage)
        .next(cmd!("lnk sandbox run --help"), "see the flags it takes")),
        _ => Ok(()),
    }
}

/// A report unless `command`'s program is one of `programs`: a run is
/// refused before it starts, not when its first program is.
pub(crate) fn listed(programs: &[PathBuf], command: &[String]) -> Result<(), Report> {
    let Some(first) = command.first() else {
        return Err(Report::error(sentence!("There's nothing to run."))
            .why(prose!("The command to run goes after --."))
            .exit(link_format::Exit::Usage));
    };
    let found = match first.contains('/') {
        true => std::path::absolute(first).ok(),
        false => link_files::find_program(
            first,
            &link_files::absolute_path(&std::env::var_os("PATH").unwrap_or_default()),
        ),
    };
    let found = found.map(|p| link_sandbox_confine::resolve(&p));
    match found {
        Some(program) if link_sandbox_confine::programs::listed(programs, &program) => Ok(()),
        Some(program) => Err(Report::refusal(
            Refuser::Sandbox,
            sentence!(
                "{} ({}) isn't one of the programs it may run.",
                Foreign::new(first),
                Shown::new(program)
            ),
        )
        .why(prose!("Its [commands] allow doesn't list it."))
        .next(
            cmd!("lnk sandbox learn --spec <file> -- {}", Foreign::new(first)),
            "see which programs it runs, to list them",
        )),
        None => Err(
            Report::error(sentence!("{} is not found.", Foreign::new(first)))
                .exit(link_format::Exit::SandboxNotFound),
        ),
    }
}

/// Shows what a spec not run before (or opening something else since:
/// its text, or a link under a path it names, changed) opens, and asks;
/// remembers it once allowed. `yes` answers for you.
pub(crate) fn asked(
    out: &mut Out,
    read: &link_spec::Read<Spec>,
    opened: &Opened,
    yes_given: bool,
) -> Result<(), Report> {
    let programs = match &opened.sandbox.commands {
        Programs::Only(programs) => Some(programs.as_slice()),
        Programs::Any => None,
    };
    let opens = opened.confinement.opens(programs);
    let mut seen = link_spec::Seen::load("sandboxes")?;
    if seen.knows(&read.path, &opens) {
        return Ok(());
    }
    if !yes_given {
        out.block(Note(sentence!(
            "{} hasn't run here before, or opens something else now.",
            Shown::new(&read.path)
        )));
        let mut table = Table::new(["IT OPENS"], sentence!("It opens nothing."));
        for line in opened.confinement.describe() {
            table.row([Foreign::new(&line)]);
        }
        out.now(table);
    }
    let allowed = out.ask(Question::confirm(sentence!("Run it?"), true).yes(yes_given))?;
    if !allowed {
        return Err(Report::refusal(Refuser::Person, sentence!("Not run.")));
    }
    Ok(seen.remember(&read.path, &opens)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run and inspection use this loader: neither may grant programs
    /// merely because the same fields came from flags instead of a file.
    #[test]
    fn files_and_flags_have_the_same_program_grants() {
        use link_sandbox::cli::{self, Command};

        link_core::descriptor::uses(&[link_spec::Exported::of::<Spec>]);
        let dir = link_test_kit::TempDir::new("sandbox-default-grants");
        let file = dir.join("sandbox.toml");
        let load = |file: Option<&Path>, flags: &[&str]| {
            let mut args = vec!["lnk-sandbox".into(), "run".into()];
            if let Some(file) = file {
                args.extend(["--spec".into(), file.as_os_str().to_owned()]);
            }
            args.extend(flags.iter().map(std::ffi::OsString::from));
            args.extend(["--".into(), "sh".into()]);
            let parsed = cli::parse_from(args).unwrap();
            let Command::SandboxRun(run) = parsed.command.command else {
                panic!("a sandbox run");
            };
            Opening::of(run.spec, parsed.fields, run.upstream, run.yes)
                .spec()
                .unwrap()
                .0
        };

        std::fs::write(&file, "schema = 1\nkind = 'sandbox'\n").unwrap();
        let closed = load(None, &[]);
        assert_eq!(closed, load(Some(&file), &[]));
        assert!(closed.commands.is_none());

        std::fs::write(
            &file,
            "schema = 1\nkind = 'sandbox'\n[commands]\nallow = ['sh']\n",
        )
        .unwrap();
        let allowed = load(None, &["--commands.allow", "sh"]);
        assert_eq!(allowed, load(Some(&file), &[]));
        assert_eq!(
            load(Some(&file), &["--commands.allow", "git"]),
            load(None, &["--commands.allow", "git"])
        );
    }

    /// A tool's server runs in `/`: a path it gives from there, as one
    /// written for its own folder would (`--write .`), is refused, never
    /// read as the whole machine (or `/usr`, from `usr`).
    #[test]
    fn flags_from_slash_name_no_folder_by_a_relative_path() {
        let root = Path::new("/");
        for relative in [".", "..", "work", "./work", "../work"] {
            let err = flag_path(Path::new(relative), root).unwrap_err();
            assert!(
                format!("{err:#}").contains("give it in full"),
                "{relative}: {err:#}"
            );
        }
        assert_eq!(
            flag_path(Path::new("/srv/work"), root).unwrap(),
            "/srv/work"
        );
        assert_eq!(
            flag_path(Path::new("."), Path::new("/srv")).unwrap(),
            "/srv"
        );
    }
}
