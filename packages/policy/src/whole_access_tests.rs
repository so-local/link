use super::*;
use std::time::{Duration, SystemTime};

#[test]
fn outside_whole_folder_authority_requires_both_a_covering_grant_and_current_access() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
    let who: PrincipalId = "service:a1b2c3d4e5f6".parse().unwrap();
    let account: AccountId = "acct-member".parse().unwrap();
    let on: Resources = "acct-owner/bucket:team-t4m9q2x7k1ab/*".parse().unwrap();
    let read: Action = "bucket.read".parse().unwrap();
    let access = Access([("read".into(), vec![Who::Principal(who.clone())])].into());
    let whole = Policy::from_scope("token", "bucket.read@acct-owner/bucket:team-t4m9q2x7k1ab/*");
    let principal = Principal::new(who.clone(), account.clone(), None, whole);
    assert!(allows_all_with_access(&principal, read, &on, &access, now).is_allowed());
    assert!(!allows_all(&principal, read, &on, now).is_allowed());
    assert!(!allows_all_with_access(&principal, read, &on, &Access::default(), now).is_allowed());
    let top = Principal::new(
        who.clone(),
        account.clone(),
        None,
        Policy::from_scope("token", "bucket.read@acct-owner/bucket:team-t4m9q2x7k1ab/"),
    );
    assert!(!allows_all_with_access(&top, read, &on, &access, now).is_allowed());
    let other: Resources = "acct-owner/bucket:team-z9y8x7w6v5u4/*".parse().unwrap();
    assert!(!allows_all_with_access(&principal, read, &other, &access, now).is_allowed());
    let any: Resources = "*/bucket:team-t4m9q2x7k1ab/*".parse().unwrap();
    assert!(!allows_all_with_access(&principal, read, &any, &access, now).is_allowed());
    let expired = Principal::new(who, account, Some(now), principal.policy().clone());
    assert!(!allows_all_with_access(&expired, read, &on, &access, now).is_allowed());
}
