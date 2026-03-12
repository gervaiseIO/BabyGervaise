#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 6 ]]; then
  echo "usage: $0 <workspace-root> <debug|release> <output-dir> <android-api> <android-abi> <rust-target>" >&2
  exit 1
fi

WORKSPACE_ROOT="$(cd "$1" && pwd)"
PROFILE="$2"
OUTPUT_DIR="$3"
ANDROID_API="$4"
ANDROID_ABI="$5"
RUST_TARGET="$6"
TMPDIR_ROOT="$WORKSPACE_ROOT/rust_core/target/android-tmp"

mkdir -p "$TMPDIR_ROOT"
export TMPDIR="${TMPDIR:-$TMPDIR_ROOT}"
export TMP="${TMP:-$TMPDIR}"
export TEMP="${TEMP:-$TMPDIR}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"

SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
NDK_ROOT="${ANDROID_NDK_HOME:-}"

find_executable() {
  local candidate
  for candidate in "$@"; do
    if [[ -n "$candidate" && -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

CARGO_BIN="$(
  find_executable \
    "${CARGO_HOME:-$HOME/.cargo}/bin/cargo" \
    "/opt/homebrew/opt/rustup/bin/cargo" \
    "/usr/local/opt/rustup/bin/cargo"
)" || CARGO_BIN="cargo"

RUSTC_BIN="$(
  find_executable \
    "${CARGO_HOME:-$HOME/.cargo}/bin/rustc" \
    "/opt/homebrew/opt/rustup/bin/rustc" \
    "/usr/local/opt/rustup/bin/rustc"
)" || RUSTC_BIN="rustc"

RUST_TOOLCHAIN_PATHS=()
if [[ "$CARGO_BIN" == */* ]]; then
  RUST_TOOLCHAIN_PATHS+=("$(dirname "$CARGO_BIN")")
fi
if [[ "$RUSTC_BIN" == */* ]]; then
  RUST_TOOLCHAIN_PATHS+=("$(dirname "$RUSTC_BIN")")
fi
if [[ ${#RUST_TOOLCHAIN_PATHS[@]} -gt 0 ]]; then
  RUST_TOOLCHAIN_PATH="$(printf '%s\n' "${RUST_TOOLCHAIN_PATHS[@]}" | awk '!seen[$0]++' | paste -sd: -)"
  export PATH="$RUST_TOOLCHAIN_PATH:$PATH"
fi
export RUSTC="$RUSTC_BIN"

if [[ -z "$NDK_ROOT" ]]; then
  NDK_PARENT="$SDK_ROOT/ndk"
  if [[ ! -d "$NDK_PARENT" ]]; then
    echo "Android NDK not found. Expected a directory at $NDK_PARENT" >&2
    exit 1
  fi
  NDK_ROOT="$(find "$NDK_PARENT" -mindepth 1 -maxdepth 1 -type d | sort | tail -n 1)"
fi

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)
    HOST_TAG="darwin-arm64"
    ;;
  Darwin-x86_64)
    HOST_TAG="darwin-x86_64"
    ;;
  Linux-x86_64)
    HOST_TAG="linux-x86_64"
    ;;
  Linux-aarch64)
    HOST_TAG="linux-aarch64"
    ;;
  *)
    echo "Unsupported host platform: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

PREBUILT_ROOT="$NDK_ROOT/toolchains/llvm/prebuilt"
if [[ ! -d "$PREBUILT_ROOT/$HOST_TAG" ]]; then
  FALLBACK_HOST_TAG="$(find "$PREBUILT_ROOT" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort | head -n 1)"
  if [[ -z "$FALLBACK_HOST_TAG" ]]; then
    echo "No NDK prebuilt toolchains found under $PREBUILT_ROOT" >&2
    exit 1
  fi
  HOST_TAG="$FALLBACK_HOST_TAG"
fi

TOOLCHAIN="$PREBUILT_ROOT/$HOST_TAG/bin"
LINKER="$TOOLCHAIN/aarch64-linux-android${ANDROID_API}-clang"
AR="$TOOLCHAIN/llvm-ar"
RANLIB="$TOOLCHAIN/llvm-ranlib"
SYSROOT="$("$RUSTC_BIN" --print sysroot)"
TARGET_LIBDIR="$SYSROOT/lib/rustlib/$RUST_TARGET/lib"

if [[ ! -x "$LINKER" ]]; then
  echo "Android linker not found at $LINKER" >&2
  exit 1
fi

if [[ ! -d "$TARGET_LIBDIR" ]]; then
  cat >&2 <<EOF
Rust target support for $RUST_TARGET is not installed for the current toolchain.

Install rustup, then run:
  rustup target add $RUST_TARGET

Current cargo binary:
  $CARGO_BIN

Current rustc binary:
  $RUSTC_BIN

Current rustc sysroot:
  $SYSROOT
EOF
  exit 1
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$AR"
export CC_aarch64_linux_android="$LINKER"
export AR_aarch64_linux_android="$AR"
export RANLIB_aarch64_linux_android="$RANLIB"

CARGO_ARGS=(
  build
  --manifest-path "$WORKSPACE_ROOT/rust_core/Cargo.toml"
  --target "$RUST_TARGET"
)

if [[ "$PROFILE" == "release" ]]; then
  CARGO_ARGS+=(--release)
fi

echo "Building baby_gervaise_core for $ANDROID_ABI ($RUST_TARGET, profile=$PROFILE)"
"$CARGO_BIN" "${CARGO_ARGS[@]}"

ARTIFACT_DIR="$WORKSPACE_ROOT/rust_core/target/$RUST_TARGET/$PROFILE"
ARTIFACT_PATH="$ARTIFACT_DIR/libbaby_gervaise_core.so"

if [[ ! -f "$ARTIFACT_PATH" ]]; then
  echo "Expected Rust artifact not found: $ARTIFACT_PATH" >&2
  exit 1
fi

DEST_DIR="$OUTPUT_DIR/$ANDROID_ABI"
mkdir -p "$DEST_DIR"
cp "$ARTIFACT_PATH" "$DEST_DIR/libbaby_gervaise_core.so"
echo "Copied $ARTIFACT_PATH -> $DEST_DIR/libbaby_gervaise_core.so"
