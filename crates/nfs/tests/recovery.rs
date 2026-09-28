//! The server forgetting the client: its lease expired (nfsd told to expire it), or nfsd
//! restarted. Only where the tests run on nfsd's machine as root (the CI); the restart only where
//! the network may be broken too (NFS_CAN_BREAK), one test at a time.

use nfs_client::{Client, Create, READ_ACCESS, WRITE_ACCESS};
use nfs_testkit as common;

async fn read_back(client: &std::sync::Arc<Client>, fh: &nfs_client::Fh, expected: &[u8]) {
    let file = client.open(fh, READ_ACCESS).await.unwrap();
    let (data, _) = file.read(0, expected.len() as u32, false).await.unwrap();
    assert!(data == expected, "the file holds other data");
    file.close().await.unwrap();
}

/// The next call finds the session gone: a new client id and session are set up, and a file open
/// before is opened again by its handle.
#[tokio::test]
async fn an_expired_client_gets_a_new_session_and_its_files_reopen() {
    let Some((setup, dir)) = common::setup("expired").await else { return };
    let config = common::config().unwrap();
    let owner = config.owner.clone();
    let client = Client::connect(config, &common::export()).await.unwrap();
    let data = vec![5u8; 256 << 10];
    let (file, _) =
        client.create(&dir, "a", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.write(0, &data).await.unwrap();
    file.commit().await.unwrap();
    if !common::expire(&owner) {
        return;
    }
    let start = std::time::Instant::now();
    file.write(0, &data).await.expect("write after the client expired");
    println!("RESULT write after the client expired: {:?}", start.elapsed());
    file.commit().await.unwrap();
    file.close().await.unwrap();
    client.getattr(&dir).await.expect("a call after the client expired");
    read_back(&setup, &file_fh(&client, &dir).await, &data).await;
}

/// Data written but not committed when nfsd restarts is lost: the new session opens the file
/// again, and COMMIT answers with another verifier, which tells the writer to send it again.
#[tokio::test]
async fn a_restart_is_recovered_and_its_lost_writes_detected() {
    if !common::can_break() {
        return;
    }
    let Some((client, dir)) = common::setup("restart").await else { return };
    let data = vec![7u8; 1 << 20];
    let (file, _) =
        client.create(&dir, "a", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    let (_, before) = file.write(0, &data).await.unwrap();
    common::restart_nfsd();
    let start = std::time::Instant::now();
    let after = file.commit().await.expect("commit after the restart");
    assert_ne!(before, after, "a restarted server answers with a new write verifier");
    let (_, again) = file.write(0, &data).await.expect("write after the restart");
    println!("RESULT commit and write after a restart: {:?}", start.elapsed());
    assert_eq!(file.commit().await.unwrap(), again);
    file.close().await.unwrap();
    read_back(&client, &file_fh(&client, &dir).await, &data).await;
}

async fn file_fh(client: &Client, dir: &nfs_client::Fh) -> nfs_client::Fh {
    client.lookup(Some(dir), "a").await.unwrap().0
}

/// Locks come back with a restart (reclaimed in the grace period): another client still cannot
/// take them.
#[tokio::test]
async fn locks_are_reclaimed_after_a_restart() {
    if !common::can_break() {
        return;
    }
    let Some((client, dir)) = common::setup("restart-locks").await else { return };
    let (file, _) =
        client.create(&dir, "a", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.lock(nfs_client::LockKind::Write, 0, 100, false).await.unwrap();
    common::restart_nfsd();
    let start = std::time::Instant::now();
    file.write(0, b"after").await.expect("write after the restart");
    println!("RESULT locks reclaimed after a restart: {:?}", start.elapsed());
    assert!(!file.locks_lost(), "the locks were not reclaimed");
    let other = Client::connect(common::config().unwrap(), &common::export()).await.unwrap();
    let theirs = other.open(&file_fh(&client, &dir).await, WRITE_ACCESS).await.unwrap();
    let taken = theirs.lock(nfs_client::LockKind::Write, 0, 10, false).await;
    assert!(taken.is_err(), "another client took a reclaimed lock");
    file.close().await.unwrap();
    theirs.lock(nfs_client::LockKind::Write, 0, 10, false).await.expect("released on close");
    theirs.close().await.unwrap();
}

/// A client whose lease expired lost its locks: another client can take them, and the file
/// says so on its next call.
#[tokio::test]
async fn an_expired_client_loses_its_locks_and_knows() {
    let Some((setup, dir)) = common::setup("expired-locks").await else { return };
    let config = common::config().unwrap();
    let owner = config.owner.clone();
    let client = Client::connect(config, &common::export()).await.unwrap();
    let (file, _) =
        client.create(&dir, "a", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.lock(nfs_client::LockKind::Write, 0, 100, false).await.unwrap();
    if !common::expire(&owner) {
        return;
    }
    let theirs = setup.open(&file_fh(&setup, &dir).await, WRITE_ACCESS).await.unwrap();
    theirs.lock(nfs_client::LockKind::Write, 0, 10, false).await.expect("the expired lock");
    file.write(0, b"late").await.expect("write after the client expired");
    assert!(file.locks_lost(), "the file did not notice its locks were lost");
    theirs.close().await.unwrap();
}
