//! Dispatching a tool through an opened toolbox.

use super::*;

impl Toolbox<'_> {
    /// Calls `call`: its answer. A tool the turn doesn't have, a refusal
    /// and an error are all answers, for the model to read.
    pub async fn call(
        &self,
        call: &Call,
        cancelled: tokio::sync::watch::Receiver<bool>,
        meta: &Value,
    ) -> Answered {
        self.call_observed(
            call,
            cancelled,
            meta,
            &crate::tools::observation::Unobserved,
        )
        .await
        .unwrap_or_else(|err| {
            Answered::unknown(&format!("Recording the tool result failed: {err:#}"))
        })
    }

    pub async fn call_observed(
        &self,
        call: &Call,
        cancelled: tokio::sync::watch::Receiver<bool>,
        meta: &Value,
        observer: &dyn Observer,
    ) -> anyhow::Result<Answered> {
        let mut answer = self.call_inner(call, cancelled, meta, observer).await?;
        answer.read_authority = self.read_authority(call);
        observer.received(&mut answer)?;
        if let Some(notice) = super::answers::failure_notice(call, &answer) {
            let mut ids = crate::said::Ids::agent(self.agent.id()).call(&call.id);
            if let Some(thread) = meta["link.local/thread"].as_str() {
                ids = ids.chat(thread);
            }
            crate::warn!(ids => "{notice}");
        }
        Ok(answer)
    }

    async fn call_inner(
        &self,
        call: &Call,
        mut cancelled: tokio::sync::watch::Receiver<bool>,
        meta: &Value,
        observer: &dyn Observer,
    ) -> anyhow::Result<Answered> {
        let Some((_, index, own)) = self.tools.iter().find(|(t, _, _)| t.name == call.name) else {
            return Ok(Answered::failed(&format!(
                "There is no tool {} this turn.",
                call.name
            )));
        };
        let arguments = match normalized_arguments(&call.arguments) {
            Ok(arguments) => arguments,
            Err(_) => return Ok(Answered::failed("Its arguments must be a JSON object.")),
        };
        let (server, started) = &self.servers[*index];
        // The server's own opinions, its table in the agent's spec
        // (`[tool.<name>]`), for a tool that takes them (web's `fetch`).
        let mut meta = meta.clone();
        meta["link.local/call"] = json!(call.id);
        if let (Value::Object(meta), Ok(own @ Value::Object(_))) = (
            &mut meta,
            serde_json::to_value(self.opinions.tool(&server.name)),
        ) {
            meta.insert(SPEC_META.into(), own);
        }
        let params = json!({"name": own, "arguments": arguments, "_meta": meta});
        let mut started = started.lock().await;
        let mut fresh = false;
        if matches!(*started, Started::Not) {
            let starting = Session::start_new(&self.client, server, self.timeout);
            *started = match tokio::time::timeout(self.timeout, starting).await {
                Ok(Ok(session)) => {
                    fresh = true;
                    let session = Arc::new(session);
                    if server.kept {
                        self.kept.put(
                            &self.agent,
                            server,
                            session.clone(),
                            life(&self.opinions, &[]),
                        );
                    }
                    Started::Open(session)
                }
                Ok(Err(err)) => Started::Failed(format!("{err:#}")),
                Err(_) => Started::Failed("it took too long to start".into()),
            };
        }
        let session = match &*started {
            Started::Open(session) => session.clone(),
            Started::Failed(why) => {
                return Ok(Answered::failed(&format!(
                    "The tool {} is unavailable: {why}",
                    server.name
                )));
            }
            Started::Not => {
                return Ok(Answered::failed(&format!(
                    "The tool {} didn't start.",
                    server.name
                )));
            }
        };
        // Let go while it's called: a round's other calls to this server
        // run at once (MCP takes requests at once in a session).
        drop(started);
        // Its id once sent (0 before), so a turn asked to end can cancel
        // it (MCP's `notifications/cancelled`).
        let sent = AtomicU64::new(0);
        let cancel = |reason: &str| {
            let id = sent.load(Ordering::Relaxed);
            let cancelled = json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
                "params": {"requestId": id, "reason": reason}});
            let session = session.clone();
            async move {
                if id == 0 {
                    return;
                }
                let ending = session.post(&cancelled);
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), ending).await;
            }
        };
        // As long as its spec lets it (`[tools] timeout`).
        let most = self.call_timeout(server);
        let calling = async {
            if *cancelled.borrow() {
                return Err(
                    NotDispatched(anyhow::anyhow!("The turn ended before dispatch.")).into(),
                );
            }
            observer
                .dispatching(Dispatch {
                    server: &server.name,
                    operation: own,
                    arguments: &params["arguments"],
                })
                .map_err(NotDispatched)?;
            tokio::select! {
                called = tokio::time::timeout(most, session.request("tools/call", params, &sent)) => {
                    match called {
                        Ok(called) => called,
                        Err(_) => {
                            cancel("it took too long").await;
                            Err(TooLong(most).into())
                        }
                    }
                }
                _ = crate::model::asked_to_end(&mut cancelled) => {
                    cancel("its turn was interrupted").await;
                    Err(Interrupted.into())
                }
            }
        };
        let receiving = async {
            let called = calling.await;
            let mut answer = self.received_answer(server, &session, *index, called).await;
            answer.read_authority = self.read_authority(call);
            observer.received(&mut answer)?;
            Ok(answer)
        };
        // What it offers now, for the turns after this one: asked
        // beside the call, not before it; what it listed before, if
        // it takes too long.
        let (called, offered) = match fresh {
            true => {
                let listing = tokio::time::timeout(self.timeout, session.list());
                let (called, offered) = tokio::join!(receiving, listing);
                (called, offered.ok().and_then(Result::ok))
            }
            false => (receiving.await, None),
        };
        if let Some(offered) = offered {
            self.listed.put(&self.agent, server, offered);
        }
        called
    }

    async fn received_answer(
        &self,
        server: &Server,
        session: &Arc<Session>,
        index: usize,
        called: anyhow::Result<Value>,
    ) -> Answered {
        match called {
            Ok(result) => {
                match super::super::answers::checked(&result, self.answer_max(&server.name)) {
                    Ok(answered) => answered,
                    Err(err) => Answered::unknown(&format!("{err:#}")),
                }
            }
            Err(err) => {
                if err.is::<NotDispatched>() {
                    return Answered::failed(&format!("{err:#}"));
                }
                if let Some(error) = err.downcast_ref::<Refused>() {
                    return Answered {
                        observation: Observation::RemoteError {
                            code: error.code,
                            message: error.message.clone(),
                            data: error.data.clone(),
                        },
                        ..Answered::failed(&format!("{err:#}"))
                    };
                }
                // Not an answer: its server may be gone, so the
                // next call starts another session.
                if !err.is::<Refused>() && !err.is::<Interrupted>() && !err.is::<TooLong>() {
                    self.kept.forget(&self.agent, server, session);
                    let mut started = self.servers[index].1.lock().await;
                    if matches!(&*started, Started::Open(open) if Arc::ptr_eq(open, session)) {
                        *started = Started::Not;
                    }
                }
                Answered::unknown(&format!("{err:#}"))
            }
        }
    }
}

#[derive(Debug)]
struct NotDispatched(anyhow::Error);

impl std::fmt::Display for NotDispatched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Request not dispatched: {:#}", self.0)
    }
}

impl std::error::Error for NotDispatched {}

fn normalized_arguments(arguments: &Value) -> anyhow::Result<Value> {
    match arguments {
        Value::Object(_) => Ok(arguments.clone()),
        Value::Null => Ok(json!({})),
        _ => anyhow::bail!("tool arguments must be a JSON object"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_captures_actual_normalized_arguments_without_changing_model_intent() {
        let intent = Value::Null;
        let actual = normalized_arguments(&intent).unwrap();
        let captured = crate::tools::dispatch(Dispatch {
            server: "file",
            operation: "read",
            arguments: &actual,
        });
        assert_eq!(intent, Value::Null);
        assert_eq!(captured.arguments.retained(), Some(&json!({})));
        assert_eq!(captured.server, "file");
        assert_eq!(captured.operation, "read");
        assert!(!serde_json::to_string(&captured).unwrap().contains("_meta"));
        assert!(normalized_arguments(&json!(["invalid"])).is_err());
    }
}
