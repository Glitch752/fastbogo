#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

find_llvm_profdata() {
  if [[ -n "${LLVM_PROFDATA:-}" ]]; then
    printf '%s\n' "$LLVM_PROFDATA"
    return 0
  fi

  local sysroot
  sysroot="$(rustc --print sysroot)"

  if [[ -x "$sysroot/lib/rustlib/$(rustc -vV | awk '/host:/ { print $2 }')/bin/llvm-profdata" ]]; then
    printf '%s\n' "$sysroot/lib/rustlib/$(rustc -vV | awk '/host:/ { print $2 }')/bin/llvm-profdata"
    return 0
  fi

  if command -v llvm-profdata >/dev/null 2>&1; then
    printf '%s\n' "$(command -v llvm-profdata)"
    return 0
  fi

  return 1
}

rust_llvm_major() {
  rustc -vV | awk '/LLVM version:/ { split($3, parts, "."); print parts[1] }'
}

tool_llvm_major() {
  "$1" --version 2>/dev/null | awk '/LLVM version/ { split($3, parts, "."); print parts[1] }'
}

if ! LLVM_PROFDATA_BIN="$(find_llvm_profdata)"; then
  echo "llvm-profdata is required for PGO validation" >&2
  echo "Install the matching Rust toolchain component with: \`rustup component add llvm-tools-preview\`" >&2
  exit 1
fi

RUST_LLVM_MAJOR="$(rust_llvm_major)"
TOOL_LLVM_MAJOR="$(tool_llvm_major "$LLVM_PROFDATA_BIN")"
if [[ -z "$TOOL_LLVM_MAJOR" || "$TOOL_LLVM_MAJOR" != "$RUST_LLVM_MAJOR" ]]; then
  echo "llvm-profdata version mismatch: rustc uses LLVM $RUST_LLVM_MAJOR, but $LLVM_PROFDATA_BIN is LLVM ${TOOL_LLVM_MAJOR:-unknown}" >&2
  echo "Install the matching Rust toolchain component with \`rustup component add llvm-tools-preview\`" >&2
  exit 1
fi

PROFILE_DIR="${PROFILE_DIR:-$ROOT_DIR/target/pgo-data}"
PROFDATA="$PROFILE_DIR/merged.profdata"

if [[ ! -f "$PROFDATA" ]]; then
  echo "missing $PROFDATA; run scripts/pgo-generate.sh first" >&2
  exit 1
fi

"$LLVM_PROFDATA_BIN" show "$PROFDATA" >/dev/null

export RUSTFLAGS="-C target-cpu=native -C profile-use=$PROFDATA -C llvm-args=-pgo-warn-missing-function"
cargo build --release
echo "built release binary with PGO profile $PROFDATA"