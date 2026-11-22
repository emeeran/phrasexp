//! Passive evdev key listener — the Linux event source that works on Wayland.
//!
//! rdev 0.5.3's `listen()` is XRecord-only: on a Wayland session it sees keystrokes only
//! from apps running under XWayland — native Wayland apps (GNOME Terminal, gedit,
//! Firefox…) are invisible, so autotext and spellcheck silently never fire. This module
//! reads `/dev/input/event*` directly instead. Requires read access to the devices
//! (the `input` group); purely passive, no grab — the compositor keeps delivering keys
//! to the focused app as usual.
//!
//! A supervisor rescan handles hotplug (AUDIT 86): every few seconds it opens any
//! newly-appeared `/dev/input/event*` node (keyboard plugged in, dock resumed after
//! suspend with renumbered devices) and restarts readers when ALL devices vanish —
//! previously that needed an app restart.

use std::collections::HashSet;
use std::fs::{read_dir, File};
use std::io::Read as _;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use rdev::{Event, EventType, Key};

/// The shared callback all device threads feed. Serialized (not parallel) because the
/// listener's typed-buffer state is single-threaded; keystrokes are human-rate, so the
/// lock is never meaningfully contended.
pub type SharedCallback = Arc<Mutex<dyn FnMut(Event) + Send>>;

/// `struct input_event` on 64-bit Linux: timeval (16B) + u16 type + u16 code + i32 value.
/// (32-bit Linux has a 16-byte layout — unsupported here; desktop targets are 64-bit.)
const RECORD_SIZE: usize = 24;

/// How often the supervisor rescans for hotplugged devices.
const RESCAN_INTERVAL_MS: u64 = 3000;

/// Open every readable evdev device, spawn a reader per device, and supervise for
/// hotplug until the device set disappears entirely. Returns false when nothing could
/// be opened so the caller can fall back to rdev's XRecord listener (X11 sessions
/// without the `input` group). Blocks for the listener's lifetime otherwise — the
/// caller relies on that for its running-flag and fallback ordering.
pub fn listen(callback: SharedCallback) -> bool {
    let Some(devices) = open_devices() else {
        return false;
    };
    if devices.is_empty() {
        return false;
    }
    crate::debug_log(&format!("evdev: {} device(s) opened", devices.len()));
    let supervisor = std::thread::spawn(move || {
        let mut opened: HashSet<String> = HashSet::new();
        let mut handles = Vec::new();
        for (node, file) in devices {
            opened.insert(node.clone());
            handles.push(spawn_reader(node, file, Arc::clone(&callback)));
        }
        loop {
            std::thread::sleep(std::time::Duration::from_millis(RESCAN_INTERVAL_MS));
            // Drop readers whose device disappeared (Ok(0) / error end).
            handles.retain(|h| !h.is_finished());
            // Open any newly-appeared devices (hotplug, resume renumbering).
            if let Some(devs) = open_devices() {
                for (node, file) in devs {
                    if opened.contains(&node) {
                        continue;
                    }
                    crate::debug_log(&format!("evdev: hotplug — opened {node}"));
                    opened.insert(node.clone());
                    handles.push(spawn_reader(node, file, Arc::clone(&callback)));
                }
            }
            // Every reader died (device swap): reopen from scratch; if nothing is
            // readable anymore, give up so the caller can fall back to rdev.
            if handles.is_empty() {
                match open_devices() {
                    Some(devs) if !devs.is_empty() => {
                        crate::debug_log("evdev: all readers gone — rescanning");
                        opened.clear();
                        for (node, file) in devs {
                            opened.insert(node.clone());
                            handles.push(spawn_reader(node, file, Arc::clone(&callback)));
                        }
                    }
                    _ => return false,
                }
            }
        }
    });
    // Block for the supervisor's lifetime, preserving listen()'s blocking contract.
    // A panicked supervisor (or one that found nothing readable on rescan) yields
    // false so the caller falls back to rdev.
    supervisor.join().unwrap_or(false)
}

