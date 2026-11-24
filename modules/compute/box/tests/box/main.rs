link_test_kit::program!(system);

mod here;
mod images;
mod lifecycle;
mod listed;
mod ownership;
mod provider;
mod setup;
mod specs;
mod users;

/// Whether executable allowlist kernel coverage is available, which a
/// box user's own sandbox needs. `LNK_REQUIRE_NATIVE` makes missing
/// coverage a failure.
#[cfg(target_os = "linux")]
fn landlock() -> bool {
    use link_test_kit::gate;
    if !link_sandbox_confine::linux::FILTERED {
        return gate::lacks(
            "executable restrictions",
            "they need a seccomp filter built for this architecture",
            Some(gate::NATIVE),
        );
    }
    match link_sandbox_confine::linux::landlock::probe() {
        Ok(_) => true,
        Err(error) => gate::lacks("Landlock", error, Some(gate::NATIVE)),
    }
}
