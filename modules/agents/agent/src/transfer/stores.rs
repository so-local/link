//! Collection stores at a move's end. File is a driver like every other place.
use crate::settings::Settings;
use link_agent_contract::moving::collections;
use link_agent_contract::moving::collections::Kept;
use link_agent_contract::moving::collections::Staged;
use link_format::WithinExt;
use link_store_contract::storage::Collection;
use link_store_contract::store::{Agent, Store};
use std::collections::BTreeMap;
use std::path::Path;

pub(super) struct Stores {
    agent: Agent,
    stores: BTreeMap<Collection, Box<dyn Store>>,
}
impl Stores {
    pub(super) fn of(settings: &Settings) -> anyhow::Result<Self> {
        let preferred = settings.spec()?.spec.storage.unwrap_or_default();
        let places = crate::machine_settings::storage()?.resolve(&preferred);
        let folder = settings.folder();
        let root = &folder.agent;
        let agent = Agent {
            id: settings
                .agent_id()
                .map(str::to_string)
                .within("the agent has no ID")?,
            owner: settings.owner()?,
        };
        let mut stores: BTreeMap<Collection, Box<dyn Store>> = BTreeMap::new();
        for collection in collections::CARRIED {
            let place = places.of(collection);
            let store: Box<dyn Store> = if place == link_store_contract::storage::FILE {
                Box::new(link_store_contract::store::file::Folder::at(
                    root, &agent.id,
                )?)
            } else {
                let dir = folder.of(Collection::Conversations);
                link_files::create_private_dir(&dir)?;
                let program = link_core::modules::ensure(
                    link_store_contract::store::module_for(place),
                    &format!(
                        "moving {}'s {} collection",
                        settings.name,
                        collection.name()
                    ),
                )?;
                Box::new(link_store_contract::store::Driver::at(place, program, &dir))
            };
            stores.insert(collection, store);
        }
        Ok(Self { agent, stores })
    }
    pub(super) fn turn_running(&self) -> anyhow::Result<bool> {
        Ok(self.stores[&Collection::Conversations]
            .leases(&self.agent, std::time::Duration::from_secs(60))?
            .iter()
            .any(|l| !l.lapsed))
    }
    pub(super) fn kept(&self) -> Kept<'_> {
        Kept::new(self.agent.clone(), |c| &*self.stores[&c])
    }
}

/// A staged stream on disk, reached only by no-follow folder handles.
pub(super) struct Folder<'a>(pub &'a Path);
impl Staged for Folder<'_> {
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        fn walk(
            dir: &link_files::nofollow::Dir,
            prefix: &str,
            out: &mut Vec<String>,
            depth: usize,
        ) -> anyhow::Result<()> {
            anyhow::ensure!(
                depth <= link_files::nofollow::DEEPEST,
                "a staged collection is nested too deeply"
            );
            for name in dir.names()? {
                let name = name.to_str().within("a collection path isn't text")?;
                let path = format!("{prefix}{name}");
                match dir.stat(std::ffi::OsStr::new(name))?.kind {
                    link_files::nofollow::Kind::File => out.push(path),
                    link_files::nofollow::Kind::Folder => walk(
                        &dir.folder(std::ffi::OsStr::new(name))?,
                        &format!("{path}/"),
                        out,
                        depth + 1,
                    )?,
                    _ => anyhow::bail!("a collection holds a link: {path}"),
                }
            }
            Ok(())
        }
        let path = self.0.join(prefix.trim_end_matches('/'));
        let mut paths = Vec::new();
        match link_files::nofollow::Dir::open_unlinked(&path) {
            Ok(dir) => walk(&dir, prefix, &mut paths, 0)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(paths)
    }
    fn read(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        use std::io::Read;
        let mut bytes = Vec::new();
        link_agent_contract::moving::open_under(self.0, Path::new(path))?
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}
