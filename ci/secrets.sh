#!/usr/bin/env bash
# Secrets in the whole history (gitleaks, its release checked against its checksum): the
# repository is public. Findings are annotated redacted.
set -euo pipefail
version=8.28.0 sum=a65b5253807a68ac0cafa4414031fd740aeb55f54fb7e55f386acb52e6a840eb
tarball=gitleaks_${version}_linux_x64.tar.gz
curl -sSL -o "/tmp/$tarball" "https://github.com/gitleaks/gitleaks/releases/download/v$version/$tarball"
echo "$sum  /tmp/$tarball" | sha256sum -c --quiet
tar xzf "/tmp/$tarball" -C /tmp gitleaks
if ! /tmp/gitleaks git --no-banner --redact --report-format csv --report-path leaks.csv . ; then
  echo "::error title=Secrets::$(cut -d, -f1-4 leaks.csv | head -20 | awk '{printf "%s%%0A", $0}')"
  exit 1
fi
echo "::notice title=Secrets::none in $(git rev-list --count HEAD) commits"
