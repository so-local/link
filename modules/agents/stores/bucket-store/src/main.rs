//! `lnk-bucket-store`: the `bucket` place's driver. An agent's
//! collections as objects in its bucket, the one `lnk storage` connected
//! for it, encrypted (`store`). It answers `link_store_contract::store`'s
//! contract, run by the storage bridge outside every sandbox.

mod store;

use link_format::{Done, Out, Report};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let Some(operation) = link_bucket_store::cli::operation().map_err(|err| {
        Report::error(link_format::sentence!("Couldn't uninstall it.")).cause(err)
    })?
    else {
        return Ok(Done::shown());
    };
    link_store_contract::store::run(out, operation, store::contract(), store::answer)
}

#[cfg(test)]
link_test_kit::program!(unit);
