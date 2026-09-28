#!/usr/bin/env bash
# Stage 1 on one runner: one transport variant measured, or the gateway checks.
# Usage: ci/stage1.sh <tcp | quic-<streams>[-bbr | -cap<gain>][-lan] | checks>
# BBR capped at 1.5 BDP unless -bbr (no cap) or -cap<gain>.
set -Eeuo pipefail
trap 'echo "failed at $BASH_SOURCE:$LINENO: $BASH_COMMAND"' ERR
variant="$1"
profile=vpn; [[ "$variant" == *-lan ]] && profile=lan
congestion=bbr/1.5
[[ "$variant" == *-bbr* ]] && congestion=bbr
[[ "$variant" =~ -cap([0-9.]+) ]] && congestion="bbr/${BASH_REMATCH[1]}"
# The builds and the server do not depend on each other: all at once.
cargo build --release --locked > build-rust.log 2>&1 & rust=$!
ci/certs.sh > /dev/null 2>&1
ci/nfsd.sh
ci/link.sh "$profile"
sudo dd if=/dev/urandom of=/srv/nfs/bench.bin bs=1M count=512 status=none
wait "$rust" || { tail -40 build-rust.log; exit 1; }
source ci/profiles.sh "$profile"
case "$variant" in
  tcp)
    ci/measure.sh "TCP 32 connections - $title" 198.51.100.1 32 8
    ci/measure.sh "TCP 32 connections - $title" 198.51.100.1 32 64 ;;
  quic-*)
    streams="${variant#quic-}"
    ci/tunnel.sh "$congestion"
    ci/measure.sh "QUIC $congestion ${streams%%-*} streams - $title" 127.0.0.1 "${streams%%-*}" 8 ;;
  checks)
    ci/tunnel.sh "$congestion"
    ci/checks.sh ;;
  *) echo "Unknown variant $variant"; exit 2 ;;
esac
