#!/usr/bin/env bash
# What the link itself gives: iperf3 with 1 and 8 TCP streams each way, and the kernel's NFS
# client reading a 64 MiB file across it for at most 10 s (which also shows nfsd answers through the link).
# Usage: ci/baseline.sh <profile>
set -euo pipefail
source "$(dirname "$0")/profiles.sh" "$1"
iperf3 -s -D
sudo dd if=/dev/urandom of=/srv/nfs/baseline.bin bs=1M count=64 status=none
sleep 1
report=""
for streams in 1 8; do
  for direction in -R ""; do
    mbps=$(sudo ip netns exec cli iperf3 -c 198.51.100.1 $direction -t 8 -P "$streams" -J \
      | python3 -c "import json,sys; print('%.2f' % (json.load(sys.stdin)['end']['sum_received']['bits_per_second'] / 8e6))")
    report+="TCP $streams stream(s) $([ -n "$direction" ] && echo download || echo upload): $mbps MB/s%0A"
  done
done
# One "ip netns exec": each one gets its own mount namespace.
rate=$(sudo ip netns exec cli sh -c "mkdir -p /mnt/nfs && mount -t nfs -o vers=4.2 \
  198.51.100.1:/ /mnt/nfs && timeout -s INT 10 dd if=/mnt/nfs/baseline.bin of=/dev/null bs=1M 2>&1 | tail -1; \
  umount /mnt/nfs" | sed 's/.*, //')
report+="Kernel NFS client 1 connection: $rate"
echo "::notice title=Baseline $title::$report"
