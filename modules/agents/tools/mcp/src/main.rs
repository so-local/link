//! A standalone stdio MCP transport for declared Link commands.

use link_mcp::{
    Catalog,
    cli::{self, Command},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    link_secretive::process::guard();
    link_core::describe_component!();
    let Command::McpServe(cli) = cli::parse();
    let catalog = Catalog::installed(cli.agent)?;
    if cli.read_calls {
        let mut output = link_format::streams::protocol();
        serde_json::to_writer(&mut output, &catalog.reads())?;
        return Ok(());
    }
    link_mcp::transport::serve(
        catalog,
        tokio::io::BufReader::new(tokio::io::stdin()),
        tokio::fs::File::from_std(link_format::streams::Protocol::file()?),
    )
    .await
}

#[cfg(test)]
link_test_kit::program!(unit);
