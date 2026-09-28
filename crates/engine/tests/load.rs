//! Several things at once: three players, and jumps while a file is uploaded.

mod player;

use nfs_client::Create;
use nfs_limits::{SEEK_MAX, SEEK_MEAN};
use player::{CHUNK, limit, report};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn three_players_at_once() {
    let Some(engine) = player::engine().await else { return };
    let players = ["movie-a.bin", "movie-b.bin", "movie-c.bin"].map(|name| {
        let engine = engine.clone();
        tokio::spawn(async move {
            let (reader, label) = player::open(&engine, name).await;
            player::play(&reader, label, reader.size() / 3 / CHUNK * CHUNK, 20).await
        })
    });
    let mut stalled = Vec::new();
    for player in players {
        stalled.push(player.await.unwrap().stalled);
    }
    limit("three players stalled", &stalled, None, Duration::ZERO);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn jumps_while_uploading() {
    let Some(engine) = player::engine().await else { return };
    let name = format!("upload-{}", std::process::id());
    let root = engine.client().root().clone();
    let (upload, _) =
        engine.create(&root, &name, Create::Unchecked(Default::default())).await.unwrap();
    let uploading = tokio::spawn(async move {
        let (start, block) = (Instant::now(), vec![9u8; 1 << 20]);
        for i in 0..64u64 {
            upload.write_at(i << 20, &block).await.unwrap();
        }
        upload.close().await.unwrap();
        start.elapsed()
    });
    let (reader, label) = player::open(&engine, "movie-a.bin").await;
    let mut seeks = Vec::new();
    for i in 1..=8u64 {
        seeks.push(
            player::play(&reader, label, i * reader.size() / 10 / CHUNK * CHUNK, 2)
                .await
                .first_byte,
        );
    }
    limit("seek while uploading", &seeks, Some(SEEK_MEAN), SEEK_MAX);
    report("upload of 64 MiB", format!("{:?}", uploading.await.unwrap()));
    drop(reader);
    engine.remove(&root, &name).await.unwrap();
    player::no_leaks(&engine).await;
}

/// An upload with nothing else going on, as fast as the link allows, then checked: the words
/// carry the fixtures' pattern (label 'u').
#[tokio::test(flavor = "multi_thread")]
async fn an_upload_alone() {
    let Some(engine) = player::engine().await else { return };
    let name = format!("upload-alone-{}", std::process::id());
    let (root, label, size) = (engine.client().root().clone(), u64::from(b'u'), 128u64 << 20);
    let (upload, _) =
        engine.create(&root, &name, Create::Unchecked(Default::default())).await.unwrap();
    let start = Instant::now();
    for at in (0..size).step_by(1 << 20) {
        let block: Vec<u8> =
            (at / 8..(at + (1 << 20)) / 8).flat_map(|w| (w | label << 48).to_le_bytes()).collect();
        upload.write_at(at, &block).await.unwrap();
    }
    upload.close().await.unwrap();
    let took = start.elapsed();
    report(
        "upload alone",
        format!("{:.1} MB/s over {} MiB", size as f64 / 1e6 / took.as_secs_f64(), size >> 20),
    );
    let fh = engine.client().lookup(Some(&root), &name).await.unwrap().0;
    let reader = engine.read(&fh).await.unwrap();
    assert_eq!(reader.size(), size);
    for at in [0, size / 3 / CHUNK * CHUNK, size - CHUNK] {
        player::check(label, at, &reader.read_at(at, CHUNK as usize).await.unwrap());
    }
    drop(reader);
    engine.remove(&root, &name).await.unwrap();
    player::no_leaks(&engine).await;
}
