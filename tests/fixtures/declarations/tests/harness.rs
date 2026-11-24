//! A command the agent's harness runs is the program's too where its
//! surfaces include cli, and never where they don't: its surfaces alone say.

link_test_kit::program!(unit);

use clap::CommandFactory;
use link_declarations_fixture::module;

#[test]
fn a_harness_command_is_the_programs_only_where_its_surfaces_say_cli() {
    let program = module::Cli::command();
    // compact is a chat command and a command line's, declared once.
    assert!(program.find_subcommand("compact").is_some());
    // steer is the harness's alone, kept for dispatch.
    assert!(program.find_subcommand("steer").is_none());
    let _: fn(module::EnvSteerArgs) -> module::Command = module::Command::EnvSteer;
}
