//! Many readers: six files jumping at once, one video with three descriptors, and files opened
//! and closed in a burst while another plays.

mod player;

use nfs_limits::{CLOSE, OPEN, SEEK_MAX, SEEK_MEAN};
use player::{CHUNK, limit, report};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn six_files_jumping_at_once() {
    let Some(engine) = player::engine().await else { return };
    let names =
        ["movie-a.bin", "movie-b.bin", "movie-c.bin", "movie-d.bin", "movie-e.bin", "movie-f.bin"];
    let tasks = names.map(|name| {
        let engine = engine.clone();
        tokio::spawn(async move {
            let (reader, label) = player::open(&engine, name).await;
            let mut seeks = Vec::new();
            for i in 1..=5u64 {
                seeks.push(
                    player::play(
                        &reader,
                        label,
                        (i * 17 % 10) * reader.size() / 10 / CHUNK * CHUNK,
                        2,
                    )
                    .await
                    .first_byte,
                );
            }
            seeks
        })
    });
    let mut seeks = Vec::new();
    for task in tasks {
        seeks.extend(task.await.unwrap());
    }
    limit("six files: seek", &seeks, Some(SEEK_MEAN), SEEK_MAX);
    // Six files share the engine's memory (96 MiB by default); blocks in flight may pass it a bit.
    let (_, peak) = engine.memory();
    report("six files: memory at most", format!("{} MiB", peak >> 20));
    assert!(peak <= (96 + 16) << 20, "{} MiB of pieces in memory", peak >> 20);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_video_three_descriptors() {
    let Some(engine) = player::engine().await else { return };
    let tasks = [1u64, 4, 7].map(|tenth| {
        let engine = engine.clone();
        tokio::spawn(async move {
            let (reader, label) = player::open(&engine, "movie-d.bin").await;
            player::play(&reader, label, tenth * reader.size() / 10 / CHUNK * CHUNK, 15)
                .await
                .stalled
        })
    });
    let mut stalled = Vec::new();
    for task in tasks {
        stalled.push(task.await.unwrap());
    }
    limit("three descriptors stalled", &stalled, None, Duration::ZERO);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn opening_and_closing_in_a_burst_while_another_plays() {
    let Some(engine) = player::engine().await else { return };
    let playing = tokio::spawn({
        let engine = engine.clone();
        async move {
            let (reader, label) = player::open(&engine, "movie-e.bin").await;
            player::play(&reader, label, 0, 15).await.stalled
        }
    });
    let (mut opens, mut closes) = (Vec::new(), Vec::new());
    for i in 0..20u64 {
        let start = Instant::now();
        let (reader, label) = player::open(&engine, "movie-f.bin").await;
        let data = reader.read_at(i * 3 * CHUNK, CHUNK as usize).await.unwrap();
        player::check(label, i * 3 * CHUNK, &data);
        opens.push(start.elapsed());
        let start = Instant::now();
        drop(reader);
        closes.push(start.elapsed());
    }
    limit("burst: open to first byte", &opens, None, OPEN);
    limit("burst: close", &closes, None, CLOSE);
    limit("burst: the other player stalled", &[playing.await.unwrap()], None, Duration::ZERO);
    player::no_leaks(&engine).await;
}
