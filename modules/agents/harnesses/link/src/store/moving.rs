//! Moving an agent's conversations from one way of keeping them to another.

use super::*;

impl Files {
    /// Moves each of `agent`'s conversations kept another way than `to`
    /// (as its spec says a new one is kept) to `to`: its events, in a folder made
    /// beside it, then put in its place, with what else its folder held
    /// (its tools, an answer still to send). The agent is stopped: no
    /// turn writes meanwhile. Each moved, with the way it was kept.
    pub fn move_conversations(
        &self,
        agent: &Agent,
        to: &Way,
    ) -> anyhow::Result<Vec<(Thread, Way)>> {
        let Some(conversations) = self.conversations(agent, false)? else {
            return Ok(Vec::new());
        };
        let mut moved = Vec::new();
        for name in conversations
            .names()
            .within(format!("reading {}", conversations.path.display()))?
        {
            if name.starts_with('.') || conversations.kind(&name) != Some(Kind::Folder) {
                continue;
            }
            let Some(folder) = conversations.folder(&name, false)? else {
                continue;
            };
            let Some(from) = kept_in(&folder)? else {
                continue;
            };
            if from == *to {
                continue;
            }
            let thread = self
                .thread_in(agent, &folder, &name, &from)
                .within(format!("finding {}'s thread", folder.path.display()))?;
            drop(folder);
            self.move_one(agent, &thread, &conversations, &from, to)
                .within(format!("moving {} to {}", thread.name(), to.name()))?;
            moved.push((thread, from));
        }
        Ok(moved)
    }

    /// The thread whose conversation is in `folder`, named `named`, kept
    /// `way`: as its driver lists it, its frames name it, or its copy's
    /// first line does; else its folder's name.
    pub(super) fn thread_in(
        &self,
        agent: &Agent,
        folder: &Held,
        named: &str,
        way: &Way,
    ) -> anyhow::Result<Thread> {
        if let Ok(bytes) = folder.read("thread.json", 256)
            && let Ok(name) = serde_json::from_slice::<String>(&bytes)
            && let Ok(thread) = Thread::new(&name)
            && thread.folder() == named
        {
            return Ok(thread);
        }
        let names: Vec<String> = match way {
            Way::Driver(name) => {
                let (driver, whose) = self.driver(agent, name)?;
                driver
                    .threads(&whose)?
                    .into_iter()
                    .map(|kept| kept.thread)
                    .collect()
            }
            Way::Compact => frames::thread(folder).into_iter().collect(),
            // Its copy's first line: `# telegram:42`.
            Way::Log => folder
                .open(MARKDOWN, libc::O_RDONLY, 0)
                .ok()
                .and_then(|copy| {
                    let mut head = Vec::new();
                    copy.take(256).read_to_end(&mut head).ok()?;
                    let first = head.split(|b| *b == b'\n').next()?.to_vec();
                    Some(
                        String::from_utf8(first)
                            .ok()?
                            .strip_prefix("# ")?
                            .to_string(),
                    )
                })
                .into_iter()
                .collect(),
        };
        names
            .iter()
            .chain([&named.to_string()])
            .filter_map(|name| Thread::new(name).ok())
            .find(|thread| thread.folder() == named)
            .within(format!("no thread's folder is {named}"))
    }

    /// Moves `thread`'s conversation, in its folder in `chats`,
    /// from `from` to `to`: each name reached from its folder's handle.
    fn move_one(
        &self,
        agent: &Agent,
        thread: &Thread,
        conversations: &Held,
        from: &Way,
        to: &Way,
    ) -> anyhow::Result<()> {
        let events = self.read(agent, thread)?;
        let name = thread.folder();
        let moving = format!("moving-{name}");
        let aside = format!("moved-{name}");
        for left in [&moving, &aside] {
            match conversations.dir.remove_all(OsStr::new(left)) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    return Err(err)
                        .within(format!("removing {}", conversations.at(left).display()));
                }
                _ => {}
            }
        }
        let made = conversations
            .dir
            .create_new_folder(OsStr::new(&moving), 0o700)
            .within(format!("creating {}", conversations.at(&moving).display()))?;
        let made = Held {
            dir: made,
            path: conversations.at(&moving),
        };
        match to {
            Way::Log => {
                let mut log = Vec::new();
                for event in &events {
                    log.extend(link_files::saved::line(event)?);
                }
                made.write(LOG, &log)?;
                made.write(MARKDOWN, markdown(thread, &events).as_bytes())?;
            }
            Way::Compact => {
                frames::start(&made, thread.name())?;
                for event in &events {
                    frames::append(&made, event.seq, &link_files::saved::line(event)?)?;
                }
            }
            Way::Driver(name) => {
                let (driver, whose) = self.driver(agent, name)?;
                // Left by a move that stopped midway.
                driver.remove(&whose, thread.name())?;
                for event in &events {
                    let value = serde_json::from_slice(&link_files::saved::print_json(event)?)?;
                    if driver.append(&whose, thread.name(), &value)? != Appended::Written {
                        bail!("lnk-{name} refused event {}", event.seq);
                    }
                }
                made.write(&format!("{DRIVER_MARK}{name}"), b"")?;
            }
        }
        conversations.rename(&name, &aside).within(format!(
            "moving {} aside",
            conversations.at(&name).display()
        ))?;
        conversations
            .rename(&moving, &name)
            .within(format!("moving {} in place", made.path.display()))?;
        // What isn't its events goes along.
        let old = conversations
            .folder(&aside, false)?
            .within(format!("reading {}", conversations.at(&aside).display()))?;
        for file in old.names()? {
            let events = [LOG, MARKDOWN, link_store_contract::store::compact::FRAMES];
            if events.contains(&file.as_str()) || file.starts_with(DRIVER_MARK) {
                continue;
            }
            old.dir
                .rename_to(OsStr::new(&file), &made.dir, OsStr::new(&file))
                .within(format!("moving {file} along"))?;
        }
        if let Way::Driver(name) = from {
            let (driver, whose) = self.driver(agent, name)?;
            driver.remove(&whose, thread.name())?;
        }
        conversations
            .dir
            .remove_all(OsStr::new(&aside))
            .within(format!("removing {}", conversations.at(&aside).display()))
    }
}
