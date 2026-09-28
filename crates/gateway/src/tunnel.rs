//! One `CONNECT` request: checked, answered, then carried to nfsd until both sides end.

use crate::{Target, source};
use bytes::Bytes;
use h3::server::RequestResolver;
use http::{Method, Response, StatusCode};
use std::net::IpAddr;

type Resolver = RequestResolver<h3_quinn::Connection, Bytes>;

pub async fn serve(resolver: Resolver, from: IpAddr, target: std::sync::Arc<Target>) {
    if let Err(error) = carry(resolver, from, &target).await {
        eprintln!("tunnel from {from}: {error}");
    }
}

/// Only a plain CONNECT (no :protocol) is carried, and always to the target whatever it names:
/// the gateway is not a proxy to anywhere else. Headers are never read.
fn check(request: &http::Request<()>) -> StatusCode {
    if request.method() != Method::CONNECT
        || request.extensions().get::<h3::ext::Protocol>().is_some()
    {
        StatusCode::METHOD_NOT_ALLOWED
    } else {
        StatusCode::OK
    }
}

async fn carry(resolver: Resolver, from: IpAddr, target: &Target) -> Result<(), nfs_tunnel::Error> {
    let (request, mut stream) = resolver.resolve_request().await?;
    let mut status = check(&request);
    let tcp = match status {
        StatusCode::OK => source::connect(from, target).await.inspect_err(|e| {
            eprintln!("tunnel from {from}: connecting to {}: {e}", target.addr);
            status = StatusCode::BAD_GATEWAY;
        }),
        _ => Err(std::io::Error::other("refused")),
    };
    stream.send_response(Response::builder().status(status).body(())?).await?;
    let Ok(tcp) = tcp else {
        return Ok(stream.finish().await?);
    };
    let (send, recv) = stream.split();
    let (up, down) = nfs_tunnel::pump::pump(send, recv, tcp).await?;
    eprintln!("tunnel from {from}: {up} bytes up, {down} down");
    Ok(())
}
