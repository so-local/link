use super::*;

#[test]
fn instructions_are_optional_verbatim_and_never_silently_unreadable() {
    let dir = link_test_kit::TempDir::new("mini-instructions");
    let path = dir.join("instructions.md");
    assert_eq!(read_instructions(&path).unwrap(), "");
    std::fs::write(&path, "Answer briefly.\n").unwrap();
    assert_eq!(read_instructions(&path).unwrap(), "Answer briefly.\n");
    std::fs::write(&path, [0xff]).unwrap();
    assert!(read_instructions(&path).is_err());
    assert!(read_instructions(dir.path()).is_err());
}
