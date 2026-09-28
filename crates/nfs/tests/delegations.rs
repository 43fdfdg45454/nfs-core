//! Read delegations: granted for a read-only open when the server can call the client back, and
//! given back as soon as the server recalls them, so another client's write waits only a moment.

use nfs_client::{Client, Create, READ_ACCESS, WRITE_ACCESS};
use nfs_testkit as common;
use std::time::{Duration, Instant};

/// Some servers grant none (ganesha by default): NFS_REQUIRE_DELEGATIONS makes that a failure.
fn none_granted() {
    assert!(common::env("NFS_REQUIRE_DELEGATIONS").is_none(), "no delegation was granted");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_delegation_goes_back_as_soon_as_another_client_writes() {
    let Some((setup, dir)) = common::setup("delegation").await else { return };
    let (file, _) =
        setup.create(&dir, "f", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.write(0, b"first").await.unwrap();
    file.close().await.unwrap();
    let reader = Client::connect(common::config().unwrap(), &common::export()).await.unwrap();
    let fh = reader.lookup(Some(&dir), "f").await.unwrap().0;
    // nfsd checks the callback path after the session starts, and gives none while its file
    // cache still holds the file open for the writer that just closed it (a few seconds).
    let start = Instant::now();
    let read = loop {
        let read = reader.open(&fh, READ_ACCESS).await.unwrap();
        if read.delegation().is_some() || start.elapsed() > Duration::from_secs(15) {
            break read;
        }
        read.close().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    if read.delegation().is_none() {
        let [null, compound, flags] = reader.callback_counts();
        println!(
            "RESULT no delegation; callbacks down {} times, CB_NULL {null}, CB_COMPOUND {compound}, last flags {flags:#x}",
            reader.callbacks_down()
        );
        return none_granted();
    }
    assert_eq!(reader.delegation(&fh), read.delegation());
    let start = Instant::now();
    let write = setup.open(&fh, WRITE_ACCESS).await.expect("open over a delegation");
    let recalled = start.elapsed();
    write.write(0, b"second").await.unwrap();
    write.close().await.unwrap();
    let [null, compound, _] = reader.callback_counts();
    println!(
        "RESULT another client's open over a delegation: {recalled:?} (CB_NULL {null}, CB_COMPOUND {compound})"
    );
    assert!(recalled < Duration::from_secs(2), "the other client waited {recalled:?}");
    assert_eq!(reader.delegation(&fh), None, "the delegation is still held");
    let (data, _) = read.read(0, 6, false).await.unwrap();
    assert_eq!(&data[..], b"second");
    read.close().await.unwrap();
}
