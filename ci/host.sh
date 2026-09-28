# Sourced by the stage scripts: build a package's tests while the server and the link come up,
# the variables that point the tests at it, and running test binaries in the client's namespace.

# prepare <package> <shard>: shard names the transport (tcp or quic), mtls, and the link (vpn).
prepare() {
  local package="$1" shard="$2" profile=lan
  [[ "$shard" == *vpn* ]] && profile=vpn
  cargo test --release --locked --no-run -p "$package" --message-format=json > build-tests.json 2> build-rust.log & tests=$!
  if [[ "$shard" == quic* ]]; then cargo build --release --locked -p nfs-gateway >> build-rust.log 2>&1; fi
  ci/certs.sh > /dev/null 2>&1
  ci/nfsd.sh
  ci/link.sh "$profile"
  wait "$tests" || { tail -40 build-rust.log; exit 1; }
  vars=(NFS_SERVER=198.51.100.1:2049 NFS_EXPORT=/ NFS_TLS_DIR="$PWD/ci-certs" RUST_BACKTRACE=1)
  [[ "$shard" == *mtls* ]] && vars+=(NFS_TLS=mtls NFS_EXPORT=/mtls)
  if [[ "$shard" == quic* ]]; then
    ci/gateway.sh bbr/1.5
    vars+=(NFS_GATEWAY=198.51.100.1:443 NFS_GATEWAY_NAME=198.51.100.1)
  fi
  report=""
}

# The package's test binaries whose name matches $1.
binaries() {
  python3 -c "
import json, re, sys
for line in open('build-tests.json'):
    m = json.loads(line)
    if m.get('reason') == 'compiler-artifact' and m.get('executable') and m['target']['kind'] == ['test'] \
            and re.fullmatch(sys.argv[1], m['target']['name']):
        print(m['executable'])" "$1"
}

# run_binary <binary> <threads>: runs it in the client's namespace; notes its result and any
# "RESULT" lines it prints.
run_binary() {
  local name start output
  name=$(basename "$1" | sed 's/-[0-9a-f]*$//')
  start=$(date +%s)
  output=$(sudo ip netns exec cli env "${vars[@]}" "$1" --nocapture --test-threads="$2" 2>&1) \
    || { echo "$output" | tail -60; exit 1; }
  report+="$name: $(grep -o 'test result: .*passed' <<< "$output" | head -1) in $(( $(date +%s) - start )) s%0A"
  # The test harness may print "test name ..." on the same line before a result.
  report+="$(sed -n 's/.*RESULT /  /p' <<< "$output" | awk '{printf "%s%%0A", $0}')"
}
