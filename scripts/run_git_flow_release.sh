#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: ./scripts/run_git_flow_release.sh <version>" >&2
  echo "Example: ./scripts/run_git_flow_release.sh 0.3.0" >&2
  exit 2
fi

repository_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

version="$1"

if [[ -n "$(git status --porcelain)" ]]; then
  echo "error: the working tree must be clean before starting a release" >&2
  exit 1
fi

cargo set-version "$version" --workspace --dry-run >/dev/null
git flow release start "$version"
./scripts/update_version.sh "$version"

git add .
git commit -m "build: bump version to $version"
git flow release finish "$version" --message "Release $version"
