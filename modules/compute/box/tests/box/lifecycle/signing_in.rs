//! Cloud box creation needs no account bootstrap.
use super::*;
#[test]
fn a_box_is_created_without_an_account_bootstrap() {
    let Some(world) = World::new("without-bootstrap", true) else {
        return;
    };
    world.lnk_box(&["box", "create", "--cloud", "acct", "--yes"]);
    assert!(!world.home.join("access.log").exists());
    let ssh = world.read("ssh.log");
    assert!(
        !ssh.contains("auth login") && !ssh.contains("auth key"),
        "{ssh}"
    );
}
