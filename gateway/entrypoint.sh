#!/bin/sh
# Installs the routing the gateway's connections from the client's own address need, runs the
# gateway (configured by NFS_GATEWAY_* variables, or by the container's arguments) and removes the
# routing when it stops. Needs the host's network (network_mode: host) and CAP_NET_ADMIN.
set -eu
mark="${NFS_GATEWAY_MARK:-0x4e46}"
table="${NFS_GATEWAY_TABLE:-100}"
cleanup() {
  nft delete table inet nfs-gateway 2>/dev/null || true
  ip rule del fwmark "$mark" lookup "$table" 2>/dev/null || true
  ip route flush table "$table" 2>/dev/null || true
}
cleanup
nft -f - <<NFT
table inet nfs-gateway {
  chain output {
    type route hook output priority mangle;
    meta mark $mark ct mark set meta mark
    ct mark $mark meta mark set ct mark
  }
}
NFT
ip rule add fwmark "$mark" lookup "$table"
ip route add local 0.0.0.0/0 dev lo table "$table"
# The gateway's own packets reach nfsd on lo with the client's address: strict reverse-path
# filtering (1) would drop them; loose (2, many distributions' default) or off lets them through.
# /proc/sys is read-only in most containers: then only a strict host needs telling.
for iface in all lo; do
  file="/proc/sys/net/ipv4/conf/$iface/rp_filter"
  [ "$(cat "$file")" != 1 ] || echo 0 2>/dev/null > "$file" \
    || echo "warning: set net.ipv4.conf.$iface.rp_filter=0 (or 2) on the host" >&2
done
trap 'kill -TERM "$gateway" 2>/dev/null' TERM INT
nfs-gateway --mark "$mark" "$@" &
gateway=$!
status=0
wait "$gateway" || status=$?
wait "$gateway" 2>/dev/null || true
cleanup
exit "$status"
