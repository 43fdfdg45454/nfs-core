#!/usr/bin/env bash
# Starts the gateway next to nfsd (198.51.100.1:443), with the routing its transparent
# connections need, requiring a client certificate of the test CA.
# With GATEWAY_IMAGE, runs that container image instead of the binary.
# Usage: ci/gateway.sh <bbr | bbr/<cap>>
set -Eeuo pipefail
trap 'echo "failed at $BASH_SOURCE:$LINENO: $BASH_COMMAND"' ERR
certs="$PWD/ci-certs"
# The gateway gets its UDP buffers by itself (CAP_NET_ADMIN), past the host's cap: started under a
# small one, the kernel's default. The clients (no such capability) get a large one afterwards.
sudo sysctl -qw net.core.rmem_max=212992 net.core.wmem_max=212992
if [ -n "${GATEWAY_IMAGE:-}" ]; then
  # The image as published: its entrypoint installs the routing; the host's rp_filter is loose,
  # as many distributions leave it.
  sudo sysctl -qw net.ipv4.conf.all.rp_filter=2 net.ipv4.conf.lo.rp_filter=2
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
# Listening, with its buffers set (right after the bind).
for _ in $(seq 100); do
  rb=$(sudo ss -Hlunm 'sport = :443' | grep -o -E 'rb[0-9]+' | head -1 | tr -d rb)
  if [ "${rb:-0}" -ge $((8 << 20)) ]; then
    sudo sysctl -qw net.core.rmem_max=16777216 net.core.wmem_max=16777216
    exit 0
  fi
  sleep 0.1
done
echo "the gateway did not start, or its receive buffer is ${rb:-unknown} bytes"; cat gateway.log; exit 1
