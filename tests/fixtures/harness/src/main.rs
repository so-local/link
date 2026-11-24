//! Contract-test controls live here, apart from the installed Mini Harness.

fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    // This test-only component can itself be put in an owned signed release.
    // Its adapter commands are Mini's, read by Mini's generated parser.
    if link_spec::entry::discovery() == Some(link_spec::entry::Discovery::Description) {
        return match describe() {
            Ok(value) => {
                println!("{value}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error:#}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    let command = link_mini_harness::cli::parse();
    if matches!(command, link_mini_harness::cli::Command::MiniHarnessAcp(_))
        && std::env::var_os("LNK_HARNESS_FIXTURE_MODEL").is_some()
    {
        link_settings::output::exit(serve())
    } else {
        link_settings::output::run(|out| link_mini_harness::fixture_adapter(out, command))
    }
}

fn describe() -> anyhow::Result<serde_json::Value> {
    let source = link_component::Source::parse(
        r#"schema = 1
[module]
id = "qa-outside"
kind = "harness"
[commands.contract]
help = "Describe the current owned harness fixture protocol."
"#,
    )?;
    let descriptor = source.resolve(
        link_component::Component {
            id: "qa-outside".into(),
            cargo_name: env!("CARGO_PKG_NAME").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            license: env!("CARGO_PKG_LICENSE").into(),
            description: "An owned external harness fixture, never a vendor harness".into(),
            program: "lnk-outside-harness".into(),
            kind: link_component::Kind::Harness,
            reads_control: true,
        },
        Vec::new(),
    )?;
    Ok(serde_json::to_value(descriptor)?)
}

fn serve() -> anyhow::Result<()> {
    let model = serde_json::from_str(&std::env::var("LNK_HARNESS_FIXTURE_MODEL")?)?;
    let tools = std::env::var("LNK_HARNESS_FIXTURE_TOOLS").is_ok_and(|v| v == "env");
    let commands = std::env::var("LNK_HARNESS_FIXTURE_COMMANDS")
        .unwrap_or_default()
        .split(',')
        .filter(|n| !n.is_empty())
        .map(String::from)
        .collect();
    let state = !std::env::var("LNK_HARNESS_FIXTURE_STATE").is_ok_and(|v| v == "none");
    link_mini_harness::fixture(model, tools, commands, state)
}
