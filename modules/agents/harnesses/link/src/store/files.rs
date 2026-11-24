//! [`Store`] as [`Files`]: the agent's folders on disk.

use super::When;
use super::*;

impl Store for Files {
    fn delete_work(
        &self,
        agent: &Agent,
        thread: &Thread,
        scope: &link_interactions::control::DeleteScope,
    ) -> anyhow::Result<()> {
        self.delete_retained(agent, thread, scope)
    }
    fn config(&self, agent: &Agent) -> anyhow::Result<Config> {
        use std::os::unix::fs::MetadataExt;
        let path = self.agents.paths(agent)?.config;
        if let Some(held) = self
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(agent)
        {
            return Ok(held.duplicate());
        }
        let reading = || format!("reading {} (lnk agent start writes it)", path.display());
        let (home, name) = parted(&path)?;
        let file = Held::root(home, false)
            .and_then(|home| {
                home.ok_or(std::io::ErrorKind::NotFound)?
                    .open(name, libc::O_RDONLY, 0)
            })
            .within(When(&reading))?;
        let meta = file.metadata().within(When(&reading))?;
        let stamp = (meta.dev(), meta.ino(), meta.len(), meta.modified().ok());
        let mut configs = self.configs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((seen, config)) = configs.get(agent)
            && *seen == stamp
        {
            return Ok(config.duplicate());
        }
        let bytes = read_open(file, CONFIG_MAX).within(When(&reading))?;
        let config: Config = link_files::saved::parse_json(&bytes).within(When(&reading))?;
        configs.insert(agent.clone(), (stamp, config.duplicate()));
        Ok(config)
    }

    fn set_config(&self, agent: &Agent, config: &Config) -> anyhow::Result<()> {
        let path = self.agents.paths(agent)?.config;
        if let Some(held) = self
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(agent)
        {
            *held = config.duplicate();
            return Ok(());
        }
        // Readable only by the user: it may hold a key.
        write_in(&path, &link_files::saved::print_json(config)?)
    }

