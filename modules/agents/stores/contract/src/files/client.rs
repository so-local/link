//! The caller's side of the files bridge ([`Client`]).

use super::*;

/// `bytes` for people: `4.2 GB`.
pub fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1_000_000_000 => format!("{:.1} GB", b as f64 / 1e9),
        b if b >= 1_000_000 => format!("{:.1} MB", b as f64 / 1e6),
        b if b >= 1_000 => format!("{:.1} KB", b as f64 / 1e3),
        b => format!("{b} bytes"),
    }
}

/// A caller of the files bridge at `socket`, as `binding`'s agent.
#[derive(Duplicate, Debug)]
pub struct Client {
    pub socket: PathBuf,
    #[duplicate]
    pub binding: Binding,
}

impl Client {
    fn ask(&self, action: &Action) -> anyhow::Result<Answer> {
        // A call answered busy is made again for a few seconds
        // ([`crate::bridge::retrying`]).
        crate::bridge::call_at(&self.socket, &self.binding, action, ANSWER_MAX).map_err(|err| {
            match err.downcast::<Failure>() {
                Ok(failure) if failure.why == Why::Conflict => Conflict {
                    name: link_format::clean(&failure.error),
                }
                .into(),
                Ok(failure) => anyhow::anyhow!("{failure}"),
                Err(err) => err,
            }
        })
    }

    fn got(answer: Answer) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        match answer {
            Answer::Got { data, version } => Ok(Some((
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .context("the bridge's answer isn't base64")?,
                version,
            ))),
            Answer::Missing { .. } => Ok(None),
            other => bail!("the bridge answered {other:?}"),
        }
    }

    fn written(answer: Answer) -> anyhow::Result<Version> {
        match answer {
            Answer::Written { version } => Ok(version),
            other => bail!("the bridge answered {other:?}"),
        }
    }

    /// File `path` of Home, and its version; none if there's none.
    pub fn read(&self, path: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.read_in(None, path)
    }

    /// Writes file `path` of Home whole, if it's as `expect` says.
    pub fn write(&self, path: &str, data: &[u8], expect: Expect) -> anyhow::Result<Version> {
        self.write_in(None, path, data, expect)
    }

    /// Every file under `folder` of Home (`memory/`; `""` for all).
    pub fn list(&self, folder: &str) -> anyhow::Result<Vec<Object>> {
        self.list_in(None, folder)
    }

    /// File `path` of `team`'s folder (Home's, for none), and its
    /// version; none if there's none.
    pub fn read_in(
        &self,
        team: Option<&TeamRef>,
        path: &str,
    ) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        Self::got(self.ask(&Action::Read {
            path: path.into(),
            team: team.cloned(),
        })?)
    }

    /// Writes file `path` of `team`'s folder (Home's, for none) whole, if
    /// it's as `expect` says.
    pub fn write_in(
        &self,
        team: Option<&TeamRef>,
        path: &str,
        data: &[u8],
        expect: Expect,
    ) -> anyhow::Result<Version> {
        Self::written(self.ask(&Action::Write {
            path: path.into(),
            data: base64::engine::general_purpose::STANDARD.encode(data),
            expect,
            team: team.cloned(),
        })?)
    }

    /// Every file under `folder` of `team`'s folder (Home's, for none)
    /// that it reaches.
    pub fn list_in(&self, team: Option<&TeamRef>, folder: &str) -> anyhow::Result<Vec<Object>> {
        match self.ask(&Action::List {
            folder: folder.into(),
            team: team.cloned(),
        })? {
            Answer::Listed { files } => Ok(files),
            other => bail!("the bridge answered {other:?}"),
        }
    }

    /// Object `name` of the index (`<engine>/<file>`).
    pub fn index_get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.index_get_in(None, name)
    }

    /// Every object of the index under `engine`, named `<engine>/<file>`.
    pub fn index_list(&self, engine: &str) -> anyhow::Result<Vec<Object>> {
        self.index_list_in(None, engine)
    }

    /// Removes object `name` of the index, if it's as `expect` says.
    pub fn index_delete(&self, name: &str, expect: Expect) -> anyhow::Result<()> {
        self.index_delete_in(None, name, expect)
    }

    /// Writes object `name` of the index, if it's as `expect` says.
    pub fn index_put(&self, name: &str, data: &[u8], expect: Expect) -> anyhow::Result<Version> {
        self.index_put_in(None, name, data, expect)
    }

    /// Object `name` of the index beside `team`'s folder (Home's, for
    /// none).
    pub fn index_get_in(
        &self,
        team: Option<&TeamRef>,
        name: &str,
    ) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        Self::got(self.ask(&Action::IndexGet {
            name: name.into(),
            team: team.cloned(),
        })?)
    }

    /// Every object under `engine` of the index beside `team`'s folder
    /// (Home's, for none).
    pub fn index_list_in(
        &self,
        team: Option<&TeamRef>,
        engine: &str,
    ) -> anyhow::Result<Vec<Object>> {
        match self.ask(&Action::IndexList {
            engine: engine.into(),
            team: team.cloned(),
        })? {
            Answer::Listed { files } => Ok(files),
            other => bail!("the bridge answered {other:?}"),
        }
    }

    /// Removes object `name` of the index beside `team`'s folder (Home's,
    /// for none), if it's as `expect` says.
    pub fn index_delete_in(
        &self,
        team: Option<&TeamRef>,
        name: &str,
        expect: Expect,
    ) -> anyhow::Result<()> {
        match self.ask(&Action::IndexDelete {
            name: name.into(),
            expect,
            team: team.cloned(),
        })? {
            Answer::Deleted { .. } => Ok(()),
            other => bail!("the bridge answered {other:?}"),
        }
    }

    /// Writes object `name` of the index beside `team`'s folder (Home's,
    /// for none), if it's as `expect` says.
    pub fn index_put_in(
        &self,
        team: Option<&TeamRef>,
        name: &str,
        data: &[u8],
        expect: Expect,
    ) -> anyhow::Result<Version> {
        Self::written(self.ask(&Action::IndexPut {
            name: name.into(),
            data: base64::engine::general_purpose::STANDARD.encode(data),
            expect,
            team: team.cloned(),
        })?)
    }
}

