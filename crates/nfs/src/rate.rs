//! A cap on the bytes a client moves per second, each way: a token bucket shared by all its
//! connections, over their byte streams (under TLS, so its overhead counts too).

use nfs_rpc::Stream;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Instant, Sleep};

/// The caps, in bytes per second (0: none). Clones share the buckets: set it once per client.
#[derive(Clone, Default)]
pub struct Rate {
    up: Option<Arc<Bucket>>,
    down: Option<Arc<Bucket>>,
}

impl Rate {
    pub fn new(up: u64, down: u64) -> Self {
        let bucket = |rate| (rate > 0).then(|| Arc::new(Bucket::new(rate)));
        Self { up: bucket(up), down: bucket(down) }
    }

    /// `inner`, held to the caps (as is, without any).
    pub fn wrap(&self, inner: Stream) -> Stream {
        if self.up.is_none() && self.down.is_none() {
            return inner;
        }
        let rate = self.clone();
        Box::new(Limited { inner, rate, read_wait: None, write_wait: None, scratch: Vec::new() })
    }
}

/// Holds up to 100 ms of its rate; an operation may overdraw it, and the next waits it back.
struct Bucket {
    rate: f64,
    burst: f64,
    state: Mutex<(f64, Instant)>,
}

impl Bucket {
    fn new(rate: u64) -> Self {
        let rate = rate as f64;
        let burst = (rate / 10.0).max(4096.0);
        Self { rate, burst, state: Mutex::new((burst, Instant::now())) }
    }

    /// How many bytes may go now (at most `want` and a burst), or how long until some may.
    fn grant(&self, want: usize) -> Result<usize, Duration> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        state.0 = (state.0 + now.duration_since(state.1).as_secs_f64() * self.rate).min(self.burst);
        state.1 = now;
        match state.0 > 0.0 {
            true => Ok(want.min(self.burst as usize)),
            false => Err(Duration::from_secs_f64(-state.0 / self.rate + 0.001)),
        }
    }

    fn spend(&self, bytes: usize) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0 -= bytes as f64;
    }
}

struct Limited {
    inner: Stream,
    rate: Rate,
    read_wait: Option<Pin<Box<Sleep>>>,
    write_wait: Option<Pin<Box<Sleep>>>,
    scratch: Vec<u8>,
}

fn poll_grant(
    bucket: &Bucket,
    wait: &mut Option<Pin<Box<Sleep>>>,
    cx: &mut Context<'_>,
    want: usize,
) -> Poll<usize> {
    loop {
        if let Some(sleep) = wait {
            ready!(sleep.as_mut().poll(cx));
            *wait = None;
        }
        match bucket.grant(want) {
            Ok(bytes) => return Poll::Ready(bytes),
            Err(time) => *wait = Some(Box::pin(tokio::time::sleep(time))),
        }
    }
}

impl AsyncRead for Limited {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let Some(bucket) = this.rate.down.clone().filter(|_| buf.remaining() > 0) else {
            return Pin::new(&mut this.inner).poll_read(cx, buf);
        };
        let bytes = ready!(poll_grant(&bucket, &mut this.read_wait, cx, buf.remaining()));
        this.scratch.resize(this.scratch.len().max(bytes), 0);
        let mut part = ReadBuf::new(&mut this.scratch[..bytes]);
        ready!(Pin::new(&mut this.inner).poll_read(cx, &mut part))?;
        bucket.spend(part.filled().len());
        buf.put_slice(part.filled());
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Limited {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let Some(bucket) = this.rate.up.clone().filter(|_| !buf.is_empty()) else {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        };
        let bytes = ready!(poll_grant(&bucket, &mut this.write_wait, cx, buf.len()));
        let written = ready!(Pin::new(&mut this.inner).poll_write(cx, &buf[..bytes]))?;
        bucket.spend(written);
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
