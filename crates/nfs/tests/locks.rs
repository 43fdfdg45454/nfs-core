//! Byte-range locks between two clients (two sessions, as two devices): conflicts, waiting for a
//! lock (woken by CB_NOTIFY_LOCK), what closing and unlocking part of a range leave, and locks
//! to the end of the file.

use nfs_client::{Client, Create, File, LockKind::*, WRITE_ACCESS};
use nfs_testkit as common;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX: u64 = u64::MAX;

/// The same new file opened by two clients.
async fn two(test: &str) -> Option<(File, File, Arc<Client>)> {
    let (a, dir) = common::setup(test).await?;
    let both = WRITE_ACCESS | nfs_client::READ_ACCESS;
    let (fa, _) = a.create(&dir, "f", Create::Guarded(Default::default()), both).await.unwrap();
    let b = Client::connect(common::config().unwrap(), &common::export()).await.unwrap();
    let fh = b.lookup(Some(&dir), "f").await.unwrap().0;
    let fb = b.open(&fh, both).await.unwrap();
    Some((fa, fb, b))
}

fn denied(result: nfs_client::Result<()>) -> bool {
    matches!(result, Err(nfs_client::Error::Nfs(nfs_client::Status::DENIED)))
}

#[tokio::test(flavor = "multi_thread")]
async fn exclusive_and_shared_locks_between_clients() {
    let Some((a, b, _keep)) = two("locks-conflict").await else { return };
    a.lock(Write, 0, 100, false).await.unwrap();
    assert!(denied(b.lock(Write, 50, 10, false).await), "an overlapping lock was granted");
    assert!(!b.can_lock(Read, 0, 100).await.unwrap());
    b.lock(Write, 100, 100, false).await.expect("a lock next to another");
    a.unlock(0, 100).await.unwrap();
    b.lock(Write, 0, 100, false).await.expect("a lock once released");
    b.unlock(0, MAX).await.unwrap();
    a.lock(Read, 0, 50, false).await.unwrap();
    b.lock(Read, 0, 50, false).await.expect("two shared locks");
    assert!(denied(b.lock(Write, 0, 50, false).await), "exclusive over another's shared lock");
    a.close().await.unwrap();
    b.lock(Write, 0, 50, false).await.expect("upgraded once the other closed");
    b.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_lock_is_granted_as_soon_as_it_is_released() {
    let Some((a, b, _keep)) = two("locks-wait").await else { return };
    a.lock(Write, 0, MAX, false).await.unwrap();
    let b = Arc::new(b);
    let waiter = tokio::spawn({
        let b = b.clone();
        async move {
            b.lock(Write, 0, 10, true).await.unwrap();
            Instant::now()
        }
    });
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!waiter.is_finished(), "the waiting lock was granted while held");
    let released = Instant::now();
    a.unlock(0, MAX).await.unwrap();
    let granted = waiter.await.unwrap().duration_since(released);
    let notifies = b.notifies_locks();
    println!("RESULT lock granted after release: {granted:?} (server notifies: {notifies})");
    // Without notices, the waiter tries again with a pause that grows up to 4 s.
    let limit = Duration::from_millis(if notifies { 1000 } else { 4500 });
    assert!(granted < limit, "granted {granted:?} after release");
    a.close().await.unwrap();
    b.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn unlocking_part_of_a_range_and_locking_to_the_end() {
    let Some((a, b, _keep)) = two("locks-ranges").await else { return };
    a.lock(Write, 0, 300, false).await.unwrap();
    a.unlock(100, 100).await.unwrap();
    b.lock(Write, 100, 100, false).await.expect("the unlocked middle");
    assert!(denied(b.lock(Write, 50, 10, false).await), "the start stayed locked");
    assert!(denied(b.lock(Write, 250, 10, false).await), "the end stayed locked");
    b.unlock(0, MAX).await.unwrap();
    a.lock(Write, 1000, MAX, false).await.unwrap();
    assert!(denied(b.lock(Read, 1 << 40, 1, false).await), "a lock to the end of the file");
    b.lock(Read, 500, 400, false).await.expect("before a lock to the end");
    a.close().await.unwrap();
    b.close().await.unwrap();
}
