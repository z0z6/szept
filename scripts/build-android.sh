#!/usr/bin/env bash
# Builds the Android app: Rust core -> Kotlin bindings -> APK.
#
# Needs: the Android SDK and NDK (ANDROID_HOME, ANDROID_NDK_HOME), JDK 17 or
# 21, and cargo-ndk (`cargo install cargo-ndk`).
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -s)" in
    Darwin) host_lib=target/debug/libkomunikator_ffi.dylib ;;
    *) host_lib=target/debug/libkomunikator_ffi.so ;;
esac

echo "==> Kotlin bindings"
cargo build --locked -p komunikator-ffi --features cli
rm -rf android/app/src/main/java/uniffi
target/debug/uniffi-bindgen generate \
    --library "$host_lib" \
    --language kotlin \
    --no-format \
    --out-dir android/app/src/main/java

echo "==> Rust core for Android"
# The list of targets must match abiFilters in android/app/build.gradle.kts.
cargo ndk -t arm64-v8a -t x86_64 \
    -o android/app/src/main/jniLibs \
    build --release --locked -p komunikator-ffi

echo "==> APK"
cd android
sh ./gradlew --no-daemon assembleDebug
echo "Gotowe: android/app/build/outputs/apk/debug/app-debug.apk"
