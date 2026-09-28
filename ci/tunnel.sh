#!/usr/bin/env bash
# Starts the gateway (ci/gateway.sh) and, in the client's namespace, the tunnel client
# on 127.0.0.1:2049: NFS clients there reach nfsd through one QUIC connection. The tunnel client
# adds a forged X-Forwarded-For, which must change nothing.
# Usage: ci/tunnel.sh <bbr | bbr/<cap>>
set -Eeuo pipefail
trap 'echo "failed at $BASH_SOURCE:$LINENO: $BASH_COMMAND"' ERR
bin="$PWD/target/release"
certs="$PWD/ci-certs"
ci/gateway.sh "$1"
sudo ip netns exec cli "$bin/nfs-tunnel-client" --listen 127.0.0.1:2049 --gateway 198.51.100.1:443 \
  --server-name 198.51.100.1 --ca "$certs/ca.pem" --cert "$certs/client.pem" --key "$certs/client.key" \
  --congestion "$1" --header x-forwarded-for:203.0.113.9 > tunnel.log 2>&1 &
for _ in $(seq 100); do
  sudo ip netns exec cli ss -Hltn | grep -q 127.0.0.1:2049 && exit 0
  sleep 0.1
done
echo "the tunnel client did not start"; cat tunnel.log gateway.log; exit 1
