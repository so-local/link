//! Final responses stay durable until their original destination accepted them.
use super::*;

const PENDING: &str = "pending";
const DELIVERY_LOCKS: &str = "delivery-locks";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    id: link_connection_contract::TransportEventId,
    thread: String,
    owed: Owed,
    text: String,
    receipt: Option<String>,
}

link_files::specification!(Answer, include_str!("owed.schema.toml"));

impl WayIn {
    pub(super) async fn answer_received(
        &self,
        thread: &str,
        owed: &Owed,
        text: &str,
        receipt: Option<&str>,
    ) -> anyhow::Result<()> {
        let id = match receipt {
            Some(receipt) => link_digest::sha256(&serde_json::to_vec(&(thread, receipt))?),
            None => link_files::random_token()?,
        };
        let mut answer = Answer {
            id: link_connection_contract::TransportEventId::new(id).map_err(anyhow::Error::msg)?,
            thread: thread.into(),
            owed: owed.clone(),
            text: text.into(),
            receipt: receipt.map(str::to_owned),
        };
        let _lock = self.delivery_lock()?;
        if let Some(store) = &self.interactions {
            use link_interactions::control::Lifecycle;
            anyhow::ensure!(
                store
                    .work_status(&self.interaction_agent, thread)?
                    .is_none_or(|s| !matches!(
                        s.lifecycle,
                        Lifecycle::Deleting | Lifecycle::Deleted
                    )),
                "the answer's work is being deleted"
            );
        }
        let path = self.owed.join(PENDING).join(format!("{}.json", answer.id));
        if path.try_exists()? {
            let dir = link_files::nofollow::Dir::open_unlinked(&self.owed.join(PENDING))?;
            let retained: Answer = link_files::saved::parse_json(&dir.read(
                std::ffi::OsStr::new(&format!("{}.json", answer.id)),
                WAY_IN.action_max,
            )?)?;
            anyhow::ensure!(
                retained.id == answer.id
                    && retained.thread == answer.thread
                    && retained.text == answer.text
                    && retained.receipt == answer.receipt,
                "a result receipt was reused with another answer"
            );
            // A retry keeps the original destination, even after a later turn changed it.
            answer = retained;
        } else {
            link_files::write_private(&path, &link_files::saved::print_json(&answer)?)?;
        }
        drop(_lock);
        self.finish_parent(self.send_pending(&answer).await?);
        Ok(())
    }

    fn delivery_lock(&self) -> anyhow::Result<link_files::nofollow::Locked> {
        let path = self.owed.join(PENDING);
        link_files::create_private_dir(&path)?;
        Ok(link_files::nofollow::Dir::open_unlinked(&path)?.lock(libc::LOCK_EX)?)
    }