/// The files bridge as a [`Store`] of the calling agent's objects, named
/// as a store of Home's holds them: `home/<path>` (read, write, list) and
/// `index/<engine>/<file>` (get, put, list, delete), so what's written for
/// a store is written the same through the bridge.
impl Store for Client {
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.get_in(None, name)
    }

    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        self.put_in(None, name, data, expect)
    }

    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        self.delete_in(None, name, expect)
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        self.objects_in(None, prefix)
    }
}

/// A team's folder through the files bridge, as a member reaches it: a
/// [`Store`] named as Home's is (`home/<path>`, `index/<engine>/<file>`),
/// its index the one kept beside the folder. The team is the member's
/// name for it ([`TeamRef`]), asked of at each call, never a path.
#[derive(Duplicate, Debug)]
pub struct InTeam {
    #[duplicate]
    pub client: Client,
    pub team: TeamRef,
}

impl Store for InTeam {
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.client.get_in(Some(&self.team), name)
    }

    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        self.client.put_in(Some(&self.team), name, data, expect)
    }

    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        self.client.delete_in(Some(&self.team), name, expect)
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        self.client.objects_in(Some(&self.team), prefix)
    }
}

impl Client {
    fn get_in(
        &self,
        team: Option<&TeamRef>,
        name: &str,
    ) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        match (name.strip_prefix(HOME), name.strip_prefix(INDEX)) {
            (Some(path), _) => self.read_in(team, path),
            (_, Some(index)) => self.index_get_in(team, index),
            _ => bail!("{} isn't Home's or its index's", link_format::clean(name)),
        }
    }

    fn put_in(
        &self,
        team: Option<&TeamRef>,
        name: &str,
        data: &[u8],
        expect: &Expect,
    ) -> anyhow::Result<Version> {
        match (name.strip_prefix(HOME), name.strip_prefix(INDEX)) {
            (Some(path), _) => self.write_in(team, path, data, expect.clone()),
            (_, Some(index)) => self.index_put_in(team, index, data, expect.clone()),
            _ => bail!("{} isn't Home's or its index's", link_format::clean(name)),
        }
    }

    fn delete_in(&self, team: Option<&TeamRef>, name: &str, expect: &Expect) -> anyhow::Result<()> {
        match name.strip_prefix(INDEX) {
            Some(index) => self.index_delete_in(team, index, expect.clone()),
            None => bail!("only the index's objects are deleted through the bridge"),
        }
    }

    fn objects_in(&self, team: Option<&TeamRef>, prefix: &str) -> anyhow::Result<Vec<Object>> {
        let named = |prefix: &str, objects: Vec<Object>| {
            objects
                .into_iter()
                .map(|o| Object {
                    path: format!("{prefix}{}", o.path),
                    ..o
                })
                .collect::<Vec<_>>()
        };
        if let Some(folder) = prefix.strip_prefix(HOME) {
            // A folder whole, as the bridge lists them.
            let folder = match folder.rfind('/') {
                Some(at) => &folder[..=at],
                None => "",
            };
            let listed = named(HOME, self.list_in(team, folder)?);
            return Ok(listed
                .into_iter()
                .filter(|o| o.path.starts_with(prefix))
                .collect());
        }
        if let Some(rest) = prefix.strip_prefix(INDEX) {
            let engine = rest.split('/').next().unwrap_or_default();
            let listed = named(INDEX, self.index_list_in(team, engine)?);
            return Ok(listed
                .into_iter()
                .filter(|o| o.path.starts_with(prefix))
                .collect());
        }
        bail!("{} isn't Home's or its index's", link_format::clean(prefix))
    }
}
