// Typed wrappers over the Tauri command layer (src-tauri/src/commands.rs).
// Tauri maps camelCase JS arg keys -> snake_case Rust params automatically.

import { invoke } from "@tauri-apps/api/core";
import type {
  AutotextStatus,
  AiAction,
  ClipboardEntry,
  ExpandResponse,
  Folder,
  FolderCreate,
  LibraryExport,
  OutputMode,
  Phrase,
  PhraseCreate,
  PhraseSummary,
  PhraseUpdate,
  PhraseVersion,
  CryptoStatus,
  Settings,
  SortKey,
  SyncReport,
  TriggerType,
} from "./types";

/** Trigger payload for a hotkey/autotext pair: autotexts bind an abbreviation, hotkeys a key. */
const triggerOf = (type: TriggerType, value: string) =>
  type === "autotext" ? { type, abbreviation: value } : { type, key: value };

export const api = {
  // Folders
  listFolders: () => invoke<Folder[]>("list_folders"),
  createFolder: (payload: FolderCreate) => invoke<Folder>("create_folder", { payload }),
  deleteFolder: (folderId: string) => invoke<void>("delete_folder", { folderId }),
  mergeFolders: (sourceId: string, targetId: string) =>
    invoke<number>("merge_folders", { sourceId, targetId }),
  setFolderEnabled: (folderId: string, enabled: boolean) =>
    invoke<boolean>("set_folder_enabled", { folderId, enabled }),

  // Phrases
  listPhrases: (
    folderId?: string | null,
    q?: string,
    includeDeleted = false,
    sort: SortKey = "name",
  ) =>
    invoke<PhraseSummary[]>("list_phrases", {
      folderId: folderId ?? null,
      q: q ?? null,
      includeDeleted,
      sort,
    }),
  getPhrase: (phraseId: string) => invoke<Phrase>("get_phrase", { phraseId }),
  createPhrase: (payload: PhraseCreate) => invoke<Phrase>("create_phrase", { payload }),
  updatePhrase: (phraseId: string, payload: PhraseUpdate) =>
    invoke<Phrase>("update_phrase", { phraseId, payload }),
  deletePhrase: (phraseId: string) => invoke<void>("delete_phrase", { phraseId }),
  saveImage: (dataUrl: string) => invoke<string>("save_image", { dataUrl }),
  readImage: (path: string) => invoke<string>("read_image", { path }),

  // Execution
  expand: (
    type: TriggerType,
    value: string,
    inputs: Record<string, string> = {},
  ) => {
    return invoke<ExpandResponse>("expand_phrase", {
      req: { trigger: triggerOf(type, value), inputs },
    });
  },
  // Resolve a trigger to its phrase WITHOUT expanding — used by the Test panel to detect
  // inputform fields and collect them before expansion.
  resolvePhrase: (type: TriggerType, value: string) => {
    return invoke<Phrase | null>("resolve_phrase", {
      trigger: triggerOf(type, value),
    });
  },

  // Type a phrase's expansion into the focused app via the Injector.
  injectPhrase: (phraseId: string, inputs?: Record<string, string>, mode: OutputMode = "type") =>
    invoke<string>("inject_phrase", { phraseId, inputs: inputs ?? null, mode }),

  // Library import/export.
  exportLibrary: () => invoke<LibraryExport>("export_library"),
  importCsv: (csvText: string, folderId?: string | null) =>
    invoke<number>("import_csv", { csvText, folderId: folderId ?? null }),
  importAhk: (ahkText: string, folderId?: string | null) =>
    invoke<number>("import_ahk", { ahkText, folderId: folderId ?? null }),
  importAtext: (atextText: string, folderId?: string | null) =>
    invoke<number>("import_atext", { atextText, folderId: folderId ?? null }),
  /** TextExpander CSV export (abbreviation,snippet,label). */
  importTextexpander: (csvText: string, folderId?: string | null) =>
    invoke<number>("import_textexpander", { csvText, folderId: folderId ?? null }),
  /** Espanso matches YAML (literal trigger/replace pairs only). */
  importEspanso: (yamlText: string, folderId?: string | null) =>
    invoke<number>("import_espanso", { yamlText, folderId: folderId ?? null }),
  importJson: (jsonText: string, folderId?: string | null) =>
    invoke<number>("import_json", { jsonText, folderId: folderId ?? null }),

  // Duplicate finder.
  findDuplicates: () => invoke<PhraseSummary[][]>("find_duplicates"),

  // Version history.
  listVersions: (phraseId: string) => invoke<PhraseVersion[]>("list_versions", { phraseId }),
  restoreVersion: (phraseId: string, versionId: string) =>
    invoke<Phrase>("restore_version", { phraseId, versionId }),

  // Silent autotext listener status + toggle.
  autotextStatus: () => invoke<AutotextStatus>("autotext_status"),
  applyCorrection: (corrected: string) => invoke<boolean>("apply_correction", { corrected }),
  deleteOfferWord: () => invoke<boolean>("delete_offer_word"),
  spellcheckIgnoreWord: (word: string) => invoke<void>("spellcheck_ignore_word", { word }),
  focusPopup: () => invoke<void>("focus_popup"),
  /** Size the popup window to the measured content (drag + adaptive sizing). */
  resizePopup: (width?: number, height?: number) =>
    invoke<void>("resize_popup", { width: width ?? null, height: height ?? null }),
  grabKeyboard: () => invoke<void>("grab_keyboard"),
  releaseKeyboard: () => invoke<void>("release_keyboard"),

  // App settings.
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (payload: Settings) => invoke<Settings>("set_settings", { payload }),

  // At-rest encryption status (keychain available + setting enabled).
  cryptoStatus: () => invoke<CryptoStatus>("crypto_status"),

  // Clipboard
  listClipboard: (limit = 50) => invoke<ClipboardEntry[]>("list_clipboard", { limit }),
  addClipboard: (text: string) =>
    invoke<ClipboardEntry>("add_clipboard", { payload: { text } }),
  clearClipboard: () => invoke<void>("clear_clipboard"),
  resetDatabase: () => invoke<void>("reset_database"),
  folderCounts: () => invoke<[string, number][]>("folder_counts"),
  testAi: () => invoke<string>("test_ai"),
  listAiModels: () => invoke<string[]>("list_ai_models"),
  listAiActions: () => invoke<AiAction[]>("list_ai_actions"),
  saveAiAction: (payload: AiAction) => invoke<AiAction>("save_ai_action", { payload }),
  deleteAiAction: (actionId: string) => invoke<void>("delete_ai_action", { actionId }),
  /** Run an AI action on the captured selection (Rust-side slot); the rewrite streams
   *  back as `ai-delta` events and the popup re-shows in result mode (`ai-result`) —
   *  nothing is pasted until ai_decide. `params` fills the template's interactive
   *  placeholders keyed by token (`{language}`, `{length}`, …). */
  runAiAction: (actionId: string, params?: Record<string, string>) =>
    invoke<string>("run_ai_action", { actionId, params: params ?? null }),
  /** Abort an in-flight streaming AI request (popup discard/cancel mid-stream). */
  aiCancel: () => invoke<void>("ai_cancel"),
  /** Resolve the result popup: replace | insert | copy | retry | discard.
   *  `text` overrides the pending result — sent when the user picked one option
   *  out of a multi-option response. */
  aiDecide: (
    decision: "replace" | "insert" | "copy" | "retry" | "discard",
    text?: string,
  ) => invoke<string>("ai_decide", { decision, text }),
  /** Generate rewording variations of a text (editor "variations" feature). */
  aiVariations: (text: string, count = 3) => invoke<string[]>("ai_variations", { text, count }),

  // File-based sync (Settings → Data).
  syncNow: () => invoke<SyncReport>("sync_now"),
  syncSetDir: (dir: string) => invoke<void>("sync_set_dir", { dir }),
  syncStatus: () =>
    invoke<{ dir: string; device: string | null; exported_at: string | null; phrases: number | null }>(
      "sync_status",
    ),
};

export function errMsg(e: unknown): string {
  if (typeof e === "object" && e !== null && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}
