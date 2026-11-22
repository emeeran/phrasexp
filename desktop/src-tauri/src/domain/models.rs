//! Domain models — the SQLite DB and JSON shapes used across the app.

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
    /// Authored as Markdown, delivered as formatted HTML (clipboard paste)
    /// with a plain-text fallback.
    Markdown,
    // Reserved — not yet supported; rejected at creation/update.
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
    MostUsed,
}

/// Per-phrase case adaptation override (1.4).
/// Adapt = follow the global setting (default, current behavior);
/// Always = adapt regardless of the global toggle;
/// Never = output the body verbatim.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaseMode {
    #[default]
    Adapt,
    Always,
    Never,
}

/// When an autotext abbreviation fires (1.4).
/// Immediate = as soon as the buffer ends with the abbreviation (default);
/// AfterDelimiter = only after the user follows it with Space/Enter/Tab/, . ; : ! ?
/// (the delimiter is erased and re-typed after the expansion).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecuteMode {
    #[default]
    Immediate,
    // Serializes as "after_delimiter" to match the TS `ExecuteMode`; the alias keeps
    // older DB rows (written as "afterdelimiter") deserializable.
    #[serde(rename = "after_delimiter", alias = "afterdelimiter")]
    AfterDelimiter,
}

/// How an expansion is delivered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    #[default]
    Type,
    Copy,
}

/// Input-form field control type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    #[default]
    Text,
    Select,
    Checkbox,
}

// --------------------------------------------------------------------------- //
// Macros — internally tagged on "type"
// --------------------------------------------------------------------------- //
fn default_datetime_format() -> String {
    "%d-%m-%Y %H:%M".to_string()
}
fn default_date_format() -> String {
    "%d-%m-%Y".to_string()
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
        #[serde(default)]
        kind: InputKind,
        #[serde(default)]
        options: Vec<String>,
    },
    /// Uniform random integer in [min, max].
    Random {
        #[serde(default)]
        min: i64,
        #[serde(default = "max_default")]
        max: i64,
    },
    /// Arithmetic expression, e.g. "2+3*4" or "round(100/3)".
    Math { expr: String },
    /// Insert the latest clipboard text.
    Clipboard,
    /// Insert another phrase's expansion (linked/nested), by phrase id.
    Nested { phrase_id: String },
    /// Value of an environment variable.
    Env { var: String },
    /// Pick one option at random from a list.
    Pick { options: Vec<String> },
    /// Contents of a file at the given path.
    File { path: String },
    /// A PNG image (stored under the app data dir) pasted into the target app.
    Image { path: String },
    /// An AI-generated response to the given prompt (BYO key, OpenAI-compatible).
    Ai { prompt: String },
    /// An AI action (user-editable preset stored in the ai_actions table) run against
    /// the current clipboard text. Resolved at expand time so editing the action
    /// updates every phrase that references it.
    AiAction { action_id: String },
    /// A fresh random UUID v4.
    Uuid,
    /// Output of a shell command (`sh -c` / `cmd /C`), trailing newline trimmed.
    Shell { command: String },
    /// A formatted citation (APA / MLA / Chicago) for a DOI, URL, or raw title —
    /// metadata is looked up on Crossref at expansion time (free, no key). An
    /// empty `source` prompts once via the inputs popup (field `cite`). Needs
    /// network — resolved in Db::expand, like Ai/AiAction.
    Cite {
        #[serde(default)]
        source: String,
        #[serde(default = "default_cite_style")]
        style: String,
    },
    /// A named, persistent auto-incrementing counter (1 on first use). Evaluated in
    /// Db::expand — it needs DB state, like Nested/Clipboard.
    Counter { name: String },
    /// Pause the injection for this many milliseconds before continuing with the next
    /// segment — lets slow apps catch up mid-template. Clamped to 0..=5000 at expand.
    Delay {
        #[serde(default = "default_delay_ms")]
        ms: i64,
    },
    /// Caret-position marker: after the remaining text is delivered, the caret moves
    /// back to this point (email/letter templates with a fill-in blank). Best-effort:
    /// exact on the typed path, approximate on the paste path (character count of the
    /// plain text, not rendered HTML).
    Cursor,
}

fn default_delay_ms() -> i64 {
    500
}

fn default_cite_style() -> String {
    "apa".to_string()
}

/// One piece of expansion output, in delivery order: a text chunk, an image paste,
/// or a timed pause. Expansion produces a chunk stream so a Delay macro can sit
/// between text segments (flat `String` views join the text and drop the pauses).
#[derive(Debug, Clone, PartialEq)]
pub enum OutChunk {
    Text(String),
    Image(String),
    /// Pause in milliseconds (clamped 0..=5000 at expand).
    Delay(i64),
    /// Caret marker: the NEXT text chunk is delivered, then the caret moves back to
    /// this position (Left keypresses — works on both typed and pasted output).
    Cursor,
}

