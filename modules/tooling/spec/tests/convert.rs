//! `lnk spec convert` end to end, on a machine where no module exports the
//! file's kind.

use std::path::Path;

use link_test_kit::{TempDir, copy_program, env};

link_test_kit::program!(system);

#[test]
fn a_kind_no_module_exports_is_refused_with_the_list_of_kinds() {
    let home = TempDir::new("spec-convert-unowned");
    // Alone in its folder, so no module beside it describes a kind.
    let program = home.join("bin/lnk-spec");
    std::fs::create_dir_all(home.join("bin")).unwrap();
    copy_program(Path::new(env!("CARGO_BIN_EXE_lnk-spec")), &program).unwrap();
    let file = home.join("spec.toml");
    std::fs::write(&file, "kind = \"unowned\"\nschema = 1\nname = 'web'\n").unwrap();
    let output = env::command(&program, &home)
        .args(["convert"])
        .arg(&file)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && output.status.code() != Some(101),
        "{said}"
    );
    assert!(said.contains("No installed module exports"), "{said}");
    assert!(said.contains("lnk spec list"), "{said}");
}
