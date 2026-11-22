//! PhraseXp portable core — P0 spike.
//!
//! P1 will move `domain`, `db`, and the `triggers/autotext` pure modules here
//! (see docs/scope-android-ime.md). For now this crate only proves the
//! toolchain: it builds as an rlib for host tests and a cdylib for Android,
//! and the JNI `ping` is callable from the keyboard app.

/// Toolchain smoke test — the Kotlin side calls this through JNI and shows
/// the result in the keyboard's status key.
pub fn ping() -> bool {
    true
}

#[cfg(target_os = "android")]
mod jni {
    use jni::objects::JClass;
    use jni::sys::jboolean;
    use jni::JNIEnv;

    #[no_mangle]
    pub extern "system" fn Java_com_phrasexp_keyboard_Core_ping(
        _env: JNIEnv,
        _class: JClass,
    ) -> jboolean {
        super::ping() as jboolean
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ping_responds() {
        assert!(super::ping());
    }
}
