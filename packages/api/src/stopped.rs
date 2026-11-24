//! The account service's stops, held between background fetches. A late
//! or empty response cannot undo a stop already observed here.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::types::{AccessTokenClaims, StoppedAccount, StoppedAccounts};

/// Keep observed stops beyond the longest session, a visitor's day,
/// with an hour's margin. Durable credentials are revoked separately.
pub const KEPT: Duration = Duration::from_secs(25 * 60 * 60);
/// The interval used by account-token checkers to fetch the list.
pub const EVERY: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default)]
pub struct Cache(Arc<RwLock<Option<StoppedAccounts>>>);

impl Cache {
    pub fn at(&self, account: &str) -> Option<u64> {
        self.0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()?
            .accounts
            .iter()
            .filter(|stop| stop.account.as_str() == account)
            .map(|stop| stop.at)
            .max()
    }
    pub fn refuses(&self, claims: &AccessTokenClaims) -> bool {
        self.refuses_since(claims.acc.as_str(), crate::jwt::authorized_at(claims))
    }

    pub fn refuses_since(&self, account: &str, issued: u64) -> bool {
        self.0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|list| crate::jwt::stopped_since(list, account, issued))
    }

    /// None until a first successful fetch. Network failure leaves the
    /// cache untouched; callers choose their startup availability policy.
    pub fn snapshot(&self) -> Option<StoppedAccounts> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_at(&self, list: StoppedAccounts, now: u64) {
        let mut held = self.0.write().unwrap_or_else(|e| e.into_inner());
        let before = held
            .take()
            .into_iter()
            .flat_map(|list| list.accounts)
            .filter(|old| old.at.saturating_add(KEPT.as_secs()) > now);
        let mut cuts: std::collections::BTreeMap<_, StoppedAccount> =
            std::collections::BTreeMap::new();
        for stop in list.accounts.into_iter().chain(before) {
            cuts.entry(stop.account.clone())
                .and_modify(|saved| saved.at = saved.at.max(stop.at))
                .or_insert(stop);
        }
        *held = Some(StoppedAccounts {
            accounts: cuts.into_values().collect(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_snapshots_keep_one_maximum_cut_per_account() {
        let cache = Cache::default();
        let account = "acct-0000000000004000800000000000000a";
        let other = "acct-0000000000004000800000000000000b";
        for at in 100..116 {
            let list = serde_json::from_value(serde_json::json!({"accounts": [
                {"account": account, "at": at}, {"account": account, "at": at - 1},
                {"account": other, "at": 99}, {"account": account, "at": at - 2},
            ]}))
            .unwrap();
            cache.set_at(list, at);
            assert_eq!(cache.snapshot().unwrap().accounts.len(), 2);
            assert_eq!(cache.at(account), Some(at));
        }
        cache.set_at(StoppedAccounts { accounts: vec![] }, 116);
        assert_eq!(cache.snapshot().unwrap().accounts.len(), 2);
        assert!(cache.refuses_since(account, 114));
        assert!(!cache.refuses_since(account, 115));
    }

    #[test]
    fn an_older_or_empty_fetch_cannot_restore_a_stopped_token() {
        let cache = Cache::default();
        let account = "acct-0000000000004000800000000000000a";
        assert!(cache.snapshot().is_none());
        let list = |at| {
            serde_json::from_value(serde_json::json!({
                "accounts": [{ "account": account, "at": at }],
            }))
            .unwrap()
        };
        cache.set_at(list(100), 100);
        assert_eq!(cache.at(account), Some(100));
        cache.set_at(list(90), 101);
        cache.set_at(StoppedAccounts { accounts: vec![] }, 102);
        assert_eq!(cache.at(account), Some(100));
        assert!(cache.refuses_since(account, 99));
        assert!(!cache.refuses_since(account, 100));
        assert!(!cache.refuses_since("acct-0000000000004000800000000000000b", 99));
        cache.set_at(StoppedAccounts { accounts: vec![] }, 100 + KEPT.as_secs());
        assert!(!cache.refuses_since(account, 99));
    }
}
