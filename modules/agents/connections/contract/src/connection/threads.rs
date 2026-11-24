use super::*;
use link_format::{Exit, Report};

pub use link_model_contract::chat::lines::SseLines;

/// The most threads [`per_thread`] answers at once: each is a model's
/// stream, its tools' sessions and its edits against the platform's
/// limits.
pub const MAX_ANSWERING: usize = 4;

/// Answers `messages` one at a time per thread, and threads side by side
/// ([`MAX_ANSWERING`] at once, the rest in turn): each thread's in the
/// order they came, while a long answer in one chat holds up no other.
/// `thread` names a message's thread (as the harness takes it,
/// `telegram:42`), and `answer` answers one.
pub async fn per_thread<M, F, Fut>(
    messages: tokio::sync::mpsc::UnboundedReceiver<M>,
    thread: impl Fn(&M) -> String,
    answer: F,
) where
    F: Fn(M) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    per_thread_replying(messages, thread, |_| None, answer).await
}

/// [`per_thread`], but a message whose `text` could answer a question
/// for the owner ([`link_interactions::reply::code_reply`]) is answered at once,
/// beside its thread's turn: that turn may be waiting for it.
pub async fn per_thread_replying<M, F, Fut>(
    messages: tokio::sync::mpsc::UnboundedReceiver<M>,
    thread: impl Fn(&M) -> String,
    text: impl Fn(&M) -> Option<&str>,
    answer: F,
) where
    F: Fn(M) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    per_thread_interacting(
        messages,
        thread,
        |m| {
            text(m).is_some_and(|text| {
                link_interactions::reply::code_reply(text).is_some()
                    || link_interactions::owner::is_command(text)
            })
        },
        answer,
    )
    .await
}

pub async fn per_thread_interacting<M, F, Fut>(
    mut messages: tokio::sync::mpsc::UnboundedReceiver<M>,
    thread: impl Fn(&M) -> String,
    urgent: impl Fn(&M) -> bool,
    answer: F,
) where
    F: Fn(M) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    use futures_util::StreamExt;
    use futures_util::stream::FuturesUnordered;
    use std::collections::{HashMap, VecDeque};

    // Each thread with messages to answer, in order; the threads waiting
    // for a turn at being answered; and those answered or waiting.
    let mut waiting: HashMap<String, VecDeque<M>> = HashMap::new();
    let mut ready: VecDeque<String> = VecDeque::new();
    let mut answering = FuturesUnordered::new();
    let mut replies = FuturesUnordered::new();
    let run = |thread: String, message: M| {
        let answered = answer(message);
        async move {
            answered.await;
            Some(thread)
        }
    };
    // A reply, answered beside the queue: its end frees no thread.
    let reply = |message: M| answer(message);
    // Up to [`MAX_ANSWERING`] threads at once, the rest waiting in turn.
    let fill = |waiting: &mut HashMap<String, VecDeque<M>>,
                ready: &mut VecDeque<String>,
                answering: &mut FuturesUnordered<_>| {
        while answering.len() < MAX_ANSWERING
            && let Some(thread) = ready.pop_front()
        {
            if let Some(message) = waiting.get_mut(&thread).and_then(VecDeque::pop_front) {
                answering.push(run(thread, message));
            }
        }
    };
    // A thread answered: its next message waits for a turn, or it's done.
    let done = |thread: String,
                waiting: &mut HashMap<String, VecDeque<M>>,
                ready: &mut VecDeque<String>| {
        if waiting.get(&thread).is_some_and(|queue| !queue.is_empty()) {
            ready.push_back(thread);
        } else {
            waiting.remove(&thread);
        }
    };
    loop {
        tokio::select! {
            message = messages.recv() => {
                let Some(message) = message else { break };
                if urgent(&message) {
                    replies.push(reply(message));
                    continue;
                }
                let key = thread(&message);
                match waiting.get_mut(&key) {
                    Some(queue) => queue.push_back(message),
                    None => {
                        waiting.insert(key.clone(), VecDeque::from([message]));
                        ready.push_back(key);
                    }
                }
            }
            Some(finished) = answering.next(), if !answering.is_empty() => {
                if let Some(finished) = finished {
                    done(finished, &mut waiting, &mut ready);
                }
            }
            Some(()) = replies.next(), if !replies.is_empty() => {}
        }
        fill(&mut waiting, &mut ready, &mut answering);
    }
    // No more to come: what's begun is finished, the turns and the
    // replies together, since a turn may be waiting for a reply.
    fill(&mut waiting, &mut ready, &mut answering);
    loop {
        tokio::select! {
            Some(finished) = answering.next(), if !answering.is_empty() => {
                if let Some(finished) = finished {
                    done(finished, &mut waiting, &mut ready);
                }
                fill(&mut waiting, &mut ready, &mut answering);
            }
            Some(()) = replies.next(), if !replies.is_empty() => {}
            else => break,
        }
    }
}

