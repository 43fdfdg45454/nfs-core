//! Every operation once, against a real server.

use nfs_testkit as common;

use nfs_client::attr::{FileType, SetAttrs, SetTime};
use nfs_client::{Create, Error, READ_ACCESS, Status, Time, WRITE_ACCESS};
use std::time::{Duration, SystemTime};

#[tokio::test]
async fn names_and_directories() {
    let Some((client, dir)) = common::setup("names").await else {
        return;
    };
    let (sub, attrs) = client
        .mkdir(&dir, "sub", &SetAttrs { mode: Some(0o750), ..Default::default() })
        .await
        .unwrap();
    assert_eq!((attrs.kind, attrs.mode & 0o777), (FileType::Directory, 0o750));
    let (file, _) =
        client.create(&dir, "a", Create::Guarded(Default::default()), WRITE_ACCESS).await.unwrap();
    file.close().await.unwrap();
    let exists = client.create(&dir, "a", Create::Guarded(Default::default()), WRITE_ACCESS).await;
    assert_eq!(exists.err(), Some(Error::Nfs(Status::EXIST)));
    client.rename(&dir, "a", &sub, "b").await.unwrap();
    let (b, _) = client.lookup(Some(&sub), "b").await.unwrap();
    client.link(&b, &dir, "c").await.unwrap();
    assert_eq!(client.getattr(&b).await.unwrap().links, 2);
    let (link, _) = client.symlink(&dir, "l", "sub/b").await.unwrap();
    assert_eq!(client.readlink(&link).await.unwrap(), "sub/b");
    let mut names: Vec<_> =
        client.readdir(&dir).await.unwrap().into_iter().map(|e| e.name).collect();
    names.sort();
    assert_eq!(names, ["c", "l", "sub"]);
    assert_eq!(client.remove(&dir, "sub").await.err(), Some(Error::Nfs(Status::NOTEMPTY)));
    client.remove(&sub, "b").await.unwrap();
    client.remove(&dir, "sub").await.unwrap();
    assert_eq!(client.lookup(Some(&dir), "sub").await.err(), Some(Error::Nfs(Status::NOENT)));
}

#[tokio::test]
async fn without_a_mode_new_files_and_directories_get_the_usual_ones() {
    // nfsd would leave them 0000: created, but not writable again except by root.
    let Some((client, dir)) = common::setup("default-mode").await else {
        return;
    };
    let (_, attrs) = client.mkdir(&dir, "d", &SetAttrs::default()).await.unwrap();
    assert_eq!(attrs.mode & 0o777, 0o755);
    for create in [Create::Guarded(Default::default()), Create::Unchecked(Default::default())] {
        let (file, attrs) = client.create(&dir, "f", create, WRITE_ACCESS).await.unwrap();
        file.close().await.unwrap();
        assert_eq!(attrs.mode & 0o777, 0o644);
        client.remove(&dir, "f").await.unwrap();
    }
}

#[tokio::test]
async fn a_new_file_has_its_mode_and_the_current_time() {
    let Some((client, dir)) = common::setup("new-file").await else {
        return;
    };
    let attrs = SetAttrs { mode: Some(0o640), ..Default::default() };
    let before = Time::from_system(SystemTime::now() - Duration::from_secs(120));
    let (file, attrs) =
        client.create(&dir, "f", Create::Guarded(attrs), WRITE_ACCESS).await.unwrap();
    file.close().await.unwrap();
    let after = Time::from_system(SystemTime::now() + Duration::from_secs(120));
    assert_eq!(attrs.mode & 0o777, 0o640);
    for time in [attrs.modified, attrs.accessed, attrs.changed] {
        assert!(before < time && time < after, "{time:?}");
    }
    let then = Time { secs: 1_000_000_000, nanos: 0 };
    let set = SetAttrs {
        modified: Some(SetTime::To(then)),
        accessed: Some(SetTime::ServerNow),
        size: Some(10),
        ..Default::default()
    };
    let attrs = client.setattr(&file_fh(&client, &dir).await, &set).await.unwrap();
    assert_eq!((attrs.modified, attrs.size), (then, 10));
}

async fn file_fh(client: &nfs_client::Client, dir: &nfs_client::Fh) -> nfs_client::Fh {
    client.lookup(Some(dir), "f").await.unwrap().0
}

#[tokio::test]
async fn write_commit_read_back() {
    let Some((client, dir)) = common::setup("io").await else {
        return;
    };
    let (file, _) = client
        .create(&dir, "data", Create::Unchecked(Default::default()), READ_ACCESS | WRITE_ACCESS)
        .await
        .unwrap();
    let data: Vec<u8> = (0..3 << 20).map(|i: u32| (i * 7 % 251) as u8).collect();
    let chunk = client.max_io() as usize;
    for (i, part) in data.chunks(chunk).enumerate() {
        assert_eq!(file.write((i * chunk) as u64, part).await.unwrap().0 as usize, part.len());
    }
    file.commit().await.unwrap();
    let mut back = Vec::new();
    loop {
        let (bytes, eof) = file.read(back.len() as u64, chunk as u32, false).await.unwrap();
        back.extend_from_slice(&bytes);
        if eof || bytes.is_empty() {
            break;
        }
    }
    assert!(back == data, "read back {} bytes", back.len());
    file.close().await.unwrap();
    let space = client.space().await.unwrap();
    assert!(space.2 > 0 && space.0 <= space.2);
}
