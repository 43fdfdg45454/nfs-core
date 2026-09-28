#!/usr/bin/env bash
# Routing for the gateway's connections from the client's address (IP_TRANSPARENT): the gateway
# marks its sockets (--mark, 0x4e46 by default), conntrack keeps the mark, and nfsd's replies on
# those connections carry it again and are delivered locally to the gateway instead of being
# routed to the client. Other connections to nfsd, such as a client's direct TCP, are untouched.
# Usage: ci/transparent.sh [mark]
set -euo pipefail
mark="${1:-0x4e46}"
sudo nft -f - <<NFT
table inet nfs-gateway {
  chain output {
    type route hook output priority mangle;
    meta mark $mark ct mark set meta mark
    ct mark $mark meta mark set ct mark
  }
}
NFT
sudo ip rule add fwmark "$mark" lookup 100
sudo ip route add local 0.0.0.0/0 dev lo table 100
# The gateway's own packets arrive on lo with the client's address as source.
sudo sysctl -qw net.ipv4.conf.all.rp_filter=0 net.ipv4.conf.lo.rp_filter=0
