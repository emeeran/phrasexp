// TypeScript mirror of the Rust domain models (src-tauri/src/domain/models.rs).
// Keep these in sync with the Rust structs — the Tauri command layer serializes 1:1.

export type ContentType = "plain" | "richtext" | "html" | "word";
export type TriggerType = "autotext" | "hotkey";

export interface DateTimeMacro {
  type: "datetime";
  format?: string; // strftime / chrono format, default "%Y-%m-%d %H:%M"
}
export interface DateMathMacro {
  type: "datemath";
  days_offset?: number; // default 0
  format?: string; // default "%Y-%m-%d"
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
export type Macro =
  | DateTimeMacro
  | DateMathMacro
  | InputFormMacro
  | RandomMacro
  | MathMacro
  | ClipboardMacro
  | NestedMacro;

export type SortKey = "name" | "updated" | "created";

export type OutputMode = "type" | "copy";

/** Shape returned by export_library. */
export interface LibraryExport {
  folders: Folder[];
  phrases: Phrase[];
}

/** A prior snapshot of a phrase (version history). */
export interface PhraseVersion {
  id: string;
  phrase_id: string;
  body: Segment[];
  triggers: TriggerBinding[];
  saved_at: string;
  preview: string;
}

export interface TextSegment {
  type: "text";
  text: string;
}
export interface MacroSegment {
  type: "macro";
  macro: Macro;
}
export type Segment = TextSegment | MacroSegment;

export interface TriggerBinding {
  type: TriggerType;
  abbreviation?: string | null;
  key?: string | null;
  regex?: boolean;
}

export interface Folder {
  id: string;
  name: string;
  parent_id?: string | null;
  created_at: string;
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
  created_at: string;
  updated_at: string;
  deleted_at?: string | null;
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
  deleted: boolean;
}

export interface ExpandResponse {
  phrase_id: string;
  abbreviation?: string | null;
  content_type: ContentType;
  output: string;
}

export interface AutotextStatus {
  enabled: boolean;
  running: boolean;
  events_seen: number;
  abbreviations: number;
}

export interface Settings {
  popup_hotkey: string;
  autotext_enabled: boolean;
  clipboard_enabled: boolean;
  autostart: boolean;
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
}
export interface PhraseUpdate {
  name?: string;
  abbreviation?: string | null;
  content_type?: ContentType;
  body?: Segment[];
  triggers?: TriggerBinding[];
  sensitive?: boolean;
}
export interface ExpandRequest {
  trigger: { type: TriggerType; abbreviation?: string; key?: string };
  inputs?: Record<string, string>;
}

// Serialized domain error from Rust (Tauri IPC has no HTTP status — code carries it).
export interface ApiError {
  code:
    | "not_found"
    | "conflict"
    | "folder_not_empty"
    | "missing_input"
    | "cycle"
    | "reserved_content_type"
    | "validation";
  message: string;
}
