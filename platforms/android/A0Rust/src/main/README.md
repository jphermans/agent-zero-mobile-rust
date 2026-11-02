# A0Rust module

Android library module exposing the Rust core (`a0-platform` crate) to the app.

## Generated content (NOT checked in)

Everything under `src/main/jniLibs/` and `src/main/kotlin/` is produced by
`scripts/build-android.sh`:

- `src/main/jniLibs/arm64-v8a/liba0_platform.so`
- `src/main/jniLibs/armeabi-v7a/liba0_platform.so`
- `src/main/jniLibs/x86_64/liba0_platform.so`
  (cdylibs built via `cargo ndk --platform 26`, 16 KB page-aligned via RUSTFLAGS)
- `src/main/kotlin/um/a0/platform/*.kt`
  (UniFFI 0.31 Kotlin bindings, **library mode**, package `um.a0.platform`)

## Checked in

- `build.gradle.kts` — wires the generated `kotlin` and `jniLibs` source sets.
- `consumer-rules.pro` — keeps the JNI surface for downstream consumers.
- this README.

Run `../../scripts/build-android.sh` from the repo root (or
`./scripts/build-android.sh`) before the first Gradle build.
