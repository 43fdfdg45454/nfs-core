#!/usr/bin/env bash
# Stage 2 on one runner: the client core's tests against nfsd, over one transport and security.
# Usage: ci/stage2.sh <tcp | tcp-mtls | quic | quic-mtls>[-vpn]
set -Eeuo pipefail
trap 'echo "failed at $BASH_SOURCE:$LINENO: $BASH_COMMAND"' ERR
source ci/host.sh
prepare nfs-client "$1"
vars+=(NFS_REQUIRE_COPY=1 NFS_REQUIRE_DELEGATIONS=1)
[[ "$1" == tcp-mtls* ]] && vars+=(NFS_TLS_REFUSALS=1)
# Outages and lost replies are tested once per transport, over the plain export.
[[ "$1" != *mtls* ]] && vars+=(NFS_CAN_BREAK=1)
breaking='resilience|kernel|recovery'
for binary in $(binaries "(?!($breaking)$).*"); do run_binary "$binary" 8; done
# Tests that break the network run one at a time (one would break the other's), and last:
# recovery restarts nfsd, and its grace period holds back every other client's opens.
for name in ${breaking//|/ }; do
  for binary in $(binaries "$name"); do run_binary "$binary" 1; done
done
echo "::notice title=Stage 2 $1::$report"
