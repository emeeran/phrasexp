//! Pure macro evaluation + expansion.
//! chrono's strftime formatting is deterministic for the tokens we use.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use super::error::{DomainError, DomainResult};
use super::models::{Macro, Segment};

const PREVIEW_LIMIT: usize = 60;

/// Pure: identical (macro, inputs, now) -> identical output.
/// Shell macro bounds: kill the child after this long, keep at most this much
/// stdout (the tail beyond the cap is discarded — the child is NOT killed for
/// exceeding it, output just stops being captured).
const SHELL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const SHELL_OUTPUT_CAP: usize = 64 * 1024;

/// Wait for `child` to exit, up to `timeout`. Returns `Some(status)` on exit,
/// `None` on expiry (caller kills the child). Poll loop on the worker thread —
/// a cross-platform `wait_timeout` without extra deps.
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
) -> Option<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

pub fn evaluate_macro(
    m: &Macro,
    inputs: &HashMap<String, String>,
    now: DateTime<Utc>,
) -> DomainResult<String> {
    // Date/time macros expand in the user's local timezone, not UTC.
    let local = now.with_timezone(&chrono::Local);
    match m {
        Macro::DateTime { format } => Ok(local.format(format).to_string()),
        Macro::DateMath {
            days_offset,
            format,
        } => Ok((local + Duration::days(*days_offset))
            .format(format)
            .to_string()),
        Macro::InputForm { field, .. } => inputs
            .get(field)
            .cloned()
            .ok_or_else(|| DomainError::MissingInput(field.clone())),
        Macro::Random { min, max } => {
            use rand::Rng;
            let lo = (*min).min(*max);
            let hi = (*min).max(*max);
            Ok(rand::thread_rng().gen_range(lo..=hi).to_string())
        }
        Macro::Math { expr } => {
            let val = meval::eval_str(expr)
                .map_err(|e| DomainError::Validation(format!("math expression: {e}")))?;
            Ok(format_number(val))
        }
        Macro::Env { var } => Ok(std::env::var(var).unwrap_or_default()),
        Macro::Pick { options } => {
            use rand::seq::SliceRandom;
            Ok(options
                .choose(&mut rand::thread_rng())
                .cloned()
                .unwrap_or_default())
        }
        Macro::File { path } => std::fs::read_to_string(path)
            .map_err(|e| DomainError::Validation(format!("file macro ({path}): {e}"))),
        // Clipboard needs the system clipboard — resolved in Db::expand, not here.
        Macro::Clipboard => Err(DomainError::Validation(
            "clipboard macro must be evaluated via Db::expand".into(),
        )),
        // Image contributes no text — collected into the expansion's image list in Db::expand.
        Macro::Image { .. } => Err(DomainError::Validation(
            "image macro must be evaluated via Db::expand".into(),
        )),
        // Nested needs DB access to fetch the linked phrase — resolved in Db::expand.
        Macro::Nested { .. } => Err(DomainError::Validation(
            "nested macro must be evaluated via Db::expand".into(),
        )),
        // Ai needs settings + a network call — resolved in Db::expand.
        Macro::Ai { .. } => Err(DomainError::Validation(
            "ai macro must be evaluated via Db::expand".into(),
        )),
        // Cite needs a Crossref lookup (network) — resolved in Db::expand.
        Macro::Cite { .. } => Err(DomainError::Validation(
            "cite macro must be evaluated via Db::expand".into(),
        )),
        // AiAction needs DB + settings + a network call — resolved in Db::expand.
        Macro::AiAction { .. } => Err(DomainError::Validation(
            "ai action macro must be evaluated via Db::expand".into(),
        )),
        Macro::Uuid => Ok(uuid::Uuid::new_v4().to_string()),
        Macro::Shell { command } => {
            // Bounded shell run: a hung command used to wedge the expansion worker
            // forever (the child inherits no output consumer and waits on it), and an
            // enormous stdout used to balloon memory. Timeout + capped capture — the
            // child is killed on expiry and the error names the timeout.
            let (program, flag) = if cfg!(target_os = "windows") {
                ("cmd", "/C")
            } else {
                ("sh", "-c")
            };
            let mut child = std::process::Command::new(program)
                .args([flag, command])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| DomainError::Validation(format!("shell macro: {e}")))?;
            let Some(status) = wait_with_timeout(&mut child, SHELL_TIMEOUT) else {
                let _ = child.kill();
                let _ = child.wait();
                return Err(DomainError::Validation(format!(
                    "shell macro timed out after {}s: {}",
                    SHELL_TIMEOUT.as_secs(),
                    command
                )));
            };
            if !status.success() {
                return Err(DomainError::Validation(format!(
                    "shell macro exited with {status}: {command}"
                )));
            }
            // Cap the captured stdout; a truncated tail is safer than unbounded memory.
            let mut buf = Vec::new();
            if let Some(mut stdout) = child.stdout.take() {
                use std::io::Read;
                let mut chunk = [0u8; 8192];
                loop {
                    match stdout.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let room = SHELL_OUTPUT_CAP.saturating_sub(buf.len());
                            buf.extend_from_slice(&chunk[..n.min(room)]);
                            if buf.len() >= SHELL_OUTPUT_CAP {
                                break;
                            }
                        }
                    }
                }
            }
            let _ = child.wait();
            Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
        }
        // Counter needs DB state — resolved in Db::expand.
        Macro::Counter { .. } => Err(DomainError::Validation(
            "counter macro must be evaluated via Db::expand".into(),
        )),
        // Delay is a delivery-time pause, not text — applied by the injection worker.
        Macro::Delay { .. } => Err(DomainError::Validation(
            "delay macro must be evaluated via Db::expand".into(),
        )),
        // Cursor is a delivery-time caret move — applied by the injection worker.
        Macro::Cursor => Err(DomainError::Validation(
            "cursor macro must be evaluated via Db::expand".into(),
        )),
    }
}

