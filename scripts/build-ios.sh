#!/usr/bin/env bash
#
# scripts/build-ios.sh
#
# Builds the Rust core (crate: a0-platform, crate-type: staticlib) for
# aarch64-apple-ios, aarch64-apple-ios-sim and x86_64-apple-ios, generates the
# UniFFI 0.31 Swift bindings (library mode) and assembles the
# A0Platform.xcframework (ios-arm64 + ios-arm64_x86_64-simulator).
#
# Generated artifacts (NOT checked in):
#   platforms/ios/Generated/                                      (intermediates)
#   platforms/ios/RustBridge/Sources/A0Platform/A0Platform.swift  (bindings)
#   platforms/ios/RustBridge/Frameworks/A0Platform.xcframework    (staticlib)
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE="a0-platform"
IOS_OUT="$REPO_ROOT/platforms/ios/Generated"
RUST_BRIDGE="$REPO_ROOT/platforms/ios/RustBridge"
DEVICE_TRIPLE="aarch64-apple-ios"
SIM_ARM_TRIPLE="aarch64-apple-ios-sim"
SIM_X86_TRIPLE="x86_64-apple-ios"

command -v cargo >/dev/null || { echo "error: cargo not found (rustup)" >&2; exit 1; }
command -v xcodebuild >/dev/null || { echo "error: xcodebuild not found (Xcode 16+)" >&2; exit 1; }
command -v uniffi-bindgen >/dev/null || { echo "error: uniffi-bindgen missing (see scripts/bootstrap.sh)" >&2; exit 1; }

export IPHONEOS_DEPLOYMENT_TARGET=17.0
cd "$REPO_ROOT"

# --- 1. Static libraries per Rust target ---------------------------------------
echo "==> Building $CRATE (staticlib) for device + simulator targets"
for triple in "$DEVICE_TRIPLE" "$SIM_ARM_TRIPLE" "$SIM_X86_TRIPLE"; do
    cargo build --release -p "$CRATE" --target "$triple"
done

DEVICE_LIB="target/$DEVICE_TRIPLE/release/liba0_platform.a"
SIM_ARM_LIB="target/$SIM_ARM_TRIPLE/release/liba0_platform.a"
SIM_X86_LIB="target/$SIM_X86_TRIPLE/release/liba0_platform.a"

mkdir -p "$IOS_OUT"

# --- 2. Simulator fat slice (arm64 sim + x86_64 sim) ---------------------------
echo "==> Lipoing simulator slice (arm64-sim + x86_64)"
SIM_FAT="$IOS_OUT/liba0_platform-sim.a"
lipo -create -output "$SIM_FAT" "$SIM_ARM_LIB" "$SIM_X86_LIB"

# --- 3. UniFFI Swift bindings FIRST (library mode) -----------------------------
# A0Platform.swift + A0Platform.h + A0Platform.modulemap; the modulemap/header
# are embedded into the xcframework Headers below so the RustBridge SPM binary
# target exposes the A0Platform Swift module to the app.
echo "==> Generating UniFFI Swift bindings (library mode -> A0Platform.swift)"
mkdir -p "$RUST_BRIDGE/Sources/A0Platform"
uniffi-bindgen generate \
    --library "$DEVICE_LIB" \
    --language swift \
    --out-dir "$IOS_OUT/swift"
cp "$IOS_OUT/swift/A0Platform.swift" "$RUST_BRIDGE/Sources/A0Platform/A0Platform.swift"

# --- 4. XCFramework (ios-arm64 + ios-arm64_x86_64-simulator) -------------------
echo "==> Assembling RustBridge/Frameworks/A0Platform.xcframework"
# Copy ALL generated C headers + the modulemap into both xcframework slice
# header dirs AND the SPM include dir (TODO(bindings): exact generated file
# names are finalized by uniffi-bindgen 0.31; we glob to stay robust).
mkdir -p "$IOS_OUT/headers-device" "$IOS_OUT/headers-sim"          "$RUST_BRIDGE/Sources/A0Platform/include"
for h in "$IOS_OUT/swift"/*.h; do
    [ -e "$h" ] || continue
    cp "$h" "$IOS_OUT/headers-device/"
    cp "$h" "$IOS_OUT/headers-sim/"
    cp "$h" "$RUST_BRIDGE/Sources/A0Platform/include/"
done
# Xcode/SPM expect the module map named exactly module.modulemap.
for m in "$IOS_OUT/swift"/*.modulemap; do
    [ -e "$m" ] || continue
    cp "$m" "$IOS_OUT/headers-device/module.modulemap"
    cp "$m" "$IOS_OUT/headers-sim/module.modulemap"
    cp "$m" "$RUST_BRIDGE/Sources/A0Platform/include/module.modulemap"
done
rm -rf "$RUST_BRIDGE/Frameworks/A0Platform.xcframework"
xcodebuild -create-xcframework \
    -library "$DEVICE_LIB" -headers "$IOS_OUT/headers-device" \
    -library "$SIM_FAT"     -headers "$IOS_OUT/headers-sim" \
    -output "$RUST_BRIDGE/Frameworks/A0Platform.xcframework"

echo "==> iOS build complete:"
find "$RUST_BRIDGE/Frameworks" -maxdepth 2 -print
