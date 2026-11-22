package com.phrasexp.keyboard

import android.inputmethodservice.InputMethodService
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout

// P0 spike keyboard: proves the three things P2 needs — the service installs,
// text can be committed into any app, and the Rust core answers over JNI.
// A real layout (QWERTY, long-press, suggestion strip) is P2 work.
class PhraseXpImeService : InputMethodService() {

    private lateinit var status: Button

    override fun onCreateInputView(): View {
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }

        fun key(label: String, onClick: () -> Unit): Button =
            Button(this).apply {
                text = label
                layoutParams = LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT,
                )
                setOnClickListener { onClick() }
                row.addView(this)
            }

        status = key(if (Core.ping()) "core ✓" else "core ✗") {
            currentInputConnection?.commitText("PhraseXp", 1)
        }
        key("hello") { currentInputConnection?.commitText("hello ", 1) }
        key("⌫") { sendDownUpKeyEvents(android.view.KeyEvent.KEYCODE_DEL) }
        key("↵") { sendDownUpKeyEvents(android.view.KeyEvent.KEYCODE_ENTER) }

        return row
    }
}
