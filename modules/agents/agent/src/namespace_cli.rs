//! CLI request parsing for the agent’s action namespaces.
use anyhow::ensure;
use link_agent::cli::Command;
use link_sandbox_client::routes::TOOL_MESSAGE_MAX;

pub(crate) fn read(request: &Option<String>) -> anyhow::Result<serde_json::Value> {
    use std::io::Read;
    let mut bytes = Vec::new();
    match request {
        Some(path) => {
            std::fs::File::open(path)?
                .take(TOOL_MESSAGE_MAX as u64 + 1)
                .read_to_end(&mut bytes)?;
        }
        None => {
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                std::io::stdin()
                    .take(TOOL_MESSAGE_MAX as u64 + 1)
                    .read_to_end(&mut bytes)?;
            }
        }
    }
    ensure!(bytes.len() <= TOOL_MESSAGE_MAX, "request too large");
    if bytes.is_empty() {
        Ok(serde_json::json!({}))
    } else {
        Ok(serde_json::from_slice(&bytes)?)
    }
}

/// The route a command that forwards a request calls, by its path
/// (`task run`), and the request's file.
pub(crate) fn command(command: &Command) -> Option<(String, &Option<String>)> {
    macro_rules! routes {
        ($($variant:ident => $path:literal),+ $(,)?) => {
            match command {
                // A person's `memory show` is read here: only its answer
                // for a program, `--json` or `--request`, is the route's.
                Command::MemoryShow(args) if !args.json && args.request.is_none() => None,
                $(Command::$variant(args) => Some(($path.to_string(), &args.request)),)+
                _ => None,
            }
        };
    }
    routes!(
        TaskRun => "task run",
        TaskList => "task list",
        TaskRead => "task read",
        TaskStop => "task stop",
        TaskAgent => "task agent",
        CronCreate => "cron create",
        CronDelete => "cron delete",
        CronList => "cron list",
        FileRead => "file read",
        FileWrite => "file write",
        FileEdit => "file edit",
        FileGlob => "file glob",
        FileGrep => "file grep",
        FileDefinition => "file definition",
        FileReferences => "file references",
        FileDiagnostics => "file diagnostics",
        WebFetch => "web fetch",
        WebSearch => "web search",
        BrowserOpen => "browser open",
        BrowserRead => "browser read",
        BrowserClick => "browser click",
        BrowserType => "browser type",
        BrowserSelect => "browser select",
        BrowserScroll => "browser scroll",
        BrowserScreenshot => "browser screenshot",
        BrowserClose => "browser close",
        DesktopScreenshot => "desktop screenshot",
        DesktopClick => "desktop click",
        DesktopType => "desktop type",
        DesktopKey => "desktop key",
        DesktopScroll => "desktop scroll",
        DesktopDrag => "desktop drag",
        ModelShow => "model show",
        ModelList => "model list",
        // Whether the model is ready: what its show says of its endpoint.
        ModelStatus => "model show",
        ModelChat => "model chat",
        ModelEmbed => "model embed",
        ModelTranscribe => "model transcribe",
        ModelCount => "model count",
        ModelUsage => "model usage",
        ModelContext => "model context",
        MemoryShow => "memory show",
    )
}