fn format_number(val: f64) -> String {
    if val.fract() == 0.0 && val.is_finite() {
        format!("{}", val as i64)
    } else {
        format!("{}", val)
    }
}

/// PhraseExpress-style case adaptation: shape the expansion to match how the
/// abbreviation was typed (SIG → ALL CAPS, sig → as-is, Sig → Title case).
pub fn case_adapt(text: &str, abbr: &str) -> String {
    let letters: Vec<char> = abbr.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return text.to_string();
    }
    let all_upper = letters.iter().all(|c| c.is_uppercase());
    let all_lower = letters.iter().all(|c| c.is_lowercase());
    if all_upper {
        text.to_uppercase()
    } else if all_lower {
        text.to_string()
    } else {
        // Mixed (incl. Title-case): capitalize the first letter only.
        let mut chars = text.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => String::new(),
        }
    }
}

/// Char-aware truncation with a trailing ellipsis when cutting was needed.
/// Trailing whitespace at the cut is trimmed so the ellipsis sits flush.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Literal text of a segment list, truncated at `PREVIEW_LIMIT` (list previews).
pub(crate) fn preview_text(segments: &[Segment]) -> String {
    let mut text = String::new();
    for seg in segments {
        if let Segment::Text { text: t } = seg {
            text.push_str(t);
        }
    }
    truncate(&text, PREVIEW_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::TriggerBinding;
    use chrono::TimeZone;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 1, 9, 30, 0).unwrap()
    }

    #[test]
    fn macro_datetime_formats_local_time() {
        let m = Macro::DateTime {
            format: "%Y-%m-%d %H:%M".into(),
        };
        // Evaluation converts to local time; derive the expectation the same way so the
        // test is timezone-independent.
        let expected = chrono::Local
            .from_utc_datetime(&fixed_now().naive_utc())
            .format("%Y-%m-%d %H:%M")
            .to_string();
        assert_eq!(
            evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap(),
            expected
        );
    }

    #[test]
    fn macro_datemath_adds_days() {
        let m = Macro::DateMath {
            days_offset: 1,
            format: "%Y-%m-%d".into(),
        };
        let expected = (chrono::Local.from_utc_datetime(&fixed_now().naive_utc())
            + Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
        assert_eq!(
            evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap(),
            expected
        );
    }

    #[test]
    fn macro_uuid_is_valid_and_unique() {
        let m = Macro::Uuid;
        let a = evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap();
        let b = evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap();
        assert_ne!(a, b);
        uuid::Uuid::parse_str(&a).unwrap();
    }

    #[test]
    fn macro_shell_captures_stdout_trimmed() {
        let m = Macro::Shell {
            command: "echo hello".into(),
        };
        assert_eq!(
            evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap(),
            "hello"
        );
    }

    #[test]
    fn macro_shell_times_out_on_hung_command() {
        // Sleep far past the 5 s bound — the kill must surface as an error naming
        // the timeout, not a wedge.
        let m = Macro::Shell {
            command: "sleep 30".into(),
        };
        let err = evaluate_macro(&m, &HashMap::new(), fixed_now())
            .unwrap_err()
            .to_string();
        assert!(err.contains("timed out"), "unexpected: {err}");
    }

    #[test]
    fn macro_shell_surfaces_nonzero_exit() {
        let m = Macro::Shell {
            command: "exit 3".into(),
        };
        let err = evaluate_macro(&m, &HashMap::new(), fixed_now())
            .unwrap_err()
            .to_string();
        assert!(err.contains("3"), "exit code missing: {err}");
    }

    #[test]
    fn inputform_uses_inputs() {
        let m = Macro::InputForm {
            field: "name".into(),
            kind: Default::default(),
            options: vec![],
        };
        let mut inputs = HashMap::new();
        inputs.insert("name".into(), "Sam".into());
        assert_eq!(evaluate_macro(&m, &inputs, fixed_now()).unwrap(), "Sam");
    }

    #[test]
    fn inputform_missing_errors() {
        let m = Macro::InputForm {
            field: "name".into(),
            kind: Default::default(),
            options: vec![],
        };
        let err = evaluate_macro(&m, &HashMap::new(), fixed_now());
        assert!(matches!(err, Err(DomainError::MissingInput(_))));
    }

    #[test]
    fn preview_truncates_long_text() {
        let long = "x".repeat(100);
        let preview = preview_text(&[Segment::Text { text: long }]);
        assert!(preview.ends_with('…'));
        assert!(preview.chars().count() <= 61);
    }

    // sanity: trigger binding validation
    #[test]
    fn autotext_trigger_requires_abbreviation() {
        let t = TriggerBinding {
            kind: crate::domain::models::TriggerType::Autotext,
            abbreviation: None,
            key: None,
            regex: false,
            execute: Default::default(),
        };
        assert!(t.validate().is_err());
    }

    #[test]
    fn macro_random_stays_in_range() {
        for _ in 0..50 {
            let m = Macro::Random { min: 1, max: 10 };
            let v: i64 = evaluate_macro(&m, &HashMap::new(), fixed_now())
                .unwrap()
                .parse()
                .unwrap();
            assert!((1..=10).contains(&v));
        }
    }

    #[test]
    fn macro_math_evaluates() {
        let m = Macro::Math {
            expr: "2+3*4".into(),
        };
        assert_eq!(
            evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap(),
            "14"
        );
        let m2 = Macro::Math {
            expr: "10/4".into(),
        };
        assert_eq!(
            evaluate_macro(&m2, &HashMap::new(), fixed_now()).unwrap(),
            "2.5"
        );
    }

    #[test]
    fn macro_env_value_or_empty() {
        std::env::set_var("PHRASEXP_TEST_ENV_VAR", "hello");
        let set = Macro::Env {
            var: "PHRASEXP_TEST_ENV_VAR".into(),
        };
        assert_eq!(
            evaluate_macro(&set, &HashMap::new(), fixed_now()).unwrap(),
            "hello"
        );
        let unset = Macro::Env {
            var: "PHRASEXP_NO_SUCH_VAR_ZZZ".into(),
        };
        assert_eq!(
            evaluate_macro(&unset, &HashMap::new(), fixed_now()).unwrap(),
            ""
        );
    }

    #[test]
    fn macro_pick_returns_an_option() {
        let m = Macro::Pick {
            options: vec!["a".into(), "b".into(), "c".into()],
        };
        for _ in 0..20 {
            let v = evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap();
            assert!(["a", "b", "c"].contains(&v.as_str()));
        }
    }

    #[test]
    fn macro_file_reads_contents() {
        let path = std::env::temp_dir().join("phrasexp_macro_file_test.txt");
        std::fs::write(&path, "file body").unwrap();
        let m = Macro::File {
            path: path.to_string_lossy().into_owned(),
        };
        assert_eq!(
            evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap(),
            "file body"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn case_adapts_to_abbreviation() {
        assert_eq!(case_adapt("best regards", "br,"), "best regards"); // lower -> as-is
        assert_eq!(case_adapt("best regards", "BR,"), "BEST REGARDS"); // upper -> ALL CAPS
        assert_eq!(case_adapt("best regards", "Br,"), "Best regards"); // title -> Title
    }
}
