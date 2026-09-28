//! TCP connections to nfsd from the client's own address (IP_TRANSPARENT). The socket mark lets
//! a routing rule deliver nfsd's replies to the gateway instead of sending them to the client:
//! see ci/transparent.sh for the rules.

use crate::Target;
use socket2::{Domain, Socket, Type};
use std::io;
use std::net::{IpAddr, SocketAddr};
use tokio::net::{TcpSocket, TcpStream};

pub async fn connect(from: IpAddr, target: &Target) -> io::Result<TcpStream> {
    let socket = Socket::new(Domain::for_address(target.addr), Type::STREAM, None)?;
    match from {
        IpAddr::V4(_) => socket.set_ip_transparent_v4(true)?,
        IpAddr::V6(_) => socket.set_ip_transparent_v6(true)?,
    }
    socket.set_mark(target.mark)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::new(from, 0).into())?;
    TcpSocket::from_std_stream(socket.into()).connect(target.addr).await
}
