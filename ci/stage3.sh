#!/usr/bin/env bash
# Stage 3 on one runner: one of the engine's scenarios (crates/engine/tests) against nfsd,
# with the good-experience limits enforced.
# Usage: ci/stage3.sh quic[-mtls][-vpn] <scenario>
set -Eeuo pipefail
trap 'echo "failed at $BASH_SOURCE:$LINENO: $BASH_COMMAND"' ERR
source ci/host.sh
prepare nfs-engine "$1"
export_dir=/srv/nfs; [[ "$1" == *mtls* ]] && export_dir=/srv/mtls-data
ci/fixtures.sh "$export_dir/fixtures" 256
vars+=(NFS_ENFORCE_LIMITS=1 NFS_REQUIRE_DELEGATIONS=1)
for binary in $(binaries "$2"); do
  run_binary "$binary" 1
done
echo "::notice title=Stage 3 $1 $2::$report"
