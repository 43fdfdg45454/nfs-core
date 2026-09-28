//! One RPC connection: calls from any task, each matched to its reply by xid, and a watchdog
//! that closes the connection when calls wait and nothing arrives for the idle timeout.

use crate::incoming::Reply;
use crate::message::{self, Auth};
use crate::progress::{LastArrival, Progress};
use crate::{Error, Stream};
use nfs_xdr::{Decoder, Encoder};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, WriteHalf};
use tokio::sync::oneshot;

pub(crate) type Waiting = HashMap<u32, (Instant, oneshot::Sender<Result<Reply, Error>>)>;

#[derive(Clone)]
pub struct Connection(Arc<Inner>);

pub(crate) struct Inner {
    pub(crate) writer: tokio::sync::Mutex<WriteHalf<Stream>>,
    /// Who answers the server's calls on this connection, once it serves callbacks.
    pub(crate) handler: Mutex<Option<Arc<dyn crate::Handler>>>,
    /// `None` once closed.
    pub(crate) waiting: Mutex<Option<Waiting>>,
    xid: AtomicU32,
}

impl Connection {
    pub fn new(stream: Stream, idle_timeout: Duration) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        let seed = now.map_or(0, |d| d.subsec_nanos()) ^ std::process::id();
        let inner = Arc::new(Inner {
            writer: writer.into(),
            handler: Mutex::default(),
            waiting: Mutex::new(Some(HashMap::new())),
            xid: seed.into(),
        });
        let last = LastArrival::new();
        tokio::spawn(crate::receive::read(
            Arc::downgrade(&inner),
            Progress { inner: reader, last: last.clone() },
        ));
        tokio::spawn(crate::receive::watch(Arc::downgrade(&inner), last, idle_timeout));
        Self(inner)
    }

    pub fn is_closed(&self) -> bool {
        self.0.waiting.lock().map_or(true, |w| w.is_none())
    }

    /// Calls waiting for their reply.
    pub fn pending(&self) -> usize {
        self.0.waiting.lock().ok().and_then(|w| w.as_ref().map(|w| w.len())).unwrap_or(0)
    }

    /// Answers the server's calls on this connection with `handler` from now on.
    pub fn serve(&self, handler: Arc<dyn crate::Handler>) {
        *self.0.handler.lock().expect("not poisoned") = Some(handler);
    }

    pub fn close(&self) {
        self.0.close(Error::Disconnected("closed".into()));
    }

    /// Calls NFSv4 procedure `procedure`; the arguments are written by `args`.
    pub async fn call(
        &self,
        procedure: u32,
        auth: &Auth,
        args: impl FnOnce(&mut Encoder),
    ) -> Result<Decoder, Error> {
        let xid = self.0.xid.fetch_add(1, Ordering::Relaxed);
        let frame = message::call(xid, procedure, auth, args);
        let (tx, rx) = oneshot::channel();
        match self.0.waiting.lock().as_mut().ok().and_then(|w| w.as_mut()) {
            Some(waiting) => waiting.insert(xid, (Instant::now(), tx)),
            None => return Err(Error::Disconnected("closed".into())),
        };
        // A TLS stream keeps encrypted data until flushed: without it, the tail of a large
        // request would wait in the client until the next one.
        let mut writer = self.0.writer.lock().await;
        if let Err(e) = async {
            writer.write_all(&frame).await?;
            writer.flush().await
        }
        .await
        {
            self.0.close(Error::Disconnected(e.to_string()));
        } else {
            crate::traffic::sent(frame.len());
        }
        drop(writer);
        let reply = rx.await.map_err(|_| Error::Disconnected("closed".into()))??;
        reply.result
    }
}

impl Inner {
    pub(crate) fn close(&self, why: Error) {
        let Some(waiting) = self.waiting.lock().ok().and_then(|mut w| w.take()) else {
            return;
        };
        waiting.into_values().for_each(|(_, tx)| _ = tx.send(Err(why.clone())));
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.close(Error::Disconnected("closed".into()));
    }
}
