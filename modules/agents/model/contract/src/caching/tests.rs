//! The choices on the figures of Claude Opus 5.5 ($4 input, $20 output,
//! $0.20 cached a million tokens) and Anthropic's two lifetimes: an 8k
//! prompt of instructions and tools, a thousand tokens an exchange, a
//! summary of 2k.

use super::*;

fn anthropic() -> Vec<Offer> {
    crate::link().find("claude-opus-5-5").unwrap().cache.clone()
}

fn opus() -> Price {
    crate::link()
        .find("claude-opus-5-5")
        .unwrap()
        .price
        .unwrap()
}

fn minutes(all: &[u64]) -> Vec<Duration> {
    all.iter().map(|m| Duration::from_secs(m * 60)).collect()
}

/// A conversation of `exchanges`, its pauses `pauses` minutes.
fn chat(exchanges: u64, pauses: &[u64]) -> Conversation {
    Conversation {
        prompt: 8000 + exchanges * 1000,
        history: exchanges * 1000,
        written: 1000,
        summary: 2000,
        pauses: minutes(pauses),
    }
}

/// Rapid fire asks for five minutes; pauses of 5 to 60 minutes, even a
/// few among quick replies, ask for an hour; pauses past an hour, five
/// minutes again: an hour's dearer write buys nothing there.
#[test]
fn the_lifetime_follows_the_pauses() {
    let offers = anthropic();
    let lasts = |pauses: &[u64]| {
        offers[lifetime(&offers, &opus(), &chat(20, pauses)).unwrap()]
            .lasts
            .clone()
    };
    assert_eq!(lasts(&[1, 1, 2, 1, 1]), "5m");
    assert_eq!(lasts(&[6, 6, 6, 6]), "1h");
    assert_eq!(lasts(&[20, 20, 20]), "1h");
    assert_eq!(lasts(&[1, 1, 1, 1, 1, 1, 1, 1, 1, 20]), "1h");
    assert_eq!(lasts(&[180, 600, 720]), "5m");
    // A conversation just begun reads little: the cheaper write wins.
    let new = Conversation {
        prompt: 1000,
        written: 1000,
        ..chat(0, &[1, 20])
    };
    assert_eq!(
        offers[lifetime(&offers, &opus(), &new).unwrap()].lasts,
        "5m"
    );
}

/// Cached for an hour and quiet till morning, a conversation is worth
/// compacting from ten exchanges, five minutes before its cache ends;
/// cached for five minutes, there's no time inside it to compact in.
#[test]
fn a_quiet_conversation_compacts_once_it_pays() {
    let offers = anthropic();
    let night = [720];
    assert_eq!(compaction(&offers[1], &opus(), &chat(9, &night)), None);
    assert_eq!(
        compaction(&offers[1], &opus(), &chat(10, &night)),
        Some(Duration::from_secs(55 * 60))
    );
    assert_eq!(compaction(&offers[0], &opus(), &chat(40, &night)), None);
}

/// A conversation that comes back within the hour, an hour's rhythm,
/// finds its cache warm, so a compaction saves only cheap reads: it
/// waits for a much longer conversation.
#[test]
fn coming_back_warm_needs_a_longer_conversation() {
    let offers = anthropic();
    let hourly = [57, 57, 57];
    assert_eq!(compaction(&offers[1], &opus(), &chat(20, &hourly)), None);
    assert!(compaction(&offers[1], &opus(), &chat(200, &hourly)).is_some());
}

/// With no price, or no offer, nothing is chosen.
#[test]
fn without_figures_nothing_is_chosen() {
    let none = Price {
        input: 0.0,
        output: 0.0,
        cached: None,
    };
    assert_eq!(lifetime(&anthropic(), &none, &chat(10, &[6])), None);
    assert_eq!(lifetime(&[], &opus(), &chat(10, &[6])), None);
}

/// Its figures are read.
#[test]
fn its_figures_are_read() {
    let read = figures();
    assert_eq!(duration(&read.margin), Some(Duration::from_secs(300)));
    assert!(read.turns > 0 && read.summary > 0 && read.kept > 0);
}
