//! The native connection link request sent to an adapter.
use super::*;

impl Adapter {
    /// Links `connection`'s account in the home (`lnk-<name> link`: a QR
    /// code, on this terminal), anew with `again`. Whether it's linked:
    /// false when it isn't and there's no terminal to link it on.
    pub fn link(&self, boxed: &Boxed, connection: &str, again: bool) -> anyhow::Result<bool> {
        let mut args = vec!["link", connection];
        if again {
            args.push("--again");
        }
        let status = self
            .run_with(boxed, &args)?
            .status()
            .within(format!("running lnk-{} link", self.name))?;
        match status.code() {
            Some(0) => Ok(true),
            Some(link_harness_contract::harness::NOT_LINKED) => Ok(false),
            _ => bail!(
                "linking {connection} on {} failed ({status}){}",
                self.name,
                blocked_hint(boxed, &status)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_actual_link_callers_argv_is_the_contracts_declared_link() {
        let root = link_test_kit::TempDir::new("adapter-link-argv");
        let program = root.join("lnk-example");
        link_test_kit::write_program(
            &program,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"${0%/*}/args\"\n",
        )
        .unwrap();
        let adapter = Adapter {
            name: "example".into(),
            program,
        };
        let mut boxed = Boxed::for_tests();
        boxed.sandbox = false;
        boxed.dir = root.path().into();
        boxed.programs = Some(root.path().into());
        // The protocol's `link`, as the contract declares it: its words,
        // its one positional and `--again` a switch.
        let contract = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../harnesses/portability/contract");
        let catalog = link_actions::Catalog::load(&[&contract]).unwrap();
        let id = "harness-contract.harness-contract.link".parse().unwrap();
        let declared = catalog.action(&id).unwrap();
        let positionals: Vec<&str> = declared.positionals().iter().map(|(n, _)| *n).collect();
        assert_eq!(positionals, ["connection"]);
        assert!(declared.args.contains_key("again"));
        for again in [false, true] {
            assert!(adapter.link(&boxed, "signal", again).unwrap());
            let captured = std::fs::read_to_string(root.join("args")).unwrap();
            let mut expected: Vec<&str> = declared.id.words();
            expected.push("signal");
            if again {
                expected.push("--again");
            }
            assert_eq!(captured.lines().collect::<Vec<_>>(), expected);
        }
    }
}
