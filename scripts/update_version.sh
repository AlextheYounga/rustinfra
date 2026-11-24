#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: ./update_version.sh <version>" >&2
  echo "Example: ./update_version.sh 0.3.0" >&2
  exit 2
fi

if ! cargo set-version --help >/dev/null 2>&1; then
  echo "error: cargo-edit is required; install it with: cargo install cargo-edit" >&2
  exit 1
fi

repository_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

cargo set-version "$1" --workspace
cargo metadata --no-deps --format-version 1 >/dev/null

echo "Updated Rustinfra workspace crates to version $1"
