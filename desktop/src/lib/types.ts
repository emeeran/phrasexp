// TypeScript mirror of the Rust domain models (src-tauri/src/domain/models.rs).
// Keep these in sync with the Rust structs — the Tauri command layer serializes 1:1.

export type ContentType = "plain" | "markdown" | "richtext" | "html" | "word";
export type TriggerType = "autotext" | "hotkey";

export interface DateTimeMacro {
  type: "datetime";
  format?: string; // strftime / chrono format, default "%d-%m-%Y %H:%M"
}
export interface DateMathMacro {
  type: "datemath";
  days_offset?: number; // default 0
  format?: string; // default "%d-%m-%Y"
}
export type InputKind = "text" | "select" | "checkbox";
export interface InputFormMacro {
  type: "inputform";
  field: string;
  kind?: InputKind;
  options?: string[];
}
export interface RandomMacro {
  type: "random";
  min?: number;
  max?: number;
}
export interface MathMacro {
  type: "math";
  expr: string;
}
export interface ClipboardMacro {
  type: "clipboard";
}
export interface NestedMacro {
  type: "nested";
  phrase_id: string;
}
export interface EnvMacro {
  type: "env";
  var: string;
}
export interface PickMacro {
  type: "pick";
  options: string[];
}
export interface FileMacro {
  type: "file";
  path: string;
}
export interface AiMacro {
  type: "ai";
  prompt: string;
}
/** A user-editable AI action preset (stored in the ai_actions table). */
export interface AiAction {
  id: string;
  name: string;
  prompt_template: string;
  sort_order: number;
  builtin: boolean;
}
/** Reference to an AI action by id — resolved at expand time. */
export interface AiActionMacro {
  type: "aiaction";
  action_id: string;
}
export interface ImageMacro {
  type: "image";
  /** Path of the PNG stored under the app data dir (see save_image). */
  path: string;
}
export interface UuidMacro {
  type: "uuid";
}
export interface ShellMacro {
  type: "shell";
  /** Shell command (`sh -c` / `cmd /C`); its stdout is inserted, newline-trimmed. */
  command: string;
}
export interface CounterMacro {
  type: "counter";
  /** Named persistent counter — increments by 1 on every expansion. */
  name: string;
}
export interface DelayMacro {
  type: "delay";
  /** Pause in milliseconds before the next segment (0–5000, clamped at expand). */
  ms: number;
}
export interface CursorMacro {
  type: "cursor";
}
export interface CiteMacro {
  type: "cite";
  /** DOI, URL, or raw title — blank prompts once at expansion. */
  source: string;
  /** apa | mla | chicago */
  style?: "apa" | "mla" | "chicago";
}
export type Macro =
  | DateTimeMacro
  | DateMathMacro
  | InputFormMacro
  | RandomMacro
  | MathMacro
  | ClipboardMacro
  | NestedMacro
  | EnvMacro
  | PickMacro
  | FileMacro
  | AiMacro
  | AiActionMacro
  | ImageMacro
  | UuidMacro
  | ShellMacro
  | CounterMacro
  | DelayMacro
  | CursorMacro
  | CiteMacro;

export type SortKey = "name" | "updated" | "created" | "most_used";

/** Per-phrase case adaptation override (1.4). Adapt = follow global setting. */
export type CaseMode = "adapt" | "always" | "never";

export type OutputMode = "type" | "copy";

/** Shape returned by export_library. */
export interface LibraryExport {
  folders: Folder[];
  phrases: Phrase[];
  /** How many exported phrases are marked sensitive — their bodies are plaintext. */
  sensitive_count?: number;
}

/** A prior snapshot of a phrase (version history). */
export interface PhraseVersion {
  id: string;
  phrase_id: string;
  saved_at: string;
  preview: string;
}

interface TextSegment {
  type: "text";
  text: string;
}
interface MacroSegment {
  type: "macro";
  macro: Macro;
}
export type Segment = TextSegment | MacroSegment;

/** When an autotext abbreviation fires (1.4). Default immediate. */
export type ExecuteMode = "immediate" | "after_delimiter";

