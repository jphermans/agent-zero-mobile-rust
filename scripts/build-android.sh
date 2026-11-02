#!/usr/bin/env bash
#
# scripts/build-android.sh
#
# Builds the Rust core (crate: a0-platform, crate-type: cdylib) for
# arm64-v8a, armeabi-v7a and x86_64 via cargo-ndk, copies the artifacts into
# the A0Rust Gradle module and generates the UniFFI 0.31 Kotlin bindings
# (library mode, package um.a0.platform).
#
# Generated artifacts (NOT checked in):
#   platforms/android/A0Rust/src/main/jniLibs/<abi>/liba0_platform.so
#   platforms/android/A0Rust/src/main/kotlin/um/a0/platform/*.kt
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE="a0-platform"
A0RUST_DIR="$REPO_ROOT/platforms/android/A0Rust"
JNI_LIBS="$A0RUST_DIR/src/main/jniLibs"
KOTLIN_OUT="$A0RUST_DIR/src/main/kotlin"

# --- 0. Toolchain sanity ------------------------------------------------------
command -v cargo >/dev/null || { echo "error: cargo not found (install rustup)" >&2; exit 1; }
command -v cargo-ndk >/dev/null || { echo "error: cargo-ndk missing (see scripts/bootstrap.sh)" >&2; exit 1; }
command -v uniffi-bindgen >/dev/null || { echo "error: uniffi-bindgen missing (see scripts/bootstrap.sh)" >&2; exit 1; }
: "${ANDROID_NDK_HOME:?ANDROID_NDK_HOME must point at the Android NDK (r28+ recommended)}"

# 16 KB page alignment: required by Android 15+ / NDK r28+ for native libraries.
export RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"

cd "$REPO_ROOT"

# --- 1. Build + place cdylibs per ABI (cargo-ndk writes jniLibs/<abi>/) --------
echo "==> Building $CRATE (cdylib) via cargo-ndk"
cargo ndk --platform 26 \
    -t arm64-v8a -t armeabi-v7a -t x86_64 \
    -o "$JNI_LIBS" \
    build --release -p "$CRATE"

# --- 2. UniFFI Kotlin bindings (library mode; package comes from the crate's
#        uniffi.toml -> um.a0.platform) ------------------------------------------
echo "==> Generating UniFFI Kotlin bindings (library mode, package um.a0.platform)"
mkdir -p "$KOTLIN_OUT"
uniffi-bindgen generate --library "target/aarch64-linux-android/release/liba0_platform.so" \
    --language kotlin --out-dir "$KOTLIN_OUT"

echo "==> Done. Native libraries:"
find "$JNI_LIBS" -name '*.so' -print
echo "==> Kotlin bindings: $KOTLIN_OUT/um/a0/platform"
