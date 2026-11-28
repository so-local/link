//! `lnk-sandbox`: the sandbox module's commands. `lnk` runs it for `lnk
//! sandbox ...`.

use link_format::{Done, Out, Prose, Report, Sentence, WithinExt, sentence};
use link_sandbox::cli::{self, Command};

mod commands;

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[
        link_sandbox::settings::saved,
        link_sandbox::bridges::tools::saved,
        link_sandbox_client::grants::saved,
        commands::stop::saved,
        link_spec::saved_seen,
    ]);
    link_core::describe_component!(link_sandbox_contract::Spec);
    link_settings::output::run(try_main)
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let done = command(out);
    // Said once the command ends: its settings are read where first needed.
    if let Some(why) = link_sandbox::settings::problem() {
        out.block(
            Report::warning(sentence!(
                "The sandbox's settings can't be read, so Link's are used."
            ))
            .why(Prose::from(Sentence::adopt(why))),
        );
    }
    done
}

fn command(out: &mut Out) -> Result<Done, Report> {
    let parsed = match cli::parse() {
        cli::Invoked::Page(page) => {
            link_sandbox::bridges::open::request(&page)?;
            return Ok(Done::shown());
        }
        cli::Invoked::Command(parsed) => *parsed,
    };
    let fields = parsed.fields;
    match parsed.command.command {
        Command::Answer(question) => {
            commands::log::answer(&question)?;
            Ok(Done::shown())
        }
        Command::SandboxRun(given) => commands::run::run_command(out, given, fields),
        Command::SandboxLearn(given) => commands::learn::learn_command(out, given, fields),
        Command::SpecShow(show) => commands::spec::show(out, &show, fields),
        Command::ToolAdd(given) => {
            commands::tool::tool(out, commands::tool::ToolCommand::Add(given))
        }
        Command::ToolList(given) => {
            commands::tool::tool(out, commands::tool::ToolCommand::List(given))
        }
        Command::ToolCallList(given) => {
            commands::tool::tool(out, commands::tool::ToolCommand::Calls(given))
        }
        Command::ToolRemove(given) => {
            commands::tool::tool(out, commands::tool::ToolCommand::Remove(given))
        }
        Command::QuestionList(given) => commands::asks::asks(out, given),
        Command::SandboxAnswer(given) => commands::asks::answer(given),
        Command::SandboxEnter(entered) => commands::enter::enter(out, entered),
        Command::SandboxCheck(given) => commands::check::check_command(out, given, fields),
        Command::PermissionList(_) => commands::permissions::permissions(out),
        Command::LogShow(given) => commands::log::log_command(out, given),
        Command::SandboxStop(given) => commands::stop::stop(out, given.name),
        Command::SandboxStart(given) => commands::stop::start(out, given.name),
        Command::SandboxOnStop(given) => commands::stop::on_stop(&given.name, &given.args),
        Command::SandboxShow(given) => commands::log::show(out, given),
        Command::SandboxList(given) => commands::stop::list(out, given),
        Command::SandboxPlaceholders(given) => Ok(Done::passed(
            link_sandbox_confine::linux::mounts::run(commands::run::placeholders(given))?,
        )),
        Command::SandboxWrap(_) => commands::proxy::wrap(out),
        Command::SandboxProxy(given) => commands::proxy::proxy(given),
        Command::SandboxBridge(given) => {
            commands::proxy::bridge(link_sandbox_route::bridge::Started {
                socket: given.socket,
                keys: given.keys,
                settings: given.settings,
                runtime: given.runtime,
            })
        }
        Command::SandboxConfine(given) => commands::confine::confine(given),
        Command::SandboxWorker(given) => commands::confine::worker(given),
        Command::SandboxWorkers(given) => {
            #[cfg(target_os = "linux")]
            {
                commands::workers::workers(given)?;
                Ok(Done::shown())
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = given;
                Err(anyhow::anyhow!("worker identities are Linux's").into())
            }
        }
        Command::SandboxDnsSocket(given) => commands::proxy::dns_socket(given.pid, given.fd),
        Command::SandboxRefusals(given) => Ok(Done::passed(commands::run::exit_code(
            link_sandbox_confine::here()?.refusals(&given.exec, &given.argv)?,
        ))),
        Command::SandboxTryExec(given) => commands::check::try_exec_command(given),
        Command::SandboxLookup(given) => commands::proxy::lookup_command(given),
        Command::SandboxProxyInside(given) => commands::proxy::proxy_inside(given),
        Command::SandboxLoopback(given) => commands::proxy::loopback(given),
        Command::SandboxMcpRemote(given) => {
            commands::proxy::runtime()
                .within("starting a runtime")?
                .block_on(link_sandbox::bridges::remote::bridge(&given.url))?;
            Ok(Done::shown())
        }
        Command::SandboxMcpShared(given) => commands::tool::mcp_shared(given),
        Command::SandboxMcpNamespace(given) => {
            commands::proxy::runtime()
                .within("starting a runtime")?
                .block_on(link_sandbox::bridges::tools::namespace::serve(
                    &given.file,
                    given.by,
                ))?;
            Ok(Done::shown())
        }
        Command::SandboxConnect(given) => commands::proxy::connect(given),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::*;
    use clap::CommandFactory;
    use link_sandbox::opening::Opening;
    use link_sandbox::spec::Spec;
    use link_sandbox_client::{Confinement, Net};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    #[test]
    fn the_cli_is_consistent() {
        cli::Cli::command().debug_assert();
    }

    /// A spec with every key, but its OS tables.
    const FULL: &str = r#"
name = "all"
version = 1
[files]
write = ["~/w"]
read = ["~/r"]
write_dot_files = ["~/w"]
hide = ["~/w/s"]
[network]
mode = "proxy"
[internet]
hosts = ["*"]
tcp = ["a.com:22"]
per_minute = 60
[lan]
hosts = ["nas.local"]
[localhost]
ports = [8080]
models = [11434]
[tools]
names = ["gh"]
[browser]
hosts = ["*"]
[commands]
allow = ["sh"]
developer_tools = true
"#;

    /// `lnk sandbox run` as `args` give it (`lnk sandbox run` first), read
    /// by the program's own entry: what it opens, and its command.
    fn run_of(args: &[&str]) -> (Opening, Vec<String>) {
        let (_, rest) = args.split_at(3);
        let line = ["lnk-sandbox", "run"]
            .iter()
            .chain(rest)
            .map(std::ffi::OsString::from);
        let parsed = cli::parse_from(line).unwrap();
        let Command::SandboxRun(run) = parsed.command.command else {
            panic!("not run");
        };
        let opening = Opening::of(run.spec, parsed.fields, run.upstream, run.yes);
        (opening, run.commands)
    }

    /// Every key of a spec is a field flag, its key path: a file and its
    /// flags make the same spec.
    #[test]
    fn every_key_of_a_spec_is_a_field_flag() {
        let file: Spec = link_spec::parse(FULL).unwrap();
        let value = toml::Value::try_from(&file).unwrap();
        let mut args = vec!["lnk", "sandbox", "run"];
        let flags: Vec<String> = link_spec::flags_of(&file).unwrap();
        args.extend(flags.iter().map(String::as_str));
        args.extend(["--", "sh"]);
        let (opening, _) = run_of(&args);
        let given = link_spec::over(&Spec::default(), &opening.fields).unwrap();
        assert_eq!(toml::Value::try_from(&given).unwrap(), value);
    }

    #[test]
    fn flags_write_a_spec_and_a_file_takes_them_over_it() {
        let (opening, _) = run_of(&[
            "lnk",
            "sandbox",
            "run",
            "--files.write",
            "work",
            "--internet.hosts",
            "a.com",
            "--files.hide",
            "~/x",
            "--",
            "bash",
        ]);
        let spec = opening.flags_spec().unwrap();
        let files = spec.files.unwrap();
        assert!(Path::new(&files.write.unwrap()[0]).is_absolute());
        assert_eq!(files.hide.unwrap(), ["~/x"]);
        assert_eq!(spec.internet.unwrap().hosts.unwrap(), ["a.com"]);
    }

    #[test]
    fn a_flags_path_may_climb_but_the_spec_it_writes_never_does() {
        assert_eq!(
            link_sandbox_confine::lexical(Path::new("/a/b/../c/./d")),
            Path::new("/a/c/d")
        );
        assert_eq!(
            link_sandbox_confine::lexical(Path::new("/../x")),
            Path::new("/x")
        );
        let (opening, _) = run_of(&[
            "lnk",
            "sandbox",
            "run",
            "--files.write",
            "../x",
            "--files.read",
            "~/a/../b",
            "--",
            "sh",
        ]);
        let files = opening.flags_spec().unwrap().files.unwrap();
        let write = files.write.unwrap();
        let above = std::env::current_dir().unwrap().parent().unwrap().join("x");
        assert_eq!(Path::new(&write[0]), above);
        let read = files.read.unwrap();
        assert_eq!(Path::new(&read[0]), link_files::home_dir().join("b"));
        // What the spec's paths are read as takes them.
        let home = link_files::home_dir();
        for path in write.iter().chain(&read) {
            link_spec::path(Path::new(path), Path::new("/"), &home).unwrap();
        }
    }

    /// The sandbox `lnk sandbox run` makes of `flags`.
    fn run_sandbox(flags: &[&str]) -> Confinement {
        let args: Vec<&str> = ["lnk", "sandbox", "run"]
            .into_iter()
            .chain(flags.iter().copied())
            .chain(["--", "sh"])
            .collect();
        let (opening, _) = run_of(&args);
        link_secretive::Duplicate::duplicate(&*opening.opened("same").unwrap().sandbox)
    }

    /// What each flag opens: `--internet.hosts` the internet through the
    /// proxy, `--files.read ~` the home, `--browser.hosts` pages.
    #[test]
    fn each_flag_opens_what_it_names() {
        let proxied = |flags: &[&str]| match run_sandbox(flags).net {
            Net::Proxied { proxy, ports } => {
                let open = proxy.pages().is_some();
                (proxy, ports, open)
            }
            other => panic!("{flags:?}: {other:?}"),
        };
        let (proxy, ports, open) = proxied(&["--internet.hosts", "*"]);
        assert!(proxy.hosts.is_empty() && !proxy.no_internet && ports.is_empty() && !open);
        let (proxy, ..) = proxied(&["--internet.hosts", "a.com", "--internet.hosts", "*.b.com"]);
        assert_eq!(proxy.hosts, ["a.com", "*.b.com"]);
        let (proxy, ..) = proxied(&[
            "--internet.hosts",
            "*",
            "--internet.tcp",
            "a.com:22",
            "--internet.per_minute",
            "60",
        ]);
        assert_eq!(
            (proxy.tcp, proxy.per_minute),
            (vec!["a.com:22".into()], Some(60))
        );
        let (proxy, ports, _) = proxied(&[
            "--internet.hosts",
            "*",
            "--tools.names",
            "gh",
            "--localhost.models",
            "11434",
            "--localhost.ports",
            "8080",
        ]);
        assert_eq!(proxy.tools(), ["gh"]);
        assert_eq!(proxy.models(), BTreeSet::from([11434]));
        assert_eq!(ports, BTreeSet::from([8080]));
        let (proxy, _, open) = proxied(&["--internet.hosts", "a.com", "--browser.hosts", "a.com"]);
        assert!(open && proxy.pages().unwrap() == ["a.com"]);
        let (proxy, _, open) = proxied(&["--internet.hosts", "*", "--browser.hosts", "*"]);
        assert!(open && proxy.pages().unwrap() == ["*"]);
        let (w, h) = ("/nowhere/work", "/nowhere/work/secrets");
        let home = run_sandbox(&["--files.read", "~", "--files.write", w, "--files.hide", h]);
        assert!(home.read_home && home.read.is_empty() && home.net == Net::Closed);
        assert_eq!(home.write, [PathBuf::from(w)]);
        assert!(home.hide.contains(&PathBuf::from(h)));
        // The network as it is: its own mode, every localhost port with it.
        let host = run_sandbox(&["--network.mode", "host", "--internet.hosts", "*"]);
        assert!(matches!(
            host.net,
            Net::Host {
                localhost: false,
                ..
            }
        ));
    }

    #[test]
    fn a_run_is_refused_before_it_starts_unless_its_program_is_listed() {
        let sh = link_sandbox_confine::resolve(
            &link_files::find_program("sh", &std::env::var_os("PATH").unwrap()).unwrap(),
        );
        assert!(listed(std::slice::from_ref(&sh), &["sh".into()]).is_ok());
        let err = listed(&[sh], &["ls".into()]).unwrap_err();
        let headline = err.headline.to_string();
        assert!(headline.contains("isn't one of the programs"), "{headline}");
    }

    #[test]
    fn flags_make_a_sandbox_of_absolute_paths() {
        let (opening, command) = run_of(&[
            "lnk",
            "sandbox",
            "run",
            "--files.write",
            "work",
            "--internet.hosts",
            "*",
            "--localhost.ports",
            "11434",
            "--",
            "bash",
            "-l",
        ]);
        assert_eq!(command, ["bash", "-l"]);
        let sandbox = opening.opened("run-1").unwrap().sandbox;
        assert!(sandbox.write[0].is_absolute() && sandbox.write[0].ends_with("work"));
        let Net::Proxied { proxy, ports, .. } = &sandbox.net else {
            panic!("{:?}", sandbox.net)
        };
        assert!(!proxy.no_internet && proxy.hosts.is_empty());
        assert_eq!(ports, &BTreeSet::from([11434]));
        assert_eq!(sandbox.hide, link_sandbox_contract::hidden_dirs());
        let (_, command) = run_of(&["lnk", "sandbox", "run", "--allow", "root", "--", "x"]);
        assert!(flagless(&command).is_err());
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
