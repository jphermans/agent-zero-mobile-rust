# Agent Zero Android ProGuard/R8 rules.

# UniFFI generated Kotlin bindings (um.a0.platform) use JNI entry points that
# R8 must not rename or remove: they are looked up by name from liba0_platform.so.
-keep class um.a0.platform.** { *; }
-keepclassmembers class um.a0.platform.** { *; }

# Keep the app-side RustBridge/KeystoreStore glue implementing the UniFFI
# SecretStore callback interface (invoked from JNI).
-keep class um.a0.mobile.rust.** { *; }

# Compose is fine by default; keep lambdas/method names for crash readability.
-keepattributes SourceFile,LineNumberTable,InnerClasses,EnclosingMethod
