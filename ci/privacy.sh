#!/usr/bin/env bash
# The repository is public: fails when a tracked file or a commit message of the push carries an
# IPv4 address outside loopback and the documentation ranges (RFC 5737), or a host name of a
# home network. The rule itself is in CLAUDE.md.
# Usage: ci/privacy.sh [commit range]
set -uo pipefail
allowed='^(127\.|0\.0\.0\.0$|192\.0\.2\.|198\.51\.100\.|203\.0\.113\.)'
ips() { grep -o -E '\b([0-9]{1,3}\.){3}[0-9]{1,3}\b' | grep -v -E "$allowed"; }
hosts() { grep -o -i -E '\b[a-z0-9-]+\.(lan|home|local|internal|localdomain)\b'; }
text="$(git grep -I -h -E '[0-9]\.[0-9]|\.(lan|home|local|internal)' -- . ':!ci/privacy.sh')"
[ -n "${1:-}" ] && text+=$'\n'"$(git log --format=%B "$1")"
found="$(ips <<< "$text"; hosts <<< "$text")"
if [ -n "$found" ]; then
  echo "Not allowed in a public repository:"
  echo "$found" | sort -u
  exit 1
fi
