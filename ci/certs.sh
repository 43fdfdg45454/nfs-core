#!/usr/bin/env bash
# Throwaway certificates for one CI run, in ci-certs/: a CA with a server and a client
# certificate, and an untrusted CA with its own client certificate for the refusal checks.
# EC P-256 keeps generation instant. The server names the test network's address only.
set -euo pipefail
mkdir -p ci-certs
cd ci-certs
new_ca() { # name
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 2 \
    -keyout "$1.key" -out "$1.pem" -subj "/CN=$1" \
    -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign,cRLSign
}
new_cert() { # name ca extensions
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout "$1.key" \
    -out "$1.csr" -subj "/CN=$1"
  printf '%s\n' "$3" > "$1.ext"
  openssl x509 -req -in "$1.csr" -CA "$2.pem" -CAkey "$2.key" -CAcreateserial -days 2 \
    -extfile "$1.ext" -out "$1.pem"
}
client="extendedKeyUsage=clientAuth
keyUsage=critical,digitalSignature"
new_ca ca
new_ca rogue-ca
new_cert server ca "subjectAltName=IP:127.0.0.1,IP:198.51.100.1
extendedKeyUsage=serverAuth
keyUsage=critical,digitalSignature"
new_cert client ca "$client"
new_cert rogue-client rogue-ca "$client"
chmod 644 ./*
