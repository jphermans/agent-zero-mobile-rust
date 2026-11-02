# scripts/

Build and bootstrap scripts for the Agent Zero Mobile Rust core.

| Script | Purpose | Runs on |
|---|---|---|
| `build-ios.sh` | Builds `liba0_platform.a` (staticlib) for `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`; lipos the simulator slice; assembles `A0Platform.xcframework`; generates UniFFI 0.31 Swift bindings (library mode) into `platforms/ios/RustBridge/`. | macOS (Xcode 16+) |
| `build-android.sh` | Builds `liba0_platform.so` (cdylib) for `arm64-v8a`, `armeabi-v7a`, `x86_64` via `cargo ndk --platform 26`; copies into `A0Rust/src/main/jniLibs/`; generates UniFFI 0.31 Kotlin bindings (library mode, package `um.a0.platform`) into `A0Rust/src/main/kotlin/`. | Linux/macOS (NDK r28+) |
| `bootstrap.sh` | Installs rustup targets (3 iOS + 3 Android), `cargo-ndk`, `uniffi-bindgen-cli` 0.31.0; prints XcodeGen/Homebrew notes. | Linux/macOS |

## Prerequisites

- Rust via rustup (stable; MSRV 1.85.0 is enforced by CI `rust.yml`).
- iOS: Xcode 16+ (`xcrun`, `lipo`, `xcodebuild`), `uniffi-bindgen` on PATH.
- Android: JDK 17, Android SDK + NDK (r28+ recommended), `ANDROID_NDK_HOME` set, `cargo-ndk`, `uniffi-bindgen` on PATH.

## Expected artifacts

- iOS: `platforms/ios/Generated/` (intermediate), `platforms/ios/RustBridge/Sources/A0Platform/*.swift`, `platforms/ios/RustBridge/Frameworks/A0Platform.xcframework`.
- Android: `platforms/android/A0Rust/src/main/jniLibs/{arm64-v8a,armeabi-v7a,x86_64}/liba0_platform.so`, `platforms/android/A0Rust/src/main/kotlin/um/a0/platform/*.kt`.

## Notes

- 16 KB page alignment: `build-android.sh` exports `RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"` (Android 15+ / NDK r28+ requirement).
- UniFFI version pin: 0.31.x; keep `uniffi-bindgen-cli` in sync with the `uniffi` dependency of the `a0-platform` crate.
- Bindings are generated in **library mode** (`uniffi-bindgen generate --library ...`), so the whole crate facade is exported from the single native library.
- Nothing in `scripts/` requires git; the scripts are plain, commented bash (`set -euo pipefail`).
