//! Pure macro evaluation + expansion — ported from `backend/app.py`.
//! chrono's strftime is compatible with Python's for the tokens we use (NFR-3 determinism).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use super::error::{DomainError, DomainResult};
use super::models::{Macro, Phrase, Segment};

const PREVIEW_LIMIT: usize = 60;

/// FR-5. Pure: identical (macro, inputs, now) -> identical output.
pub fn evaluate_macro(
    m: &Macro,
    inputs: &HashMap<String, String>,
    now: DateTime<Utc>,
) -> DomainResult<String> {
    match m {
        Macro::DateTime { format } => Ok(now.format(format).to_string()),
        Macro::DateMath {
            days_offset,
            format,
        } => Ok((now + Duration::days(*days_offset)).format(format).to_string()),
        Macro::InputForm { field } => inputs
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
        // Clipboard needs the system clipboard — resolved in Db::expand, not here.
        Macro::Clipboard => Err(DomainError::Validation(
            "clipboard macro must be evaluated via Db::expand".into(),
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

/// FR-4/FR-10. Walk segments in order, concatenating literal text and evaluated macros.
/// `clip` supplies the Clipboard-macro content (None when not available).
pub fn expand_phrase(
    phrase: &Phrase,
    inputs: &HashMap<String, String>,
    now: DateTime<Utc>,
    clip: Option<&str>,
) -> DomainResult<String> {
    let mut out = String::new();
    for seg in &phrase.body {
        match seg {
            Segment::Text { text } => out.push_str(text),
            Segment::Macro { macro_value } => match macro_value {
                Macro::Clipboard => out.push_str(clip.unwrap_or("")),
                other => out.push_str(&evaluate_macro(other, inputs, now)?),
            },
        }
    }
    Ok(out)
}

/// FR-8. Non-delivery preview: literal text only, truncated.
pub fn literal_preview(phrase: &Phrase) -> String {
    let mut text = String::new();
    for seg in &phrase.body {
        if let Segment::Text { text: t } = seg {
            text.push_str(t);
        }
    }
    if text.chars().count() > PREVIEW_LIMIT {
        let truncated: String = text.chars().take(PREVIEW_LIMIT).collect();
        format!("{}…", truncated.trim_end())
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{ContentType, TriggerBinding};
    use chrono::TimeZone;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 1, 9, 30, 0).unwrap()
    }

    fn phrase_with(body: Vec<Segment>) -> Phrase {
        Phrase {
            id: "p1".into(),
            folder_id: "f1".into(),
            name: "t".into(),
            abbreviation: None,
            content_type: ContentType::Plain,
            body,
            triggers: vec![],
            sensitive: false,
            created_at: "2026-08-01T09:30:00Z".into(),
            updated_at: "2026-08-01T09:30:00Z".into(),
            deleted_at: None,
        }
    }

    #[test]
    fn macro_datetime_formats() {
        let m = Macro::DateTime {
            format: "%Y-%m-%d".into(),
        };
        assert_eq!(evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap(), "2026-08-01");
    }

    #[test]
    fn macro_datemath_adds_days() {
        let m = Macro::DateMath {
            days_offset: 1,
            format: "%Y-%m-%d".into(),
        };
        assert_eq!(evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap(), "2026-08-02");
    }

    #[test]
    fn inputform_uses_inputs() {
        let m = Macro::InputForm {
            field: "name".into(),
        };
        let mut inputs = HashMap::new();
        inputs.insert("name".into(), "Sam".into());
        assert_eq!(evaluate_macro(&m, &inputs, fixed_now()).unwrap(), "Sam");
    }

    #[test]
    fn inputform_missing_errors() {
        let m = Macro::InputForm {
            field: "name".into(),
        };
        let err = evaluate_macro(&m, &HashMap::new(), fixed_now());
        assert!(matches!(err, Err(DomainError::MissingInput(_))));
    }

    #[test]
    fn expand_ordered_segments_with_input() {
        let p = phrase_with(vec![
            Segment::Text {
                text: "Hello ".into(),
            },
            Segment::Macro {
                macro_value: Macro::InputForm {
                    field: "name".into(),
                },
            },
            Segment::Text {
                text: "!".into(),
            },
        ]);
        let mut inputs = HashMap::new();
        inputs.insert("name".into(), "Sam".into());
        assert_eq!(expand_phrase(&p, &inputs, fixed_now(), None).unwrap(), "Hello Sam!");
    }

    #[test]
    fn expand_is_deterministic() {
        let p = phrase_with(vec![Segment::Macro {
            macro_value: Macro::DateTime {
                format: "%Y".into(),
            },
        }]);
        let now = fixed_now();
        assert_eq!(
            expand_phrase(&p, &HashMap::new(), now, None).unwrap(),
            expand_phrase(&p, &HashMap::new(), now, None).unwrap()
        );
        assert_eq!(expand_phrase(&p, &HashMap::new(), now, None).unwrap(), "2026");
    }

    #[test]
    fn preview_truncates_long_text() {
        let long = "x".repeat(100);
        let p = phrase_with(vec![Segment::Text { text: long }]);
        let preview = literal_preview(&p);
        assert!(preview.ends_with('…'));
        assert!(preview.chars().count() <= 61);
    }

    // sanity: trigger binding validation (FR-3)
    #[test]
    fn autotext_trigger_requires_abbreviation() {
        let t = TriggerBinding {
            kind: crate::domain::models::TriggerType::Autotext,
            abbreviation: None,
            key: None,
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
        let m = Macro::Math { expr: "2+3*4".into() };
        assert_eq!(evaluate_macro(&m, &HashMap::new(), fixed_now()).unwrap(), "14");
        let m2 = Macro::Math { expr: "10/4".into() };
        assert_eq!(evaluate_macro(&m2, &HashMap::new(), fixed_now()).unwrap(), "2.5");
    }

    #[test]
    fn case_adapts_to_abbreviation() {
        assert_eq!(case_adapt("best regards", "br,"), "best regards"); // lower -> as-is
        assert_eq!(case_adapt("best regards", "BR,"), "BEST REGARDS"); // upper -> ALL CAPS
        assert_eq!(case_adapt("best regards", "Br,"), "Best regards"); // title -> Title
    }
}
