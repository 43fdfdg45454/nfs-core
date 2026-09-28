//! Copies done by the server, against a real one.

use nfs_client::{Create, Error, READ_ACCESS, Status, WRITE_ACCESS};
use nfs_testkit as common;

#[tokio::test]
async fn server_side_copies() {
    let Some((client, dir)) = common::setup("copies").await else { return };
    let access = READ_ACCESS | WRITE_ACCESS;
    let (source, _) =
        client.create(&dir, "s", Create::Unchecked(Default::default()), access).await.unwrap();
    source.write(0, &[5; 300_000]).await.unwrap();
    source.commit().await.unwrap();
    let (copy, _) =
        client.create(&dir, "c", Create::Unchecked(Default::default()), access).await.unwrap();
    // nfsd copies; some servers (nfs-ganesha) do not: required only where NFS_REQUIRE_COPY says.
    match source.copy_to(&copy, 0, 0, 300_000).await {
        Err(Error::Nfs(Status::NOTSUPP)) if std::env::var("NFS_REQUIRE_COPY").is_err() => return,
        result => assert_eq!(result, Ok(300_000)),
    }
    assert_eq!(copy.read(299_990, 100, false).await.unwrap().0.len(), 10);
    let (clone, _) =
        client.create(&dir, "k", Create::Unchecked(Default::default()), access).await.unwrap();
    match source.clone_to(&clone, 0, 0, 0).await {
        Ok(()) => assert_eq!(client.getattr(&clone.fh).await.unwrap().size, 300_000),
        Err(error) => assert_eq!(error, Error::Nfs(Status::NOTSUPP)),
    }
}
