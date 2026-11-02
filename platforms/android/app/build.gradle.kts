plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "um.a0.mobile"
    compileSdk = 35

    defaultConfig {
        applicationId = "um.a0.mobile"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            // Keep in sync with scripts/build-android.sh targets.
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            // TODO(release): configure a real signingConfig; unsigned for now.
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    packaging {
        jniLibs {
            // 16 KB page alignment (Android 15+ / NDK r28+): the native libs
            // produced by scripts/build-android.sh are already aligned via
            // RUSTFLAGS max-page-size; do NOT fall back to legacy packaging.
            useLegacyPackaging = false
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
    implementation(project(":A0Rust"))

    val composeBom = platform("androidx.compose:compose-bom:2024.10.01")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material3:material3-window-size-class")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.security:security-crypto:1.1.0-alpha06")
    debugImplementation("androidx.compose.ui:ui-tooling")
}
