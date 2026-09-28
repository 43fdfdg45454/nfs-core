#!/usr/bin/env bash
# Runs one command. On failure publishes its last lines as an error annotation (job logs are
# not readable from outside), then fails; on success passes its notices through.
# Usage: ci/run.sh <title> <command...>
title="$1"
shift
log="$(mktemp)"
"$@" > "$log" 2>&1
status=$?
if [ "$status" -ne 0 ]; then
  # GitHub cuts long annotations: only the end, without colors or cargo's progress.
  body="$(sed -e 's/\x1b\[[0-9;]*m//g' "$log" | grep -vE '^\s*$|^\s*(Compiling|Checking|Downloaded?) ' \
    | tail -n 40 | cut -c1-300 | tail -c 3500 \
    | sed -e 's/%/%25/g' -e 's/\r//g' -e 's/::/: :/g' | awk '{printf "%s%%0A", $0}')"
  echo "::error title=${title} (exit ${status})::${body}"
  exit "$status"
fi
grep '^::notice' "$log" || true
