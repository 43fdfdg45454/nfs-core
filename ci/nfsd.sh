#!/usr/bin/env bash
# nfsd with tlshd (the server side of RPC-with-TLS) and three exports under the NFSv4 pseudo-root:
# /srv/nfs (any transport), /srv/nfs/tls and /srv/nfs/mtls. Needs ci/certs.sh first.
set -euo pipefail
packages="nfs-kernel-server ktls-utils iperf3"
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends $packages \
  || { sudo apt-get update -q
       sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends $packages; }

sudo install -d -m 755 /etc/tlshd
sudo install -m 644 ci-certs/ca.pem ci-certs/server.pem /etc/tlshd/
sudo install -m 600 ci-certs/server.key /etc/tlshd/
printf '%s\n' '[authenticate]' '[authenticate.server]' 'x509.truststore=/etc/tlshd/ca.pem' \
  'x509.certificate=/etc/tlshd/server.pem' 'x509.private_key=/etc/tlshd/server.key' \
  | sudo tee /etc/tlshd.conf /etc/tlshd/config > /dev/null
# tlshd before 0.10 ignores x509.truststore and checks client certificates against the system
# trust store only.
sudo install -m 644 ci-certs/ca.pem /usr/local/share/ca-certificates/nfs-core-test-ca.crt
sudo update-ca-certificates > /dev/null
sudo modprobe tls
sudo systemctl restart tlshd

# nfsd applies a nested export's xprtsec policy only at a mount point: bind mounts make them so.
sudo mkdir -p /srv/nfs/tls /srv/nfs/mtls /srv/nfs/origin /srv/tls-data /srv/mtls-data /srv/origin-data
sudo mount --bind /srv/tls-data /srv/nfs/tls
sudo mount --bind /srv/mtls-data /srv/nfs/mtls
sudo mount --bind /srv/origin-data /srv/nfs/origin
sudo chmod 1777 /srv/nfs /srv/nfs/tls /srv/nfs/mtls /srv/nfs/origin
options=rw,insecure,no_root_squash,no_subtree_check
export_line() { # path options
  echo "$1 127.0.0.1($options,$2) 198.51.100.0/24($options,$2)"
}
{
  export_line /srv/nfs fsid=0,crossmnt,xprtsec=none:tls:mtls
  export_line /srv/nfs/tls fsid=2,xprtsec=tls:mtls
  export_line /srv/nfs/mtls fsid=3,xprtsec=mtls
  # Only the client's own addresses: through the gateway, access proves nfsd sees them.
  echo "/srv/nfs/origin 198.51.100.2($options,fsid=4) 198.51.100.3($options,fsid=4)"
} | sudo tee /etc/exports > /dev/null
# A restart (crates/nfs/tests/recovery.rs) waits out the grace period: 15 s, not 90.
sudo nfsconf --set nfsd grace-time 15
# NFSv4 only: with v3 nfsd starts lockd, and while lockd is in grace nfsd grants no delegation,
# though opens already work (the delegation test failed in the fastest shard).
sudo nfsconf --set nfsd vers2 n
sudo nfsconf --set nfsd vers3 n
sudo systemctl restart nfs-kernel-server
sudo exportfs -ra
