//! The endpoint's checks: its key, senders and agents, its answers,
//! threads, a turn cut short, its tool calls and narrowed tools.

use super::*;

impl Kit {
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    pub(super) fn endpoint_checks(&self, url: &str, contract: &Contract) -> anyhow::Result<()> {
        self.runtime.block_on(async {
            let client = reqwest::Client::builder().no_proxy().build()?;
            let owned = url.to_string();
            let ask_as = |agent: Option<&'static str>,
                          key: Option<&'static str>,
                          thread: &'static str,
                          text: &'static str| {
                let url = owned.clone();
                async move {
                    let saying = Saying {
                        agent,
                        key,
                        thread,
                        text,
                        ..Saying::default()
                    };
                    say(&url, saying).await
                }
            };
            // As Link's bridges send it: naming the agent.
            let ask = |key: Option<&'static str>, thread: &'static str, text: &'static str| {
                ask_as(Some(AGENT), key, thread, text)
            };

            for key in [None, Some("another-key")] {
                let status = ask(key, "conformance:1", "Hello").await?.status;
                ensure!(
                    status == 401 || status == 403,
                    "with {key:?} for its key, the endpoint answered {status}, not 401"
                );
            }
            ensure!(
                self.seen().asked.is_empty(),
                "a request without the key reached the model"
            );

            if contract.proof {
                let challenge = "0123456789abcdef".repeat(4);
                let origin = url.split("/v1/").next().unwrap_or(url);
                let origin = origin.trim_end_matches(link_connection_contract::acp::ACP_PATH);
                let proof = client
                    .get(format!("{origin}{}", harness::PROOF_PATH))
                    .header(harness::CHALLENGE_HEADER, &challenge)
                    .header(link_connection_contract::AGENT_HEADER, AGENT)
                    .send()
                    .await?;
                let proof = proof
                    .headers()
                    .get(harness::PROOF_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                ensure!(
                    harness::proof_matches(&KEY.to_string().into(), &challenge, &proof),
                    "its contract says proof, but it didn't prove it holds the key"
                );
            }

            // Answered as it streams, as Link's connections ask.
            let said = ask(Some(KEY), "conformance:1", "Hello").await?;
            ensure!(said.ok(), "a request was answered {}", said.status);
            ensure!(
                said.text.ends_with("the last: Hello"),
                "the answer isn't the model's: {:?}",
                said.text
            );

            // Again, and the thread kept.
            let said = ask(Some(KEY), "conformance:1", "Again").await?;
            ensure!(said.ok(), "a request was answered {}", said.status);
            ensure!(
                said.text.ends_with("the last: Again"),
                "the answer isn't the model's: {:?}",
                said.text
            );
            let sent = self.last_asked();
            ensure!(
                sent.contains("Hello") && sent.contains("the last: Hello"),
                "the model wasn't sent the thread before: {sent}"
            );

            // Another thread knows nothing of the first.
            let other = ask(Some(KEY), "conformance:2", "Other").await?;
            ensure!(other.ok(), "a request was answered {}", other.status);
            let sent = self.last_asked();
            ensure!(
                sent.contains("Other") && !sent.contains("Hello"),
                "another thread was sent the first's messages: {sent}"
            );

            // Another agent's request, with the key: refused before the
            // model sees it. One naming no agent is its own agent's.
            if contract.agent {
                let asked = self.seen().asked.len();
                let status = ask_as(Some(OTHER_AGENT), Some(KEY), "conformance:5", "Hello")
                    .await?
                    .status;
                ensure!(
                    status == 404,
                    "its contract says agent, but a request for an agent it doesn't run \
                     ({OTHER_AGENT}) was answered {status}, not 404"
                );
                ensure!(
                    self.seen().asked.len() == asked,
                    "a request for an agent it doesn't run reached the model"
                );
                let none = ask_as(None, Some(KEY), "conformance:5", "Hello").await?;
                ensure!(
                    none.ok(),
                    "a request naming no agent was answered {}, not as its own agent's",
                    none.status
                );
            }

            // A message from someone who isn't the owner: answered, the
            // model told who; one from no sender refused.
            if contract.from {
                let from_in = |thread: &'static str, sender: &'static str, name: Option<&'static str>, text: &'static str| {
                    say(
                        url,
                        Saying {
                            agent: Some(AGENT),
                            key: Some(KEY),
                            thread,
                            text,
                            from: Some(sender),
                            from_name: name,
                            hops: 1,
                            ..Saying::default()
                        },
                    )
                };
                let from = |sender: &'static str| from_in("conformance:6", sender, None, "Time to stretch.");
                let answered = from("schedule:abcdef123456").await?;
                ensure!(
                    answered.ok(),
                    "its contract says from, but a message from a schedule was answered {}",
                    answered.status
                );
                let sent = self.last_asked();
                ensure!(
                    sent.contains("Time to stretch.") && sent.contains("abcdef123456"),
                    "its contract says from, but the model wasn't told who a message is from: \
                     {sent}"
                );
                // A teammate's, in its conversation with this agent: the
                // model told it's that agent's.
                let teammate =
                    from_in("agent-fedcba654321", "agent:fedcba654321", None, "Time to stretch.").await?;
                ensure!(
                    teammate.ok(),
                    "its contract says from, but a teammate's message was answered {}",
                    teammate.status
                );
                let sent = self.last_asked();
                ensure!(
                    sent.contains("fedcba654321"),
                    "its contract says from, but the model wasn't told a teammate's message is \
                     its: {sent}"
                );
                // A background command telling how it ended.
                let ended = from("run:fedcba654321").await?;
                ensure!(
                    ended.ok(),
                    "its contract says from, but a message from a background command was \
                     answered {}",
                    ended.status
                );
                ensure!(
                    self.last_asked().contains("fedcba654321"),
                    "its contract says from, but the model wasn't told a message is from a \
                     background command"
                );
                // Someone in a shared chat, by their name on it.
                let person = from_in("conformance:6", PERSON, Some(PERSON_NAME), "Book the room.").await?;
                ensure!(
                    person.ok(),
                    "its contract says from, but a message from someone in a shared chat was \
                     answered {}",
                    person.status
                );
                let sent = self.last_asked();
                ensure!(
                    sent.contains("Book the room.") && sent.contains(PERSON_NAME),
                    "its contract says from, but the model wasn't told the name of who's \
                     speaking in a shared chat: {sent}"
                );
                let asked = self.seen().asked.len();
                // A name that isn't one: a control character in ACP's JSON.
                let broken = "Al\u{7}e";
                for (sender, name) in [(PERSON, broken), ("schedule:abcdef123456", "Ada")] {
                    let status = from_in("conformance:6", sender, Some(name), "Hi.").await?.status;
                    ensure!(
                        status == 400,
                        "a sender's name {name:?} beside {sender} was answered {status}, not 400"
                    );
                }
                ensure!(
                    self.seen().asked.len() == asked,
                    "a message with a name that isn't a person's reached the model"
                );
                let status = from("owner:me").await?.status;
                ensure!(
                    status == 400,
                    "a message from no sender a message may be from was answered {status}, \
                     not 400"
                );
                ensure!(
                    self.seen().asked.len() == asked,
                    "a message from no sender reached the model"
                );
            }

            // The answers of turns finished after a crash: handed over to
            // the key only, as a list (none here: nothing crashed), once,
            // or in two phases.
            if contract.recover {
                let (status, _) = saying::recover(url, None, json!({})).await?;
                ensure!(
                    status == 401,
                    "its contract says recover, but asking for recovered answers without the \
                     key was answered {status}, not 401"
                );
                let (status, body) = saying::recover(url, Some(KEY), json!({})).await?;
                ensure!(
                    status == 200 && body["answers"].is_array(),
                    "its contract says recover, but asking for recovered answers was answered \
                     {status} {body}, not {{\"answers\": [...]}}"
                );
                // In two phases: each answer with its id, kept until it's
                // said done; an id it doesn't know is no error.
                let (status, body) =
                    saying::recover(url, Some(KEY), json!({"two_phase": true})).await?;
                let answers: Vec<link_connection_contract::Recovered> =
                    serde_json::from_value(body["answers"].clone()).unwrap_or_default();
                ensure!(
                    status == 200
                        && body["answers"].is_array()
                        && answers.iter().all(|a| a.id.is_some()),
                    "its contract says recover, but asked in two phases it answered {status} \
                     {body}, not each answer with its id"
                );
                let (status, _) =
                    saying::recover(url, Some(KEY), json!({"done": ["conformance"]})).await?;
                ensure!(
                    status == 200,
                    "its contract says recover, but being told an answer went was answered \
                     {status}"
                );
            }

            // A turn cut short: its caller goes once the model is asked,
            // before it answers. The turn runs on, and the next message
            // finds its answer in the thread.
            let cut = tokio::spawn(ask(Some(KEY), "conformance:3", SLOW_ASK));
            self.until("the model to be asked the message cut short", |seen| {
                seen.asked.iter().any(|a| last_text(a) == SLOW_ASK)
            })
            .await?;
            cut.abort();
            self.until("the model to answer the message cut short", |seen| {
                seen.answered.iter().any(|a| a == SLOW_ASK)
            })
            .await?;
            // Time to keep what it was sent.
            tokio::time::sleep(Duration::from_secs(1)).await;
            let after = ask(Some(KEY), "conformance:3", "After").await?;
            ensure!(
                after.ok(),
                "a request after one cut short was answered {}",
                after.status
            );
            let sent = self.last_asked();
            ensure!(
                sent.contains(&format!("the last: {SLOW_ASK}")),
                "a turn cut short, its caller gone, left no answer in the thread: {sent}"
            );

            // A tool, called through the bridge.
            let called = ask(Some(KEY), "conformance:4", TOOL_ASK).await?;
            ensure!(
                called.ok(),
                "a request to call a tool was answered {}",
                called.status
            );
            let text = called.text;
            let named = {
                let seen = self.seen();
                let offered = seen
                    .asked
                    .iter()
                    .find(|a| last_text(a) == TOOL_ASK)
                    .is_some_and(|a| tool_named(a).is_some());
                ensure!(
                    offered,
                    "the model wasn't offered the kit's tool ({TOOL}, in the settings' tools and at \
                     $LNK_TOOL_KIT), or was never asked"
                );
                ensure!(
                    !seen.called.is_empty(),
                    "the model called the kit's tool, but the tool was never called"
                );
                ensure!(
                    seen.called.iter().all(|c| c["name"] == TOOL_CALL),
                    "the tool was called by another name: {:?}",
                    seen.called
                );
                ensure!(
                    text.contains(STAMP),
                    "the tool's answer didn't reach the model, or its answer the caller: {text:?}"
                );
                seen
                    .asked
                    .iter()
                    .find(|a| last_text(a) == TOOL_ASK)
                    .and_then(tool_named)
                    .unwrap_or_default()
            };
            if contract.tools {
                self.narrowed_checks(url, &named).await?;
            }
            anyhow::Ok(())
        })
    }

    /// A thread narrowed to some tools (`Said::tools`), for a harness whose
    /// contract says `tools`: `named` is its name for the kit's tool.
    async fn narrowed_checks(&self, url: &str, named: &str) -> anyhow::Result<()> {
        let ask = |tools: Option<&str>| {
            let saying = Saying {
                agent: Some(AGENT),
                key: Some(KEY),
                thread: "conformance:7",
                text: TOOL_ASK,
                ..Saying::default()
            };
            let tools = tools.map(str::to_string);
            async move {
                say(
                    url,
                    Saying {
                        tools: tools.as_deref(),
                        ..saying
                    },
                )
                .await
            }
        };
        // Narrowed to the kit's tool: still offered, and called saying so.
        let called = self.seen().called.len();
        let answered = ask(Some(named)).await?;
        ensure!(
            answered.ok(),
            "its contract says tools, but a message narrowing its thread to {named} was \
             answered {}",
            answered.status
        );
        let said = {
            let seen = self.seen();
            ensure!(
                seen.called.len() > called,
                "its contract says tools, but a thread narrowed to {named} didn't call it"
            );
            seen.called[seen.called.len() - 1]["_meta"]["link.local/tools"].clone()
        };
        ensure!(
            said.as_array()
                .is_some_and(|tools| tools.iter().any(|t| t == named)),
            "its contract says tools, but a call didn't say its thread's tools in its _meta \
             (link.local/tools): {said}"
        );
        // Narrowed to none; then neither a wider list nor none widens it.
        for (tools, what) in [
            (Some(""), "narrowed to no tools"),
            (Some(named), "narrowed to none, then given a wider list,"),
            (
                None,
                "narrowed to none, then sent a message without a list,",
            ),
        ] {
            let called = self.seen().called.len();
            let answered = ask(tools).await?;
            ensure!(
                answered.ok(),
                "its contract says tools, but a message to a thread {what} was answered {}",
                answered.status
            );
            let (offered, calls) = {
                let seen = self.seen();
                (seen.asked.last().and_then(tool_named), seen.called.len())
            };
            ensure!(
                offered.is_none() && calls == called,
                "its contract says tools, but a thread {what} was offered the kit's tool"
            );
        }
        let asked = self.seen().asked.len();
        let status = ask(Some("not a tool")).await?.status;
        ensure!(
            status == 400,
            "Said::tools that isn't a list of tools was answered {status}, not 400"
        );
        ensure!(
            self.seen().asked.len() == asked,
            "Said::tools that isn't a list of tools reached the model"
        );
        Ok(())
    }
}
