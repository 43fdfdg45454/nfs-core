#!/usr/bin/env bash
# A GitHub release v<version> of this commit, once per version (the workspace's in Cargo.toml).
# Usage: ci/release.sh <version>
set -euo pipefail
version="$1"
gh release view "v$version" > /dev/null 2>&1 && { echo "v$version exists"; exit 0; }
image="ghcr.io/$GITHUB_REPOSITORY_OWNER/nfs-gateway:$version"
gh release create "v$version" --target "$GITHUB_SHA" --title "nfs-core $version" \
  --notes "The gateway's image: \`$image\` (amd64, arm64). Setup: [docs/gateway.md](https://github.com/$GITHUB_REPOSITORY/blob/v$version/docs/gateway.md)."
echo "::notice title=Release::v$version"