impl OutChunk {
    /// Flatten a chunk stream to (joined text, image paths). Delays are dropped —
    /// the flat views (preview, expand, tests) show only the resulting text.
    pub fn flatten(chunks: &[OutChunk]) -> (String, Vec<String>) {
        let mut text = String::new();
        let mut images = Vec::new();
        for c in chunks {
            match c {
                OutChunk::Text(t) => text.push_str(t),
                OutChunk::Image(p) => images.push(p.clone()),
                OutChunk::Delay(_) | OutChunk::Cursor => {}
            }
        }
        (text, images)
    }
}

fn max_default() -> i64 {
    100
}

/// A user-editable AI action preset (the "actions" menu of the AI features).
/// `prompt_template` may reference `{clipboard}` / `{selection}` — both resolve to
/// the clipboard text captured at run time. One additional free parameter is
/// supported: any other `{token}` (e.g. `{language}`, `{length}`) makes the action
/// interactive — the frontend prompts once and the answer replaces every occurrence.
/// Builtin rows are seeded on migration and cannot be deleted, only edited.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiAction {
    pub id: String,
    pub name: String,
    pub prompt_template: String,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub builtin: bool,
}

/// Stable ids for the seeded builtin actions (also used by the frontend picker).
/// The 2026-11 menu revamp curated this to a single ordered 13-tool set — the
/// order here IS the popup menu order (first 9 rows get 1-9 quick keys). Existing
/// installs are converged onto it by the REVAMPED/RETIRED passes in `run_migrations`;
/// fresh installs seed it directly.
pub const BUILTIN_AI_ACTIONS: [(&str, &str, &str); 13] = [
    (
        "ai-grammar",
        "Grammar check",
        "Fix grammar, spelling, and punctuation only. Do not change word choice, sentence structure, tone, or meaning. Keep formatting and line breaks exactly. If the text is already correct, return it unchanged:\n{clipboard}",
    ),
    (
        "ai-improve",
        "Rewrite for clarity",
        "Rewrite for maximum clarity. Use simple, direct sentences; cut filler and redundancy. Preserve every fact, the original meaning and tone, and the formatting:\n{clipboard}",
    ),
    (
        "ai-formal",
        "Professional tone",
        "Rewrite in a professional, formal tone — precise, polite, no slang or contractions. Keep the meaning and roughly the same length:\n{clipboard}",
    ),
    (
        "ai-casual",
        "Friendly / casual tone",
        "Rewrite in a friendly, casual tone — contractions, everyday words, warm phrasing; natural, not childish. Keep the meaning and key content:\n{clipboard}",
    ),
    (
        "ai-humanize",
        "Humanize",
        "Rewrite so the text reads as naturally human-written: vary sentence length and rhythm, remove robotic or formulaic phrasing (e.g. \"delve\", \"in today's fast-paced world\", \"it is important to note\") and other AI tell-tale patterns. Preserve the meaning, facts, and formatting — add nothing:\n{clipboard}",
    ),
    (
        "ai-academic",
        "Academic tone",
        "Rewrite in an academic register — precise, formal, objective, appropriately hedged. Keep discipline-standard terminology; preserve citations, quotes, and technical terms exactly:\n{clipboard}",
    ),
    (
        "ai-prompt-opt",
        "Optimize prompt",
        "Rewrite this as an optimized prompt for a modern LLM: a clear task, the relevant context, explicit constraints, and the exact output format. Return only the prompt text:\n{clipboard}",
    ),
    (
        "ai-shorten",
        "Shorter",
        "Shorten this text to roughly half its length. Cut redundancy and filler; keep every key point, name, and number. Add nothing:\n{clipboard}",
    ),
    (
        "ai-expand",
        "Expand",
        "Expand this text with relevant detail and elaboration to roughly twice its length. Stay on topic, keep the author's voice, and add no invented facts:\n{clipboard}",
    ),
    (
        "ai-summarize",
        "Summarize",
        "Summarize the text — target length {length} (e.g. 30 words, 3 bullets), output as {format} (e.g. paragraph, bullet points). Keep only the key points; no commentary:\n{clipboard}",
    ),
    (
        "ai-translate",
        "Translate (Tamil ⇄ English)",
        "Detect the language of the text, then translate: Tamil → English, anything else → Tamil. Translate naturally — preserve meaning, formatting, and line breaks; keep proper nouns and technical terms:\n{clipboard}",
    ),
    (
        "ai-active-voice",
        "Active voice",
        "Convert every passive-voice sentence to active voice. Keep the meaning, tense, and roughly the length; leave already-active sentences unchanged. Keep formatting and line breaks:\n{clipboard}",
    ),
    (
        "ai-passive-voice",
        "Passive voice",
        "Convert active-voice sentences to passive voice where a natural passive form exists. Keep the meaning, tense, and roughly the length. Keep formatting and line breaks:\n{clipboard}",
    ),
];