/// Refuses bot `id` when another agent has it: each agent needs its own,
/// made as `make_another` says.
pub fn check_taken(id: &str, taken: &[Taken], make_another: &str) -> Result<(), Report> {
    use link_format::{Foreign, Name, cmd, phrase, prose, sentence};
    let Some(other) = taken.iter().find(|t| t.id == id) else {
        return Ok(());
    };
    let headline = match &other.machine {
        Some(machine) => sentence!(
            "That bot is agent {}'s already, running on {}.",
            Name::new("agent", link_format::clean(&other.agent)),
            Name::new("installation", link_format::clean(machine))
        ),
        None => sentence!(
            "That bot is agent {}'s already.",
            Name::new("agent", link_format::clean(&other.agent))
        ),
    };
    Err(Report::error(headline)
        .why(prose!(
            "Each agent needs its own bot: {}.",
            Foreign::new(make_another)
        ))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk agent connect <connection>"),
            phrase!("connect again with a bot of its own"),
        )
        .next(
            cmd!(
                "lnk agent -a {} disconnect <connection>",
                Name::new("agent", link_format::clean(&other.agent))
            ),
            phrase!("free it from the agent that has it"),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bot_another_agent_has_is_refused() {
        let taken = [Taken {
            id: "123".into(),
            agent: "main".into(),
            machine: Some("gcp-1".into()),
        }];
        check_taken("456", &taken, "make another").unwrap();
        let err = check_taken("123", &taken, "make another").unwrap_err();
        assert_eq!(
            err.headline.to_string(),
            "That bot is agent main's already, running on gcp-1."
        );
        assert_eq!(err.exit, Exit::Usage);
    }

    #[test]
    fn server_sent_events_come_whole() {
        let mut lines = SseLines::default();
        assert!(lines.push(b"data: {\"a\"").is_empty());
        assert_eq!(
            lines.push(b": 1}\n\nevent: x\ndata:[DONE]\r\n"),
            vec!["{\"a\": 1}".to_string(), "[DONE]".to_string()]
        );
        // A long line in many pieces, byte by byte, then a partial one.
        let line = format!("data: {}\n", "x".repeat(5000));
        let mut got = Vec::new();
        for byte in line.bytes().chain("data: y\ndata: z".bytes()) {
            got.extend(lines.push(&[byte]));
        }
        assert_eq!(got, vec!["x".repeat(5000), "y".to_string()]);
        assert_eq!(lines.push(b"\n"), vec!["z".to_string()]);
    }

    /// No more than [`MAX_ANSWERING`] threads are answered at once, and
    /// every message is answered.
    #[tokio::test]
    async fn at_most_so_many_threads_at_once() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (queue, messages) = tokio::sync::mpsc::unbounded_channel::<(u32, u32)>();
        for thread in 0..10 {
            for n in 0..3 {
                queue.send((thread, n)).unwrap();
            }
        }
        drop(queue);
        let (now, most, answered) = (
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
        );
        per_thread(
            messages,
            |(thread, _)| thread.to_string(),
            |_| {
                let (now, most, answered) = (now.clone(), most.clone(), answered.clone());
                async move {
                    let at = now.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(at, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    now.fetch_sub(1, Ordering::SeqCst);
                    answered.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .await;
        assert_eq!(answered.load(Ordering::SeqCst), 30);
        assert_eq!(most.load(Ordering::SeqCst), MAX_ANSWERING);
    }

    /// A reply that could answer a question is answered while its
    /// thread's turn still runs, as that turn may wait for it; any other
    /// message waits its turn.
    #[tokio::test]
    async fn a_reply_is_answered_beside_its_threads_turn() {
        use std::sync::{Arc, Mutex};
        let (queue, messages) = tokio::sync::mpsc::unbounded_channel::<&str>();
        let log = Arc::new(Mutex::new(Vec::new()));
        let (release, released) = tokio::sync::watch::channel(false);
        for message in ["wipe it", "later", "k7m2qx"] {
            queue.send(message).unwrap();
        }
        drop(queue);
        per_thread_replying(
            messages,
            |_| "telegram:42".to_string(),
            |message| Some(*message),
            |message| {
                let (log, release, mut released) = (log.clone(), &release, released.clone());
                async move {
                    log.lock().unwrap().push(message);
                    match message {
                        // The turn waits for the owner's answer.
                        "wipe it" => released.wait_for(|r| *r).await.map(|_| ()).unwrap(),
                        "k7m2qx" => {
                            release.send_replace(true);
                        }
                        _ => {}
                    }
                }
            },
        )
        .await;
        // Ended at all: the reply freed the turn waiting for it, and the
        // message behind that turn came only after it.
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 3);
        assert_eq!(log.last(), Some(&"later"), "{log:?}");
    }

    /// Each thread's messages are answered in order, one at a time, and
    /// another thread's alongside: a long answer holds up no other chat.
    #[tokio::test]
    async fn threads_are_answered_side_by_side_each_in_order() {
        use std::sync::{Arc, Mutex};
        let (queue, messages) = tokio::sync::mpsc::unbounded_channel::<(&str, u32)>();
        let log = Arc::new(Mutex::new(Vec::new()));
        let (release, released) = tokio::sync::watch::channel(false);
        // Thread a's first answer waits until thread b's is written.
        for message in [("a", 1), ("a", 2), ("b", 1), ("a", 3), ("b", 2)] {
            queue.send(message).unwrap();
        }
        drop(queue);
        per_thread(
            messages,
            |(thread, _)| thread.to_string(),
            |(thread, n)| {
                let (log, release, mut released) = (log.clone(), &release, released.clone());
                async move {
                    log.lock().unwrap().push(format!("{thread}{n} starts"));
                    if (thread, n) == ("a", 1) {
                        released.wait_for(|r| *r).await.unwrap();
                    }
                    tokio::task::yield_now().await;
                    log.lock().unwrap().push(format!("{thread}{n} ends"));
                    if (thread, n) == ("b", 1) {
                        release.send_replace(true);
                    }
                }
            },
        )
        .await;
        let log = log.lock().unwrap().clone();
        let at = |entry: &str| log.iter().position(|e| e == entry).unwrap();
        assert_eq!(log.len(), 10, "{log:?}");
        // b answered while a1 waited.
        assert!(at("b1 ends") < at("a1 ends"), "{log:?}");
        // Within a thread, one at a time and in order.
        for (thread, last) in [("a", 3), ("b", 2)] {
            for n in 1..last {
                assert!(
                    at(&format!("{thread}{n} ends")) < at(&format!("{thread}{} starts", n + 1)),
                    "{log:?}"
                );
            }
        }
    }
}
