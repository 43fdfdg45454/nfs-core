#!/usr/bin/env bash
# What the gateway must never do: show nfsd another origin, carry to anything but nfsd, let in
# a client without a certificate of the CA, or touch the export's TLS. Needs ci/tunnel.sh first.
set -Eeuo pipefail
trap 'echo "failed at $BASH_SOURCE:$LINENO: $BASH_COMMAND"' ERR
root="$PWD"
bin="$root/target/release"
certs="$root/ci-certs"
report=""
netns() { sudo ip netns exec cli env "PATH=$PATH" GITHUB_ACTIONS=true "$@"; }
peers() { sudo ss -Htn state established '( sport = :2049 )' | awk '{print $4}' | cut -d: -f1 | sort -u | xargs; }
write_origin() { # file: through the tunnel, into the export only 198.51.100.2 and .3 may use
  netns sh -c "mkdir -p /mnt/o && mount -t nfs -o vers=4.2,soft,timeo=100,retrans=4 127.0.0.1:/origin /mnt/o \
    && echo ok > /mnt/o/$1 && umount /mnt/o"
}

write_origin first
report+="Origin: written through the tunnel with a forged X-Forwarded-For; nfsd sees: $(peers)%0A"

# A second tunnel client for each refusal, on its own port.
refused() { # port expected-log-text tunnel-client-args...
  local port="$1" expected="$2"; shift 2
  netns "$bin/nfs-tunnel-client" --listen "127.0.0.1:$port" --gateway 198.51.100.1:443 \
    --server-name 198.51.100.1 --ca "$certs/ca.pem" "$@" > "refused-$port.log" 2>&1 &
  sleep 0.5
  netns "$bin/nfs-rpc-probe" --server "127.0.0.1:$port" --seconds 1 > /dev/null 2>&1 && return 1
  sleep 0.5
  grep -q "$expected" "refused-$port.log" || { cat "refused-$port.log"; return 1; }
  report+="Refused as expected: $(grep -o "tunnel: .*" "refused-$port.log" | head -1 | cut -c1-120)%0A"
}
identity=(--cert "$certs/client.pem" --key "$certs/client.key")
# The authority is not a destination: a tunnel asked for another port still reaches nfsd.
netns "$bin/nfs-tunnel-client" --listen 127.0.0.1:2052 --gateway 198.51.100.1:443 \
  --server-name 198.51.100.1 --ca "$certs/ca.pem" "${identity[@]}" --authority 198.51.100.1:22 \
  > other-port.log 2>&1 &
sleep 0.5
netns "$bin/nfs-rpc-probe" --server 127.0.0.1:2052 --seconds 1 > /dev/null 2>&1 \
  || { echo "a tunnel naming port 22 did not reach nfsd"; cat other-port.log; exit 1; }
report+="A tunnel naming port 22 reached nfsd, and only nfsd%0A"
refused 2053 "Connection error" --cert "$certs/rogue-client.pem" --key "$certs/rogue-client.key"
refused 2054 "Connection error"

# The gateway's name is looked up at each new QUIC connection, as TCP does: pointed first at an
# address nobody answers, then at the gateway, the same tunnel client gets through.
echo "198.51.100.9 gateway.test" | sudo tee -a /etc/hosts > /dev/null
netns "$bin/nfs-tunnel-client" --listen 127.0.0.1:2055 --gateway gateway.test:443 \
  --server-name 198.51.100.1 --ca "$certs/ca.pem" "${identity[@]}" > renamed.log 2>&1 &
sleep 0.5
probe() { netns timeout 20 "$bin/nfs-rpc-probe" --server 127.0.0.1:2055 --seconds 1 > /dev/null 2>&1; }
probe && { echo "a name pointing nowhere reached nfsd"; exit 1; }
sudo sed -i 's/^198\.51\.100\.9 gateway\.test$/198.51.100.1 gateway.test/' /etc/hosts
probe || { echo "the name, pointed at the gateway, was not looked up again"; cat renamed.log; exit 1; }
report+="A name looked up at each connection: pointed elsewhere and then at the gateway, it got through%0A"

# The export's mutual TLS end to end: nfs-core's client does RPC-with-TLS with tlshd inside the
# tunnel (the tunnel client on 127.0.0.1:2049 carries it).
output=$(netns NFS_SERVER=127.0.0.1:2049 NFS_EXPORT=/mtls NFS_TLS=mtls NFS_TLS_DIR="$certs" \
  NFS_WRITE=1 NFS_SECONDS=3 "$bin/nfs-bench" 2>&1) || { echo "$output" | tail -20; exit 1; }
report+="Export mTLS through the tunnel: $(tail -1 <<< "$output")%0A"

# A new address: the QUIC connection cannot follow (no migration), the client connects again from
# the new address, and nfsd sees that one.
sudo ip netns exec cli sysctl -qw net.ipv4.conf.all.promote_secondaries=1 net.ipv4.conf.vc.promote_secondaries=1
sudo ip netns exec cli ip addr add 198.51.100.3/24 dev vc
sudo ip netns exec cli ip addr del 198.51.100.2/24 dev vc
start=$(date +%s)
write_origin second
report+="New address: written again after $(( $(date +%s) - start )) s; nfsd sees: $(peers)"
echo "::notice title=Gateway checks::$report"
