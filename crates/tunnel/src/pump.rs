//! Copies bytes both ways between a `CONNECT` stream and the TCP connection it carries, and
//! ends each direction as the other side ended it (a FIN for a FIN).

use bytes::{Buf, Bytes, BytesMut};
use h3::error::StreamError;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

type Send = h3_quinn::SendStream<Bytes>;
type Recv = h3_quinn::RecvStream;
const CHUNK: usize = 64 << 10;

/// What a stream is carried to: a TCP connection, or one end of an in-process pipe.
pub trait Io: AsyncRead + AsyncWrite + std::marker::Send + 'static {}
impl<T: AsyncRead + AsyncWrite + std::marker::Send + 'static> Io for T {}

/// The sending half of a `CONNECT` stream, on the client or on the gateway.
pub trait Sender {
    fn send(
        &mut self,
        data: Bytes,
    ) -> impl Future<Output = Result<(), StreamError>> + std::marker::Send;
    fn finish(&mut self) -> impl Future<Output = Result<(), StreamError>> + std::marker::Send;
}

/// The receiving half of a `CONNECT` stream.
pub trait Receiver {
    fn recv(
        &mut self,
    ) -> impl Future<Output = Result<Option<Bytes>, StreamError>> + std::marker::Send;
}

macro_rules! halves {
    ($side:ident) => {
        impl Sender for h3::$side::RequestStream<Send, Bytes> {
            async fn send(&mut self, data: Bytes) -> Result<(), StreamError> {
                self.send_data(data).await
            }
            async fn finish(&mut self) -> Result<(), StreamError> {
                h3::$side::RequestStream::finish(self).await
            }
        }
        impl Receiver for h3::$side::RequestStream<Recv, Bytes> {
            async fn recv(&mut self) -> Result<Option<Bytes>, StreamError> {
                Ok(self.recv_data().await?.map(|mut buf| buf.copy_to_bytes(buf.remaining())))
            }
        }
    };
}
halves!(client);
halves!(server);

/// Runs until both directions have ended; returns the bytes sent up (`io` to stream) and down.
pub async fn pump(
    mut send: impl Sender,
    mut recv: impl Receiver,
    io: impl Io,
) -> io::Result<(u64, u64)> {
    let (mut tcp_read, mut tcp_write) = tokio::io::split(io);
    let up = async {
        let (mut buf, mut total) = (BytesMut::new(), 0u64);
        loop {
            buf.reserve(CHUNK);
            match tcp_read.read_buf(&mut buf).await? {
                0 => {
                    break send.finish().await.map_err(io::Error::other).map(|()| total);
                }
                n => total += n as u64,
            }
            send.send(buf.split().freeze()).await.map_err(io::Error::other)?;
        }
    };
    let down = async {
        let mut total = 0u64;
        while let Some(data) = recv.recv().await.map_err(io::Error::other)? {
            total += data.len() as u64;
            tcp_write.write_all(&data).await?;
        }
        tcp_write.shutdown().await.map(|()| total)
    };
    tokio::try_join!(up, down)
}
