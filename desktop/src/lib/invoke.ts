// Typed wrappers over the Tauri command layer (src-tauri/src/commands.rs).
// Tauri maps camelCase JS arg keys -> snake_case Rust params automatically.

import { invoke } from "@tauri-apps/api/core";
import type {
  AutotextStatus,
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
  Settings,
  SortKey,
  TriggerType,
} from "./types";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

export const api = {
  // Folders (FR-2)
  listFolders: () => call<Folder[]>("list_folders"),
  createFolder: (payload: FolderCreate) => call<Folder>("create_folder", { payload }),
  deleteFolder: (folderId: string) => call<void>("delete_folder", { folderId }),

  // Phrases (FR-1,6,7,8)
  listPhrases: (
    folderId?: string | null,
    q?: string,
    includeDeleted = false,
    sort: SortKey = "name",
  ) =>
    call<PhraseSummary[]>("list_phrases", {
      folderId: folderId ?? null,
      q: q ?? null,
      includeDeleted,
      sort,
    }),
  getPhrase: (phraseId: string) => call<Phrase>("get_phrase", { phraseId }),
  createPhrase: (payload: PhraseCreate) => call<Phrase>("create_phrase", { payload }),
  updatePhrase: (phraseId: string, payload: PhraseUpdate) =>
    call<Phrase>("update_phrase", { phraseId, payload }),
  deletePhrase: (phraseId: string) => call<void>("delete_phrase", { phraseId }),

  // Execution (FR-4)
  expand: (
    type: TriggerType,
    value: string,
    inputs: Record<string, string> = {},
  ) => {
    const trigger =
      type === "autotext" ? { type, abbreviation: value } : { type, key: value };
    return call<ExpandResponse>("expand_phrase", { req: { trigger, inputs } });
  },

  // M3: type a phrase's expansion into the focused app via the Injector.
  injectPhrase: (phraseId: string, inputs?: Record<string, string>, mode: OutputMode = "type") =>
    call<string>("inject_phrase", { phraseId, inputs: inputs ?? null, mode }),

  // Library import/export.
  exportLibrary: () => call<LibraryExport>("export_library"),
  importCsv: (csvText: string, folderId?: string | null) =>
    call<number>("import_csv", { csvText, folderId: folderId ?? null }),
  importAhk: (ahkText: string, folderId?: string | null) =>
    call<number>("import_ahk", { ahkText, folderId: folderId ?? null }),

  // Phase 1: duplicate finder.
  findDuplicates: () => call<PhraseSummary[][]>("find_duplicates"),

  // Phase 1: version history.
  listVersions: (phraseId: string) => call<PhraseVersion[]>("list_versions", { phraseId }),
  restoreVersion: (phraseId: string, versionId: string) =>
    call<Phrase>("restore_version", { phraseId, versionId }),

  // M4: silent autotext listener status + toggle.
  autotextStatus: () => call<AutotextStatus>("autotext_status"),
  setAutotext: (enabled: boolean) => call<boolean>("set_autotext", { enabled }),

  // M5: app settings.
  getSettings: () => call<Settings>("get_settings"),
  setSettings: (payload: Settings) => call<Settings>("set_settings", { payload }),

  // Clipboard (FR-9)
  listClipboard: (limit = 50) => call<ClipboardEntry[]>("list_clipboard", { limit }),
  addClipboard: (text: string) =>
    call<ClipboardEntry>("add_clipboard", { payload: { text } }),
  clearClipboard: () => call<void>("clear_clipboard"),
  resetDatabase: () => call<void>("reset_database"),
  folderCounts: () => call<[string, number][]>("folder_counts"),
  testAi: () => call<string>("test_ai"),
  listAiModels: () => call<string[]>("list_ai_models"),
};

export function errMsg(e: unknown): string {
  if (typeof e === "object" && e !== null && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}
