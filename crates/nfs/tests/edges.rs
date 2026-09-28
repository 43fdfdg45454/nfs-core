//! Limits and odd cases: long directories, unusual names, renames over things, offsets past
//! 4 GiB, empty files, many calls at once, and what an open does not allow.

use nfs_client::{Create, Error, READ_ACCESS, Status, WRITE_ACCESS};
use nfs_testkit as common;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn a_directory_of_two_thousand_entries_is_listed_whole() {
    let Some((client, dir)) = common::setup("long-dir").await else { return };
    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..2000 {
        let (client, dir) = (client.clone(), dir.clone());
        tasks.spawn(async move {
            let create = Create::Guarded(Default::default());
            let (file, _) =
                client.create(&dir, &format!("entry-{i:04}"), create, WRITE_ACCESS).await?;
            file.close().await
        });
    }
    while let Some(done) = tasks.join_next().await {
        done.unwrap().unwrap();
    }
    let mut names: Vec<_> =
        client.readdir(&dir).await.unwrap().into_iter().map(|e| e.name).collect();
    names.sort();
    let expected: Vec<_> = (0..2000).map(|i| format!("entry-{i:04}")).collect();
    assert_eq!(names, expected);
}

#[tokio::test]
async fn unusual_names_and_one_too_long() {
    let Some((client, dir)) = common::setup("names-odd").await else { return };
    let long = "n".repeat(255);
    for name in ["canción ñ 日本語.txt", "with  two spaces ", ".hidden", "-dash", long.as_str()]
    {
        let (file, _) = client
            .create(&dir, name, Create::Guarded(Default::default()), WRITE_ACCESS)
            .await
            .unwrap();
        file.close().await.unwrap();
        assert!(client.lookup(Some(&dir), name).await.is_ok(), "{name:?} not found");
    }
    let names: Vec<_> = client.readdir(&dir).await.unwrap().into_iter().map(|e| e.name).collect();
    assert!(names.contains(&"canción ñ 日本語.txt".to_string()), "{names:?}");
    let too_long = "n".repeat(256);
    let refused = client.create(&dir, &too_long, Create::Guarded(Default::default()), WRITE_ACCESS);
    assert_eq!(refused.await.err(), Some(Error::Nfs(Status::NAMETOOLONG)));
}

#[tokio::test]
async fn renames_over_a_file_and_into_itself() {
    let Some((client, dir)) = common::setup("renames").await else { return };
    for (name, data) in [("a", b"from a"), ("b", b"from b")] {
        let create = Create::Guarded(Default::default());
        let (file, _) = client.create(&dir, name, create, WRITE_ACCESS).await.unwrap();
        file.write(0, data).await.unwrap();
        file.close().await.unwrap();
    }
    client.rename(&dir, "a", &dir, "b").await.expect("rename over a file");
    let (b, attrs) = client.lookup(Some(&dir), "b").await.unwrap();
    assert_eq!(attrs.size, 6);
    let file = client.open(&b, READ_ACCESS).await.unwrap();
    assert_eq!(&file.read(0, 6, false).await.unwrap().0[..], b"from a");
    file.close().await.unwrap();
    assert_eq!(client.lookup(Some(&dir), "a").await.err(), Some(Error::Nfs(Status::NOENT)));
    let (sub, _) = client.mkdir(&dir, "sub", &Default::default()).await.unwrap();
    let into_itself = client.rename(&dir, "sub", &sub, "inner").await;
    assert_eq!(into_itself.err(), Some(Error::Nfs(Status::INVAL)));
    client.rename(&dir, "b", &dir, "b").await.expect("rename onto itself");
}

#[tokio::test]
async fn offsets_past_four_gib_and_an_empty_file() {
    let Some((client, dir)) = common::setup("offsets").await else { return };
    let both = READ_ACCESS | WRITE_ACCESS;
    let (file, _) =
        client.create(&dir, "sparse", Create::Guarded(Default::default()), both).await.unwrap();
    let far = 5u64 << 30;
    file.write(far, b"far away").await.unwrap();
    file.commit().await.unwrap();
    assert_eq!(client.getattr(&file.fh).await.unwrap().size, far + 8);
    assert_eq!(&file.read(far, 8, false).await.unwrap().0[..], b"far away");
    let (hole, _) = file.read(1 << 32, 4096, false).await.unwrap();
    assert!(hole.len() == 4096 && hole.iter().all(|b| *b == 0), "the hole is not zeros");
    let (tail, eof) = file.read(far + 8, 100, false).await.unwrap();
    assert!(tail.is_empty() && eof);
    file.close().await.unwrap();
    let (empty, _) =
        client.create(&dir, "empty", Create::Guarded(Default::default()), both).await.unwrap();
    let (data, eof) = empty.read(0, 100, false).await.unwrap();
    assert!(data.is_empty() && eof, "an empty file read {} bytes", data.len());
    empty.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn three_hundred_calls_at_once_over_the_session_slots() {
    let Some((client, dir)) = common::setup("many-calls").await else { return };
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..300 {
        let (client, dir) = (client.clone(), dir.clone());
        tasks.spawn(async move { client.getattr(&dir).await });
    }
    while let Some(done) = tasks.join_next().await {
        done.unwrap().expect("a call among many");
    }
}

#[tokio::test]
async fn an_open_for_reading_does_not_write() {
    let Some((client, dir)) = common::setup("open-mode").await else { return };
    let (file, _) =
        client.create(&dir, "f", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.close().await.unwrap();
    let fh = client.lookup(Some(&dir), "f").await.unwrap().0;
    let read_only = Arc::new(client.open(&fh, READ_ACCESS).await.unwrap());
    assert_eq!(read_only.write(0, b"x").await.err(), Some(Error::Nfs(Status::OPENMODE)));
    read_only.close().await.unwrap();
}

/// The server's root lists the exports under it even when some require security this client
/// does not use: those come with the reason instead of failing the listing (NFS4ERR_WRONGSEC).
#[tokio::test]
async fn the_root_lists_exports_this_client_cannot_enter() {
    let Some((client, _)) = common::setup("root-listing").await else { return };
    let entries = client.readdir(client.root()).await.expect("listing the export's root");
    // The CI's nfsd has an mtls export under the root (ci/nfsd.sh).
    if let Some(mtls) = entries.iter().find(|e| e.name == "mtls")
        && common::env("NFS_TLS").is_none()
    {
        assert_eq!(mtls.attrs.error, Status::WRONGSEC.0, "{:?}", mtls.attrs);
    }
}
