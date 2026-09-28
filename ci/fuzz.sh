#!/usr/bin/env bash
# One fuzz target for a while (cargo-fuzz, fuzz/). A failure is annotated with what failed and the
# input that did it, in base64: logs cannot be read.
# Usage: ci/fuzz.sh <target> <seconds>
set -uo pipefail
target="$1" seconds="$2"
mkdir -p "fuzz/corpus/$target"
# The target is explicit: a prebuilt cargo-fuzz (musl) would build for its own, where sanitizers cannot run.
cargo fuzz run -O --target x86_64-unknown-linux-gnu "$target" -- -max_total_time="$seconds" -rss_limit_mb=2048 -timeout=10 2>&1 | tee fuzz.log
status=${PIPESTATUS[0]}
runs=$(grep -o 'Done [0-9]* runs' fuzz.log | tail -1)
if [ "$status" -eq 0 ]; then
  echo "::notice title=Fuzz $target::${runs:-done} in $seconds s; corpus $(ls "fuzz/corpus/$target" | wc -l) inputs"
  exit 0
fi
what=$(grep -m1 -E "panicked|ERROR: libFuzzer|SUMMARY" fuzz.log | cut -c1-300)
input=$(ls fuzz/artifacts/"$target"/* 2>/dev/null | head -1)
if [ -z "$input" ]; then
  # Nothing found by the fuzzer: it failed before (the build, the toolchain).
  echo "::error title=Fuzz $target did not run::$(tail -25 fuzz.log | cut -c1-200 | awk '{printf "%s%%0A", $0}')"
  exit 1
fi
echo "::error title=Fuzz $target::$what%0Ainput (base64): $(base64 -w0 "$input" | cut -c1-6000)"
exit 1
