#!/bin/sh
# Builds GitLance (release) and opens it.
#   ./run.sh              the repository this is run from, else the last opened one
#   ./run.sh <path>       the repository at <path>
#   ./run.sh --debug ...  a debug build, faster to compile
set -eu

caller=$(pwd)
cd "$(dirname "$0")"

profile=release
if [ "${1:-}" = "--debug" ]; then
    profile=debug
    shift
fi

if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "run.sh: cargo not found — install Rust with rustup: https://rustup.rs" >&2
    exit 1
fi

case "${1:-$caller}" in
    /*) repo=${1:-$caller} ;;
    *) repo=$caller/$1 ;;
esac

if [ "$profile" = release ]; then
    cargo build --release
else
    cargo build
fi
exec "target/$profile/gitlance" "$repo"
