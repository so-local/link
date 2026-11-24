//! A permission snapshot: filesystem names are resolved before validation
//! and consent, then stay literal through wrapping and metadata.

use link_sandbox_client::{Confinement, Programs, RunWith};
use link_secretive::Duplicate;

pub struct Resolved(Confinement);

impl Resolved {
    pub fn new(policy: Confinement) -> anyhow::Result<Self> {
        Ok(Self(link_sandbox_confine::resolve_files(&policy)?))
    }

    pub fn set_programs(&mut self, programs: Programs) {
        self.0.commands = programs;
    }

    pub fn terminal(&mut self, terminal: bool) {
        self.0.terminal = terminal;
    }

    pub fn hide_more(&mut self, paths: Vec<std::path::PathBuf>) {
        self.0
            .hide
            .extend(paths.iter().map(|p| link_sandbox_confine::resolve(p)));
    }

    /// A session's folder, written: refused through a link a non-root
    /// user owns, as every grant is.
    pub fn session_folder(&mut self, folder: &std::path::Path) -> anyhow::Result<()> {
        let folder = link_sandbox_confine::resolve_grant(folder)?;
        self.0.write.insert(0, folder.clone());
        self.0.write_dot_files.insert(0, folder);
        Ok(())
    }

    pub fn given(
        &mut self,
        with: RunWith,
        machine: &super::Machine,
    ) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
        crate::given(&mut self.0, with, machine)
    }

    pub fn opens(&self, programs: Option<&[std::path::PathBuf]>) -> String {
        super::opens_at(&self.0, programs, true)
    }

    pub fn describe(&self) -> Vec<String> {
        super::describe_at(&self.0, true)
    }
}

impl std::ops::Deref for Resolved {
    type Target = Confinement;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Duplicate for Resolved {
    fn duplicate(&self) -> Self {
        Self(self.0.duplicate())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checked_snapshot_does_not_follow_a_later_root_link() {
        let temp = link_test_kit::TempDir::new("resolved-policy");
        let root = temp.canonicalize().unwrap();
        let grant = root.join("grant");
        let outside = root.join("outside");
        std::fs::create_dir(&grant).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let raw: Confinement = serde_json::from_value(serde_json::json!({
            "name":"snapshot", "write":[grant], "read":[], "commands":"any", "net":"closed"
        }))
        .unwrap();
        let frozen = Resolved::new(raw).unwrap();
        let permissions = frozen.opens(None);
        std::fs::rename(&grant, root.join("moved")).unwrap();
        std::os::unix::fs::symlink(&outside, &grant).unwrap();
        assert_eq!(frozen.opens(None), permissions);
        assert!(
            !frozen
                .describe()
                .join("\n")
                .contains(&outside.display().to_string())
        );
        let rules = link_sandbox_confine::Rules::from_resolved(&frozen);
        assert_eq!(rules.common.write, [grant]);
    }

    #[test]
    fn replacing_a_protected_snapshot_cannot_make_it_pass_validation() {
        let temp = link_test_kit::TempDir::new("resolved-protection");
        let home = temp.canonicalize().unwrap();
        let protected = home.join(".ssh");
        let safe = home.join("safe");
        std::fs::create_dir(&protected).unwrap();
        std::fs::create_dir(&safe).unwrap();
        let raw = serde_json::from_value(serde_json::json!({
            "name":"snapshot", "write":[protected], "read":[], "commands":"any", "net":"closed"
        }))
        .unwrap();
        let frozen = Resolved::new(raw).unwrap();
        std::fs::rename(&protected, home.join("moved")).unwrap();
        std::os::unix::fs::symlink(&safe, &protected).unwrap();
        let machine = super::super::Machine {
            home: &home,
            settings: &home.join("settings"),
            hidden: &[],
            spec: None,
            run_path: std::ffi::OsStr::new("/usr/bin:/bin"),
        };
        let error = super::super::check(&frozen, &machine).unwrap_err();
        assert!(error.to_string().contains("dot folder"), "{error}");
    }
}
