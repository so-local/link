//! Mini Harness: the minimum model/tool loop, integrated with Link.

fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    link_core::saved::own(&link_mini_harness::SAVED);
    link_core::describe_component!();
    link_settings::output::run(link_mini_harness::run)
}
