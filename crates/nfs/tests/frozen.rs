//! A client that stops answering (a phone app Android froze in the background: its process
//! blocked, its connections still open) must leave nothing that another client's change waits
//! on: a read delegation goes back when the file closes, and every one when the client leaves.

use nfs_client::{Client, Create, Fh, File, READ_ACCESS, WRITE_ACCESS};
use nfs_testkit as common;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A read open with a delegation, if nfsd gives one within 15 s (it checks the callback path
/// after the session starts, and gives none while its cache holds the file open for a writer).
async fn delegated(reader: &Arc<Client>, fh: &Fh) -> File {
    let start = Instant::now();
    loop {
        let read = reader.open(fh, READ_ACCESS).await.unwrap();
        if read.delegation().is_some() || start.elapsed() > Duration::from_secs(15) {
            return read;
        }
        read.close().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// A reader in a runtime of its own that gets `fh` with a delegation, does `then`, and freezes:
/// its only thread blocked until the returned sender speaks. Also whether it got the delegation.
async fn frozen<F, Then>(fh: Fh, then: Then) -> (bool, std::sync::mpsc::Sender<()>)
where
    Then: FnOnce(Arc<Client>, File) -> F + Send + 'static,
    F: Future<Output = ()>,
{
    let (said, heard) = std::sync::mpsc::channel();
    let (thaw, thawed) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let reader =
                Client::connect(common::config().unwrap(), &common::export()).await.unwrap();
            let file = delegated(&reader, &fh).await;
            let granted = file.delegation().is_some();
            then(reader.clone(), file).await;
            said.send(granted).unwrap();
            _ = thawed.recv();
        });
    });
    (tokio::task::spawn_blocking(move || heard.recv().unwrap()).await.unwrap(), thaw)
}

/// Another client removes the file the frozen reader had: at once, or the reader kept something.
async fn removed_at_once<F, Then>(test: &str, then: Then)
where
    Then: FnOnce(Arc<Client>, File) -> F + Send + 'static,
    F: Future<Output = ()>,
{
    let Some((setup, dir)) = common::setup(test).await else { return };
    let (file, _) =
        setup.create(&dir, "f", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.write(0, b"copied").await.unwrap();
    file.close().await.unwrap();
    let fh = setup.lookup(Some(&dir), "f").await.unwrap().0;
    let (granted, thaw) = frozen(fh, then).await;
    if !granted {
        assert!(common::env("NFS_REQUIRE_DELEGATIONS").is_none(), "no delegation was granted");
        return;
    }
    let start = Instant::now();
    let removed = tokio::time::timeout(Duration::from_secs(30), setup.remove(&dir, "f")).await;
    let took = start.elapsed();
    _ = thaw.send(());
    println!("RESULT {test}: another client removed the file in {took:?}");
    assert!(
        matches!(removed, Ok(Ok(()))) && took < Duration::from_secs(5),
        "the remove waited {took:?}: {removed:?}"
    );
}

/// Read, closed (a copy to the phone), then frozen: the delegation went back with the close.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_read_and_closed_keeps_no_delegation() {
    removed_at_once("frozen-closed", |_, file| async move {
        file.read(0, 6, false).await.unwrap();
        file.close().await.unwrap();
    })
    .await;
}

/// Still open when the client disconnects (as unused), then frozen.
#[tokio::test(flavor = "multi_thread")]
async fn leaving_gives_every_delegation_back() {
    removed_at_once("frozen-left", |reader, file| async move {
        file.read(0, 6, false).await.unwrap();
        reader.close();
    })
    .await;
}