/// Spawn one blocking reader thread for a device node; lives until the device goes.
fn spawn_reader(
    node: String,
    mut file: File,
    callback: SharedCallback,
) -> std::thread::JoinHandle<()> {
    // Tag each event with its source node — duplicate keyboards show up in the
    // debug log as the same key arriving from two different devices.
    let tag: std::sync::Arc<str> = node.into();
    std::thread::spawn(move || {
        // Event names exist only for the debug log (listen.rs); skip the per-event
        // allocation entirely when debugging is off.
        let debug = crate::debug_enabled();
        let mut buf = [0u8; RECORD_SIZE * 16];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break, // device gone (unplugged): stop reading it
                Ok(n) => {
                    for ev in buf[..n].as_chunks::<RECORD_SIZE>().0 {
                        let Some(event_type) = decode(ev) else {
                            continue;
                        };
                        // `*` = autorepeat (kernel value 2), as opposed to a physical
                        // press (1). Splits "keyboard chatter" from "held key too long".
                        let val = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                        // into_inner: a poisoned lock must not kill the listener.
                        let mut cb = callback.lock().unwrap_or_else(|e| e.into_inner());
                        cb(Event {
                            time: SystemTime::now(),
                            name: debug.then(|| {
                                if val == 2 {
                                    format!("{tag}*")
                                } else {
                                    tag.to_string()
                                }
                            }),
                            event_type,
                        });
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    })
}

fn open_devices() -> Option<Vec<(String, File)>> {
    let mut out = Vec::new();
    for entry in read_dir("/dev/input").ok()? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("event") {
            continue;
        }
        if let Ok(file) = File::open(entry.path()) {
            out.push((name, file));
        }
    }
    Some(out)
}

/// Decode one `input_event` record. Only EV_KEY events map to anything; unknown keycodes
/// (mouse buttons, media keys, numpad) are dropped — the listener treats them as noise,
/// which matches rdev's Unknown-key handling downstream without clearing the buffer.
fn decode(b: &[u8]) -> Option<EventType> {
    let type_ = u16::from_ne_bytes([b[16], b[17]]);
    let code = u16::from_ne_bytes([b[18], b[19]]);
    let value = i32::from_ne_bytes([b[20], b[21], b[22], b[23]]);
    if type_ != 1 {
        return None; // EV_KEY only
    }
    let key = map_code(code)?;
    match value {
        0 => Some(EventType::KeyRelease(key)),
        1 => Some(EventType::KeyPress(key)),
        // 2 = kernel typematic autorepeat. The Wayland compositor implements its own
        // repeat timer and DROPS these, so counting them desyncs the typed buffer from
        // the on-screen text (">emg" heard as ">>.emg") and every match silently fails.
        // This keyboard repeats every 24ms — a slightly-held key used to spray them.
        _ => None,
    }
}

/// Linux input keycode → rdev Key, covering what the listener consumes: characters,
/// navigation, modifiers, function keys. Numpad and media keys are intentionally
/// unmapped (dropped, not buffer-clearing Unknowns).
fn map_code(code: u16) -> Option<Key> {
    use Key::*;
    Some(match code {
        1 => Escape,
        2 => Num1,
        3 => Num2,
        4 => Num3,
        5 => Num4,
        6 => Num5,
        7 => Num6,
        8 => Num7,
        9 => Num8,
        10 => Num9,
        11 => Num0,
        12 => Minus,
        13 => Equal,
        14 => Backspace,
        15 => Tab,
        16 => KeyQ,
        17 => KeyW,
        18 => KeyE,
        19 => KeyR,
        20 => KeyT,
        21 => KeyY,
        22 => KeyU,
        23 => KeyI,
        24 => KeyO,
        25 => KeyP,
        26 => LeftBracket,
        27 => RightBracket,
        28 => Return,
        29 => ControlLeft,
        30 => KeyA,
        31 => KeyS,
        32 => KeyD,
        33 => KeyF,
        34 => KeyG,
        35 => KeyH,
        36 => KeyJ,
        37 => KeyK,
        38 => KeyL,
        39 => SemiColon,
        40 => Quote,
        41 => BackQuote,
        42 => ShiftLeft,
        43 => BackSlash,
        44 => KeyZ,
        45 => KeyX,
        46 => KeyC,
        47 => KeyV,
        48 => KeyB,
        49 => KeyN,
        50 => KeyM,
        51 => Comma,
        52 => Dot,
        53 => Slash,
        54 => ShiftRight,
        56 => Alt,
        57 => Space,
        58 => CapsLock,
        59 => F1,
        60 => F2,
        61 => F3,
        62 => F4,
        63 => F5,
        64 => F6,
        65 => F7,
        66 => F8,
        67 => F9,
        68 => F10,
        87 => F11,
        88 => F12,
        97 => ControlRight,
        100 => AltGr, // RightAlt == AltGr, as rdev maps it
        102 => Home,
        103 => UpArrow,
        104 => PageUp,
        105 => LeftArrow,
        106 => RightArrow,
        107 => End,
        108 => DownArrow,
        109 => PageDown,
        110 => Insert,
        111 => Delete,
        125 => MetaLeft,
        126 => MetaRight,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(type_: u16, code: u16, value: i32) -> [u8; RECORD_SIZE] {
        let mut b = [0u8; RECORD_SIZE];
        b[16..18].copy_from_slice(&type_.to_ne_bytes());
        b[18..20].copy_from_slice(&code.to_ne_bytes());
        b[20..24].copy_from_slice(&value.to_ne_bytes());
        b
    }

    #[test]
    fn decodes_press_release_repeat_and_noise() {
        assert_eq!(decode(&rec(1, 30, 1)), Some(EventType::KeyPress(Key::KeyA)));
        assert_eq!(
            decode(&rec(1, 30, 0)),
            Some(EventType::KeyRelease(Key::KeyA))
        );
        // Kernel autorepeat (value 2) must be dropped: the compositor ignores it, so
        // counting it makes the buffer diverge from the on-screen text.
        assert_eq!(decode(&rec(1, 30, 2)), None);
        assert_eq!(decode(&rec(1, 999, 1)), None); // unmapped keycode dropped
        assert_eq!(decode(&rec(2, 30, 1)), None); // non-EV_KEY (e.g. mouse move) dropped
        assert_eq!(decode(&rec(1, 114, 1)), None); // media key dropped, buffer untouched
    }

    #[test]
    fn every_key_the_listener_consumes_is_mapped() {
        // A missing mapping here would silently break buffer/correction logic that
        // XRecord used to cover: delimiters, nav clears, modifiers, the toggle hotkey.
        for code in [
            28u16, 14, 15, 1, 57, 58, 42, 54, 29, 97, 56, 100, 125, 126, 102, 103, 104, 105, 106,
            107, 108, 109, 110, 111, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88, 2, 11, 12, 13,
            39, 40, 51, 52, 53, 30, 44,
        ] {
            assert!(map_code(code).is_some(), "keycode {code} unmapped");
        }
    }

    #[test]
    fn shifted_a_stream_decodes_in_order() {
        // Shift held + A pressed → KeyPress(ShiftLeft) then KeyPress(KeyA); the existing
        // callback's modifier tracking turns that pair into 'A' via key_to_char.
        let mut buf = Vec::new();
        buf.extend_from_slice(&rec(1, 42, 1));
        buf.extend_from_slice(&rec(1, 30, 1));
        let events: Vec<EventType> = buf
            .as_chunks::<RECORD_SIZE>()
            .0
            .iter()
            .filter_map(|ev| decode(ev))
            .collect();
        assert_eq!(
            events,
            vec![
                EventType::KeyPress(Key::ShiftLeft),
                EventType::KeyPress(Key::KeyA)
            ]
        );
    }
}
