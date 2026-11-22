//! Text injection — the "type the expansion into the focused app" layer.
//!
//! Session-aware: `EnigoInjector` for Windows / macOS / Linux-X11; `YdotoolInjector`
//! for Linux-Wayland (uinput-based, works where Wayland blocks synthetic input).
//! Silent autotext listening is a separate concern (rdev).

#[cfg(target_os = "linux")] // only the ydotool path shells out
use std::process::Command;
use std::sync::Arc;

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::models::ContentType;

/// Inject text / backspaces / rich paste into the currently focused application.
pub trait Injector: Send + Sync {
    /// Type text keystroke-by-keystroke. Use for SHORT strings (e.g. spellcheck word
    /// corrections) — fast, but XTest can drop chars on long rapid input.
    fn type_text(&self, text: &str) -> DomainResult<()>;
    /// Paste text atomically via clipboard + Ctrl+V. Use for LONG expansions — can't drop
    /// chars. Saves/restores the user's clipboard.
    fn paste_text(&self, text: &str) -> DomainResult<()>;
    fn backspace(&self, n: usize) -> DomainResult<()>;
    /// Put HTML on the clipboard and paste (Ctrl+V) — for Markdown phrases. Saves/restores
    /// the user's clipboard, same as `paste_text`.
    fn paste_html(&self, html: &str, alt_text: &str) -> DomainResult<()>;
    /// Paste a PNG image file (clipboard image + Ctrl+V). Saves/restores the user's
    /// clipboard, same as `paste_text`.
    fn paste_image(&self, path: &str) -> DomainResult<()>;
    /// Copy the current selection in the focused app (Ctrl/Cmd+C) — used by the
    /// selection-based AI flow to capture selected text via the clipboard.
    fn copy_selection(&self) -> DomainResult<()>;
    /// Move the caret left `n` characters (for the {cursor} macro's end position).
    fn move_left(&self, n: usize) -> DomainResult<()>;
    /// Move the caret right `n` characters — collapses an active selection to its end
    /// (selection-AI "insert": keep the original, paste the rewrite after it).
    fn move_right(&self, n: usize) -> DomainResult<()>;
}

/// Windows, macOS, Linux-X11 (enigo uses X11/XTest on Linux).
pub struct EnigoInjector;

impl Injector for EnigoInjector {
    fn type_text(&self, text: &str) -> DomainResult<()> {
        let mut enigo = enigo()?;
        enigo.text(text).map_err(enigo_err)
    }

    fn backspace(&self, n: usize) -> DomainResult<()> {
        let mut enigo = enigo()?;
        for _ in 0..n {
            enigo.key(Key::Backspace, Click).map_err(enigo_err)?;
        }
        Ok(())
    }

    fn paste_html(&self, html: &str, alt_text: &str) -> DomainResult<()> {
        paste_with_clipboard(
            |c| c.set_html(html, Some(alt_text)),
            || {
                let mut enigo = enigo()?;
                paste_chord_enigo(&mut enigo)?;
                Ok(())
            },
        )
    }

    fn paste_image(&self, path: &str) -> DomainResult<()> {
        let image = clip_image(path)?;
        paste_with_clipboard(
            |c| c.set_image(image),
            || {
                let mut enigo = enigo()?;
                paste_chord_enigo(&mut enigo)?;
                Ok(())
            },
        )
    }

    fn paste_text(&self, text: &str) -> DomainResult<()> {
        paste_with_clipboard(
            |c| c.set_text(text),
            || {
                let mut enigo = enigo()?;
                paste_chord_enigo(&mut enigo)?;
                Ok(())
            },
        )
    }

    fn copy_selection(&self) -> DomainResult<()> {
        let mut enigo = enigo()?;
        enigo.key(PASTE_MODIFIER, Press).map_err(enigo_err)?;
        enigo.key(Key::Unicode('c'), Press).map_err(enigo_err)?;
        enigo.key(Key::Unicode('c'), Release).map_err(enigo_err)?;
        enigo.key(PASTE_MODIFIER, Release).map_err(enigo_err)?;
        Ok(())
    }

