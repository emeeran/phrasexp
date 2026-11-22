//! Clipboard auto-capture — polls the system clipboard and records text changes
//! into the bounded history. Toggleable via the `clipboard_enabled` flag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::db::Db;
use crate::domain::models::ClipboardEntry;

// Clipboard changes aren't time-critical (it's history capture), so poll on a leisurely
// interval to minimize idle wakeups/CPU. A captured entry may lag by at most this long.
const POLL_MS: u64 = 1500;

pub fn start(
    db: Arc<Db>,
    enabled: Arc<AtomicBool>,
    max_length: Arc<std::sync::atomic::AtomicU32>,
    max_entries: Arc<std::sync::atomic::AtomicU32>,
) {
    std::thread::spawn(move || {
        // Created inside the thread (arboard::Clipboard is not guaranteed Send).
        let mut clip = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[phrasexp] clipboard init error: {e}");
                return;
            }
        };
        let mut last: Option<String> = clip.get_text().ok().filter(|s| !s.is_empty());

        loop {
            std::thread::sleep(Duration::from_millis(POLL_MS));
            if !enabled.load(Ordering::Relaxed) {
                last = None;
                continue;
            }
            let max = max_length.load(std::sync::atomic::Ordering::Relaxed) as usize;
            match clip.get_text() {
                Ok(text) => {
                    if text.is_empty() || Some(&text) == last.as_ref() {
                        continue;
                    }
                    if max > 0 && text.len() > max {
                        last = Some(text);
                        continue;
                    }
                    crate::debug_log(&format!("clipboard captured: {} chars", text.len()));
                    last = Some(text.clone());
                    let cap = max_entries.load(std::sync::atomic::Ordering::Relaxed) as usize;
                    let entry = ClipboardEntry {
                        id: uuid::Uuid::new_v4().to_string(),
                        text,
                        captured_at: crate::domain::now_iso(),
                    };
                    let _ = db.add_clipboard(&entry, cap);
                }
                Err(e) => {
                    crate::debug_log(&format!("clipboard get_text error: {e}"));
                }
            }
        }
    });
}
