//! Changing an existing file in place, as an editor does: the rest stays, reads through the same
//! file see what was written before it is closed, and a reader afterwards gets the new version.

mod player;

use nfs_client::Create;

#[tokio::test(flavor = "multi_thread")]
async fn writing_in_the_middle_of_an_existing_file() {
    let Some(engine) = player::engine().await else { return };
    let (root, name) = (engine.client().root().clone(), format!("edit-{}", std::process::id()));
    let (writer, _) =
        engine.create(&root, &name, Create::Unchecked(Default::default())).await.unwrap();
    writer.write_at(0, &vec![b'a'; 3 << 20]).await.unwrap();
    writer.close().await.unwrap();
    let fh = engine.client().lookup(Some(&root), &name).await.unwrap().0;
    let before = engine.read(&fh).await.unwrap();
    assert_eq!(before.read_at(1 << 20, 4).await.unwrap(), &b"aaaa"[..]);
    let edit = engine.write(&fh, true).await.unwrap();
    edit.write_at((1 << 20) + 2, b"XY").await.unwrap();
    edit.write_at(3 << 20, b"tail").await.unwrap();
    assert_eq!(edit.read_at(1 << 20, 6).await.unwrap(), &b"aaXYaa"[..], "reads see the writes");
    edit.close().await.unwrap();
    drop(before);
    let after = engine.read(&fh).await.unwrap();
    assert_eq!(after.size(), (3 << 20) + 4);
    assert_eq!(after.read_at(1 << 20, 6).await.unwrap(), &b"aaXYaa"[..]);
    assert_eq!(after.read_at(3 << 20, 4).await.unwrap(), &b"tail"[..]);
    assert_eq!(after.read_at(0, 4).await.unwrap(), &b"aaaa"[..]);
    drop(after);
    engine.remove(&root, &name).await.unwrap();
    player::no_leaks(&engine).await;
}
