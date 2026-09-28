# Link profiles, sourced by the CI scripts: netem for each direction, MTU and a title.
# vpn is the reference for every good-experience limit (see CLAUDE.md).
case "$1" in
  vpn) netem="delay 50ms loss 0.3% rate 100mbit limit 10000"
       mtu=1420
       title="VPN 100 ms RTT 0.3% loss 100 Mbps MTU 1420" ;;
  lan) netem="delay 3ms rate 400mbit limit 10000"
       mtu=1500
       title="LAN 6 ms RTT 400 Mbps" ;;
  *) echo "Unknown profile $1" >&2; return 2 ;;
esac