/// Builtins retired by merging into a near-duplicate sibling: (retired id, its
/// original name, its original template, target id). Seeding deletes a retired
/// row only while it still matches these originals — a user-edited action is
/// kept untouched. Fresh installs never get the retired rows at all.
pub const MERGED_AI_ACTIONS: [(&str, &str, &str, &str); 4] = [
    (
        "ai-clarity",
        "Rewrite for clarity",
        "Rewrite for maximum clarity and precision:\n{clipboard}",
        "ai-improve",
    ),
    (
        "ai-simplify",
        "Simplify language",
        "Simplify the language so a non-expert can understand:\n{clipboard}",
        "ai-explain",
    ),
    (
        "ai-to-prompt",
        "Convert to prompt",
        "Convert this text into a well-structured AI prompt:\n{clipboard}",
        "ai-prompt-opt",
    ),
    (
        "ai-tamil",
        "Translate to Tamil (professional)",
        "Translate to Tamil and rewrite professionally:\n{clipboard}",
        "ai-translate",
    ),
];

/// 2026-11 menu revamp: (id, new name, new template, pre-revamp templates). On
/// existing installs the seeding pass retitles, re-prompts, and re-sorts an
/// untouched row (matched on template verbatim) onto the curated 13-tool menu; a
/// user-edited template is kept — it just stays where it was in the menu. The
/// `sort_order` written is the id's position in `BUILTIN_AI_ACTIONS`.
pub const REVAMPED_AI_ACTIONS: [(&str, &str, &str, &[&str]); 10] = [
    (
        "ai-grammar",
        BUILTIN_AI_ACTIONS[0].1,
        BUILTIN_AI_ACTIONS[0].2,
        &["Fix grammar, spelling, and punctuation only — do not change wording, style, or meaning:\n{clipboard}"],
    ),
    (
        "ai-improve",
        BUILTIN_AI_ACTIONS[1].1,
        BUILTIN_AI_ACTIONS[1].2,
        &["Improve this writing — tighten clarity and flow, upgrade word choice. Keep the author's voice and meaning:\n{clipboard}"],
    ),
    (
        "ai-formal",
        BUILTIN_AI_ACTIONS[2].1,
        BUILTIN_AI_ACTIONS[2].2,
        &["Rewrite in a formal, professional tone. Keep the meaning and roughly the same length:\n{clipboard}"],
    ),
    (
        "ai-casual",
        BUILTIN_AI_ACTIONS[3].1,
        BUILTIN_AI_ACTIONS[3].2,
        &["Rewrite in a casual, friendly tone — contractions, everyday words. Keep the meaning:\n{clipboard}"],
    ),
    (
        "ai-academic",
        BUILTIN_AI_ACTIONS[5].1,
        BUILTIN_AI_ACTIONS[5].2,
        &["Rewrite in an academic register — precise, formal, appropriately hedged. Preserve citations and technical terms:\n{clipboard}"],
    ),
    (
        "ai-prompt-opt",
        BUILTIN_AI_ACTIONS[6].1,
        BUILTIN_AI_ACTIONS[6].2,
        &["Rewrite this as a well-structured prompt for a large language model: state the task, give the context, and specify the output format. Return only the prompt:\n{clipboard}"],
    ),
    (
        "ai-shorten",
        BUILTIN_AI_ACTIONS[7].1,
        BUILTIN_AI_ACTIONS[7].2,
        &["Shorten this text — cut redundancy, keep every key point and the original tone:\n{clipboard}"],
    ),
    (
        "ai-expand",
        BUILTIN_AI_ACTIONS[8].1,
        BUILTIN_AI_ACTIONS[8].2,
        &["Expand this text with relevant detail and elaboration — roughly double the length, same voice:\n{clipboard}"],
    ),
    (
        "ai-summarize",
        BUILTIN_AI_ACTIONS[9].1,
        BUILTIN_AI_ACTIONS[9].2,
        &["Summarize — target length {length} (e.g. 30 words, 3 bullets). Output as {format}:\n{clipboard}"],
    ),
    (
        "ai-translate",
        BUILTIN_AI_ACTIONS[10].1,
        BUILTIN_AI_ACTIONS[10].2,
        &[
            // Pre-revamp: interactive target language. The revamp makes the
            // builtin auto Tamil⇄English; a user who wants a fixed language
            // edits the template back to "Translate to {language}.…" and it sticks.
            "Translate to {language}. Preserve formatting and line breaks; keep technical terms and proper nouns unless a natural equivalent exists:\n{clipboard}",
        ],
    ),
];

