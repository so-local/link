//! Append acknowledgements distinguish committed observations from conflicts.

use super::*;

impl Harness {
    /// Adds `event` after `events`, as the next number: when another wrote
    /// that number first (a message come while it runs), the events since
    /// are read and it goes after them. `events` ends with it.
    pub(super) fn append_next(
        &self,
        agent: &Agent,
        thread: &Thread,
        event: &mut Event,
        events: &mut Vec<Event>,
    ) -> anyhow::Result<()> {
        append(
            event,
            events,
            |event, before| self.store.append(agent, thread, event, before),
            |seq| self.store.read_from(agent, thread, seq),
        )
    }
}

fn append(
    event: &mut Event,
    events: &mut Vec<Event>,
    mut write: impl FnMut(&Event, &[Event]) -> anyhow::Result<()>,
    mut newer: impl FnMut(u64) -> anyhow::Result<Vec<Event>>,
) -> anyhow::Result<()> {
    for _ in 0..64 {
        let seq = next_seq(events);
        // An answer's mark that names it is renumbered with it.
        if let Some(window) = &mut event.window
            && event.seq != 0
            && window.answered == event.seq
        {
            window.answered = seq;
        }
        event.seq = seq;
        match write(event, events) {
            Ok(()) => {
                events.push(event.clone());
                return Ok(());
            }
            Err(err) => {
                let newer = newer(event.seq)?;
                if newer.first().is_none_or(|e| e.seq != event.seq) {
                    return Err(err);
                }
                let committed = newer.first() == Some(event);
                events.extend(newer);
                if committed {
                    return Ok(());
                }
            }
        }
    }
    anyhow::bail!("the conversation kept moving under it")
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_test_kit::fault::{Moment, Once};
    use std::cell::RefCell;

    #[test]
    fn a_lost_append_acknowledgement_keeps_its_original_sequence_once() {
        let fault = RefCell::new(Once::new(Moment::After));
        let stored = RefCell::new(Vec::<Event>::new());
        let mut event = Event {
            role: event::EVIDENCE.into(),
            text: "dispatch marker".into(),
            ..Default::default()
        };
        let mut held = vec![];
        append(
            &mut event,
            &mut held,
            |event, _| {
                fault.borrow_mut().run(
                    || {
                        stored.borrow_mut().push(event.clone());
                        Ok(())
                    },
                    || anyhow::anyhow!("append reply lost"),
                )
            },
            |seq| {
                Ok(stored
                    .borrow()
                    .iter()
                    .filter(|event| event.seq >= seq)
                    .cloned()
                    .collect())
            },
        )
        .unwrap();
        fault.borrow().assert_injected();
        assert_eq!(event.seq, 1);
        assert_eq!(stored.borrow().as_slice(), &[event.clone()]);
        assert_eq!(held, vec![event]);
    }

    #[test]
    fn a_different_concurrent_input_moves_the_proposal_before_identity_is_assigned() {
        let stored = RefCell::new(vec![Event {
            seq: 1,
            ..Event::user("arrived meanwhile")
        }]);
        let mut event = Event {
            role: event::ASSISTANT.into(),
            text: "proposal".into(),
            ..Default::default()
        };
        let mut held = vec![];
        append(
            &mut event,
            &mut held,
            |event, _| {
                let mut stored = stored.borrow_mut();
                anyhow::ensure!(event.seq == stored.len() as u64 + 1, "sequence occupied");
                stored.push(event.clone());
                Ok(())
            },
            |seq| {
                Ok(stored
                    .borrow()
                    .iter()
                    .filter(|event| event.seq >= seq)
                    .cloned()
                    .collect())
            },
        )
        .unwrap();
        assert_eq!(event.seq, 2);
        assert_eq!(stored.borrow().len(), 2);
        assert_eq!(held, *stored.borrow());
    }
}
