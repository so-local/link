//! A question answered from what a chat's next turn would be sent,
//! without taking a turn or keeping anything: `/chat:ask`.

use super::*;

/// The window a turn walking on from `mark` sends of `events`: the
/// summary it starts with, if any, and the events after it.
fn shown(events: &[Event], mark: &window::Mark) -> (Option<Event>, Vec<Event>) {
    let from = events
        .iter()
        .position(|e| e.seq == mark.seq)
        .unwrap_or_default();
    let mut start = from + window::walk(&events[from..], mark).0;
    let head = compact::head(events).cloned();
    if let Some(covers) = head.as_ref().map(|h| h.covers)
        && let Some(after) = events.iter().position(|e| e.seq >= covers)
    {
        start = start.max(after);
    }
    let shown = events[start.min(events.len())..]
        .iter()
        .filter(|e| e.role != event::SUMMARY)
        .cloned()
        .collect();
    (head, shown)
}

/// The question `/chat:ask` asks, after the conversation as it stands.
pub const BTW: &str = "[Link: your owner asks this on the side, while the conversation goes on. \
Answer it from what you know and the conversation above, briefly. You can't call tools for it, \
and neither it nor your answer is kept.]";

impl Harness {
    /// `/chat:ask`'s answer to `question`: the model asked with the
    /// conversation as it stands and the question after it, no tool
    /// callable, while a turn runs too. Nothing is kept.
    pub(crate) async fn btw(
        &self,
        agent: &Agent,
        thread: &Thread,
        question: &str,
    ) -> anyhow::Result<String> {
        let (config, read) = self.conversing_read(agent, thread)?;
        let toolbox = self.toolbox(agent, &config).await;
        let prompt = match self.instructions(agent, thread, &config, &toolbox) {
            Ok(prompt) => prompt,
            Err(err) => {
                toolbox.close().await;
                return Err(err);
            }
        };
        let prepared = self
            .prepare(agent, thread, &config, &prompt, &toolbox, Some(&read))
            .await;
        toolbox.close().await;
        let prepared = prepared?;
        let (head, mut said) = window_of(&prepared);
        said.splice(0..0, head);
        said.push(Event {
            seq: said.last().map_or(1, |e| e.seq + 1),
            ..Event::user(&format!("{BTW}\n\n{question}"))
        });
        let parts = |part: &event::Part| self.store.part(agent, thread, &part.file);
        let messages = off_runtime(|| model::messages(&said, false, &parts));
        let ask = &prepared.ask;
        // Kept apart from the chat's own calls, as its work.
        let chat = format!("{}/btw", thread.name());
        let prompt = model::Prompt {
            system: ask.instructions,
            messages: &messages,
            tools: &[],
            no_calls: true,
            answer: ask.answer,
            window: ask.window,
            price: ask.price,
            above: &ask.above,
            thinking: ask.thinking,
            fast: ask.fast,
            chat: Some(&chat),
            spans: ask.spans,
            parent: Some(thread.name()),
            cache: None,
        };
        let (unread, _) = mpsc::channel(1);
        let (_, cancelled) = watch::channel(false);
        let answer = crate::fallback::answer(
            &self.client,
            &ask.config.model,
            ask.second.as_ref(),
            &self.off,
            &prompt,
            &mut None,
            &unread,
            false,
            cancelled,
        )
        .await?;
        Ok(answer.text)
    }
}

/// The window `prepared` would send: the summary it starts with, if any,
/// and the events after it.
fn window_of(prepared: &Prepared<'_>) -> (Option<Event>, Vec<Event>) {
    let mark = prepared.mark.clone().or_else(|| {
        prepared
            .events
            .first()
            .map(|first| window::Mark::at(first, prepared.budget))
    });
    match &mark {
        Some(mark) => shown(&prepared.events, mark),
        None => (None, Vec::new()),
    }
}
