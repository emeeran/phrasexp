# PhraseXp Keyboard (Android IME) — P0 spike

Scope and phases: `docs/scope-android-ime.md`. This spike proves the toolchain:
an installable IME whose keys commit text into any app, with the portable Rust
core (`../core/`) loaded as a JNI library (the leftmost key shows `core ✓`).

## Build

One-time: JDK 17, Android SDK (platform 34, build-tools), Rust targets and
`cargo-ndk`:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
```

Then:

```sh
scripts/build-android-core.sh     # builds .so → android/app/src/main/jniLibs/
cd android && gradle assembleDebug
# APK: android/app/build/outputs/apk/debug/app-debug.apk
```

CI does the same on every push touching `core/` or `android/`
(`.github/workflows/android.yml`) — no local SDK needed to verify changes.

## Install & try (device or emulator, API 26+)

```sh
adb install app/build/outputs/apk/debug/app-debug.apk
```

System Settings → System → Languages & input → On-screen keyboards → enable
**PhraseXp**, then switch keyboards (notification bar or `adb shell ime set
com.phrasexp.keyboard/.PhraseXpImeService`). Focus any text field, tap
`hello`.

## Layout

Deliberately four keys (`core ✓ | hello | ⌫ | ↵`). The QWERTY layout, rolling
trigger buffer, and expansion are P2 — this spike only proves the commit path
and the JNI bridge.
