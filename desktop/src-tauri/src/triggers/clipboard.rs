//! Clipboard auto-capture (M5) — polls the system clipboard and records text changes
//! into the bounded history (FR-9). Toggleable via the `clipboard_enabled` flag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::db::Db;
use crate::domain::models::ClipboardEntry;

const POLL_MS: u64 = 800;

pub fn start(db: Arc<Db>, enabled: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        // Created inside the thread (arboard::Clipboard is not guaranteed Send).
        let mut clip = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("clipboard init error: {e}");
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
            if let Ok(text) = clip.get_text() {
                if !text.is_empty() && Some(&text) != last.as_ref() {
                    last = Some(text.clone());
                    let entry = ClipboardEntry {
                        id: uuid::Uuid::new_v4().to_string(),
                        text,
                        captured_at: chrono::Utc::now().to_rfc3339(),
                    };
                    let _ = db.add_clipboard(&entry);
                }
            }
        }
    });
}
