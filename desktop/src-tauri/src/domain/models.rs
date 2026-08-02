//! Domain models — ported 1:1 from `backend/app.py` so the SQLite DB and JSON shapes are
//! compatible between the Python reference and this Rust app.

use serde::{Deserialize, Serialize};

use super::error::{DomainError, DomainResult};

// --------------------------------------------------------------------------- //
// Enums
// --------------------------------------------------------------------------- //
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    #[default]
    Plain,
    // Reserved in v1 (delivery is plain-only); rejected at creation/update.
    Richtext,
    Html,
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    Autotext,
    Hotkey,
}

/// Sort order for the phrase list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Name,
    Updated,
    Created,
}

/// How an expansion is delivered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    #[default]
    Type,
    Copy,
}

// --------------------------------------------------------------------------- //
// Macros — internally tagged on "type" (FR-5)
// --------------------------------------------------------------------------- //
fn default_datetime_format() -> String {
    "%Y-%m-%d %H:%M".to_string()
}
fn default_date_format() -> String {
    "%Y-%m-%d".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Macro {
    DateTime {
        #[serde(default = "default_datetime_format")]
        format: String,
    },
    DateMath {
        #[serde(default)]
        days_offset: i64,
        #[serde(default = "default_date_format")]
        format: String,
    },
    InputForm {
        field: String,
    },
    /// Uniform random integer in [min, max].
    Random {
        #[serde(default)]
        min: i64,
        #[serde(default = "max_default")]
        max: i64,
    },
    /// Arithmetic expression, e.g. "2+3*4" or "round(100/3)".
    Math {
        expr: String,
    },
    /// Insert the latest clipboard text.
    Clipboard,
    /// Insert another phrase's expansion (linked/nested), by phrase id.
    Nested {
        phrase_id: String,
    },
}

fn max_default() -> i64 {
    100
}

// --------------------------------------------------------------------------- //
// Segments — internally tagged on "type" (FR-10)
// --------------------------------------------------------------------------- //
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Segment {
    Text {
        text: String,
    },
    Macro {
        // Field is named `macro` in the JSON; `macro` is awkward as a Rust identifier.
        #[serde(rename = "macro")]
        macro_value: Macro,
    },
}

// --------------------------------------------------------------------------- //
// Trigger binding (FR-3)
// --------------------------------------------------------------------------- //
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriggerBinding {
    #[serde(rename = "type")]
    pub kind: TriggerType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abbreviation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl TriggerBinding {
    /// autotext ⇒ abbreviation required; hotkey ⇒ key required.
    pub fn validate(&self) -> DomainResult<()> {
        match self.kind {
            TriggerType::Autotext if self.abbreviation.is_none() || self.abbreviation.as_deref() == Some("") => {
                Err(DomainError::Validation("autotext trigger requires 'abbreviation'".into()))
            }
            TriggerType::Hotkey if self.key.is_none() || self.key.as_deref() == Some("") => {
                Err(DomainError::Validation("hotkey trigger requires 'key'".into()))
            }
            _ => Ok(()),
        }
    }
}

// --------------------------------------------------------------------------- //
// Aggregates
// --------------------------------------------------------------------------- //
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phrase {
    pub id: String,
    pub folder_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abbreviation: Option<String>,
    #[serde(default)]
    pub content_type: ContentType,
    pub body: Vec<Segment>,
    #[serde(default)]
    pub triggers: Vec<TriggerBinding>,
    #[serde(default)]
    pub sensitive: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub id: String,
    pub text: String,
    pub captured_at: String,
}

impl Phrase {
    pub fn validate(&self) -> DomainResult<()> {
        if self.body.is_empty() {
            return Err(DomainError::Validation(
                "phrase body must contain at least one segment".into(),
            ));
        }
        if self.content_type != ContentType::Plain {
            return Err(DomainError::ReservedContentType(format!(
                "content_type '{}' is reserved (v1 supports plain only)",
                serde_json::to_string(&self.content_type).unwrap_or_default()
            )));
        }
        for t in &self.triggers {
            t.validate()?;
        }
        Ok(())
    }
}

// --------------------------------------------------------------------------- //
// Request DTOs (frontend → Rust)
// --------------------------------------------------------------------------- //
#[derive(Debug, Clone, Deserialize)]
pub struct FolderCreate {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TriggerRequest {
    #[serde(rename = "type")]
    pub kind: TriggerType,
    #[serde(default)]
    pub abbreviation: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhraseCreate {
    pub folder_id: String,
    pub name: String,
    #[serde(default)]
    pub abbreviation: Option<String>,
    #[serde(default)]
    pub content_type: ContentType,
    pub body: Vec<Segment>,
    #[serde(default)]
    pub triggers: Vec<TriggerBinding>,
    #[serde(default)]
    pub sensitive: bool,
}

impl PhraseCreate {
    /// Canonicalize: ≤1 autotext trigger; autotext abbreviation must equal / supplies the
    /// phrase abbreviation (FR-6 uniqueness is on the phrase-level field).
    pub fn normalize(&mut self) -> DomainResult<()> {
        let autotext: Vec<&TriggerBinding> = self
            .triggers
            .iter()
            .filter(|t| t.kind == TriggerType::Autotext)
            .collect();
        if autotext.len() > 1 {
            return Err(DomainError::Validation(
                "at most one autotext trigger is allowed".into(),
            ));
        }
        if let Some(t) = autotext.first() {
            let ta = t.abbreviation.clone().unwrap_or_default();
            if let Some(a) = &self.abbreviation {
                if a != &ta {
                    return Err(DomainError::Validation(
                        "autotext trigger abbreviation must match phrase abbreviation".into(),
                    ));
                }
            }
            self.abbreviation = Some(ta);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PhraseUpdate {
    pub name: Option<String>,
    pub abbreviation: Option<String>,
    pub content_type: Option<ContentType>,
    pub body: Option<Vec<Segment>>,
    pub triggers: Option<Vec<TriggerBinding>>,
    pub sensitive: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExpandRequest {
    pub trigger: Option<TriggerRequest>, // Option: macro-eval path can be called with a phrase id directly
    #[serde(default)]
    pub inputs: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClipboardCreate {
    pub text: String,
}

// --------------------------------------------------------------------------- //
// Response DTOs (Rust → frontend)
// --------------------------------------------------------------------------- //
#[derive(Debug, Clone, Serialize)]
pub struct PhraseSummary {
    pub id: String,
    pub folder_id: String,
    pub name: String,
    pub abbreviation: Option<String>,
    pub content_type: ContentType,
    pub sensitive: bool,
    pub content_preview: String, // "***" when sensitive (FR-8)
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExpandResponse {
    pub phrase_id: String,
    pub abbreviation: Option<String>,
    pub content_type: ContentType,
    pub output: String,
}

// --------------------------------------------------------------------------- //
// Settings (M5) — single-row app config
// --------------------------------------------------------------------------- //
fn default_popup_hotkey() -> String {
    "Control+Alt+Space".to_string()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_popup_hotkey")]
    pub popup_hotkey: String,
    #[serde(default = "default_true")]
    pub autotext_enabled: bool,
    #[serde(default = "default_true")]
    pub clipboard_enabled: bool,
    #[serde(default)]
    pub autostart: bool,
}

impl Settings {
    /// Built-in defaults (used to seed the settings row on first run).
    pub fn defaults() -> Self {
        Settings {
            popup_hotkey: default_popup_hotkey(),
            autotext_enabled: true,
            clipboard_enabled: true,
            autostart: false,
        }
    }
}
