//! Posts in and verdicts out on the webhook protocol's lines.
use link_format::{Done, Out, Report, WithinExt};
fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(try_main)
}
fn try_main(_out: &mut Out) -> Result<Done, Report> {
    if !link_webhook_connection::cli::sifts().within("uninstalling")? {
        return Ok(Done::shown());
    }
    link_connection_contract::pushed::sift_lines(
        link_webhook_connection::sift,
        std::io::stdin().lock(),
        link_format::streams::protocol(),
    )
    .within("sifting the posts")?;
    Ok(Done::shown())
}
