//! Rank a workspace's documented features without reading cases or running commands.

use std::path::PathBuf;
use std::process::ExitCode;

use link_docs_check::{
    Tree,
    priority::{self, Weights},
};
use link_format::{Done, Out, Report, Section, Table, sentence};

#[derive(clap::Parser)]
struct Cli {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    weights: Option<PathBuf>,
    #[command(flatten)]
    output: link_spec::shared::Output,
}

fn run(out: &mut Out) -> Result<Done, Report> {
    let operation = (|| -> Result<priority::Ranking, String> {
        let given: Cli = link_spec::entry::plain();
        let weights = match given.weights {
            Some(file) => {
                toml::from_str(&std::fs::read_to_string(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?
            }
            None => Weights::bundled(),
        };
        let root = given.root.canonicalize().map_err(|e| e.to_string())?;
        let large = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("large-files.toml");
        priority::rank(&Tree::read(&root, false, &large), weights)
    })();
    let ranking = operation.map_err(|e| {
        Report::error(sentence!("Couldn't rank the documented features."))
            .why(link_format::Prose::from(link_format::Sentence::adopt(&e)))
    })?;
    out.reply(&ranking);
    let mut table = Table::new(
        [
            "Feature",
            "Points",
            "Incoming",
            "Mentioned",
            "Outgoing",
            "Examples",
        ],
        sentence!("There are no declared features."),
    );
    for f in &ranking.features {
        table.row([
            f.member.clone(),
            f.score.to_string(),
            f.incoming_pages.len().to_string(),
            f.mentioned_pages.len().to_string(),
            f.outgoing_pages.len().to_string(),
            f.examples.len().to_string(),
        ]);
    }
    out.block(Section::new("Documentation prominence").with(table));
    let mut queue = Table::new(
        [
            "Order",
            "Example",
            "Prior feature picks",
            "Gain",
            "Priority",
        ],
        sentence!("There are no source examples."),
    );
    for pick in ranking
        .priority_queue
        .iter()
        .take(ranking.weights.selection.preview)
    {
        queue.row([
            pick.order.to_string(),
            pick.argv.join(" "),
            pick.selected_before.to_string(),
            pick.marginal_points.to_string(),
            format!("{:.1}", pick.priority),
        ]);
    }
    out.block(Section::new("Suggested coverage order").with(queue));
    out.block(
        Section::new("Scope").with(link_format::Prose::from(sentence!(
            "{} documents and {} README example branches. Generated navigation does not score.",
            ranking.documents,
            ranking.example_branches
        ))),
    );
    Ok(Done::shown())
}

fn main() -> ExitCode {
    link_format::run(run)
}

#[cfg(test)]
link_test_kit::program!(unit);
