//! Next to the kernel's NFS client, as another machine would be: its delegations are recalled
//! for our opens, and its locks (fcntl) and ours exclude each other. Only where the test runs as
//! root next to nfsd and may mount (NFS_CAN_BREAK), one at a time.

use nfs_client::{Client, Create, LockKind, READ_ACCESS, WRITE_ACCESS};
use nfs_testkit as common;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MOUNT: &str = "/mnt/nfs-core-kernel";

/// A new file at the export's root, and the kernel's mount of the export.
async fn file(test: &str) -> Option<(Arc<Client>, nfs_client::Fh, String)> {
    if !common::can_break() {
        return None;
    }
    let (client, _) = common::setup(test).await?;
    let name = format!("{test}-{}", std::process::id());
    let create = Create::Guarded(Default::default());
    let (file, _) = client.create(client.root(), &name, create, WRITE_ACCESS).await.unwrap();
    file.write(0, b"before").await.unwrap();
    file.close().await.unwrap();
    let (host, export) = (common::host().unwrap(), common::export());
    common::run(&format!(
        "mkdir -p {MOUNT} && (mountpoint -q {MOUNT} || mount -t nfs4 {host}:{export} {MOUNT})"
    ));
    Some((client.clone(), client.lookup(None, &name).await.unwrap().0, format!("{MOUNT}/{name}")))
}

fn python(script: &str) -> Child {
    Command::new("python3").args(["-c", script]).spawn().unwrap()
}

/// A script that holds the file (open, or locked) until killed or done, once it says "ready":
/// over the VPN link that takes a few round trips.
async fn holder(script: &str) -> Child {
    let mut child = Command::new("python3")
        .args(["-c", &format!("{script}\nprint('ready', flush=True)\ntime.sleep(3)")])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(stdout), &mut line).unwrap();
        assert_eq!(line.trim(), "ready", "the kernel's side did not get the file");
    })
    .await
    .unwrap();
    child
}

/// Opening to write a file the kernel's client holds open (with a read delegation, if nfsd gave
/// one): nfsd recalls it and answers NFS4ERR_DELAY meanwhile; the open waits and succeeds.
#[tokio::test]
async fn opening_a_file_the_kernel_holds_waits_for_the_recall() {
    let Some((client, fh, path)) = file("kernel-deleg").await else { return };
    let mut holder = holder(&format!("import time; f = open('{path}'); f.read()")).await;
    let start = Instant::now();
    let file = client.open(&fh, WRITE_ACCESS).await.expect("open while delegated");
    file.write(0, b"after!").await.unwrap();
    file.close().await.unwrap();
    let took = start.elapsed();
    _ = holder.kill();
    _ = holder.wait();
    println!("RESULT open during the kernel's delegation recall: {took:?}");
    assert!(took < Duration::from_secs(15), "the open waited {took:?}");
}

/// The kernel's fcntl lock keeps ours out until it goes, and ours keeps the kernel's out.
#[tokio::test]
async fn kernel_locks_and_ours_exclude_each_other() {
    let Some((client, fh, path)) = file("kernel-locks").await else { return };
    let mut holder = holder(&format!(
        "import fcntl, time; f = open('{path}', 'r+'); fcntl.lockf(f, fcntl.LOCK_EX)"
    ))
    .await;
    let file = client.open(&fh, READ_ACCESS | WRITE_ACCESS).await.unwrap();
    assert!(file.lock(LockKind::Write, 0, 10, false).await.is_err(), "locked over the kernel's");
    let start = Instant::now();
    file.lock(LockKind::Write, 0, 10, true).await.expect("the kernel's lock went");
    _ = holder.wait();
    println!("RESULT lock after the kernel's went: {:?} (held 3 s)", start.elapsed());
    assert!(start.elapsed() < Duration::from_secs(8), "waited {:?}", start.elapsed());
    let status = python(&format!(
        "import fcntl, sys; f = open('{path}', 'r+')\n\
         try: fcntl.lockf(f, fcntl.LOCK_EX | fcntl.LOCK_NB)\n\
         except OSError: sys.exit(1)"
    ))
    .wait()
    .unwrap();
    assert_eq!(status.code(), Some(1), "the kernel locked over ours");
    file.close().await.unwrap();
}
