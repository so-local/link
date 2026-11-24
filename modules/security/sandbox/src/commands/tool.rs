//! `lnk sandbox tool`: tools a sandboxed program calls outside.

use link_format::{
    Done, Foreign, Name, Note, Out, Report, Sentence, Table, Text, Value, WithinExt, cmd, phrase,
    prose, sentence,
};
use link_sandbox::bridges::tools;
use link_sandbox::cli::{self};
use link_sandbox_proxy::wire;

use super::*;

pub(crate) fn mcp_shared(
    cli::SandboxMcpSharedArgs {
        name,
        defined,
        dir,
        runtime,
    }: cli::SandboxMcpSharedArgs,
) -> Result<Done, Report> {
    // Link's settings as its proxy was handed them: it runs with
    // that proxy's HOME, its harness's.
    let dir = match dir {
        Some(dir) => link_sandbox::bridges::shared::check_dir(&dir)?,
        None => wire::dir()?,
    };
    link_sandbox::settings::read_from(&dir);
    let runtime = match runtime {
        Some(runtime) => runtime,
        None => link_files::layout::Root::Runtime.try_path()?,
    };
    link_sandbox::bridges::shared::serve(
        &runtime,
        &dir,
        &name,
        &defined,
        link_sandbox::settings::get().shared_idle,
    )?;
    Ok(Done::shown())
}

/// One of `lnk sandbox tool`'s commands, with its arguments.
pub(crate) enum ToolCommand {
    Add(cli::ToolAddArgs),
    List(cli::ToolListArgs),
    Calls(cli::ToolCallListArgs),
    Remove(cli::ToolRemoveArgs),
}

/// `lnk sandbox tool`.
pub(crate) fn tool(out: &mut Out, command: ToolCommand) -> Result<Done, Report> {
    let dir = wire::dir()?;
    match command {
        ToolCommand::Add(cli::ToolAddArgs {
            name,
            add: allow,
            ask,
            ask_if,
            ask_if_hint,
            mut env,
            shared,
            url,
            header_file,
            command,
        }) => {
            // Left out, every call asks.
            let ask = match ask {
                Some(cli::ToolAddArgsAsk::None) => tools::Ask::None,
                Some(cli::ToolAddArgsAsk::All) | None => tools::Ask::All,
            };
            let headers = match &header_file {
                Some(file) => link_secretive::read_secret(file)?,
                None => link_secretive::Secret::default(),
            };
            let command = match &url {
                Some(url) => {
                    link_sandbox::bridges::remote::check_url(url)?;
                    let lines = headers.expose().lines().filter(|l| !l.trim().is_empty());
                    for (i, header) in lines.enumerate() {
                        link_sandbox::bridges::remote::check_header(header)
                            .within(format!("line {} of --header-file", i + 1))?;
                        env.push(format!(
                            "{}{i}={header}",
                            link_sandbox::bridges::remote::HEADER_ENV
                        ));
                    }
                    let exe = std::env::current_exe()
                        .and_then(|exe| exe.canonicalize())
                        .within("finding this program")?;
                    vec![exe.display().to_string(), "mcp-remote".into(), url.clone()]
                }
                None => command,
            };
            let mut tool = tools::Tool::here(&name, command, &env, ask, allow)?
                .asking_if(ask_if)?
                .hinting(ask_if_hint)?;
            tool.shared = shared;
            let replaced = tools::save(&dir, &tool)?;
            out.block(Note(Sentence::adopt(&asking(&tool))));
            let tool_name = Name::new("tool", &name);
            Ok(Done::success(sentence!(
                "{} {}: {}.",
                if replaced {
                    "Replaced tool"
                } else {
                    "Added tool"
                },
                tool_name.clone(),
                Foreign::new(&tool.argv.join(" "))
            ))
            .next(
                cmd!(
                    "lnk sandbox run --commands.toolchains {} --tools.names {} -- <command>",
                    "shell",
                    tool_name.clone()
                ),
                "a sandbox calls it once it names it",
            )
            .next(
                cmd!(
                    "lnk agent permission add --tools.names {}",
                    tool_name.clone()
                ),
                "let an agent call it",
            )
            .next(
                cmd!("lnk sandbox tool-call list {}", tool_name),
                "list its calls",
            ))
        }
        ToolCommand::List(cli::ToolListArgs { json }) => {
            let all = tools::list(&dir);
            if json {
                // A line each, as its readers parse it.
                for tool in &all {
                    link_actions::answer::line::<link_sandbox_contract::spec::Tool>(
                        out,
                        &serde_json::json!({
                            "name": tool.name, "argv": tool.argv, "ask": tool.ask, "allow": tool.allow,
                            "ask_if": tool.ask_if,
                        }),
                    )?;
                }
                return Ok(Done::shown());
            }
            let mut table = Table::new(
                ["TOOL", "COMMAND", "ASKS"],
                sentence!("There are no tools."),
            );
            for tool in &all {
                table.row([
                    Value::from(Name::new("tool", &tool.name)),
                    Value::from(Foreign::new(&tool.argv.join(" "))),
                    Value::from(asking(tool)),
                ]);
            }
            out.block(table);
            Ok(Done::shown().next(
                cmd!("lnk sandbox tool add <name> -- <command>"),
                "add a tool",
            ))
        }
        ToolCommand::Calls(cli::ToolCallListArgs { name }) => {
            let tool = tools::load(&dir, &name)?;
            let calls = runtime()
                .within("starting a runtime")?
                .block_on(tools::calls(&tool))?;
            let mut table = Table::new(
                ["CALL", "ASKS", "SERVER SAYS", "ABOUT"],
                sentence!("{} has no calls.", Name::new("tool", &name)),
            );
            for (call, hints, about) in &calls {
                let flag =
                    |k: &str| hints.get(k).and_then(serde_json::Value::as_bool) == Some(true);
                let said = match (flag("readOnlyHint"), flag("destructiveHint")) {
                    (true, _) => "says it reads",
                    (_, true) => "says it may delete",
                    _ => "",
                };
                let asks = if tool.asks(call) { "asks" } else { "allowed" };
                table.row([
                    Value::from(Foreign::new(call)),
                    Value::from(asks),
                    Value::from(said),
                    Value::from(Foreign::new(about)),
                ]);
            }
            out.block(table);
            if !calls.is_empty() {
                out.block(Text(prose!(
                    "What a server says of its calls is its own word: it never skips asking."
                )));
            }
            Ok(Done::shown())
        }
        ToolCommand::Remove(cli::ToolRemoveArgs { name }) => match tools::remove(&dir, &name)? {
            true => Ok(Done::success(sentence!(
                "Removed tool {}: no sandbox reaches it anymore.",
                Name::new("tool", &name)
            ))),
            false => {
                out.block(Note(sentence!(
                    "There's no tool {}.",
                    Name::typed("tool", &name)
                )));
                Ok(Done::shown().next(
                    cmd!("lnk sandbox tool list"),
                    phrase!("see the tools there are"),
                ))
            }
        },
    }
}

/// Which of `tool`'s calls ask, in a line.
pub(crate) fn asking(tool: &tools::Tool) -> String {
    let said = match (tool.ask, tool.allow.as_slice()) {
        (tools::Ask::None, _) => return "No call asks you (--ask none).".into(),
        (tools::Ask::All, []) => "Every call asks you first.".to_string(),
        (tools::Ask::All, allowed) => {
            format!("Every call asks you first but {}.", allowed.join(", "))
        }
    };
    match tool.ask_if.as_slice() {
        [] => said,
        by => format!(
            "{said} Asked whatever that says, unless given false: {}.",
            by.join(", ")
        ),
    }
}