export interface TriggerBinding {
  type: TriggerType;
  abbreviation?: string | null;
  key?: string | null;
  regex?: boolean;
  execute?: ExecuteMode;
}

export interface Folder {
  id: string;
  name: string;
  parent_id?: string | null;
  enabled?: boolean;
}

export interface Phrase {
  id: string;
  folder_id: string;
  name: string;
  abbreviation?: string | null;
  content_type: ContentType;
  body: Segment[];
  triggers: TriggerBinding[];
  sensitive: boolean;
  app_scope?: string | null;
  /** Usage stats (1.4): expansion counter + last-used, never reset by edits. */
  expanded_count?: number;
  last_used_at?: string | null;
  /** Case adaptation override (1.4). */
  case_mode?: CaseMode;
  /** Free-form tags (lowercase) — searchable via `#tag`. */
  tags?: string[];
}

export interface ClipboardEntry {
  id: string;
  text: string;
  captured_at: string;
}

// Response DTOs
export interface PhraseSummary {
  id: string;
  folder_id: string;
  name: string;
  abbreviation?: string | null;
  content_type: ContentType;
  sensitive: boolean;
  content_preview: string;
  expanded_count?: number;
  last_used_at?: string | null;
  tags?: string[];
}

export interface ExpandResponse {
  output: string;
}

export interface AutotextStatus {
  enabled: boolean;
  running: boolean;
  /** macOS only: false = Accessibility not granted (listener runs but is deaf). */
  accessibility_granted?: boolean | null;
  /** macOS only: Secure Input is suppressing global keystroke taps. */
  secure_input_active?: boolean | null;
}

export interface Settings {
  popup_hotkey: string;
  autotext_enabled: boolean;
  clipboard_enabled: boolean;
  clipboard_max_length?: number;
  clipboard_max_entries?: number;
  spellcheck_enabled?: boolean;
  case_adapt_enabled?: boolean;
  spellcheck_ignore?: string;
  spellcheck_mode?: string;
  spellcheck_delivery?: string;
  spellcheck_hotkey?: string;
  spellcheck_exclude_apps?: string;
  /** Window-title substrings where autotext expansion is fully disabled. */
  autotext_exclude_apps?: string;
  autostart: boolean;
  start_minimized?: boolean;
  ai_enabled?: boolean;
  ai_base_url?: string;
  ai_api_key?: string;
  ai_model?: string;
  /** Global brand voice — sent as a system message with every AI call. */
  ai_instructions?: string;
  /** Sampling temperature (0–2, provider scale). */
  ai_temperature?: number;
  /** Output cap in tokens; 0 = provider default. */
  ai_max_tokens?: number;
  /** Hotkey: capture the selection and offer AI actions on it. Empty = disabled. */
  ai_selection_hotkey?: string;
  /** Opt-in: after a sentence ends, offer an AI grammar pass via the offer popup.
   *  Off by default — it sends typed text to the AI provider (needs AI enabled). */
  review_sentence?: boolean;
  /** Encrypt sensitive phrases, the AI key, and clipboard history at rest. */
  encrypt_at_rest?: boolean;
  /** File-based sync folder (empty = off). Holds phrasexp-library.json. */
  sync_dir?: string;
}

/** What one sync run did (Settings → Data → sync). */
export interface SyncReport {
  imported: number;
  updated: number;
  deleted: number;
  stored: number;
}

/** Runtime status of at-rest encryption (from the `crypto_status` command). */
export interface CryptoStatus {
  available: boolean;
}

// Request DTOs
export interface FolderCreate {
  name: string;
  parent_id?: string | null;
}
export interface PhraseCreate {
  folder_id: string;
  name: string;
  abbreviation?: string | null;
  content_type?: ContentType;
  body: Segment[];
  triggers?: TriggerBinding[];
  sensitive?: boolean;
  app_scope?: string | null;
  case_mode?: CaseMode;
  tags?: string[];
}
export interface PhraseUpdate {
  name?: string;
  folder_id?: string | null;
  abbreviation?: string | null;
  content_type?: ContentType;
  body?: Segment[];
  triggers?: TriggerBinding[];
  sensitive?: boolean;
  app_scope?: string | null;
  case_mode?: CaseMode;
  tags?: string[];
}
