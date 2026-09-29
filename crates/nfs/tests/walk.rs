//! Paths looked up a step at a time: deeper than one call carries, and links on the way.

use nfs_client::attr::{FileType, SetAttrs};
use nfs_testkit as common;

#[tokio::test]
async fn deep_paths_and_links_on_the_way() {
    let Some((client, dir)) = common::setup("walk").await else { return };
    let mut deepest = dir.clone();
    for _ in 0..30 {
        deepest = client.mkdir(&deepest, "d", &SetAttrs::default()).await.unwrap().0;
    }
    let (fh, attrs) = client.lookup(Some(&dir), &["d"; 30].join("/")).await.unwrap();
    assert_eq!((fh, attrs.kind), (deepest, FileType::Directory));
    // The server does not follow a link: it is the last step, and the lookup past it fails.
    client.symlink(&dir, "l", "d").await.unwrap();
    let (steps, error) = client.walk(Some(&dir), "l/d/d").await.unwrap();
    assert_eq!(steps.iter().map(|(_, a)| a.kind).collect::<Vec<_>>(), [FileType::Symlink]);
    assert!(error.is_some());
    let (steps, error) = client.walk(Some(&dir), "d/d/l").await.unwrap();
    assert_eq!((steps.len(), error), (2, Some(nfs_client::Error::Nfs(nfs_client::Status::NOENT))));
}