/// Builtins dropped by the 2026-11 menu revamp (superseded by the curated set):
/// (id, name, template). Seeding deletes a retired row only while it still
/// matches these originals verbatim — a user-edited action survives as a custom
/// action. Fresh installs never get these ids at all.
pub const RETIRED_AI_ACTIONS: [(&str, &str, &str); 10] = [
    (
        "ai-explain",
        "Explain simply",
        "Explain this in simple terms a non-expert can follow — short sentences, a concrete example if helpful:\n{clipboard}",
    ),
    (
        "ai-continue",
        "Continue writing",
        "Continue writing from where this text stops, matching its tone, style, and formatting. Return ONLY the continuation — do not repeat the original text:\n{clipboard}",
    ),
    (
        "ai-confident",
        "Confident tone",
        "Rewrite in a confident, assertive tone — remove hedging and qualifiers, keep the claims:\n{clipboard}",
    ),
    (
        "ai-bullets",
        "Bullet points",
        "Convert into a concise bullet-point list — one point per line, starting with '- ', keeping the original order:\n{clipboard}",
    ),
    (
        "ai-tone-detect",
        "Detect tone",
        "Identify the tone of this text. Reply with 2-4 comma-separated lowercase tone labels only (e.g. formal, friendly, urgent) — no other words:\n{clipboard}",
    ),
    (
        "ai-friendlier",
        "Friendlier tone",
        "Rewrite in a warmer, friendlier tone while keeping the meaning and key content:\n{clipboard}",
    ),
    (
        "ai-direct",
        "More direct",
        "Rewrite more directly — cut hedging, filler, and apologies. Keep the claims and the meaning:\n{clipboard}",
    ),
    (
        "ai-fluency",
        "Improve fluency",
        "Rewrite for natural fluency — idiomatic phrasing a native speaker would use. Keep the meaning and register:\n{clipboard}",
    ),
    (
        "ai-creative",
        "Creative rewrite",
        "Rewrite this text creatively — vivid, original phrasing that keeps the meaning:\n{clipboard}",
    ),
    (
        "ai-paraphrase",
        "Paraphrase",
        "Paraphrase this text in a {mode} style — same meaning, fresh wording, similar length:\n{clipboard}",
    ),
];

/// Builtins whose template changed in place (e.g. Translate now prompts for the
/// target language via `{language}`): (id, old template, new template). Seeding
/// upgrades a row only while it still carries the old template verbatim — a
/// user-edited template is kept. The chain's final target is pre-revamp text;
/// `REVAMPED_AI_ACTIONS` carries rows the rest of the way onto the current set.
pub const RETEMPLATED_AI_ACTIONS: [(&str, &str, &str); 22] = [
    (
        "ai-translate",
        "Translate to French:\n{clipboard}",
        "Translate to {language}:\n{clipboard}",
    ),
    // Summarize gains a target-length parameter (QuillBot-style; report 1.3) — the
    // frontend prompts for it, so the upgrade is seamless on untouched rows.
    (
        "ai-summarize",
        "Summarize:\n{clipboard}",
        "Summarize — target length {length} (e.g. 30 words, 3 bullets):\n{clipboard}",
    ),
    // Summarize gains an output-format token (paragraphs vs bullets) — now that
    // templates support multiple interactive tokens, the prompt asks once for both.
    (
        "ai-summarize",
        "Summarize — target length {length} (e.g. 30 words, 3 bullets):\n{clipboard}",
        "Summarize — target length {length} (e.g. 30 words, 3 bullets). Output as {format}:\n{clipboard}",
    ),    (
        "ai-translate",
        "Translate to {language}:\n{clipboard}",
        "Translate to {language}. Preserve formatting and line breaks; keep technical terms and proper nouns unless a natural equivalent exists:\n{clipboard}",
    ),
    (
        "ai-formal",
        "Rewrite in a formal, professional tone:\n{clipboard}",
        "Rewrite in a formal, professional tone. Keep the meaning and roughly the same length:\n{clipboard}",
    ),
    (
        "ai-casual",
        "Rewrite in a casual, friendly tone:\n{clipboard}",
        "Rewrite in a casual, friendly tone — contractions, everyday words. Keep the meaning:\n{clipboard}",
    ),
    (
        "ai-shorten",
        "Shorten while keeping key points:\n{clipboard}",
        "Shorten this text — cut redundancy, keep every key point and the original tone:\n{clipboard}",
    ),
    (
        "ai-expand",
        "Expand and elaborate on:\n{clipboard}",
        "Expand this text with relevant detail and elaboration — roughly double the length, same voice:\n{clipboard}",
    ),
    (
        "ai-grammar",
        "Fix grammar and spelling:\n{clipboard}",
        "Fix grammar, spelling, and punctuation only — do not change wording, style, or meaning:\n{clipboard}",
    ),
    (
        "ai-explain",
        "Explain this in simple terms:\n{clipboard}",
        "Explain this in simple terms a non-expert can follow — short sentences, a concrete example if helpful:\n{clipboard}",
    ),
    (
        "ai-continue",
        "Continue writing this text, matching its tone and style:\n{clipboard}",
        "Continue writing from where this text stops, matching its tone, style, and formatting. Return ONLY the continuation — do not repeat the original text:\n{clipboard}",
    ),
    (
        "ai-improve",
        "Improve this writing — clarity, flow, and word choice — keeping the meaning:\n{clipboard}",
        "Improve this writing — tighten clarity and flow, upgrade word choice. Keep the author's voice and meaning:\n{clipboard}",
    ),
    (
        "ai-confident",
        "Rewrite in a confident, assertive tone:\n{clipboard}",
        "Rewrite in a confident, assertive tone — remove hedging and qualifiers, keep the claims:\n{clipboard}",
    ),
    (
        "ai-bullets",
        "Convert into a concise bullet-point list:\n{clipboard}",
        "Convert into a concise bullet-point list — one point per line, starting with '- ', keeping the original order:\n{clipboard}",
    ),
    (
        "ai-prompt-opt",
        "Optimize this prompt for an LLM — make it specific, structured, and unambiguous:\n{clipboard}",
        "Rewrite this as a well-structured prompt for a large language model: state the task, give the context, and specify the output format. Return only the prompt:\n{clipboard}",
    ),
    (
        "ai-fluency",
        "Rewrite for natural fluency — smooth, idiomatic phrasing that reads effortlessly:\n{clipboard}",
        "Rewrite for natural fluency — idiomatic phrasing a native speaker would use. Keep the meaning and register:\n{clipboard}",
    ),
    (
        "ai-academic",
        "Rewrite in an academic, scholarly register — precise, formal, hedged where appropriate:\n{clipboard}",
        "Rewrite in an academic register — precise, formal, appropriately hedged. Preserve citations and technical terms:\n{clipboard}",
    ),
    (
        "ai-tone-detect",
        "Analyze the tone of this text. Reply with 2-4 comma-separated tone labels only (e.g. formal, friendly, urgent) — nothing else:\n{clipboard}",
        "Identify the tone of this text. Reply with 2-4 comma-separated lowercase tone labels only (e.g. formal, friendly, urgent) — no other words:\n{clipboard}",
    ),
    (
        "ai-friendlier",
        "Rewrite in a warmer, friendlier tone while keeping the meaning:\n{clipboard}",
        "Rewrite in a warmer, friendlier tone while keeping the meaning and key content:\n{clipboard}",
    ),
    (
        "ai-direct",
        "Rewrite more directly and confidently — cut hedging, filler, and apologies:\n{clipboard}",
        "Rewrite more directly — cut hedging, filler, and apologies. Keep the claims and the meaning:\n{clipboard}",
    ),
    (
        "ai-creative",
        "Rewrite this text creatively — vivid, original phrasing while keeping the meaning:\n{clipboard}",
        "Rewrite this text creatively — vivid, original phrasing that keeps the meaning:\n{clipboard}",
    ),
    (
        "ai-paraphrase",
        "Paraphrase this text in a {mode} style — same meaning, fresh wording:\n{clipboard}",
        "Paraphrase this text in a {mode} style — same meaning, fresh wording, similar length:\n{clipboard}",
    ),
];

