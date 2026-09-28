#!/usr/bin/env bash
# The engine scenarios' videos, written on the server itself (not across the link): each 8-byte
# word holds its offset divided by 8, plus the file's label (the letter before ".bin") << 48.
# The engine's change and delete scenarios alter theirs (edited, grown, shrunk, gone): a second run
# needs them written again.
# Usage: ci/fixtures.sh <directory> <MiB per file>
set -euo pipefail
sudo mkdir -p "$1"
pids=()
for name in movie-a movie-b movie-c movie-d movie-e movie-f gone-g gone-h edited-i grown-j shrunk-k; do
  sudo python3 - "$1/$name.bin" "$2" "${name: -1}" <<'PY' &
import sys
from array import array
path, mib, label = sys.argv[1], int(sys.argv[2]), ord(sys.argv[3])
words, step = mib << 17, 1 << 20
with open(path, "wb") as f:
    for start in range(0, words, step):
        f.write(array("Q", range((label << 48) + start, (label << 48) + min(start + step, words))).tobytes())
PY
  pids+=($!)
done
for pid in "${pids[@]}"; do wait "$pid"; done
