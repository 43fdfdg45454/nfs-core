#!/usr/bin/env bash
# Starts the gateway next to nfsd (198.51.100.1:443), with the routing its transparent
# connections need, requiring a client certificate of the test CA.
# With GATEWAY_IMAGE, runs that container image instead of the binary.
# Usage: ci/gateway.sh <bbr | bbr/<cap>>
set -Eeuo pipefail
trap 'echo "failed at $BASH_SOURCE:$LINENO: $BASH_COMMAND"' ERR
certs="$PWD/ci-certs"
sudo sysctl -qw net.core.rmem_max=16777216 net.core.wmem_max=16777216
if [ -n "${GATEWAY_IMAGE:-}" ]; then
  # The image as published: its entrypoint installs the routing; the host sets rp_filter.
  sudo sysctl -qw net.ipv4.conf.all.rp_filter=0 net.ipv4.conf.lo.rp_filter=0
  # Configured by environment variables only, as the guide's compose file does (the image
  # defaults to /certs/ca.pem for the client CA), and with the default target: this host.
  docker run -d --name nfs-gateway --network host --cap-add NET_ADMIN -v "$certs:/certs:ro" \
    -e NFS_GATEWAY_LISTEN=198.51.100.1:443 \
    -e NFS_GATEWAY_CONGESTION="$1" -e NFS_GATEWAY_CERT=/certs/server.pem \
    -e NFS_GATEWAY_KEY=/certs/server.key "$GATEWAY_IMAGE" > /dev/null
  (docker logs -f nfs-gateway > gateway.log 2>&1 &)
else
  ci/transparent.sh
  sudo "$PWD/target/release/nfs-gateway" --listen 198.51.100.1:443 --target 127.0.0.1:2049 --congestion "$1" \
  --cert "$certs/server.pem" --key "$certs/server.key" --client-ca "$certs/ca.pem" > gateway.log 2>&1 &
fi
for _ in $(seq 100); do
  sudo ss -Hlun | grep -q 198.51.100.1:443 && exit 0
  sleep 0.1
done
echo "the gateway did not start"; cat gateway.log; exit 1
