//! Report each workspace's click depth: how many links a reader follows
//! from its root README to its documents.

use std::path::PathBuf;
use std::process::ExitCode;

use link_docs_check::Tree;

#[cfg(test)]
link_test_kit::program!(unit);

#[derive(clap::Parser)]
#[command(about = "Report workspace documentation click depth")]
struct Cli {
    #[arg(long)]
    root: Vec<PathBuf>,
}

fn main() -> ExitCode {
    let Cli { root: mut roots } = link_spec::entry::plain();
    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }
    let large = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("large-files.toml");
    for root in roots {
        print!(
            "{}",
            link_docs_check::clicks::report(&Tree::read(&root, false, &large))
        );
    }
    ExitCode::SUCCESS
}
