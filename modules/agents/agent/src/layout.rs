//! The agent's folder, laid out as `link_agent_contract::folder` says, in
//! one place ([`lay_out`]): wherever an agent is made, arrives or starts,
//! Home's folders and one folder a collection in its own (`agent/`) are
//! made. Nothing is moved from anywhere else, and nothing is followed
//! through a link.

use link_format::WithinExt;

use crate::settings::Settings;

/// Makes each folder of `agent`'s layout that isn't there; what's there is
/// left. An error where its own folder, or a collection's in it, is a link.
pub fn lay_out(agent: &Settings) -> anyhow::Result<()> {
    let folder = agent.folder();
    std::fs::create_dir_all(&folder.home).within(format!("creating {}", folder.home.display()))?;
    folder.make()
}

#[cfg(test)]
mod tests {
    use link_agent_contract::folder::Folder;

    /// An agent's folder, as `dir`, its Home `dir/home`.
    fn agent_folder(name: &str) -> (std::path::PathBuf, Folder) {
        let dir = std::env::temp_dir().join(format!("lnk-layout-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let folder = Folder::new(&dir, None);
        (dir, folder)
    }

    /// Each folder of the layout is made, and what is there is left.
    #[test]
    fn each_folder_of_the_layout_is_made_and_the_rest_left() {
        let (dir, folder) = agent_folder("made");
        std::fs::write(folder.home.join("notes.txt"), "mine").unwrap();
        folder.make().unwrap();
        folder.make().unwrap();
        assert!(folder.agent.join("skills").is_dir());
        assert_eq!(
            std::fs::read_to_string(folder.home.join("notes.txt")).unwrap(),
            "mine"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A link where its own folder or a collection's is: refused, never
    /// followed.
    #[test]
    fn a_link_for_its_own_folder_is_refused() {
        let (dir, folder) = agent_folder("own-link");
        let elsewhere = dir.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(&folder.agent).unwrap();
        std::os::unix::fs::symlink(&elsewhere, folder.agent.join("skills")).unwrap();
        assert!(folder.make().is_err());
        assert!(std::fs::read_dir(&elsewhere).unwrap().next().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
