//! Shared grammar for hotkey strings ("Control+Alt+Space"). The global-shortcut
//! backend (`lib.rs`) and the key-listener backend (`triggers::autotext::listen`)
//! map the resolved key token to their own enums, but the split + modifier-alias
//! table must stay single-source: a settings string flows through both paths.

/// Split a hotkey string into held modifiers and the terminal key token.
/// Returns `(shift, ctrl, alt, meta, key_token)`; `key_token` is the last
/// non-modifier part (already lowercased), or `None` if there isn't one.
pub(crate) fn split_chord(s: &str) -> Option<(bool, bool, bool, bool, String)> {
    let (mut sh, mut ct, mut al, mut me) = (false, false, false, false);
    let mut key: Option<String> = None;
    for raw in s.split('+') {
        let part = raw.trim().to_lowercase();
        match part.as_str() {
            "control" | "ctrl" => ct = true,
            "shift" => sh = true,
            "alt" | "option" | "opt" => al = true,
            "meta" | "super" | "win" | "cmd" | "command" => me = true,
            other => key = Some(other.to_string()),
        }
    }
    Some((sh, ct, al, me, key?))
}

#[cfg(test)]
mod tests {
    use super::split_chord;

    #[test]
    fn splits_modifiers_and_terminal_key() {
        let (sh, ct, al, me, key) = split_chord("Control+Alt+A").unwrap();
        assert_eq!((sh, ct, al, me), (false, true, true, false));
        assert_eq!(key, "a");
    }

    #[test]
    fn resolves_modifier_aliases() {
        let (_, ct, al, me, key) = split_chord("ctrl+opt+win+f8").unwrap();
        assert_eq!((ct, al, me), (true, true, true));
        assert_eq!(key, "f8");
    }

    #[test]
    fn terminal_key_wins_over_duplicates() {
        let (sh, _, _, _, key) = split_chord("Shift+A+Shift+Space").unwrap();
        assert!(sh);
        assert_eq!(key, "space");
    }

    #[test]
    fn modifiers_only_have_no_key() {
        assert!(split_chord("Control+Shift").is_none());
    }
}
