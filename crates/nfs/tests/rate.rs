//! The rate cap on a client's streams, on tokio's paused clock: a transfer takes its size over
//! the rate, each way, and streams of one client share it.

use nfs_client::Rate;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};
use tokio::time::Instant;

const RATE: u64 = 1_000_000;
const SIZE: usize = 4_000_000;

fn about(took: Duration, seconds: f64) {
    let took = took.as_secs_f64();
    assert!(
        (seconds * 0.9..seconds * 1.1).contains(&took),
        "took {took:.2} s, not about {seconds} s"
    );
}

#[tokio::test(start_paused = true)]
async fn each_way_takes_its_size_over_the_rate() {
    let rate = Rate::new(RATE, RATE / 2);
    let (near, mut far) = duplex(64 << 10);
    let mut limited = rate.wrap(Box::new(near));
    let start = Instant::now();
    let writer = tokio::spawn(async move {
        limited.write_all(&vec![7; SIZE]).await.unwrap();
        let mut back = vec![0; SIZE];
        limited.read_exact(&mut back).await.unwrap();
        back
    });
    let mut up = vec![0; SIZE];
    far.read_exact(&mut up).await.unwrap();
    about(start.elapsed(), 4.0);
    let start = Instant::now();
    far.write_all(&up).await.unwrap();
    assert_eq!(writer.await.unwrap(), up);
    about(start.elapsed(), 8.0);
}

#[tokio::test(start_paused = true)]
async fn streams_of_one_client_share_the_rate() {
    let rate = Rate::new(RATE, 0);
    let start = Instant::now();
    let copies = (0..4).map(|_| {
        let (near, mut far) = duplex(64 << 10);
        let mut limited = rate.wrap(Box::new(near));
        tokio::spawn(async move {
            tokio::spawn(async move { limited.write_all(&vec![1; SIZE / 4]).await.unwrap() });
            let mut got = vec![0; SIZE / 4];
            far.read_exact(&mut got).await.unwrap();
        })
    });
    for copy in copies.collect::<Vec<_>>() {
        copy.await.unwrap();
    }
    about(start.elapsed(), 4.0);
}

#[tokio::test(start_paused = true)]
async fn no_cap_is_the_stream_as_is() {
    let (near, mut far) = duplex(64 << 10);
    let mut stream = Rate::default().wrap(Box::new(near));
    let start = Instant::now();
    tokio::spawn(async move { stream.write_all(&vec![3; SIZE]).await.unwrap() });
    far.read_exact(&mut vec![0; SIZE]).await.unwrap();
    assert!(start.elapsed() < Duration::from_millis(1));
}
