#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

cargo test --locked --manifest-path prns-ffi/Cargo.toml --features linux-packet --lib ethernet::tests
cargo clippy --locked --manifest-path prns-ffi/Cargo.toml --features linux-packet --all-targets -- -D warnings
cargo clippy --locked --manifest-path prns-interfaces/impls/tokio/Cargo.toml --features wifi-halow --all-targets -- -D warnings
