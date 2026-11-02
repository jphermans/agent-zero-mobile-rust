# Consumer rules for apps depending on A0Rust:
# the UniFFI JNI surface is looked up by name from liba0_platform.so.
-keep class um.a0.platform.** { *; }
