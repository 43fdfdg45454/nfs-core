#!/usr/bin/env bash
# 20 s of reading and 20 s of writing through nfs-core's client (nfs-bench) over <connections>
# connections, 8 calls of 1 MiB in flight each. While reading, a NULL RPC every 100 ms on a connection of its own shows
# how long a small request waits behind the bulk. CPU of the gateway and the tunnel client per MB.
# Usage: ci/measure.sh <title> <server> <connections> <nfsd threads>
set -Eeuo pipefail
trap 'echo "failed at $BASH_SOURCE:$LINENO: $BASH_COMMAND"' ERR
title="$1" server="$2" connections="$3" threads="$4"
bench="$PWD/target/release/nfs-bench"
probe="$PWD/target/release/nfs-rpc-probe --server $server:2049"
echo "$threads" | sudo tee /proc/fs/nfsd/threads > /dev/null
ticks() { # CPU ticks (1/100 s) used so far by the tunnel's processes
  local total=0
  for pid in $(pgrep -x nfs-gateway) $(pgrep -x nfs-tunnel-clie); do
    read -r -a stat < "/proc/$pid/stat"
    total=$((total + stat[13] + stat[14]))
  done
  echo "$total"
}
netns() { sudo ip netns exec cli env "PATH=$PATH" "$@"; }
queue() { # every 0.5 s, the bytes waiting in the link's queue toward the client (downloads) and
  # toward the server (uploads), until killed
  while sleep 0.5; do
    echo "$(tc -s qdisc show dev vh | grep -o -E 'backlog [0-9]+b' | grep -o -E '[0-9]+') \
$(sudo ip netns exec cli tc -s qdisc show dev vc | grep -o -E 'backlog [0-9]+b' | grep -o -E '[0-9]+')"
  done
}
summary() { # file: mean and max queue of each direction, in KB
  awk '{d+=$1; u+=$2; if ($1>dm) dm=$1; if ($2>um) um=$2; n++}
    END {printf "link queue down mean %.0f max %.0f KB, up mean %.0f max %.0f KB", d/n/1e3, dm/1e3, u/n/1e3, um/1e3}' "$1"
}
report="Idle: $(netns $probe --seconds 3)%0A"
for mode in read write; do
  before=$(ticks)
  queue > queue.txt & sampler=$!
  if [ "$mode" = read ]; then netns $probe --seconds 15 --label "Under load: " > probe.txt & fi
  line=$(netns NFS_SERVER="$server:2049" NFS_EXPORT=/ NFS_CONNECTIONS="$connections" \
    NFS_IN_FLIGHT=8 NFS_SECONDS=20 $([ "$mode" = write ] && echo NFS_WRITE=1) "$bench" 2>&1 | tail -1)
  mbs=$(grep -o -E '[0-9.]+ MB/s' <<< "$line" | tail -1 | cut -d' ' -f1)
  cpu=$(( $(ticks) - before ))
  kill "$sampler"
  report+="$line; tunnel CPU $(awk "BEGIN { printf \"%.1f\", $cpu * 10 / ($mbs * 20 + 0.001) }") ms/MB; $(summary queue.txt)%0A"
  if [ "$mode" = read ]; then wait; report+="$(cat probe.txt)%0A"; fi
done
echo "::notice title=$title ($threads nfsd threads)::$report"
