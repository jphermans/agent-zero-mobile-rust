// A0Rust: Android library module that exposes the Rust core to the app.
//
// Checked in: this build file + src/main/README.md
// Generated (by scripts/build-android.sh, NOT checked in):
//   src/main/jniLibs/<abi>/liba0_platform.so
//   src/main/kotlin/um/a0/platform/*.kt  (UniFFI 0.31 bindings, package um.a0.platform)
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "um.a0.platform"
    compileSdk = 35

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    sourceSets {
        getByName("main") {
            // Generated UniFFI Kotlin bindings (scripts/build-android.sh).
            java.srcDirs("src/main/kotlin")
            // Rust cdylibs per ABI (scripts/build-android.sh).
            jniLibs.srcDir("src/main/jniLibs")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    // UniFFI-generated Kotlin performs FFI through JNA (com.sun.jna imports
    // in the generated bindings); Android needs the @aar artifact variant.
    implementation("net.java.dev.jna:jna:5.16.0@aar")
}
