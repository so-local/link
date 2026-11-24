//! What keeps an agent's conversations, as its callers call it: the
//! [`Store`] interface, each request typed.

use super::*;

/// What keeps an agent's conversations, as its callers call it: a driver
/// run here ([`Driver`]), or one behind the store bridge ([`Bridged`]).
/// Each call is one [`Request`]; the rest are those calls, typed.
pub trait Store: Sync {
    /// Its name, for what it says: the driver's (`sqlite`).
    fn name(&self) -> &str;

    /// One call, its reply: an error when it says one.
    fn call(&self, request: &Request) -> anyhow::Result<Reply>;

    /// [`Request::Append`].
    fn append(&self, agent: &Agent, thread: &str, event: &Value) -> anyhow::Result<Appended> {
        let size = serde_json::to_vec(event)?.len() as u64;
        anyhow::ensure!(
            size <= EVENT_MAX,
            "the event is larger than {EVENT_MAX} bytes"
        );
        let request = Request::Append {
            agent: agent.clone(),
            thread: thread.to_string(),
            event: event.clone(),
        };
        match self.call(&request)? {
            Reply::Done => Ok(Appended::Written),
            Reply::Taken { last } => Ok(Appended::Taken(last)),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::Read`].
    fn read(&self, agent: &Agent, thread: &str, first: u64) -> anyhow::Result<Vec<Value>> {
        let request = Request::Read {
            agent: agent.clone(),
            thread: thread.to_string(),
            first,
        };
        match self.call(&request)? {
            Reply::Events { events } => Ok(events),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::Last`].
    fn last(&self, agent: &Agent, thread: &str) -> anyhow::Result<u64> {
        let request = Request::Last {
            agent: agent.clone(),
            thread: thread.to_string(),
        };
        match self.call(&request)? {
            Reply::Last { last } => Ok(last),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::Threads`].
    fn threads(&self, agent: &Agent) -> anyhow::Result<Vec<Kept>> {
        match self.call(&Request::Threads {
            agent: agent.clone(),
        })? {
            Reply::Threads { threads } => Ok(threads),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::Remove`].
    fn remove(&self, agent: &Agent, thread: &str) -> anyhow::Result<()> {
        let request = Request::Remove {
            agent: agent.clone(),
            thread: thread.to_string(),
        };
        match self.call(&request)? {
            Reply::Done => Ok(()),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::Take`].
    fn take(
        &self,
        agent: &Agent,
        thread: &str,
        holder: &str,
        lapse: Duration,
    ) -> anyhow::Result<Took> {
        let request = Request::Take {
            agent: agent.clone(),
            thread: thread.to_string(),
            holder: holder.to_string(),
            lapse_ms: millis(lapse),
        };
        match self.call(&request)? {
            Reply::Took { took } => Ok(took),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::Renew`]: false when the lease isn't `holder`'s.
    fn renew(&self, agent: &Agent, thread: &str, holder: &str) -> anyhow::Result<bool> {
        let request = Request::Renew {
            agent: agent.clone(),
            thread: thread.to_string(),
            holder: holder.to_string(),
        };
        match self.call(&request)? {
            Reply::Done => Ok(true),
            Reply::Lost => Ok(false),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::End`].
    fn end(&self, agent: &Agent, thread: &str, holder: &str) -> anyhow::Result<()> {
        let request = Request::End {
            agent: agent.clone(),
            thread: thread.to_string(),
            holder: holder.to_string(),
        };
        match self.call(&request)? {
            Reply::Done => Ok(()),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::Leases`].
    fn leases(&self, agent: &Agent, lapse: Duration) -> anyhow::Result<Vec<Lease>> {
        let request = Request::Leases {
            agent: agent.clone(),
            lapse_ms: millis(lapse),
        };
        match self.call(&request)? {
            Reply::Leases { leases } => Ok(leases),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobPut`]: an error for a job a driver doesn't keep
    /// ([`check_job`]), never sent.
    fn job_put(&self, agent: &Agent, place: Place, job: &Job) -> anyhow::Result<()> {
        check_job(job)?;
        let request = Request::JobPut {
            agent: agent.clone(),
            place,
            job: job.clone(),
        };
        match self.call(&request)? {
            Reply::Done => Ok(()),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobGet`].
    fn job_get(&self, agent: &Agent, place: Place, id: &str) -> anyhow::Result<Option<Job>> {
        let request = Request::JobGet {
            agent: agent.clone(),
            place,
            id: id.to_string(),
        };
        match self.call(&request)? {
            Reply::Job { job } => Ok(job),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobList`].
    fn job_list(&self, agent: &Agent, place: Place) -> anyhow::Result<Vec<Job>> {
        let request = Request::JobList {
            agent: agent.clone(),
            place,
        };
        match self.call(&request)? {
            Reply::Jobs { jobs } => Ok(jobs),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobCount`].
    fn job_count(&self, agent: &Agent, place: Place) -> anyhow::Result<u64> {
        let request = Request::JobCount {
            agent: agent.clone(),
            place,
        };
        match self.call(&request)? {
            Reply::Count { count } => Ok(count),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobShift`]: whether it was in `from`.
    fn job_shift(&self, agent: &Agent, id: &str, from: Place, to: Place) -> anyhow::Result<bool> {
        let request = Request::JobShift {
            agent: agent.clone(),
            id: id.to_string(),
            from,
            to,
        };
        match self.call(&request)? {
            Reply::Found { found } => Ok(found),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobRemove`]: whether it was there.
    fn job_remove(&self, agent: &Agent, place: Place, id: &str) -> anyhow::Result<bool> {
        let request = Request::JobRemove {
            agent: agent.clone(),
            place,
            id: id.to_string(),
        };
        match self.call(&request)? {
            Reply::Found { found } => Ok(found),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobNext`].
    fn job_next(&self, agent: &Agent) -> anyhow::Result<Option<u64>> {
        match self.call(&Request::JobNext {
            agent: agent.clone(),
        })? {
            Reply::Next { next } => Ok(next),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobDay`]: the day and how many messages went on it.
    fn job_day(&self, agent: &Agent) -> anyhow::Result<(u64, u32)> {
        match self.call(&Request::JobDay {
            agent: agent.clone(),
        })? {
            Reply::Day { day, sent } => Ok((day, sent)),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobSetDay`].
    fn job_set_day(&self, agent: &Agent, day: u64, sent: u32) -> anyhow::Result<()> {
        let request = Request::JobSetDay {
            agent: agent.clone(),
            day,
            sent,
        };
        match self.call(&request)? {
            Reply::Done => Ok(()),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::JobMark`].
    fn job_mark(&self, agent: &Agent) -> anyhow::Result<u64> {
        match self.call(&Request::JobMark {
            agent: agent.clone(),
        })? {
            Reply::Mark { mark } => Ok(mark),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::FileRead`]: the file's bytes and version, none when
    /// there's none.
    fn file_read(&self, agent: &Agent, path: &str) -> anyhow::Result<Option<(Vec<u8>, u64)>> {
        use base64::Engine as _;
        match self.call(&Request::FileRead {
            agent: agent.clone(),
            path: path.to_string(),
        })? {
            Reply::File { file: None } => Ok(None),
            Reply::File { file: Some(file) } => {
                let data = base64::engine::general_purpose::STANDARD
                    .decode(&file.data)
                    .with_context(|| {
                        format!("{} answered a file that isn't base64", self.name())
                    })?;
                anyhow::ensure!(
                    data.len() as u64 <= file_max(path),
                    "{} answered a file larger than {} bytes",
                    self.name(),
                    file_max(path)
                );
                Ok(Some((data, file.version)))
            }
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::FileList`]: the paths under `folder`.
    fn file_list(&self, agent: &Agent, folder: &str) -> anyhow::Result<Vec<String>> {
        match self.call(&Request::FileList {
            agent: agent.clone(),
            folder: folder.to_string(),
        })? {
            Reply::Files { files } => {
                anyhow::ensure!(
                    files.len() <= FILES_MAX,
                    "{} answered more than {FILES_MAX} paths",
                    self.name()
                );
                Ok(files)
            }
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::FileWrite`]: an error for a path or a size a driver
    /// doesn't keep, never sent.
    fn file_write(
        &self,
        agent: &Agent,
        path: &str,
        data: &[u8],
        version: u64,
    ) -> anyhow::Result<Written> {
        use base64::Engine as _;
        check_file_path(path, false)?;
        check_file_size(path, data.len())?;
        match self.call(&Request::FileWrite {
            agent: agent.clone(),
            path: path.to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(data),
            version,
        })? {
            Reply::Wrote { version } => Ok(Written::Version(version)),
            Reply::Stale { version } => Ok(Written::Stale(version)),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::FileRemove`]: whether it was removed.
    fn file_remove(&self, agent: &Agent, path: &str, version: u64) -> anyhow::Result<bool> {
        check_file_path(path, false)?;
        match self.call(&Request::FileRemove {
            agent: agent.clone(),
            path: path.to_string(),
            version,
        })? {
            Reply::Found { found } => Ok(found),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::FileAppend`]: the file's new version.
    fn file_append(&self, agent: &Agent, path: &str, data: &[u8]) -> anyhow::Result<u64> {
        use base64::Engine as _;
        check_file_path(path, false)?;
        check_file_size(path, data.len())?;
        match self.call(&Request::FileAppend {
            agent: agent.clone(),
            path: path.to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(data),
        })? {
            Reply::Wrote { version } => Ok(version),
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::ToolsRead`]: none when nothing narrowed `thread`.
    fn tools_read(&self, agent: &Agent, thread: &str) -> anyhow::Result<Option<Vec<String>>> {
        match self.call(&Request::ToolsRead {
            agent: agent.clone(),
            thread: thread.to_string(),
        })? {
            Reply::Tools { tools } => {
                if let Some(tools) = &tools {
                    check_tools(tools).with_context(|| format!("{} answered", self.name()))?;
                }
                Ok(tools)
            }
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::ToolsList`]: each conversation narrowed, by its
    /// thread, and what it may use.
    fn tools_list(&self, agent: &Agent) -> anyhow::Result<BTreeMap<String, Vec<String>>> {
        match self.call(&Request::ToolsList {
            agent: agent.clone(),
        })? {
            Reply::Narrowed { narrowed } => {
                for tools in narrowed.values() {
                    check_tools(tools).with_context(|| format!("{} answered", self.name()))?;
                }
                Ok(narrowed)
            }
            other => unexpected(self.name(), &other),
        }
    }

    /// [`Request::ToolsNarrow`]: what `thread` may use now. A driver
    /// answering none, or a tool not in `tools`, has widened it: an
    /// error.
    fn tools_narrow(
        &self,
        agent: &Agent,
        thread: &str,
        tools: &[String],
    ) -> anyhow::Result<Vec<String>> {
        match self.call(&Request::ToolsNarrow {
            agent: agent.clone(),
            thread: thread.to_string(),
            tools: tools.to_vec(),
        })? {
            Reply::Tools { tools: Some(now) } if now.iter().all(|t| tools.contains(t)) => Ok(now),
            Reply::Tools { .. } => bail!("{} widened a conversation's tools", self.name()),
            other => unexpected(self.name(), &other),
        }
    }
}
