#!/bin/sh
# Builds GitLance (release) and opens it.
#   ./run.sh              the tabs of the last session
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

repo=
case "${1:-}" in
    "") ;;
    /*) repo=$1 ;;
    *) repo=$caller/$1 ;;
esac

if [ "$profile" = release ]; then
    cargo build --release
else
    cargo build
fi
if [ -n "$repo" ]; then
    exec "target/$profile/gitlance" "$repo"
fi
exec "target/$profile/gitlance"