// --------------------------------------------------------------------------- //
// Segments — internally tagged on "type"
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
// Trigger binding
// --------------------------------------------------------------------------- //
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriggerBinding {
    #[serde(rename = "type")]
    pub kind: TriggerType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abbreviation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// When true (autotext), `abbreviation` is a regex matched against the typed buffer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub regex: bool,
    /// 1.4: fire immediately (default) or after a delimiter. Literal autotext only —
    /// regex triggers ignore this and always fire immediately.
    #[serde(default, skip_serializing_if = "is_default_execute")]
    pub execute: ExecuteMode,
}

fn is_default_execute(m: &ExecuteMode) -> bool {
    *m == ExecuteMode::Immediate
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Comma-separated settings field (ignore lists, app scopes) → trimmed, lowercase,
/// empty-free set. One definition because these lists are parsed in several places
/// that must agree (startup, set_settings, per-use checks).
pub fn csv_set(s: &str) -> std::collections::HashSet<String> {
    s.split(',')
        .map(|w| w.trim().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

impl TriggerBinding {
    /// autotext ⇒ abbreviation required; hotkey ⇒ key required.
    pub fn validate(&self) -> DomainResult<()> {
        match self.kind {
            TriggerType::Autotext
                if self.abbreviation.is_none() || self.abbreviation.as_deref() == Some("") =>
            {
                Err(DomainError::Validation(
                    "autotext trigger requires 'abbreviation'".into(),
                ))
            }
            TriggerType::Hotkey if self.key.is_none() || self.key.as_deref() == Some("") => Err(
                DomainError::Validation("hotkey trigger requires 'key'".into()),
            ),
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
    /// A disabled folder's phrases are kept but do not trigger (autotext/hotkey) and don't
    /// contribute to the spellcheck vocabulary — a "mute" without deletion.
    #[serde(default = "default_true")]
    pub enabled: bool,
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
    /// If set, the phrase is only active when the focused window title contains this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_scope: Option<String>,
    /// Usage stats: how many times this phrase has been expanded. Not reset on edit.
    #[serde(default)]
    pub expanded_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    /// Free-form tags (lowercase, trimmed) — searchable via `#tag` in the UI.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Case adaptation override (1.4); default Adapt = follow the global toggle.
    #[serde(default)]
    pub case_mode: CaseMode,
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
        if !matches!(
            self.content_type,
            ContentType::Plain | ContentType::Markdown
        ) {
            return Err(DomainError::ReservedContentType(format!(
                "content_type '{:?}' is reserved (supported: plain, markdown)",
                self.content_type
            )));
        }
        for t in &self.triggers {
            t.validate()?;
        }
        Ok(())
    }

    /// True when the body reads files, environment variables, or runs shell commands.
    /// These macros are fine when the user authors them locally, but a phrase arriving
    /// over an untrusted boundary — JSON import, sync snapshot — must not carry them:
    /// expansion would execute `sh -c` / read arbitrary paths with no prompt. Import
    /// and sync both refuse such phrases (see commands::import_json / sync::sync_now).
    pub fn has_untrusted_macros(&self) -> bool {
        self.body.iter().any(|seg| {
            matches!(
                seg,
                Segment::Macro {
                    macro_value: Macro::Shell { .. }
                        | Macro::File { .. }
                        | Macro::Env { .. }
                }
            )
        })
    }
}

// --------------------------------------------------------------------------- //
// Request DTOs (frontend → Rust)
// --------------------------------------------------------------------------- //
#[derive(Debug, Clone, Deserialize)]
pub struct FolderCreate {
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TriggerRequest {
    #[serde(rename = "type")]
    pub kind: TriggerType,
    pub abbreviation: Option<String>,
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhraseCreate {
    pub folder_id: String,
    pub name: String,
    pub abbreviation: Option<String>,
    #[serde(default)]
    pub content_type: ContentType,
    pub body: Vec<Segment>,
    #[serde(default)]
    pub triggers: Vec<TriggerBinding>,
    #[serde(default)]
    pub sensitive: bool,
    pub app_scope: Option<String>,
    pub case_mode: Option<CaseMode>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl PhraseCreate {
    /// Canonicalize: ≤1 autotext trigger; autotext abbreviation must equal / supplies the
    /// phrase abbreviation (uniqueness is on the phrase-level field).
    pub fn normalize(&mut self) -> DomainResult<()> {
        if let Some(t) = canonicalize_autotext(&self.triggers)? {
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

/// Autotext canonicalization shared by the create (`normalize`) and update
/// (`apply_update`) paths: at most one autotext trigger is allowed, and when one is
/// present it owns the phrase abbreviation. Returns the single binding, if any.
pub fn canonicalize_autotext(triggers: &[TriggerBinding]) -> DomainResult<Option<&TriggerBinding>> {
    let autotext: Vec<&TriggerBinding> = triggers
        .iter()
        .filter(|t| t.kind == TriggerType::Autotext)
        .collect();
    if autotext.len() > 1 {
        return Err(DomainError::Validation(
            "at most one autotext trigger is allowed".into(),
        ));
    }
    Ok(autotext.first().copied())
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PhraseUpdate {
    pub name: Option<String>,
    pub folder_id: Option<String>,
    pub abbreviation: Option<String>,
    pub content_type: Option<ContentType>,
    pub body: Option<Vec<Segment>>,
    pub triggers: Option<Vec<TriggerBinding>>,
    pub sensitive: Option<bool>,
    pub app_scope: Option<String>,
    pub case_mode: Option<CaseMode>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExpandRequest {
    pub trigger: TriggerRequest,
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
    pub content_preview: String, // "***" when sensitive
    pub deleted: bool,
    pub expanded_count: u32,
    pub last_used_at: Option<String>,
    pub tags: Vec<String>,
}

/// A snapshot of a phrase's body + triggers (version history).
#[derive(Debug, Clone, Serialize)]
pub struct PhraseVersion {
    pub id: String,
    pub phrase_id: String,
    pub body: Vec<Segment>,
    pub triggers: Vec<TriggerBinding>,
    pub saved_at: String,
    pub preview: String, // literal text preview, for the UI list
}

#[derive(Debug, Clone, Serialize)]
pub struct ExpandResponse {
    pub phrase_id: String,
    pub abbreviation: Option<String>,
    pub content_type: ContentType,
    pub output: String,
}

// --------------------------------------------------------------------------- //
// Settings — single-row app config
// --------------------------------------------------------------------------- //
fn default_popup_hotkey() -> String {
    "Control+Alt+Space".to_string()
}
fn default_true() -> bool {
    true
}
fn default_clipboard_max() -> u32 {
    10000
}
fn default_clipboard_entries() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_popup_hotkey")]
    pub popup_hotkey: String,
    #[serde(default = "default_true")]
    pub autotext_enabled: bool,
    #[serde(default = "default_true")]
    pub clipboard_enabled: bool,
    /// Max characters to capture per clipboard entry (default 10000; 0 = unlimited).
    #[serde(default = "default_clipboard_max")]
    pub clipboard_max_length: u32,
    /// Max number of clipboard entries to keep (default 100; 0 = unlimited).
    #[serde(default = "default_clipboard_entries")]
    pub clipboard_max_entries: u32,
    #[serde(default = "default_true")]
    pub spellcheck_enabled: bool,
    #[serde(default = "default_true")]
    pub case_adapt_enabled: bool,
    #[serde(default)]
    pub spellcheck_ignore: String,
    /// "conservative" (dist 1 only), "normal" (current), "aggressive" (dist 3 + Soundex all lengths).
    #[serde(default = "default_spellcheck_mode")]
    pub spellcheck_mode: String,
    /// How a spellcheck correction is delivered: "silent" (auto-correct inline, the
    /// historic behaviour) or "offer" (pop up candidate(s) near the cursor and only
    /// correct on explicit Accept — sidesteps the cursor/undo race of silent mode).
    #[serde(default = "default_spellcheck_delivery")]
    pub spellcheck_delivery: String,
    /// Global hotkey to toggle spellcheck on/off.
    #[serde(default = "default_spellcheck_hotkey")]
    pub spellcheck_hotkey: String,
    /// Opt-in "review sentence": after a sentence-ending delimiter + space, run an
    /// AI grammar pass over the just-typed sentence and offer the correction via the
    /// offer popup. Off by default — this sends typed text to the AI provider, so it
    /// must never activate silently (scope doc A2: loud, opt-in, per-app-scoped).
    #[serde(default)]
    pub review_sentence: bool,
    /// Comma-separated window-title substrings: spellcheck won't auto-correct when the
    /// focused window's title contains any of these (e.g. "code, terminal, vim").
    #[serde(default)]
    pub spellcheck_exclude_apps: String,
    /// Comma-separated window-title substrings where autotext EXPANSION is fully
    /// disabled (e.g. password managers, terminals). Distinct from per-phrase app_scope.
    #[serde(default)]
    pub autotext_exclude_apps: String,
    #[serde(default)]
    pub autostart: bool,
    /// Start with the main window hidden (to tray). The tray icon still reopens it.
    #[serde(default)]
    pub start_minimized: bool,
    // AI integration (BYO key, OpenAI-compatible endpoint).
    #[serde(default)]
    pub ai_enabled: bool,
    #[serde(default = "default_ai_base_url")]
    pub ai_base_url: String,
    #[serde(default)]
    pub ai_api_key: String,
    #[serde(default = "default_ai_model")]
    pub ai_model: String,
    /// Global hotkey: capture the current text selection and offer AI actions on it
    /// (rewrite/translate/… in place). Empty = disabled.
    #[serde(default = "default_ai_selection_hotkey")]
    pub ai_selection_hotkey: String,
    /// Global "brand voice" — prepended to every AI request as a system message
    /// so all actions/macros share one style directive. Empty = no system message.
    #[serde(default)]
    pub ai_instructions: String,
    /// Sampling temperature sent with every AI call (provider scale 0–2).
    #[serde(default = "default_ai_temperature")]
    pub ai_temperature: f64,
    /// Output cap in tokens; 0 = provider default. Long generations need a raise.
    #[serde(default)]
    pub ai_max_tokens: i64,
    /// Encrypt sensitive phrases, the AI key, and clipboard history at rest
    /// (AES-256-GCM, DEK in the OS keychain). Inert when no keychain is available.
    #[serde(default = "default_true")]
    pub encrypt_at_rest: bool,
    /// File-based sync target directory (TextExpander-style multi-device, local-first).
    /// Empty = sync off. Holds a `library.json` snapshot merged last-writer-wins.
    #[serde(default)]
    pub sync_dir: String,
    /// Sync snapshot passphrase (3.1). Empty = plaintext snapshot (legacy). When set,
    /// the snapshot is sealed with AES-256-GCM under a PBKDF2-HMAC-SHA256 key derived
    /// from it — every device sharing the folder must use the SAME passphrase.
    /// Stored like the AI key: envelope-encrypted at rest when that feature is on.
    #[serde(default)]
    pub sync_passphrase: String,
}

fn default_ai_base_url() -> String {
    "https://api.groq.com/openai/v1".to_string()
}
fn default_ai_selection_hotkey() -> String {
    "Control+Alt+W".to_string()
}
fn default_ai_model() -> String {
    "llama-3.3-70b-versatile".to_string()
}
fn default_ai_temperature() -> f64 {
    1.0
}
fn default_spellcheck_mode() -> String {
    "normal".to_string()
}
fn default_spellcheck_delivery() -> String {
    "silent".to_string()
}
fn default_spellcheck_hotkey() -> String {
    "Control+Shift+Space".to_string()
}

impl Settings {
    /// Built-in defaults (used to seed the settings row on first run).
    pub fn defaults() -> Self {
        Settings {
            popup_hotkey: default_popup_hotkey(),
            autotext_enabled: true,
            clipboard_enabled: true,
            clipboard_max_length: default_clipboard_max(),
            clipboard_max_entries: default_clipboard_entries(),
            spellcheck_enabled: true,
            case_adapt_enabled: true,
            spellcheck_ignore: String::new(),
            spellcheck_mode: default_spellcheck_mode(),
            spellcheck_delivery: default_spellcheck_delivery(),
            spellcheck_hotkey: default_spellcheck_hotkey(),
            spellcheck_exclude_apps: String::new(),
            autotext_exclude_apps: String::new(),
            review_sentence: false,
            autostart: false,
            start_minimized: false,
            ai_enabled: false,
            ai_base_url: default_ai_base_url(),
            ai_api_key: String::new(),
            ai_model: default_ai_model(),
            ai_instructions: String::new(),
            ai_temperature: default_ai_temperature(),
            ai_max_tokens: 0,
            ai_selection_hotkey: default_ai_selection_hotkey(),
            encrypt_at_rest: true,
            sync_dir: String::new(),
            sync_passphrase: String::new(),
        }
    }
}

#[cfg(test)]
mod ai_action_tests {
    use super::*;

    /// Every retemplate CHAIN's final target must be handed to the revamp pass —
    /// a drift here would strand upgraded (untouched) rows on stale pre-revamp
    /// prompts. Chains may have multiple steps; only the last matters.
    #[test]
    fn retemplate_targets_feed_the_revamp() {
        let mut last_target: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::new();
        for (id, _old, new) in RETEMPLATED_AI_ACTIONS {
            last_target.insert(id, new);
        }
        for (id, target) in last_target {
            if let Some(revamped) = REVAMPED_AI_ACTIONS
                .iter()
                .find(|(rid, _, _, olds)| *rid == id && olds.contains(&target))
            {
                let builtin = BUILTIN_AI_ACTIONS
                    .iter()
                    .find(|(bid, _, _)| *bid == id)
                    .unwrap_or_else(|| panic!("revamp target for unknown builtin {id}"));
                assert_eq!(builtin.2, revamped.2, "{id}: revamp drifted from builtin");
            } else {
                // Chain ends at a retired builtin — the retire pass must match
                // that exact text so untouched rows don't strand on stale tools.
                let retired = RETIRED_AI_ACTIONS
                    .iter()
                    .find(|(rid, _, t)| *rid == id && *t == target)
                    .unwrap_or_else(|| {
                        panic!("retemplate chain for {id} ends at '{target}' — neither revamp nor retire picks it up")
                    });
                assert_eq!(retired.2, target, "{id}: retire text drifted");
            }
        }
    }

    /// Every revamp row must point at its builtin's CURRENT name + template, and
    /// its listed pre-revamp templates must never equal the current one (else the
    /// upgrade would fire forever, fighting user edits back).
    #[test]
    fn revamp_targets_match_current_builtins() {
        for (id, name, template, olds) in REVAMPED_AI_ACTIONS {
            let builtin = BUILTIN_AI_ACTIONS
                .iter()
                .find(|(bid, _, _)| *bid == id)
                .unwrap_or_else(|| panic!("revamp of unknown builtin {id}"));
            assert_eq!(builtin.1, name, "{id}: revamp name drifted from builtin");
            assert_eq!(builtin.2, template, "{id}: revamp template drifted from builtin");
            assert!(
                !olds.contains(&template),
                "{id}: a pre-revamp template equals the current one"
            );
        }
    }

    /// Retired ids must be gone from the builtin set — otherwise seeding would
    /// re-insert the row the retire pass just deleted (INSERT OR IGNORE).
    #[test]
    fn retired_ids_are_not_builtins() {
        for (id, _, _) in RETIRED_AI_ACTIONS {
            assert!(
                !BUILTIN_AI_ACTIONS.iter().any(|(bid, _, _)| *bid == id),
                "{id} is retired but still a builtin"
            );
        }
    }

    /// Builtin ids must be unique — seeding keys rows on them.
    #[test]
    fn builtin_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (id, _, _) in BUILTIN_AI_ACTIONS {
            assert!(seen.insert(id), "duplicate builtin id {id}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_macro_detection_covers_shell_file_env() {
        let mk = |m: Macro| Phrase {
            id: "p1".into(),
            folder_id: "f".into(),
            name: "n".into(),
            abbreviation: None,
            content_type: ContentType::Plain,
            body: vec![Segment::Macro { macro_value: m }],
            triggers: Vec::new(),
            sensitive: false,
            app_scope: None,
            expanded_count: 0,
            last_used_at: None,
            case_mode: Default::default(),
            created_at: String::new(),
            updated_at: String::new(),
            deleted_at: None,
            tags: Vec::new(),
        };
        assert!(mk(Macro::Shell { command: "ls".into() }).has_untrusted_macros());
        assert!(mk(Macro::File { path: "/etc/x".into() }).has_untrusted_macros());
        assert!(mk(Macro::Env { var: "HOME".into() }).has_untrusted_macros());
        // Safe macros must NOT be flagged (Nested/Clipboard/etc. stay importable).
        assert!(!mk(Macro::Nested { phrase_id: "x".into() }).has_untrusted_macros());
        assert!(!mk(Macro::Clipboard).has_untrusted_macros());
        assert!(!mk(Macro::DateTime { format: "%Y".into() }).has_untrusted_macros());
    }

    #[test]
    fn execute_mode_json_matches_frontend_and_old_rows() {
        // TS sends "after_delimiter" (types.ts); rows written before the rename
        // carry "afterdelimiter" — both must deserialize.
        assert_eq!(
            serde_json::from_str::<ExecuteMode>("\"after_delimiter\"").unwrap(),
            ExecuteMode::AfterDelimiter
        );
        assert_eq!(
            serde_json::from_str::<ExecuteMode>("\"afterdelimiter\"").unwrap(),
            ExecuteMode::AfterDelimiter
        );
        // Serializes back in the frontend-facing spelling.
        assert_eq!(
            serde_json::to_string(&ExecuteMode::AfterDelimiter).unwrap(),
            "\"after_delimiter\""
        );
        assert_eq!(
            serde_json::to_string(&ExecuteMode::Immediate).unwrap(),
            "\"immediate\""
        );
    }
}
