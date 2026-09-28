//! A reader that notes when bytes last arrived: a connection is stalled only when nothing moves,
//! not while a long reply is still arriving over a slow link.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::io::{AsyncRead, ReadBuf};

/// Milliseconds since `origin` of the last arrival.
#[derive(Clone)]
pub struct LastArrival {
    origin: Instant,
    millis: Arc<AtomicU64>,
}

impl LastArrival {
    pub fn new() -> Self {
        Self { origin: Instant::now(), millis: Arc::default() }
    }

    fn touch(&self) {
        self.millis.store(self.origin.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    pub fn get(&self) -> Instant {
        self.origin + std::time::Duration::from_millis(self.millis.load(Ordering::Relaxed))
    }
}

pub struct Progress<R> {
    pub inner: R,
    pub last: LastArrival,
}

impl<R: AsyncRead + Unpin> AsyncRead for Progress<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            crate::traffic::received(buf.filled().len() - before);
            self.last.touch();
        }
        poll
    }
}
