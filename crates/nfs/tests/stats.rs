//! What the client tells about its connections: all up after connecting, the bytes counted, and
//! over QUIC the path to the gateway.

use nfs_testkit as common;

#[tokio::test]
async fn stats_show_the_connections_and_the_traffic() {
    let Some((client, dir)) = common::setup("stats").await else { return };
    let before = nfs_rpc::traffic();
    client.getattr(&dir).await.unwrap();
    let (after, stats) = (nfs_rpc::traffic(), client.stats());
    println!("RESULT stats: {stats:?}");
    assert!(after.0 > before.0 && after.1 > before.1, "no bytes counted: {before:?} {after:?}");
    assert_eq!(stats.alive, stats.lanes, "connections down");
    assert!(stats.connects as usize >= stats.lanes, "fewer connections made than up");
    assert_eq!(stats.quic, stats.path.is_some(), "the QUIC path, and only over QUIC");
    if let Some(path) = stats.path {
        assert!(path.rtt > std::time::Duration::ZERO && path.sent > 0, "an empty path: {path:?}");
    }
}
