# nfs-core

[![CI](https://github.com/43fdfdg45454/nfs-core/actions/workflows/ci.yml/badge.svg)](https://github.com/43fdfdg45454/nfs-core/actions/workflows/ci.yml)
[![Fuzz](https://github.com/43fdfdg45454/nfs-core/actions/workflows/fuzz.yml/badge.svg)](https://github.com/43fdfdg45454/nfs-core/actions/workflows/fuzz.yml)
[![Security](https://github.com/43fdfdg45454/nfs-core/actions/workflows/security.yml/badge.svg)](https://github.com/43fdfdg45454/nfs-core/actions/workflows/security.yml)

An NFSv4.2 client in Rust built for links with latency and loss (a phone on a VPN), and a
gateway that carries it over QUIC. No platform-specific code: the Android app
[nfs-android](https://github.com/43fdfdg45454/nfs-android) builds on these crates.

## Why

Over a VPN with 100 ms of round trip and 0.3 % loss, one TCP connection carries about 0.4 MB/s,
and that is what a classic NFS client gets. nfs-core reads and writes over several lanes at once
(QUIC streams with BBR through the gateway, or several TCP connections), puts what a reader waits
for ahead of everything else, and keeps what it read in memory and on disk. On that link, in CI:

| | nfs-core (QUIC) | One TCP connection¹ | 8 TCP connections (iperf3) |
|---|---|---|---|
| Steady read | 10.6 MB/s | 0.4 MB/s | 8.5 MB/s |
| Upload | 10.6 MB/s | 0.4 MB/s | — |

¹ The ceiling of one TCP connection on that link (≈ MSS/RTT × 1.22/√p), which is what a classic
NFS client gets; `ci/baseline.sh` measures it with iperf3 and the kernel's client.

Seeking to a scene takes 116 ms on average (323 ms at most), a burst of seeks 212 ms at most, and
going back to something already seen comes from the cache.

Good-experience limits are enforced by the tests on that profile: open ≤ 1.5 s, seek ≤ 1 s on
average and ≤ 3 s always, close ≤ 300 ms, back to something seen ≤ 100 ms, and playback at 1 MB/s
with a 2 s buffer never stalls.

## What is in it

| Crate | What it does |
|---|---|
| `nfs-xdr` | XDR encoding and decoding, bounded: no length from the network is trusted. |
| `nfs-rpc` | ONC RPC over any byte stream: many calls in flight, AUTH_SYS, RPC-with-TLS (TLS 1.3 only). |
| `nfs-client` | NFSv4.1/4.2 sessions: slots and replays, recovery and reclaim after a server restart, delegations and callbacks, byte-range locks, COPY/CLONE, live stats, optional caps on bytes per second up and down. |
| `nfs-engine` | Files for applications: 128 KiB pieces, urgent reads first, read-ahead in bursts, a memory budget, a disk cache per file version, parallel writes with COMMIT. |
| `nfs-tunnel` | QUIC with BBR (capped at 1.5 BDP), HTTP/3 `CONNECT` streams, the client side of the tunnel. |
| `nfs-gateway` | Runs next to nfsd: each client stream becomes a TCP connection to nfsd **from the client's own address**, so `/etc/exports` and the export's TLS apply exactly as without it. |
| `nfs-tools`, `nfs-testkit`, `nfs-limits` | Tunnel client, RPC probe and throughput bench (`nfs-bench`), the shared test setup, the limits every test enforces. |

Every layer on the wire is a standard: NFSv4.2 (RFC 7862), NFSv4.1 (RFC 8881), ONC RPC
(RFC 5531), RPC-with-TLS (RFC 9289), HTTP/3 (RFC 9114), QUIC (RFC 9000). Without the gateway, a
server is reached over plain NFSv4.2 on TCP.

## The gateway

Next to nfsd, with host networking (the image is `ghcr.io/43fdfdg45454/nfs-gateway`, amd64 and
arm64, tagged `latest` and with each [release](https://github.com/43fdfdg45454/nfs-core/releases)'s version):

```yaml
services:
  nfs-gateway:
    image: ghcr.io/43fdfdg45454/nfs-gateway:latest
    network_mode: host
    cap_add: [NET_ADMIN]
    restart: unless-stopped
    volumes:
      - ./certs:/certs:ro   # gateway.pem, gateway.key, ca.pem (the clients' CA)
    environment:
      NFS_GATEWAY_LISTEN: 0.0.0.0:443   # the UDP port clients connect to
```

It tunnels to nfsd on this host by default, asks every client for a certificate of `ca.pem`, and
needs `rp_filter` loose (2) or off on the host. Every option, the firewall, certificates and
troubleshooting: [docs/gateway.md](docs/gateway.md).

## Using the crates

```rust
use nfs_client::{Client, Config, READ_ACCESS, Security, Transport};
use nfs_rpc::{Auth, SysCred};

let auth = Auth::Sys(SysCred { machine: "phone".into(), uid: 1000, gid: 1000, gids: vec![] });
let transport = Transport::Tcp("nas.example.net:2049".into());
let config = Config::new(transport, Security::None, auth, "a-stable-id-per-installation");
let client = Client::connect(config, "/export").await?;
let (fh, attrs) = client.lookup(None, "movies/film.mkv").await?;

// Straight NFS…
let file = client.open(&fh, READ_ACCESS).await?;
let (data, eof) = file.read(0, 1 << 20, false).await?;

// …or through the engine: read-ahead, caches and priorities for players.
let engine = nfs_engine::Engine::new(client, nfs_engine::Config::default());
let reader = engine.read(&fh).await?;
let bytes = reader.read_at(attrs.size / 2, 128 << 10).await?;
```

`Security::tls(rustls_config, "nas.example.net")` turns on RPC-with-TLS (mutual, with a client
certificate in the rustls configuration); `Transport::Quic(tunnel)` goes through the gateway. `config.rate = Rate::new(up, down)` caps the
bytes per second each way (0: no cap), shared by all of the client's connections.
Both reach their server the same way: `host:port`, the name looked up at each new connection (the
system's resolver and cache decide, so a network change that changes the address is followed) and
each address tried in turn, IPv6 or IPv4.

## How it is tested

All in GitHub Actions, in parallel, against a real nfsd with `tlshd` and exports with and without
TLS and mTLS, over a simulated VPN (netem: 100 ms, 0.3 % loss, MTU 1420, 100 Mb/s) and a LAN:

- **Stage 1**: the transport alone (QUIC with 1–8 streams and BBR against TCP, iperf3 and the
  kernel's client as baselines), and the gateway's guarantees (real client address, refusals of
  foreign CAs and clients without a certificate, the export's mTLS end to end, address changes).
- **Stage 2**: the client over TCP and QUIC × no TLS and mTLS × LAN and VPN: operations, copies,
  delegations and callbacks, locks, server restarts and expired leases, outages and network
  changes, a server that does not answer, and next to the kernel's own NFS client.
- **Stage 3**: real-size scenarios (films of 250–700 MiB) through the engine with a strict 2 s
  player: playback, scene search, bursts of seeks, several players, files changed, grown,
  truncated or deleted by another client.
- **Fuzzing** (`fuzz/`): everything a server can send, on each push and daily.
- **Security**: `cargo deny` (RustSec, licenses, sources), secrets in the history, zizmor on the
  workflows, CodeQL, Trivy on the gateway's image; `unsafe` code is forbidden in the workspace.

Locally the tests need a server: `NFS_SERVER=host:2049 NFS_EXPORT=/export cargo test` (see
`crates/testkit`); without `NFS_SERVER` they skip.

## Status

Stages 1–3 (transport, client, engine) are done; nfs-android builds on them. The
design, the plan and the decisions taken along the way are in `CLAUDE.md` and `DECISIONES.md` (in
Spanish).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
