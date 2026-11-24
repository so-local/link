//! Observe admission and exclusion at their actual lock boundaries.

use super::*;
use futures_util::poll;

#[tokio::test]
async fn different_sessions_can_run_together_but_one_session_is_serialized() {
    let activity = Activity::new(false);
    let first = activity.enter("one").await;
    let other = activity.enter("two").await;
    let mut waiting = Box::pin(activity.enter("one"));
    assert!(poll!(waiting.as_mut()).is_pending());
    drop(first);
    let resumed = waiting.await;
    drop((other, resumed));
    assert!(crate::harness::lock(&activity.threads).is_empty());
}

#[tokio::test]
async fn single_session_execution_serializes_different_sessions() {
    let activity = Activity::new(true);
    let first = activity.enter("one").await;
    let mut second = Box::pin(activity.enter("two"));
    assert!(poll!(second.as_mut()).is_pending());
    drop(first);
    let second = second.await;
    drop(second);
    assert!(crate::harness::lock(&activity.threads).is_empty());
}

#[tokio::test]
async fn capture_waits_for_admitted_work_then_excludes_later_operations() {
    let activity = Activity::new(false);
    let first = activity.enter("one").await;
    let mut queued = Box::pin(activity.enter("one"));
    assert!(poll!(queued.as_mut()).is_pending());
    let mut capture = Box::pin(activity.capture());
    assert!(poll!(capture.as_mut()).is_pending());
    let mut later = Box::pin(activity.enter("two"));
    assert!(poll!(later.as_mut()).is_pending());
    drop(first);
    let queued = queued.await;
    assert!(poll!(capture.as_mut()).is_pending());
    drop(queued);
    let captured = capture.await.unwrap();
    assert!(poll!(later.as_mut()).is_pending());
    drop(captured);
    later.await;
    assert!(crate::harness::lock(&activity.threads).is_empty());
}

#[tokio::test]
async fn cancelling_a_waiting_operation_does_not_leave_a_busy_session() {
    let activity = Activity::new(false);
    let first = activity.enter("one").await;
    let mut cancelled = Box::pin(activity.enter("one"));
    assert!(poll!(cancelled.as_mut()).is_pending());
    drop(cancelled);
    drop(first);
    assert!(crate::harness::lock(&activity.threads).is_empty());
    activity.capture().await.unwrap();
}

#[tokio::test]
async fn blocking_capture_ownership_outlives_its_waiting_caller() {
    let activity = Activity::new(false);
    let captured = activity.capture().await.unwrap();
    let retained = captured.clone();
    let mut later = Box::pin(activity.enter("one"));
    assert!(poll!(later.as_mut()).is_pending());
    drop(captured);
    assert!(poll!(later.as_mut()).is_pending());
    drop(retained);
    later.await;
}
