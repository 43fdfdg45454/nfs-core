//! The connection's two background tasks: the reader, which hands each reply to its caller, and
//! the watchdog, which closes the connection when calls wait and nothing arrives.

use crate::connection::Inner;
use crate::incoming::{Call, Incoming, parse};
use crate::progress::{LastArrival, Progress};
use crate::{Error, Stream, record};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::io::ReadHalf;

pub(crate) async fn read(inner: Weak<Inner>, mut reader: Progress<ReadHalf<Stream>>) {
    loop {
        let result = record::read(&mut reader).await;
        let Some(inner) = inner.upgrade() else { return };
        let reply = match result.map_err(|e| Error::Disconnected(e.to_string())).and_then(parse) {
            Ok(Incoming::Reply(reply)) => reply,
            Ok(Incoming::Call(call)) => {
                answer(inner, call);
                continue;
            }
            Err(why) => return inner.close(why),
        };
        let tx = inner.waiting.lock().ok().and_then(|mut w| w.as_mut()?.remove(&reply.xid));
        if let Some((_, tx)) = tx {
            _ = tx.send(Ok(reply));
        }
    }
}

/// A call from the server, answered by the connection's handler (PROG_UNAVAIL without one) in the
/// background: the writer may be busy sending a large request.
fn answer(inner: Arc<Inner>, call: Call) {
    let handler = inner.handler.lock().ok().and_then(|h| h.clone());
    let results =
        handler.and_then(|h| h.call(call.program, call.version, call.procedure, call.args));
    let frame = crate::incoming::answer(call.xid, results);
    tokio::spawn(async move {
        let mut writer = inner.writer.lock().await;
        if writer.write_all(&frame).await.is_err() || writer.flush().await.is_err() {
            drop(writer);
            inner.close(Error::Disconnected("could not answer the server".into()));
        }
    });
}

pub(crate) async fn watch(inner: Weak<Inner>, last: LastArrival, idle_timeout: Duration) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(inner) = inner.upgrade() else { return };
        let oldest = match inner.waiting.lock().ok().as_ref().and_then(|w| w.as_ref()) {
            None => return,
            Some(waiting) => waiting.values().map(|(sent, _)| *sent).min(),
        };
        if oldest.is_some_and(|sent| sent.max(last.get()).elapsed() > idle_timeout) {
            return inner.close(Error::Stalled);
        }
    }
}
