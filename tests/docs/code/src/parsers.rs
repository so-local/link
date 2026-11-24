//! The real command parsers used by documentation and parser-contract tests.

/// The programs whose command line is generated from their version 1
/// declarations (`link-actions-build`), so no parser-first check reads
/// them: their parser is the declarations'.
/// Each is shown as a person types it, its program under `lnk
/// <namespace>`, since the program itself never reads its namespace.
pub fn declared() -> Vec<(&'static str, clap::Command)> {
    [
        ("agent", link_agent::cli::command()),
        ("archive", link_archive_module::cli::command()),
        ("aws", link_aws::flags::command()),
        ("box", link_box::cli::command()),
        ("browser-tool", link_browser_tool::cli::command()),
        ("bucket-store", link_bucket_store::cli::command()),
        ("bundle", link_bundle::cli::command()),
        ("cloud", link_cloud::cli::command()),
        ("code-tool", link_code_tool::cli::command()),
        ("core", link_cli::cli::command()),
        ("deepseek", link_deepseek::cli::command()),
        ("desktop-tool", link_desktop_tool::cli::command()),
        ("discord", link_discord::cli::command()),
        (
            "document-converter",
            link_document_converter::cli::command(),
        ),
        ("env", link_env::cli::command()),
        ("file-store", link_file_store::cli::command()),
        ("file-tool", link_file_tool::cli::command()),
        ("gcp", link_gcp::flags::command()),
        ("harness", link_harness::cli::command()),
        ("hermes", link_hermes::cli::command()),
        ("manual", link_manual::cli::command()),
        ("mcp", link_mcp::cli::command()),
        ("measure", link_measure::cli::command()),
        ("memory-tool", link_memory_tool::cli::command()),
        ("mini-harness", link_mini_harness::cli::command()),
        ("model", link_model::cli::command()),
        ("openclaw", link_openclaw::cli::command()),
        ("run-tool", link_run_tool::cli::command()),
        ("sandbox", link_sandbox::cli::command()),
        ("scheduler-tool", link_scheduler_tool::cli::command()),
        ("secret", link_secret::cli::command()),
        ("signal", link_signal::cli::command()),
        ("slack", link_slack::cli::command()),
        ("spec", link_spec_module::cli::command()),
        ("sqlite-store", link_sqlite_store::cli::command()),
        ("storage", link_storage::cli::command()),
        ("telegram", link_telegram::cli::command()),
        ("web-tool", link_web_tool::cli::command()),
        (
            "webhook-connection",
            link_webhook_connection::cli::command(),
        ),
    ]
    .into_iter()
    .map(|(namespace, program)| {
        // Core's program is lnk itself; another's sits under its namespace.
        let typed = match namespace == link_actions::core().module {
            true => program,
            false => clap::Command::new("lnk").subcommand(program.name(namespace)),
        };
        (namespace, typed)
    })
    .collect()
}

/// Every program's parser, including its hidden protocol commands.
pub fn commands() -> Vec<clap::Command> {
    named().into_iter().map(|(_, command)| command).collect()
}

/// Every program's parser, by its name.
pub fn named() -> Vec<(&'static str, clap::Command)> {
    declared()
}
