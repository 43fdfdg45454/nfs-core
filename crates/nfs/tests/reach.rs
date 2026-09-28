//! A server nobody answers for: connecting fails in about the reach timeout, not the idle one.
//! Needs no server: it runs everywhere.

use nfs_client::{Client, Config, Security, Transport};
use nfs_rpc::{Auth, SysCred};
use std::time::{Duration, Instant};

#[tokio::test]
async fn nobody_answering_fails_in_the_reach_timeout() {
    // TEST-NET-1 (RFC 5737): routed nowhere, so nothing ever answers.
    let auth = Auth::Sys(SysCred { machine: "test".into(), uid: 0, gid: 0, gids: vec![] });
    let transport = Transport::Tcp("192.0.2.1:2049".into());
    let mut config = Config::new(transport, Security::None, auth, "reach-test");
    config.reach_timeout = Duration::from_secs(1);
    let start = Instant::now();
    assert!(Client::connect(config, "/").await.is_err(), "connected to nobody");
    println!("RESULT connecting to nobody failed in {:?}", start.elapsed());
    assert!(start.elapsed() < Duration::from_secs(6), "took {:?}", start.elapsed());
}
