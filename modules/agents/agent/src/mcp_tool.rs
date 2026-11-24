//! Link MCP, the tool `[tool.mcp]` gives an agent, served by `lnk-mcp`
//! behind the ordinary sandbox MCP approval boundary.
use link_agent::settings::Settings;
use link_format::WithinExt;

pub(crate) fn handed(agent: &Settings) -> Option<(String, String)> {
    (agent.spec().ok()?.tool("mcp").enabled == Some(true))
        .then(|| ("mcp".into(), format!("mcp-{}", agent.key())))
}

/// The calls of Link MCP an agent makes without asking, its reads
/// (`manual`, `agent__list`), as the Link MCP module declares them: what its
/// grant opens, the rest asking first.
pub(crate) fn reads(agent: &Settings) -> anyhow::Result<Vec<String>> {
    let selector = link_agent::settings::ids::written(&agent.key());
    let program = link_core::find_module("mcp").within("the Link MCP module is not installed")?;
    let output = link_secretive::process::command(&program)
        .args(read_calls(&selector))
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "the Link MCP module could not describe its read calls: {}",
        link_format::clean(&String::from_utf8_lossy(&output.stderr))
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(crate) fn define(agent: &Settings) -> anyhow::Result<()> {
    let name = format!("mcp-{}", agent.key());
    let Some(_) = handed(agent) else {
        let _ = link_core::lnk()
            .args(["sandbox", "tool", "remove", &name])
            .output();
        return Ok(());
    };
    let selector = link_agent::settings::ids::written(&agent.key());
    let program = link_core::find_module("mcp").within("the Link MCP module is not installed")?;
    let reads = reads(agent)?;
    let mut command = link_core::lnk();
    command
        .env("LNK_INSTALLATION_DIR", link_files::try_installation_dir()?)
        .args([
            "sandbox",
            "tool",
            "add",
            &name,
            "--ask",
            "all",
            "--env",
            "LNK_INSTALLATION_DIR",
        ]);
    // Only declared reads bypass approval. An empty list grants nothing.
    for read in reads {
        command.args(["--add", &read]);
    }
    let output = command
        .arg("--")
        .arg(program)
        .args(serving(&selector))
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "registering the Link MCP tool failed: {}",
        link_format::clean(&String::from_utf8_lossy(&output.stderr))
    );
    crate::confine::forget_defined_tools();
    Ok(())
}

/// What `lnk-mcp` is given to list `selector`'s read calls. Its program
/// never sees its namespace word: `--agent` goes to its default, `serve`.
fn read_calls(selector: &str) -> [&str; 3] {
    ["--agent", selector, "--read-calls"]
}

/// What `lnk-mcp` is given to serve Link MCP to `selector`.
fn serving(selector: &str) -> [&str; 2] {
    ["--agent", selector]
}

#[cfg(test)]
mod tests {
    /// The built `lnk-mcp` takes what the agent gives it: its read calls
    /// answer as a JSON list, and serving parses (its help, so nothing
    /// waits on a client).
    #[test]
    fn the_mcp_program_takes_what_the_agent_gives_it() {
        let home = link_test_kit::TempDir::new("mcp-tool");
        let programs = link_test_kit::programs();
        let program = programs.join("lnk-mcp");
        let run = |args: &[&str]| {
            link_test_kit::env::command(&program, &home)
                .env("LNK_SERVICES", "off")
                .env("LNK_EXE", programs.join("lnk"))
                .args(args)
                .output()
                .unwrap()
        };
        let reads = run(&super::read_calls("main"));
        assert!(
            reads.status.success(),
            "{}",
            String::from_utf8_lossy(&reads.stderr)
        );
        let calls: Vec<String> = serde_json::from_slice(&reads.stdout).unwrap();
        assert!(calls.iter().all(|call| !call.is_empty()));
        let mut serve = super::serving("main").to_vec();
        serve.push("--help");
        let help = run(&serve);
        assert!(
            help.status.success(),
            "{}",
            String::from_utf8_lossy(&help.stderr)
        );
    }
}
