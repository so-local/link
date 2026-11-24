//! `lnk-file-store` run as its callers run it: here, and behind the store
//! bridge, the store's contract, files, narrowed tools and jobs kept in
//! the agent's folder as a person reads them.

link_test_kit::program!(unit);

use link_store_contract::store::{Agent, Driver, Store as _};
use link_test_kit::TempDir;

fn agent() -> Agent {
    Agent {
        id: "aaaaaaaaaaaa".into(),
        owner: "me@box".into(),
    }
}

fn program() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_lnk-file-store"))
}

#[test]
fn it_keeps_the_contract() {
    let dir = TempDir::new("file-contract");
    let driver = Driver::at("file", program(), &dir.join("agent"))
        .with_env(link_store_contract::store::AGENT_ENV, "aaaaaaaaaaaa");
    let contract = driver.contract().unwrap();
    assert!(contract.serve && contract.files && contract.jobs);
    link_store_contract::store::check(&driver, &agent());
    link_store_contract::store::check_files(&driver, &agent());
    link_store_contract::store::check_narrowing(&driver, &agent());
    link_store_contract::store::check_jobs(&driver, &agent());
    // As a person reads it: the instructions a plain file, the
    // conversation a log and a copy.
    assert!(dir.join("agent/instructions/notes.md").is_file());
    let event = serde_json::json!({"seq": 1, "role": "user", "text": "hi"});
    driver.append(&agent(), "telegram:7", &event).unwrap();
    assert!(dir.join("agent/chats/telegram-7/events.jsonl").is_file());
}

#[test]
fn it_keeps_the_contract_behind_the_store_bridge() {
    let dir = TempDir::new("file-bridged");
    let driver = Driver::at("file", program(), &dir.join("agent"))
        .with_env(link_store_contract::store::AGENT_ENV, "aaaaaaaaaaaa");
    let bridged = link_store_contract::testing::store_bridge(&dir, "aaaaaaaaaaaa", driver);
    link_store_contract::store::check(&bridged, &agent());
    link_store_contract::store::check_files(&bridged, &agent());
    link_store_contract::store::check_narrowing(&bridged, &agent());
    link_store_contract::store::check_jobs(&bridged, &agent());
    // As its scheduler finds them.
    assert!(dir.join("agent/schedule/day.json").is_file());
}
