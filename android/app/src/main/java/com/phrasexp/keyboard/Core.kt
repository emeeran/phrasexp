package com.phrasexp.keyboard

// JNI facade onto the portable Rust core (core/ crate). One object, small
// surface — the FFI boundary stays a facade, not per-function bindings.
object Core {
    init {
        System.loadLibrary("phrasexp_core")
    }

    // Toolchain smoke test: true when the Rust .so loaded and answers.
    external fun ping(): Boolean
}
