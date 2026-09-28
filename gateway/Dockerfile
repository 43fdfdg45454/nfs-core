# The gateway, built for the runner's own architecture (the CI builds amd64 and arm64 natively
# and joins them under one tag).
FROM rust:1-slim-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p nfs-gateway

FROM debian:trixie-slim
# Links the image to its repository on ghcr.io (visibility and access follow the repository).
LABEL org.opencontainers.image.source=https://github.com/43fdfdg45454/nfs-core
RUN apt-get update && apt-get install -y --no-install-recommends nftables iproute2 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/nfs-gateway /usr/local/bin/nfs-gateway
COPY gateway/entrypoint.sh /usr/local/bin/entrypoint.sh
# Defaults for a /certs volume; NFS_GATEWAY_TARGET defaults to this host, port 2049.
ENV NFS_GATEWAY_LISTEN=0.0.0.0:443 \
    NFS_GATEWAY_CERT=/certs/gateway.pem \
    NFS_GATEWAY_KEY=/certs/gateway.key \
    NFS_GATEWAY_CLIENT_CA=/certs/ca.pem
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
