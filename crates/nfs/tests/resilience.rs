//! Outages, address changes and lost replies: only where the test may break the client's network
//! (NFS_CAN_BREAK, in the CI's client namespace).

use nfs_testkit as common;

use common::Deaf;
use nfs_client::{Create, Error, READ_ACCESS, Status, WRITE_ACCESS};
use std::time::Duration;

const IDLE: u64 = 4;

/// A RENAME and a REMOVE whose replies are lost run once: the connection is dropped, the call is
/// replayed on a new one with the same slot and sequence id, and the server answers from its
/// reply cache instead of running them again (which would fail with NFS4ERR_NOENT).
#[tokio::test]
async fn lost_replies_of_changes_run_once() {
    if !common::can_break() {
        return;
    }
    let Some((setup, dir)) = common::setup("lost-replies").await else {
        return;
    };
    let client = common::client_with_idle(IDLE).await.unwrap();
    let (file, _) =
        client.create(&dir, "a", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.close().await.unwrap();
    let deaf = Deaf::new();
    let rename = tokio::spawn({
        let (client, dir) = (client.clone(), dir.clone());
        async move { client.rename(&dir, "a", &dir, "b").await }
    });
    tokio::time::sleep(Duration::from_secs(IDLE + 2)).await;
    drop(deaf);
    assert_eq!(rename.await.unwrap(), Ok(()));
    let deaf = Deaf::new();
    let remove = tokio::spawn({
        let (client, dir) = (client.clone(), dir.clone());
        async move { client.remove(&dir, "b").await }
    });
    tokio::time::sleep(Duration::from_secs(IDLE + 2)).await;
    drop(deaf);
    assert_eq!(remove.await.unwrap(), Ok(()));
    assert_eq!(setup.lookup(Some(&dir), "b").await.err(), Some(Error::Nfs(Status::NOENT)));
}

/// Reads go on across an outage longer than the idle timeout, and across a new client address.
#[tokio::test]
async fn reads_survive_an_outage_and_a_new_address() {
    if !common::can_break() {
        return;
    }
    let Some((setup, dir)) = common::setup("outage").await else {
        return;
    };
    let (file, _) = setup
        .create(&dir, "f", Create::Unchecked(Default::default()), READ_ACCESS | WRITE_ACCESS)
        .await
        .unwrap();
    file.write(0, &[7; 1 << 20]).await.unwrap();
    file.commit().await.unwrap();
    let client = common::client_with_idle(IDLE).await.unwrap();
    let file = client.open(&file.fh, READ_ACCESS).await.unwrap();
    common::run("ip link set vc down");
    let read =
        tokio::spawn(async move { file.read(0, 1 << 20, false).await.map(|(data, _)| data.len()) });
    tokio::time::sleep(Duration::from_secs(IDLE + 3)).await;
    common::run(
        "sysctl -qw net.ipv4.conf.vc.promote_secondaries=1 && ip link set vc up \
        && ip addr add 198.51.100.3/24 dev vc && ip addr del 198.51.100.2/24 dev vc",
    );
    assert_eq!(read.await.unwrap(), Ok(1 << 20));
    common::run("ip addr add 198.51.100.2/24 dev vc && ip addr del 198.51.100.3/24 dev vc");
}

/// Writes go on across an outage longer than the idle timeout, and a delegation is still
/// recalled through the connections that replaced the lost ones.
#[tokio::test]
async fn writes_and_callbacks_survive_an_outage() {
    if !common::can_break() {
        return;
    }
    let Some((setup, dir)) = common::setup("outage-writes").await else {
        return;
    };
    let client = common::client_with_idle(IDLE).await.unwrap();
    let both = READ_ACCESS | WRITE_ACCESS;
    let (file, _) =
        client.create(&dir, "f", Create::Guarded(Default::default()), both).await.unwrap();
    let data: Vec<u8> = (0..8u32 << 20).map(|i| (i % 253) as u8).collect();
    let file = std::sync::Arc::new(file);
    let writes = tokio::spawn({
        let (file, data) = (file.clone(), data.clone());
        async move {
            for (i, part) in data.chunks(1 << 20).enumerate() {
                file.write((i << 20) as u64, part).await?;
            }
            file.commit().await
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let deaf = Deaf::new();
    tokio::time::sleep(Duration::from_secs(IDLE + 2)).await;
    drop(deaf);
    writes.await.unwrap().expect("writes across the outage");
    let (back, _) = file.read(3 << 20, 1 << 20, false).await.unwrap();
    assert!(back[..] == data[3 << 20..4 << 20], "the data changed across the outage");
    file.close().await.unwrap();
    let reader = client.open(&file.fh, READ_ACCESS).await.unwrap();
    if reader.delegation().is_some() {
        let start = std::time::Instant::now();
        setup.open(&file.fh, WRITE_ACCESS).await.unwrap().close().await.unwrap();
        assert!(start.elapsed() < Duration::from_secs(2), "recall took {:?}", start.elapsed());
    }
    reader.close().await.unwrap();
}

/// A new address, told to the client (as the app does when the phone changes networks): calls
/// go on at once on new connections (and a new QUIC connection), not after the old ones time
/// out (30 s here).
#[tokio::test]
async fn a_network_change_reconnects_at_once() {
    if !common::can_break() {
        return;
    }
    let Some((setup, dir)) = common::setup("network-change").await else {
        return;
    };
    let (file, _) =
        setup.create(&dir, "f", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.write(0, &[3; 4096]).await.unwrap();
    file.close().await.unwrap();
    let client = common::client_with_idle(30).await.unwrap();
    let file = client.open(&file.fh, READ_ACCESS).await.unwrap();
    file.read(0, 4096, false).await.unwrap();
    common::run(
        "sysctl -qw net.ipv4.conf.vc.promote_secondaries=1 \
        && ip addr add 198.51.100.3/24 dev vc && ip addr del 198.51.100.2/24 dev vc",
    );
    let start = std::time::Instant::now();
    client.network_changed().await;
    let read = file.read(0, 4096, false).await.map(|(data, _)| data.len());
    let took = start.elapsed();
    common::run("ip addr add 198.51.100.2/24 dev vc && ip addr del 198.51.100.3/24 dev vc");
    assert_eq!(read, Ok(4096));
    println!("RESULT read after a network change: {took:?}");
    assert!(took < Duration::from_secs(5), "took {took:?}");
    file.close().await.unwrap();
}