    fn move_left(&self, n: usize) -> DomainResult<()> {
        let mut enigo = enigo()?;
        for _ in 0..n {
            enigo.key(Key::LeftArrow, Click).map_err(enigo_err)?;
        }
        Ok(())
    }

    fn move_right(&self, n: usize) -> DomainResult<()> {
        let mut enigo = enigo()?;
        for _ in 0..n {
            enigo.key(Key::RightArrow, Click).map_err(enigo_err)?;
        }
        Ok(())
    }
}

/// The OS paste modifier for a synthetic V: Cmd on macOS, Ctrl everywhere else.
#[cfg(target_os = "macos")]
const PASTE_MODIFIER: Key = Key::Meta;
#[cfg(not(target_os = "macos"))]
const PASTE_MODIFIER: Key = Key::Control;

/// Map an enigo key/text failure to a DomainError. Enigo failures mean the
/// synthetic input never reached the target app — reporting them (instead of
/// returning Ok) is what keeps LAST_DELIVERY/usage stats and error logs honest.
fn enigo_err(e: enigo::InputError) -> crate::domain::error::DomainError {
    crate::domain::error::DomainError::Validation(format!("enigo: {e}"))
}

/// Paste chord via enigo (XTest / Win / macOS).
fn paste_chord_enigo(enigo: &mut Enigo) -> DomainResult<()> {
    enigo.key(PASTE_MODIFIER, Press).map_err(enigo_err)?;
    enigo.key(Key::Unicode('v'), Press).map_err(enigo_err)?;
    enigo.key(Key::Unicode('v'), Release).map_err(enigo_err)?;
    enigo.key(PASTE_MODIFIER, Release).map_err(enigo_err)?;
    Ok(())
}

/// Linux-Wayland — shells out to `ydotool` (uinput virtual keyboard).
/// Requires `ydotool` installed + `ydotoole` daemon + `/dev/uinput` access.
#[cfg(target_os = "linux")]
pub struct YdotoolInjector;

/// Ctrl+V via ydotool (29=LeftCtrl, 47=v).
#[cfg(target_os = "linux")]
fn ctrl_v_ydotool() -> DomainResult<()> {
    ydotool_key("29:1 47:1 47:0 29:0")
}

