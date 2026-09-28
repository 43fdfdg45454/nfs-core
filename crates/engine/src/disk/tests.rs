//! The disk cache on its own: versions, sharing, the index, and making room.

use super::DiskCache;
use crate::PIECE;
use bytes::Bytes;
use nfs_client::Fh;
use std::time::Duration;

fn cache(test: &str, limit: u64) -> std::sync::Arc<DiskCache> {
    let dir = std::env::temp_dir().join(format!("nfs-disk-test-{}-{test}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    DiskCache::new(dir, limit).unwrap()
}

fn fh(n: u8) -> Fh {
    Fh(Bytes::from(vec![n; 16]))
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(200)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_version_open_twice_is_one_file_and_its_index_outlives_pending_writes() {
    let cache = cache("shared", 1 << 30);
    let first = cache.file(&fh(1), 7, 8 * PIECE).unwrap();
    first.write(3, Bytes::from(vec![3; PIECE as usize]));
    // Reopened at once: the piece being written is there, not a truncated file.
    let second = cache.file(&fh(1), 7, 8 * PIECE).unwrap();
    assert_eq!(second.read(3, PIECE).await.unwrap()[0], 3);
    drop((first, second));
    settle().await;
    let again = cache.file(&fh(1), 7, 8 * PIECE).unwrap();
    assert!(again.has(3) && !again.has(2));
    assert_eq!(again.read(3, PIECE).await.unwrap(), vec![3; PIECE as usize]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_version_drops_the_old_and_forgetting_drops_all() {
    let cache = cache("versions", 1 << 30);
    drop(cache.file(&fh(2), 1, PIECE).unwrap());
    drop(cache.file(&fh(3), 1, PIECE).unwrap());
    let new = cache.file(&fh(2), 2, PIECE).unwrap();
    assert!(!new.has(0), "a new version starts empty");
    assert_eq!((cache.versions(&fh(2)), cache.versions(&fh(3))), (1, 1));
    cache.forget(&fh(2));
    assert_eq!((cache.versions(&fh(2)), cache.versions(&fh(3))), (0, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_least_recently_opened_goes_first() {
    let cache = cache("room", 20 * PIECE);
    for n in 4..=6 {
        let file = cache.file(&fh(n), 1, 8 * PIECE).unwrap();
        (0..8).for_each(|i| file.write(i, Bytes::from(vec![n; PIECE as usize])));
        drop(file);
        settle().await;
    }
    assert!(cache.used() <= 20 * PIECE, "{} bytes used", cache.used());
    let counts = [4, 5, 6].map(|n| cache.versions(&fh(n)));
    assert_eq!(counts, [0, 1, 1], "the oldest is gone, the rest kept");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_index_never_marks_a_new_file_at_the_same_path() {
    let cache = cache("late-index", 1 << 30);
    let old = cache.file(&fh(7), 1, 8 * PIECE).unwrap();
    old.write(2, Bytes::from(vec![2; PIECE as usize]));
    settle().await;
    cache.forget(&fh(7));
    let new = cache.file(&fh(7), 1, 8 * PIECE).unwrap();
    assert!(!new.has(2), "the removed file is still served");
    drop(old);
    settle().await;
    drop(new);
    settle().await;
    let again = cache.file(&fh(7), 1, 8 * PIECE).unwrap();
    assert!(!again.has(2), "a piece of the removed file is taken as present");
}
