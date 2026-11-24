//! Refreshes Link's models file with OpenRouter's current figures, as a
//! release does before it builds (`.github/workflows/release.yml`):
//!
//! ```sh
//! curl -fsS https://openrouter.ai/api/v1/models > models.json
//! cargo run -p link-model --example refresh-models -- models.json modules/agents/model/contract/src/models/link.toml
//! ```
//!
//! The file is written only when the refreshed one reads back whole. A
//! figure that moves past what a provider could change (a window more
//! than halved, a price tenfold) keeps the committed one, said as a
//! warning in the release's log.

#[derive(clap::Parser)]
struct Cli {
    listed: std::path::PathBuf,
    file: std::path::PathBuf,
}

fn main() -> anyhow::Result<()> {
    let Cli { listed, file } = link_spec::entry::plain();
    let text = std::fs::read_to_string(&file)?;
    let current = link_model_contract::parse(&text)?;
    let models: serde_json::Value = serde_json::from_slice(&std::fs::read(listed)?)?;
    let (refreshed, refused) = link_model_contract::refresh(&current, &models)?;
    for refused in refused {
        println!("::warning::{refused}");
    }
    let printed = link_model_contract::print(&link_model_contract::head(&text), &refreshed)?;
    std::fs::write(file, printed)?;
    println!(
        "{} models, {} before",
        refreshed.model.len(),
        current.model.len()
    );
    Ok(())
}