/// Run `ydotool key` with a press/release sequence like "29:1 47:1 47:0 29:0"
/// (input codes per the Linux uinput table; see call sites). A zero spawn status
/// with a nonzero exit means `ydotoold` isn't running or `/dev/uinput` is denied —
/// the keystrokes never landed, so that must surface as an error, not Ok.
#[cfg(target_os = "linux")]
fn ydotool_key(codes: &str) -> DomainResult<()> {
    let status = Command::new("ydotool")
        .arg("key")
        .args(codes.split_whitespace())
        .status()
        .map_err(|e| DomainError::Validation(format!("ydotool key: {e}")))?;
    if !status.success() {
        return Err(DomainError::Validation(format!(
            "ydotool key failed ({status}) — is ydotoold running?"
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
impl Injector for YdotoolInjector {
    fn type_text(&self, text: &str) -> DomainResult<()> {
        let out = Command::new("ydotool")
            .args(["type", "--", text])
            .output()
            .map_err(|e| DomainError::Validation(format!("ydotool not available: {e}")))?;
        if !out.status.success() {
            return Err(DomainError::Validation(format!(
                "ydotool type failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        Ok(())
    }

    fn backspace(&self, n: usize) -> DomainResult<()> {
        // Linux input code 14 = KEY_BACKSPACE; "14:1 14:0" = press + release.
        if n == 0 {
            return Ok(());
        }
        ydotool_key("14:1 14:0 ".repeat(n).trim_end())
    }

    fn paste_html(&self, html: &str, alt_text: &str) -> DomainResult<()> {
        paste_with_clipboard(|c| c.set_html(html, Some(alt_text)), ctrl_v_ydotool)
    }

    fn paste_image(&self, path: &str) -> DomainResult<()> {
        let image = clip_image(path)?;
        paste_with_clipboard(|c| c.set_image(image), ctrl_v_ydotool)
    }

    fn paste_text(&self, text: &str) -> DomainResult<()> {
        paste_with_clipboard(|c| c.set_text(text), ctrl_v_ydotool)
    }

    fn copy_selection(&self) -> DomainResult<()> {
        // Ctrl+C via ydotool (29=LeftCtrl, 46=c).
        ydotool_key("29:1 46:1 46:0 29:0")
    }

    fn move_left(&self, n: usize) -> DomainResult<()> {
        // Left arrow = 105 (KEY_LEFT).
        if n == 0 {
            return Ok(());
        }
        ydotool_key("105:1 105:0 ".repeat(n).trim_end())
    }

    fn move_right(&self, n: usize) -> DomainResult<()> {
        // Right arrow = 106 (KEY_RIGHT).
        if n == 0 {
            return Ok(());
        }
        ydotool_key("106:1 106:0 ".repeat(n).trim_end())
    }
}

/// A held handle to the shared Enigo — derefs to `&mut Enigo` at the call sites.
pub(crate) struct EnigoHandle(std::sync::MutexGuard<'static, Option<Enigo>>);

impl std::ops::Deref for EnigoHandle {
    type Target = Enigo;
    fn deref(&self) -> &Enigo {
        self.0.as_ref().expect("enigo slot filled before return")
    }
}

impl std::ops::DerefMut for EnigoHandle {
    fn deref_mut(&mut self) -> &mut Enigo {
        self.0.as_mut().expect("enigo slot filled before return")
    }
}

/// Build/reuse the shared Enigo instance, mapping init errors to DomainError.
/// Constructing an Enigo opens the X display (or platform equivalent) — doing
/// that per keystroke injection churns a connection per expansion. One
/// process-global instance, lock-guarded per operation (injection sites are
/// short and never nest).
pub(crate) fn enigo() -> Result<EnigoHandle, DomainError> {
    static ENIGO: std::sync::OnceLock<std::sync::Mutex<Option<Enigo>>> =
        std::sync::OnceLock::new();
    let cell = ENIGO.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(
            Enigo::new(&Settings::default())
                .map_err(|e| DomainError::Validation(format!("enigo init: {e}")))?,
        );
    }
    Ok(EnigoHandle(guard))
}

/// The echo-quiet condvar: the listener pings it after stamping `last_echo`, so
/// `quiet_clear` sleeps instead of polling the clock.
pub static ECHO_CV: std::sync::Condvar = std::sync::Condvar::new();

/// Called by the key listener on every observed echo of an injected key.
pub(crate) fn notify_echo() {
    ECHO_CV.notify_all();
}

/// Decode a PNG into clipboard image data (`arboard::set_image`).
fn clip_image(path: &str) -> DomainResult<arboard::ImageData<'static>> {
    let (rgba, w, h) = png_rgba(path)?;
    Ok(arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: std::borrow::Cow::Owned(rgba),
    })
}

/// Time to let a slow target app read the clipboard after a Ctrl+V before we restore the user's
/// prior content. Synchronous (blocking) on purpose: a detached restore would let two rapid
/// clipboard expansions clobber each other's saved state. 200ms covers slow paste consumers
/// (LibreOffice / Electron over IPC) that the previous 80ms race intermittently lost.
const CLIPBOARD_PASTE_SETTLE_MS: u64 = 200;

/// Put content on the clipboard via `set`, trigger a paste (Ctrl+V) via `paste`, then — after a
/// short settle — restore the user's prior clipboard. Shared by the text and HTML paths so both
/// save/restore (HTML used to clobber). Always restores: a readback gate would false-negative on
/// clipboard managers (Klipper/GPaste), and the blocking call makes an intervening copy impossible.
fn paste_with_clipboard(
    set: impl FnOnce(&mut arboard::Clipboard) -> Result<(), arboard::Error>,
    paste: impl FnOnce() -> DomainResult<()>,
) -> DomainResult<()> {
    let mut clip = arboard::Clipboard::new()
        .map_err(|e| DomainError::Validation(format!("clipboard: {e}")))?;
    let saved = clip.get_text().ok().filter(|s| !s.is_empty());
    set(&mut clip).map_err(|e| DomainError::Validation(format!("clipboard set: {e}")))?;
    paste()?;
    // Let the target app read the clipboard, then restore the user's prior clipboard.
    std::thread::sleep(std::time::Duration::from_millis(CLIPBOARD_PASTE_SETTLE_MS));
    if let Some(prev) = saved {
        let _ = clip.set_text(&prev);
    }
    Ok(())
}

/// Decode a PNG file into RGBA8 pixels for the clipboard (`arboard::set_image`).
/// `normalize_to_color8` yields 8-bit gray/gray-alpha/rgb/rgba — all normalized to RGBA here.
fn png_rgba(path: &str) -> DomainResult<(Vec<u8>, u32, u32)> {
    let file = std::fs::File::open(path)
        .map_err(|e| DomainError::Validation(format!("image ({path}): {e}")))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| DomainError::Validation(format!("image ({path}): {e}")))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| DomainError::Validation(format!("image ({path}): {e}")))?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((w as usize * h as usize) * 4);
            for px in buf.as_chunks::<3>().0 {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity((w as usize * h as usize) * 4);
            for px in buf.as_chunks::<2>().0 {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity((w as usize * h as usize) * 4);
            for g in buf {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        other => {
            return Err(DomainError::Validation(format!(
                "image ({path}): unsupported color type {other:?}"
            )))
        }
    };
    Ok((rgba, w, h))
}

/// Last fully-delivered expansion (content type + chunk stream), for the
/// repeat-last-output hotkey. Process-global: the app is single-instance and has
/// one injector, so there is exactly one "last output".
static LAST_DELIVERY: std::sync::Mutex<
    Option<(ContentType, Vec<crate::domain::models::OutChunk>)>,
> = std::sync::Mutex::new(None);

/// Retention cap for LAST_DELIVERY (bytes across text chunks + path strings):
/// bigger deliveries aren't recorded (repeat-last-output stays a no-op for them).
const LAST_DELIVERY_MAX: usize = 64 * 1024;

/// Snapshot of the last successful delivery (cloned — repeating must not consume it).
pub fn last_delivery() -> Option<(ContentType, Vec<crate::domain::models::OutChunk>)> {
    LAST_DELIVERY.lock().ok().and_then(|g| g.clone())
}

/// Best-effort injection step: the surrounding flow can't abort on failure (the
/// field is already mid-edit), but the failure must leave a trace — silent
/// `let _ =` is what made the "nothing happened" class of bug undiagnosable.
pub fn log_if_err(r: DomainResult<()>, step: &str) {
    if let Err(e) = r {
        eprintln!("[phrasexp] inject {step} failed: {e}");
        crate::debug_log(&format!("inject {step} failed: {e}"));
    }
}

/// Deliver an ordered chunk stream: text chunks in sequence, image pastes and timed
/// pauses at their body positions (a Delay lets slow apps catch up mid-template).
/// Each text chunk is delivered independently — a delay between two chunks means two
/// separate type/paste operations. A complete delivery is remembered for repeat-last-output.
pub fn deliver_chunks(
    injector: &dyn Injector,
    content_type: ContentType,
    chunks: &[crate::domain::models::OutChunk],
) -> DomainResult<()> {
    use crate::domain::models::OutChunk;
    let result = (|| -> DomainResult<()> {
        // {cursor} semantics: the next text chunk after a Cursor marker is delivered,
        // then the caret moves back to the marker by Left-pressing its char count.
        let mut cursor_pending = false;
        for chunk in chunks {
            match chunk {
                OutChunk::Text(text) => {
                    deliver_text(injector, content_type, text)?;
                    if cursor_pending {
                        cursor_pending = false;
                        // Markdown delivered as HTML: the count is the plain-text
                        // length — approximate by design (documented best-effort).
                        injector.move_left(text.chars().count())?;
                    }
                }
                OutChunk::Image(path) => injector.paste_image(path)?,
                OutChunk::Delay(ms) => {
                    std::thread::sleep(std::time::Duration::from_millis(
                        (*ms).clamp(0, 5000) as u64
                    ));
                }
                OutChunk::Cursor => cursor_pending = true,
            }
        }
        Ok(())
    })();
    if result.is_ok() {
        // Bound the retained delivery: repeat-last-output exists for text snippets,
        // not multi-MB documents — skipping huge expansions keeps the static from
        // pinning arbitrary output forever.
        let total: usize = chunks.iter().map(|c| match c {
            OutChunk::Text(t) => t.len(),
            OutChunk::Image(p) => p.len(),
            _ => 0,
        }).sum();
        if let Ok(mut g) = LAST_DELIVERY.lock() {
            if total <= LAST_DELIVERY_MAX {
                *g = Some((content_type, chunks.to_vec()));
            } else {
                *g = None;
            }
        }
    } else if let Err(e) = &result {
        // A failed delivery leaves the user's typed abbreviation erased with nothing
        // in its place — the headline "nothing happened" failure. Unconditional
        // stderr (like the other real-failure sites) plus the debug facility; the
        // expansion caller still gets the Err for its own surface.
        eprintln!("[phrasexp] injection failed — abbreviation erased, nothing typed: {e}");
        crate::debug_log(&format!("deliver_chunks failed: {e}"));
    }
    result
}

/// Deliver one text chunk according to the phrase's content type.
fn deliver_text(
    injector: &dyn Injector,
    content_type: ContentType,
    text: &str,
) -> DomainResult<()> {
    if text.is_empty() {
        return Ok(());
    }
    match content_type {
        ContentType::Markdown => injector.paste_html(&markdown_to_html(text), text)?,
        _ => {
            if types_reliably(text) {
                // Type real key events: clipboard+Ctrl+V is not "paste" in terminals
                // (they want Ctrl+Shift+V), so short plain text must be typed to
                // expand there at all.
                injector.type_text(text)?;
            } else {
                injector.paste_text(text)?;
            }
        }
    }
    Ok(())
}

/// Text that can be delivered by typing instead of pasting: ASCII (ydotool's uinput
/// keymap has no non-ASCII layout) and short enough that per-key typing stays quick.
/// Known limit: long or Unicode text would need xdotool `--clearmodifiers`+`type`
/// if anyone ever needs that in a terminal.
fn types_reliably(text: &str) -> bool {
    text.is_ascii() && text.chars().count() <= 200
}

/// Render Markdown source to an HTML fragment.
fn markdown_to_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// Pick the injector for the current OS/session.
/// Long expansions paste (robust); short spellcheck corrections type via enigo (fast).
pub fn make_injector() -> Arc<dyn Injector> {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var("XDG_SESSION_TYPE")
            .map(|s| s.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false);
        if wayland {
            return Arc::new(YdotoolInjector);
        }
    }
    Arc::new(EnigoInjector)
}

/// Test-only injector that records its calls as strings, so injection sequencing
/// (backspace counts, typed payloads) can be asserted without a real OS.
#[cfg(test)]
pub(crate) mod test_injector {
    use super::{deliver_chunks, types_reliably, ContentType, DomainResult, Injector};
    use std::sync::Mutex;

    pub struct RecordingInjector {
        pub calls: Mutex<Vec<String>>,
    }

    impl RecordingInjector {
        pub fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
        fn record(&self, s: String) {
            if let Ok(mut g) = self.calls.lock() {
                g.push(s);
            }
        }
        pub fn snapshot(&self) -> Vec<String> {
            self.calls.lock().map(|g| g.clone()).unwrap_or_default()
        }
    }

    impl Injector for RecordingInjector {
        fn type_text(&self, text: &str) -> DomainResult<()> {
            self.record(format!("type_text({text:?})"));
            Ok(())
        }
        fn paste_text(&self, text: &str) -> DomainResult<()> {
            self.record(format!("paste_text({text:?})"));
            Ok(())
        }
        fn backspace(&self, n: usize) -> DomainResult<()> {
            self.record(format!("backspace({n})"));
            Ok(())
        }
        fn paste_html(&self, html: &str, alt_text: &str) -> DomainResult<()> {
            self.record(format!("paste_html({html:?}, {alt_text:?})"));
            Ok(())
        }
        fn paste_image(&self, _path: &str) -> DomainResult<()> {
            Ok(())
        }
        fn copy_selection(&self) -> DomainResult<()> {
            self.record("copy_selection".to_string());
            Ok(())
        }
        fn move_left(&self, n: usize) -> DomainResult<()> {
            self.record(format!("move_left({n})"));
            Ok(())
        }
        fn move_right(&self, n: usize) -> DomainResult<()> {
            self.record(format!("move_right({n})"));
            Ok(())
        }
    }

    #[test]
    fn recording_injector_captures_order() {
        let inj = RecordingInjector::new();
        let _ = inj.backspace(4);
        let _ = inj.type_text("the ");
        assert_eq!(inj.snapshot(), vec!["backspace(4)", "type_text(\"the \")"]);
    }

    #[test]
    fn short_ascii_types_rather_than_pastes() {
        // Terminals don't bind Ctrl+V as paste, so deliver() must type these.
        assert!(types_reliably("sudo apt install"));
        assert!(!types_reliably("héllo — uñicode pastes"));
        assert!(!types_reliably(&"x".repeat(201)));
        assert!(types_reliably(&"x".repeat(200)));
    }

    #[test]
    fn deliver_types_plain_but_pastes_markdown() {
        use crate::domain::models::OutChunk;
        let inj = RecordingInjector::new();
        deliver_chunks(
            &inj,
            ContentType::Plain,
            &[OutChunk::Text("sudo apt install".into())],
        )
        .unwrap();
        deliver_chunks(
            &inj,
            ContentType::Markdown,
            &[OutChunk::Text("**bold**".into())],
        )
        .unwrap();
        // Delays sleep (unobservable here) but must not disturb the call stream.
        deliver_chunks(
            &inj,
            ContentType::Plain,
            &[OutChunk::Delay(0), OutChunk::Text("x".into())],
        )
        .unwrap();
        assert_eq!(
            inj.snapshot(),
            vec![
                "type_text(\"sudo apt install\")".to_string(),
                format!(
                    "paste_html({:?}, {:?})",
                    "<p><strong>bold</strong></p>\n", "**bold**"
                ),
                "type_text(\"x\")".to_string(),
            ]
        );
    }

    /// Roundtrip: encode a 2×1 RGBA PNG, decode via the same path `paste_image` uses,
    /// and assert pixels survive. Guards the decode/normalize logic against drift.
    #[test]
    fn png_rgba_roundtrip() {
        let path = std::env::temp_dir().join("phrasexp_png_roundtrip_test.png");
        let (w, h) = (2u32, 1u32);
        let pixels: Vec<u8> = vec![10, 20, 30, 255, 200, 150, 100, 128];
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = png::Encoder::new(file, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();

        let (rgba, dw, dh) = super::png_rgba(path.to_str().unwrap()).unwrap();
        assert_eq!((dw, dh), (w, h));
        assert_eq!(rgba, pixels);
        let _ = std::fs::remove_file(&path);
    }

    /// RGB (no alpha) input must normalize to opaque RGBA.
    #[test]
    fn png_rgb_becomes_opaque_rgba() {
        let path = std::env::temp_dir().join("phrasexp_png_rgb_test.png");
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = png::Encoder::new(file, 1, 1);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&[1, 2, 3])
            .unwrap();

        let (rgba, w, h) = super::png_rgba(path.to_str().unwrap()).unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(rgba, vec![1, 2, 3, 255]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn successful_delivery_is_recorded_for_repeat() {
        let inj = RecordingInjector::new();
        // LAST_DELIVERY is process-global and tests run in parallel — don't assert an
        // initial empty state; seed a known value and assert overwrite/peek instead.
        deliver_chunks(
            &inj,
            ContentType::Plain,
            &[crate::domain::models::OutChunk::Text("seed".into())],
        )
        .unwrap();
        deliver_chunks(
            &inj,
            ContentType::Plain,
            &[crate::domain::models::OutChunk::Text("once".into())],
        )
        .unwrap();
        // Peek semantics: repeating must not consume the recording.
        let first = super::last_delivery().expect("recorded after success");
        assert!(super::last_delivery().is_some());
        let (ct, chunks) = first;
        assert_eq!(ct, ContentType::Plain);
        assert_eq!(
            chunks,
            vec![crate::domain::models::OutChunk::Text("once".into())]
        );
        // A failed delivery never overwrites the last good one.
        struct FailingInjector;
        impl Injector for FailingInjector {
            fn type_text(&self, _: &str) -> DomainResult<()> {
                Err(crate::domain::error::DomainError::Validation("boom".into()))
            }
            fn paste_text(&self, _: &str) -> DomainResult<()> {
                Ok(())
            }
            fn backspace(&self, _: usize) -> DomainResult<()> {
                Ok(())
            }
            fn paste_html(&self, _: &str, _: &str) -> DomainResult<()> {
                Ok(())
            }
            fn paste_image(&self, _: &str) -> DomainResult<()> {
                Ok(())
            }
            fn copy_selection(&self) -> DomainResult<()> {
                Ok(())
            }
            fn move_left(&self, _: usize) -> DomainResult<()> {
                Ok(())
            }
            fn move_right(&self, _: usize) -> DomainResult<()> {
                Ok(())
            }
        }
        let _ = deliver_chunks(
            &FailingInjector,
            ContentType::Plain,
            &[crate::domain::models::OutChunk::Text("nope".into())],
        );
        let (_, chunks) = super::last_delivery().unwrap();
        assert_eq!(
            chunks,
            vec![crate::domain::models::OutChunk::Text("once".into())]
        );
    }

    #[test]
    fn cursor_moves_caret_back_after_tail_text() {
        let inj = RecordingInjector::new();
        deliver_chunks(
            &inj,
            ContentType::Plain,
            &[
                crate::domain::models::OutChunk::Text("Hello ".into()),
                crate::domain::models::OutChunk::Cursor,
                crate::domain::models::OutChunk::Text("world".into()),
            ],
        )
        .unwrap();
        // Tail delivered first, then caret returns over it: 5 chars of "world".
        assert_eq!(
            inj.snapshot(),
            vec![
                "type_text(\"Hello \")".to_string(),
                "type_text(\"world\")".to_string(),
                "move_left(5)".to_string(),
            ]
        );
        // No cursor marker → no caret moves.
        let inj = RecordingInjector::new();
        deliver_chunks(
            &inj,
            ContentType::Plain,
            &[crate::domain::models::OutChunk::Text("plain".into())],
        )
        .unwrap();
        assert!(!inj.snapshot().iter().any(|c| c.starts_with("move_left")));
    }
}