    fn sending_lock(&self, thread: &str) -> anyhow::Result<Option<std::fs::File>> {
        let path = self.owed.join(DELIVERY_LOCKS);
        link_files::create_private_dir(&path)?;
        let dir = link_files::nofollow::Dir::open_unlinked(&path)?;
        let file = dir.open_file(
            std::ffi::OsStr::new(&link_digest::sha256(thread.as_bytes())),
            libc::O_CREAT | libc::O_RDWR,
            0o600,
        )?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn pending_answers(&self) -> anyhow::Result<Vec<Answer>> {
        let path = self.owed.join(PENDING);
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(err) => return Err(err.into()),
        };
        let dir = link_files::nofollow::Dir::open_unlinked(&path)?;
        let mut answers = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }
            let bytes = dir.read(&entry.file_name(), WAY_IN.action_max)?;
            let answer: Answer = link_files::saved::parse_json(&bytes)?;
            anyhow::ensure!(
                entry.file_name() == std::ffi::OsStr::new(&format!("{}.json", answer.id)),
                "invalid pending answer identity"
            );
            answers.push(answer);
        }
        answers.sort_by(|a, b| {
            a.thread
                .cmp(&b.thread)
                .then_with(|| receipt_seq(a).cmp(&receipt_seq(b)))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(answers)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "poll the cross-process delivery lock without blocking an asynchronous worker"
    )]
    async fn send_pending(&self, answer: &Answer) -> anyhow::Result<Option<Taken>> {
        let _sending = loop {
            anyhow::ensure!(
                !self.completion_fenced(&answer.thread)?,
                "the answer is retained while its work is fenced"
            );
            if let Some(held) = self.sending_lock(&answer.thread)? {
                break held;
            }
            tokio::time::sleep(RETRY_FIRST).await;
        };
        if !self
            .owed
            .join(PENDING)
            .join(format!("{}.json", answer.id))
            .try_exists()?
        {
            return Ok(None);
        }
        anyhow::ensure!(
            !self.completion_fenced(&answer.thread)?,
            "the answer is retained while its work is fenced"
        );
        let parent = self
            .send_answer(&answer.thread, &answer.owed, &answer.text, &answer.id)
            .await?;
        if let Some(store) = &self.interactions {
            store.result_delivered(&self.interaction_agent, &answer.thread)?;
        }
        if let Some(receipt) = &answer.receipt {
            link_connection_contract::recovered_done(&self.endpoint, vec![receipt.clone()]).await?;
        }
        let dir = link_files::nofollow::Dir::open_unlinked(&self.owed.join(PENDING))?;
        dir.remove_file(std::ffi::OsStr::new(&format!("{}.json", answer.id)))?;
        Ok(parent)
    }

    pub(super) async fn retry_answers(&self) -> anyhow::Result<()> {
        let answers = {
            let _lock = self.delivery_lock()?;
            self.pending_answers()?
        };
        let mut parents = Vec::new();
        for answer in answers {
            match self.send_pending(&answer).await {
                Ok(Some(parent)) => parents.push(parent),
                Ok(None) => {}
                Err(err) => {
                    link_format::daemon::warn!(
                        "an answer remains queued for its original destination: {err:#}"
                    );
                }
            }
        }
        for parent in parents {
            self.finish_parent(Some(parent));
        }
        Ok(())
    }

    fn finish_parent(&self, taken: Option<Taken>) {
        if let Some(taken) = taken {
            let bridge = self.delivery_copy();
            tokio::spawn(async move {
                if let Err(err) = bridge.finish(taken).await {
                    link_format::daemon::warn!(
                        "a retained parent input has not finished processing: {err:#}"
                    );
                }
            });
        }
    }

    pub(super) fn delivery_copy(&self) -> Self {
        Self {
            interaction_agent: self.interaction_agent.clone(),
            interactions: self.interactions.clone(),
            keys: self.keys.clone(),
            owed: self.owed.clone(),
            endpoint: self.endpoint.duplicate(),
            terms: self.terms.clone(),
            connections: self
                .connections
                .iter()
                .map(|(id, (path, bridge))| (id.clone(), (path.clone(), bridge.duplicate())))
                .collect(),
            teams: self.teams.clone(),
            held: self.held.clone(),
            received: self.received.clone(),
            per_day: self.per_day,
            hops: self.hops,
            chains: self.chains.clone(),
            counting: self.counting.clone(),
        }
    }

    pub(super) fn delete_deliveries(&self, thread: &str) -> anyhow::Result<()> {
        let _lock = self.delivery_lock()?;
        let _sending = self
            .sending_lock(thread)?
            .within("a delivery is still in progress; deletion remains pending")?;
        for answer in self
            .pending_answers()?
            .into_iter()
            .filter(|a| a.thread == thread)
        {
            let dir = link_files::nofollow::Dir::open_unlinked(&self.owed.join(PENDING))?;
            dir.remove_file(std::ffi::OsStr::new(&format!("{}.json", answer.id)))?;
        }
        match std::fs::remove_file(self.owed_path(thread)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) fn delete_selected_deliveries(&self) -> anyhow::Result<()> {
        let Some(store) = &self.interactions else {
            return Ok(());
        };
        for member in store.pending_deletions_for(&self.interaction_agent, "bridge")? {
            self.delete_deliveries(&member.work.session)?;
            store.deletion_applied(&member, "bridge")?;
        }
        Ok(())
    }
}

fn receipt_seq(answer: &Answer) -> u64 {
    answer
        .receipt
        .as_deref()
        .and_then(|r| r.rsplit_once('/'))
        .and_then(|(_, seq)| seq.parse().ok())
        .unwrap_or_default()
}
