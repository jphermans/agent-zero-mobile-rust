#!/usr/bin/env bash
#
# scripts/bootstrap.sh
#
# One-time development bootstrap: installs the Rust cross-compilation targets
# and binding tooling used by build-ios.sh / build-android.sh. Platform
# prerequisites (Xcode, Android SDK/NDK) are printed but NOT installed.
#
set -euo pipefail

echo "==> Installing Rust iOS targets"
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios

echo "==> Installing Rust Android targets"
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

echo "==> Installing cargo-ndk"
cargo install cargo-ndk

echo "==> uniffi-bindgen runs from the workspace: cargo run -p a0-platform --features bindgen --bin uniffi-bindgen"

echo "==> Optional (macOS): XcodeGen to regenerate AgentZero.xcodeproj from project.yml"
if command -v brew >/dev/null; then
    echo "    run: brew install xcodegen"
else
    echo "    (install Homebrew or XcodeGen manually to regenerate the Xcode project)"
fi

echo "==> Android prerequisites (manual): JDK 17, Android SDK + NDK (r28+ recommended"
echo "    for 16 KB page alignment), ANDROID_NDK_HOME pointing at the NDK."
echo "==> iOS prerequisites (manual): Xcode 16+ (xcodebuild, lipo)."
echo "==> Bootstrap done."
