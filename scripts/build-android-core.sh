#!/usr/bin/env bash
# Build the portable core (.so) for Android ABIs into the keyboard app's jniLibs.
# Requires: rustup target add aarch64-linux-android x86_64-linux-android
#           cargo install cargo-ndk
set -euo pipefail
cd "$(dirname "$0")/../core"
OUT=../android/app/src/main/jniLibs
cargo ndk -t arm64-v8a -t x86_64 -o "$OUT" build --release
echo "built:"; ls -la "$OUT"/*/libphrasexp_core.so
