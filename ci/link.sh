#!/usr/bin/env bash
# The client side lives in the network namespace "cli" behind a veth pair: the server is
# 198.51.100.1, the client 198.51.100.2, and netem shapes both directions per ci/profiles.sh.
# Usage: ci/link.sh <profile>
set -euo pipefail
source "$(dirname "$0")/profiles.sh" "$1"
sudo ip netns add cli
sudo ip link add vh type veth peer name vc
sudo ip link set vc netns cli
sudo ip addr add 198.51.100.1/24 dev vh
sudo ip link set vh mtu "$mtu" up
sudo ip netns exec cli ip addr add 198.51.100.2/24 dev vc
sudo ip netns exec cli ip link set vc mtu "$mtu" up
sudo ip netns exec cli ip link set lo up
sudo tc qdisc add dev vh root netem $netem
sudo ip netns exec cli tc qdisc add dev vc root netem $netem