    fn instruction_read(
        &self,
        agent: &Agent,
        path: &str,
    ) -> anyhow::Result<Option<InstructionRead>> {
        // A path of plain names in the agent's files, none of them a
        // link: nothing read elsewhere.
        let plain = !path.is_empty()
            && path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        anyhow::ensure!(plain, "{path:?} isn't a file in the agent's files");
        let collection = if path == link_harness_contract::harness::layout::NOTES {
            Some(link_store_contract::storage::Collection::Memory)
        } else {
            link_agent_contract::folder::seen_collection(path)
        };
        // Kept off this machine: read through the storage bridge alone.
        if let Some(collection) = collection
            && let Some((store, whose)) = self.placed(agent, collection)?
        {
            use link_store_contract::store::Store as _;
            let Some((bytes, _)) = store.file_read(&whose, path)? else {
                return Ok(None);
            };
            return InstructionRead::from_bytes(
                path,
                link_files::reading::ReadSnapshot::Whole {
                    content: link_files::reading::ContentDigest::of(&bytes),
                },
                &bytes[..bytes.len().min(INSTRUCTIONS_MAX as usize)],
                true,
            );
        }
        // A file of its instructions, skills or conversations in its own
        // folder, any other in Home (`memory/` there is Home's documents,
        // never memory's index), as the agent sees them: each folder on
        // the way opened from the one before.
        let root = match collection {
            Some(_) => self.own(agent, false)?,
            None => Held::root(&self.agents.paths(agent)?.workspace, false)?,
        };
        let Some(mut at) = root else {
            return Ok(None);
        };
        let (folders, name) = path.rsplit_once('/').unwrap_or(("", path));
        for part in folders.split('/').filter(|part| !part.is_empty()) {
            // A link, or a file where a folder goes, is left out and said,
            // as a skill that's one is: the agent's turns go on.
            match at.folder(part, false) {
                Ok(Some(next)) => at = next,
                Ok(None) => return Ok(None),
                Err(err) if left_out(&err) => {
                    crate::warn!("{}: {err}, left out", at.at(part).display());
                    return Ok(None);
                }
                Err(err) => {
                    return Err(err).within(format!("reading {}", at.at(part).display()));
                }
            }
        }
        let relative = path;
        let path = at.at(name);
        let file = match at.open(name, libc::O_RDONLY, 0) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            // A link, a folder or a pipe: left out, said.
            Err(err) if left_out(&err) => {
                crate::warn!("{}: {err}, left out", path.display());
                return Ok(None);
            }
            Err(err) => return Err(err).within(format!("reading {}", path.display())),
        };
        let mut bytes = Vec::new();
        file.take(INSTRUCTIONS_MAX + 1)
            .read_to_end(&mut bytes)
            .within(format!("reading {}", path.display()))?;
        InstructionRead::bounded(relative, &bytes, INSTRUCTIONS_MAX as usize, false)
    }

    fn skills(&self, agent: &Agent) -> anyhow::Result<Vec<Skill>> {
        // Kept off this machine: listed and read through the storage
        // bridge alone, each named as its folder is.
        if let Some((store, whose)) =
            self.placed(agent, link_store_contract::storage::Collection::Skills)?
        {
            use link_store_contract::store::Store as _;
            let folder = format!("{}/", link_harness_contract::harness::layout::SKILLS);
            let mut names: Vec<String> = store
                .file_list(&whose, &folder)?
                .iter()
                .filter_map(|path| {
                    let rest = path.strip_prefix(&folder)?;
                    let (name, file) = rest.split_once('/')?;
                    (file == skill::FILE && skill::check_name(name).is_ok())
                        .then(|| name.to_string())
                })
                .collect();
            names.sort();
            names.dedup();
            let mut skills = Vec::new();
            for name in names.into_iter().take(SKILLS_MAX) {
                let path = format!("{folder}{name}/{}", skill::FILE);
                let Some((bytes, _)) = store.file_read(&whose, &path)? else {
                    continue;
                };
                let text = &bytes[..bytes.len().min(INSTRUCTIONS_MAX as usize)];
                if let Ok(found) = skill::parse(&String::from_utf8_lossy(text))
                    && found.name == name
                {
                    skills.push(found);
                }
            }
            return Ok(skills);
        }
        let Some(own) = self.own(agent, false)? else {
            return Ok(Vec::new());
        };
        // A link there is refused.
        let Some(folder) = own.folder(link_harness_contract::harness::layout::SKILLS, false)?
        else {
            return Ok(Vec::new());
        };
        let mut names: Vec<String> = folder
            .names()
            .within(format!("reading {}", folder.path.display()))?
            .into_iter()
            .filter(|name| skill::check_name(name).is_ok())
            .collect();
        names.sort();
        let mut skills = Vec::new();
        for name in names.into_iter().take(SKILLS_MAX) {
            // A folder, not a link to one, nor through one.
            let Ok(Some(at)) = folder.folder(&name, false) else {
                continue;
            };
            let Ok(file) = at.open(skill::FILE, libc::O_RDONLY, 0) else {
                continue;
            };
            // Named as its folder is, as the standard says.
            if let Some(found) = self.skill(at.at(skill::FILE), file)
                && found.name == name
            {
                skills.push(found);
            }
        }
        Ok(skills)
    }

    fn read_from(&self, agent: &Agent, thread: &Thread, first: u64) -> anyhow::Result<Vec<Event>> {
        let folder = self.folder(agent, thread, false)?;
        let folder = match (self.kept(agent, folder.as_ref())?, folder) {
            (Some(Way::Driver(name)), _) => {
                let (driver, whose) = self.driver(agent, &name)?;
                let key = (
                    format!("store.{name}"),
                    agent.owner().into(),
                    agent.id().into(),
                    thread.name().into(),
                );
                return self.recent.read(key, first, |from| {
                    driver
                        .read(&whose, thread.name(), from)?
                        .into_iter()
                        .map(event_of)
                        .collect()
                });
            }
            (Some(Way::Compact), Some(folder)) => {
                let key = (
                    "compact".into(),
                    agent.owner().into(),
                    agent.id().into(),
                    thread.name().into(),
                );
                return self
                    .recent
                    .read(key, first, |from| frames::read(&folder, from));
            }
            (Some(Way::Log), Some(folder)) => folder,
            _ => return Ok(Vec::new()),
        };
        read_log(&folder, first, &self.logs).within(format!("reading {}", folder.at(LOG).display()))
    }

    fn snapshot(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Snapshot> {
        self.snapshot_read(agent, thread)
    }

    fn first_of(
        &self,
        agent: &Agent,
        thread: &Thread,
        repeat: &Repeat,
    ) -> anyhow::Result<Option<Event>> {
        let folder = self.folder(agent, thread, false)?;
        match (self.kept(agent, folder.as_ref())?, folder) {
            (Some(Way::Log), Some(folder)) => self
                .firsts
                .first(&folder, repeat)
                .within(format!("reading {}", folder.at(LOG).display())),
            _ => Ok(self
                .read(agent, thread)?
                .into_iter()
                .find(|event| repeat.names(event))),
        }
    }

    fn append(
        &self,
        agent: &Agent,
        thread: &Thread,
        event: &Event,
        before: &[Event],
    ) -> anyhow::Result<()> {
        if event.seq == 0 || event.seq > SEQ_MAX {
            bail!("a conversation holds at most {SEQ_MAX} messages");
        }
        let line = link_files::saved::line(event)?;
        if line.len() as u64 > EVENT_MAX + 1 {
            bail!("the message is larger than {EVENT_MAX} bytes");
        }
        let folder = match self.way(agent, thread)? {
            (Way::Log, Some(folder)) => folder,
            (Way::Compact, Some(folder)) => return frames::append(&folder, event.seq, &line),
            (Way::Driver(name), _) => {
                let (driver, whose) = self.driver(agent, &name)?;
                let value = serde_json::from_slice(&line)?;
                return match driver.append(&whose, thread.name(), &value)? {
                    Appended::Written => Ok(()),
                    Appended::Taken(last) if event.seq <= last => bail!(
                        "lnk-{name} holds number {} of {} already: another turn came first",
                        event.seq,
                        thread.name()
                    ),
                    Appended::Taken(last) => bail!(
                        "{} is the next number in {} (lnk-{name}), not {}",
                        last + 1,
                        thread.name(),
                        event.seq
                    ),
                };
            }
            (_, None) => bail!("{} has no folder to keep it in", thread.name()),
        };
        let path = folder.at(LOG);
        let writing = || format!("writing {}", path.display());
        let mut file = folder
            .open(LOG, libc::O_RDWR | libc::O_CREAT, 0o600)
            .within(When(&writing))?;
        // One writer at a time, of any process: the last number is read
        // and the next written while it's held.
        nofollow::lock_fd(file.as_raw_fd(), libc::LOCK_EX).within(When(&writing))?;
        let last = last_seq(&mut file, true)
            .within(When(&writing))?
            .unwrap_or_default();
        if event.seq <= last {
            bail!(
                "{} holds number {} already: another turn came first",
                path.display(),
                event.seq
            );
        }
        if event.seq != last + 1 {
            bail!(
                "{} is the next number in {}, not {}",
                last + 1,
                path.display(),
                event.seq
            );
        }
        file.seek(std::io::SeekFrom::End(0))
            .within(When(&writing))?;
        file.write_all(&line).within(When(&writing))?;
        drop(file);
        if self.copy(agent, thread, &folder, event, before).is_err() {
            crate::warn!(
                "event {} committed; its conversation copy could not be updated",
                event.seq
            );
        }
        Ok(())
    }

    fn last_events(&self, agent: &Agent, thread: &Thread, n: u64) -> anyhow::Result<Vec<Event>> {
        let folder = self.folder(agent, thread, false)?;
        let last = match (self.kept(agent, folder.as_ref())?, &folder) {
            (Some(Way::Driver(name)), _) => {
                let (driver, whose) = self.driver(agent, &name)?;
                driver.last(&whose, thread.name())?
            }
            (Some(Way::Compact), Some(folder)) => frames::last(folder)?,
            (Some(Way::Log), Some(folder)) => log_last(folder)?,
            _ => return Ok(Vec::new()),
        };
        if last == 0 || n == 0 {
            return Ok(Vec::new());
        }
        self.read_from(agent, thread, last.saturating_sub(n - 1).max(1))
    }

    fn threads(&self, agent: &Agent) -> anyhow::Result<Vec<Thread>> {
        let mut threads = Vec::new();
        for (named, folder) in self.each_folder(agent) {
            if folder.kind("fork.pending").is_some() {
                continue;
            }
            let Some(way) =
                kept_in(&folder)?.or_else(|| folder.kind("thread.json").map(|_| Way::Log))
            else {
                continue;
            };
            threads.push(self.thread_in(agent, &folder, &named, &way)?);
        }
        threads.sort_by(|a, b| a.name().cmp(b.name()));
        threads.dedup();
        Ok(threads)
    }

    fn tools(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Option<Vec<String>>> {
        // Kept with the conversation, off this machine: an error there
        // fails the turn rather than give it every tool.
        if let Some((driver, whose)) = self.conversations_off(agent)? {
            return driver.tools_read(&whose, thread.name());
        }
        let Some(folder) = self.folder(agent, thread, false)? else {
            return Ok(None);
        };
        let path = folder.at(TOOLS);
        let bytes = match folder.read(TOOLS, TOOLS_FILE_MAX) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            // Unreadable, it narrows to nothing rather than to every tool.
            Err(err) => return Err(err).within(format!("reading {}", path.display())),
        };
        let tools = link_store_contract::store::folder::read_tools(&bytes)
            .within(format!("reading {}", path.display()))?;
        Ok(Some(tools))
    }

    fn narrow_tools(&self, agent: &Agent, thread: &Thread, tools: &[String]) -> anyhow::Result<()> {
        if let Some((driver, whose)) = self.conversations_off(agent)? {
            // Narrowed by the driver in one step, never on this disk.
            return driver
                .tools_narrow(&whose, thread.name(), tools)
                .map(|_| ());
        }
        let narrowed: Vec<String> = match self.tools(agent, thread)? {
            Some(had) => tools.iter().filter(|t| had.contains(t)).cloned().collect(),
            None => tools.to_vec(),
        };
        let folder = self.made(agent, thread)?;
        // Whole or not at all.
        folder
            .write(
                TOOLS,
                &link_store_contract::store::folder::tools_file(&narrowed)?,
            )
            .within(format!("writing {}", folder.at(TOOLS).display()))?;
        // Narrowed before its first event: its copy names its thread, which
        // its folder's name can't (`:` is `-` there), so a move reads it
        // back as itself.
        if kept_in(&folder)?.is_none() && folder.kind(MARKDOWN).is_none() {
            folder
                .write(
                    MARKDOWN,
                    link_store_contract::store::copy::header(thread.name()).as_bytes(),
                )
                .within(format!("writing {}", folder.at(MARKDOWN).display()))?;
        }
        Ok(())
    }

    fn take_turn(&self, agent: &Agent, thread: &Thread, holder: &str) -> anyhow::Result<Took> {
        let folder = match self.way(agent, thread)? {
            (Way::Driver(name), folder) => {
                let (driver, whose) = self.driver(agent, &name)?;
                let took = driver.take(&whose, thread.name(), holder, self.lapse(agent))?;
                if took != link_store_contract::store::Took::Held {
                    self.clear_interrupt(agent, thread, folder.as_ref());
                }
                return Ok(match took {
                    link_store_contract::store::Took::Took => Took::Took,
                    link_store_contract::store::Took::Lapsed => Took::Lapsed,
                    link_store_contract::store::Took::Held => Took::Held,
                });
            }
            (_, Some(folder)) => folder,
            (_, None) => bail!("{} has no folder to keep it in", thread.name()),
        };
        let lease = Lease {
            holder: holder.to_string(),
            thread: thread.name().to_string(),
            started: secs(),
            renewed: secs(),
        };
        // At most a few tries: another process taking the lapsed lease
        // at once makes this one see it held.
        for _ in 0..4 {
            // Read first: a lease renewed lately is held, and nothing is
            // written.
            let judged = match folder.read(TURN, MARK_MAX) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    match folder.write_new(TURN, &serde_json::to_vec(&lease)?) {
                        Ok(()) => {
                            let _ = folder.remove(INTERRUPT);
                            return Ok(Took::Took);
                        }
                        Err(_) if folder.kind(TURN).is_some() => continue,
                        Err(err) => return Err(err),
                    }
                }
                read => read.ok(),
            };
            let lapse = self.lapse(agent);
            if !self.lapsed_as(&folder, judged.as_deref(), lapse) {
                return Ok(Took::Held);
            }
            // Taken over by one taker at a time, and only while it's the
            // lease judged lapsed: replaced in one move, never removed,
            // so no other finds it gone and takes it as new.
            if let Err(err) = folder.dir.create_new(OsStr::new(TAKE), 0o600) {
                // Left by a taker that crashed: let go, as a lease is.
                let left = folder
                    .modified(TAKE)
                    .is_some_and(|at| at.elapsed().unwrap_or_default() >= lapse);
                if err.kind() == std::io::ErrorKind::AlreadyExists && left {
                    let _ = folder.remove(TAKE);
                    continue;
                }
                return Ok(Took::Held);
            }
            let took = match folder.read(TURN, MARK_MAX).ok() == judged {
                true => folder
                    .write(TURN, &serde_json::to_vec(&lease)?)
                    .within(format!("writing {}", folder.at(TURN).display()))
                    .map(|()| true),
                false => Ok(false),
            };
            let _ = folder.remove(TAKE);
            if took? {
                let _ = folder.remove(INTERRUPT);
                return Ok(Took::Lapsed);
            }
        }
        Ok(Took::Held)
    }

    fn renew_turn(&self, agent: &Agent, thread: &Thread, holder: &str) -> anyhow::Result<()> {
        let folder = self.folder(agent, thread, false)?;
        if let Some(name) = self.driver_of(agent, folder.as_ref()) {
            let (driver, whose) = self.driver(agent, &name)?;
            if !driver.renew(&whose, thread.name(), holder)? {
                bail!("the turn's lease isn't this turn's any more");
            }
            return Ok(());
        }
        let lease = folder.as_ref().and_then(|folder| self.lease(folder));
        let (Some(folder), Some(mut lease)) = (folder, lease.filter(|l| l.holder == holder)) else {
            bail!("the turn's lease isn't this turn's any more");
        };
        lease.renewed = secs();
        // Written aside under a name of its own, then moved in place.
        folder
            .write(TURN, &serde_json::to_vec(&lease)?)
            .within(format!("writing {}", folder.at(TURN).display()))
    }

    fn end_turn(&self, agent: &Agent, thread: &Thread, holder: &str) -> anyhow::Result<()> {
        let folder = self.folder(agent, thread, false)?;
        if let Some(name) = self.driver_of(agent, folder.as_ref()) {
            let (driver, whose) = self.driver(agent, &name)?;
            let held = driver
                .leases(&whose, self.lapse(agent))?
                .iter()
                .any(|l| l.thread == thread.name() && l.holder == holder);
            if held {
                self.clear_interrupt(agent, thread, folder.as_ref());
            }
            return driver.end(&whose, thread.name(), holder);
        }
        let Some(folder) = folder else {
            return Ok(());
        };
        if self.lease(&folder).is_some_and(|l| l.holder == holder) {
            let _ = folder.remove(INTERRUPT);
            folder
                .remove(TURN)
                .within(format!("ending the turn in {}", folder.path.display()))?;
        }
        Ok(())
    }

    fn turn_running(&self, agent: &Agent, thread: &Thread) -> bool {
        let Ok(folder) = self.folder(agent, thread, false) else {
            return false;
        };
        if let Some(name) = self.driver_of(agent, folder.as_ref()) {
            let Ok((driver, whose)) = self.driver(agent, &name) else {
                return false;
            };
            return driver
                .leases(&whose, self.lapse(agent))
                .is_ok_and(|leases| {
                    leases
                        .iter()
                        .any(|l| l.thread == thread.name() && !l.lapsed)
                });
        }
        folder.is_some_and(|folder| {
            folder.kind(TURN).is_some() && !self.lapsed(&folder, self.lapse(agent))
        })
    }

    fn interrupt_turn(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<()> {
        if self.marks_held(agent) {
            self.marks()
                .interrupted
                .insert((agent.clone(), thread.name().to_string()));
            return Ok(());
        }
        let folder = self.made(agent, thread)?;
        folder
            .write(INTERRUPT, b"")
            .within(format!("writing {}", folder.at(INTERRUPT).display()))
    }

    fn interrupted(&self, agent: &Agent, thread: &Thread) -> bool {
        if self.marks_held(agent) {
            return self
                .marks()
                .interrupted
                .contains(&(agent.clone(), thread.name().to_string()));
        }
        self.folder(agent, thread, false)
            .ok()
            .flatten()
            .is_some_and(|folder| folder.kind(INTERRUPT).is_some())
    }

    fn set_recovered(&self, agent: &Agent, thread: &Thread, seq: u64) -> anyhow::Result<()> {
        if self.marks_held(agent) {
            self.marks()
                .recovered
                .insert((agent.clone(), thread.clone()), seq);
            return Ok(());
        }
        let folder = self.made(agent, thread)?;
        let kept = serde_json::json!({"thread": thread.name(), "seq": seq});
        folder
            .write(RECOVERED, kept.to_string().as_bytes())
            .within(format!("writing {}", folder.at(RECOVERED).display()))
    }

    fn take_recovered(&self, agent: &Agent) -> Vec<(Thread, u64)> {
        if self.marks_held(agent) {
            let mut marks = self.marks();
            let mut taken = Vec::new();
            marks.recovered.retain(|(whose, thread), seq| {
                let mine = whose == agent;
                if mine {
                    taken.push((thread.clone(), *seq));
                }
                !mine
            });
            taken.sort();
            return taken;
        }
        static TAKES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let mut taken = Vec::new();
        for (name, folder) in self.each_folder(agent) {
            // Moved aside first, under a name of its own: one taker only.
            let n = TAKES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let aside = format!("{RECOVERED}.{}-{n}", std::process::id());
            if folder.rename(RECOVERED, &aside).is_err() {
                continue;
            }
            let kept = recovered_in(&folder, &aside, &name);
            let _ = folder.remove(&aside);
            taken.extend(kept);
        }
        taken.sort();
        taken
    }

    fn recovered(&self, agent: &Agent) -> Vec<(Thread, u64)> {
        if self.marks_held(agent) {
            let mut kept: Vec<(Thread, u64)> = self
                .marks()
                .recovered
                .iter()
                .filter(|((whose, _), _)| whose == agent)
                .map(|((_, thread), seq)| (thread.clone(), *seq))
                .collect();
            kept.sort();
            return kept;
        }
        let mut kept: Vec<(Thread, u64)> = self
            .each_folder(agent)
            .into_iter()
            .filter_map(|(name, folder)| recovered_in(&folder, RECOVERED, &name))
            .collect();
        kept.sort();
        kept
    }

    fn drop_recovered(&self, agent: &Agent, thread: &Thread, seq: u64) -> anyhow::Result<()> {
        if self.marks_held(agent) {
            let mut marks = self.marks();
            let key = (agent.clone(), thread.clone());
            // Only that answer's: one kept since stays.
            if marks.recovered.get(&key) == Some(&seq) {
                marks.recovered.remove(&key);
            }
            return Ok(());
        }
        let Some(folder) = self.folder(agent, thread, false)? else {
            return Ok(());
        };
        let ours = Some((thread.clone(), seq));
        if recovered_in(&folder, RECOVERED, &thread.folder()) != ours {
            return Ok(());
        }
        // Only that answer's: one kept since goes back.
        let aside = format!("{RECOVERED}.{}", link_files::random_token()?);
        if folder.rename(RECOVERED, &aside).is_err() {
            return Ok(());
        }
        if recovered_in(&folder, &aside, &thread.folder()) != ours {
            let _ = folder.dir.link(OsStr::new(&aside), OsStr::new(RECOVERED));
        }
        folder
            .remove(&aside)
            .within(format!("removing {}", folder.at(&aside).display()))
    }

    fn tool_lists(&self, agent: &Agent) -> Option<Lists> {
        let path = self.agents.paths(agent).ok()?.tool_lists;
        let (home, name) = parted(&path).ok()?;
        let bytes = Held::root(home, false).ok()??.read(name, TOOL_LISTS_MAX);
        serde_json::from_slice(&bytes.ok()?).ok()
    }

    fn set_tool_lists(&self, agent: &Agent, lists: &Lists) -> anyhow::Result<()> {
        let path = self.agents.paths(agent)?.tool_lists;
        let bytes = serde_json::to_vec(lists)?;
        if bytes.len() as u64 > TOOL_LISTS_MAX {
            bail!("the tools' lists are larger than {TOOL_LISTS_MAX} bytes");
        }
        write_in(&path, &bytes)
    }

    fn keep_part(
        &self,
        agent: &Agent,
        thread: &Thread,
        file: &str,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let name = part_name(file)?;
        // Beside its events, whichever way they're kept here.
        let Some(folder) = self.way(agent, thread)?.1 else {
            bail!(
                "{} is kept off this machine, and images and documents are kept only \
                 beside a conversation kept on it",
                thread.name()
            );
        };
        let parts = folder
            .folder(PARTS, true)?
            .within(format!("making {}", folder.at(PARTS).display()))?;
        parts
            .write(name, bytes)
            .within(format!("writing {}", parts.at(name).display()))
    }

    fn part(&self, agent: &Agent, thread: &Thread, file: &str) -> anyhow::Result<Vec<u8>> {
        let name = part_name(file)?;
        let parts = match self.folder(agent, thread, false)? {
            Some(folder) => folder.folder(PARTS, false)?,
            None => None,
        };
        let Some(parts) = parts else {
            bail!("{file} isn't there");
        };
        parts
            .read(name, crate::event::PART_MAX)
            .within(format!("reading {}", parts.at(name).display()))
    }

    fn export(&self, agent: &Agent, name: &str, bytes: &[u8]) -> anyhow::Result<String> {
        let file = format!("{PARTS}/{name}");
        let name = part_name(&file)?;
        let exports = self
            .own(agent, true)?
            .within("the agent's folder")?
            .folder(EXPORTS, true)?
            .within("its exports")?;
        exports
            .write(name, bytes)
            .within(format!("writing {}", exports.at(name).display()))?;
        Ok(format!("{EXPORTS}/{name}"))
    }

    fn lapsed_turns(&self, agent: &Agent) -> Vec<Thread> {
        let lapse = self.lapse(agent);
        let mut threads = Vec::new();
        let mut drivers = std::collections::BTreeMap::<String, Vec<String>>::new();
        for (named, folder) in self.each_folder(agent) {
            if let Some(name) = self.driver_of(agent, Some(&folder)) {
                drivers.entry(name).or_default().push(named);
                continue;
            }
            if folder.kind(TURN).is_none() || !self.lapsed(&folder, lapse) {
                continue;
            }
            let thread = self
                .lease(&folder)
                .and_then(|l| Thread::new(&l.thread).ok());
            // Only a thread whose folder this is: a lease naming another
            // is no one's.
            if let Some(thread) = thread
                && thread.folder() == named
            {
                threads.push(thread);
            }
        }
        // A driver's leases: of a conversation whose folder says so.
        for (name, folders) in drivers {
            let Ok((driver, whose)) = self.driver(agent, &name) else {
                continue;
            };
            let Ok(leases) = driver.leases(&whose, lapse) else {
                continue;
            };
            for lease in leases.into_iter().filter(|l| l.lapsed) {
                if let Ok(thread) = Thread::new(&lease.thread)
                    && folders.contains(&thread.folder())
                {
                    threads.push(thread);
                }
            }
        }
        threads.sort();
        threads
    }
}
