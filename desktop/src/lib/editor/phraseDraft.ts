// Pure save-payload construction for the phrase editor — extracted from
// PhraseEditor.svelte's save() so the shape is unit-testable. Behavior is
// characterized by phraseDraft.test.ts; keep both in sync.

import type {
  CaseMode,
  ContentType,
  ExecuteMode,
  Segment,
  TriggerBinding,
} from "$lib/types";

/** Editor form state that feeds the save payload (no AI-draft or preview state). */
export interface PhraseDraft {
  name: string;
  abbreviation: string;
  hotkey: string;
  isRegex: boolean;
  execute: ExecuteMode;
  appScope: string;
  tagsText: string;
  contentType: ContentType;
  caseMode: CaseMode;
  sensitive: boolean;
  segments: Segment[];
  /** The folder select's value at save time. */
  editFolderId: string;
  /** Mount-time folder prop (used when the select left the value unset). */
  folderId: string;
  /** First folder in the list — last-resort guarantee so folder_id is never missing. */
  firstFolderId?: string;
}

export type SavePayload = {
  name: string;
  folder_id: string;
  abbreviation: string | null;
  body: Segment[];
  triggers: TriggerBinding[];
  sensitive: boolean;
  app_scope: string | null;
  content_type: ContentType;
  case_mode: CaseMode;
  tags: string[];
};

/** Validation failure message, or the ready-to-send payload. Mirrors save(). */
export function buildSavePayload(
  d: PhraseDraft,
): { error: string } | { payload: SavePayload } {
  if (!d.name.trim()) return { error: "Name is required" };
  if (d.segments.length === 0) return { error: "Body needs at least one segment" };
  // Guarantee a folder: the <select> binding can leave editFolderId unset, which would
  // omit folder_id from the payload and make create_phrase fail with "missing field".
  const folder = d.editFolderId || d.firstFolderId || d.folderId;
  if (!folder) return { error: "Select a folder first" };

  const triggers: TriggerBinding[] = [];
  if (d.abbreviation.trim())
    triggers.push({
      type: "autotext",
      abbreviation: d.abbreviation.trim(),
      regex: d.isRegex,
      // delayed firing only makes sense for literal abbreviations
      execute: d.isRegex ? "immediate" : d.execute,
    });
  if (d.hotkey.trim()) triggers.push({ type: "hotkey", key: d.hotkey.trim() });

  return {
    payload: {
      name: d.name.trim(),
      folder_id: folder,
      abbreviation: d.abbreviation.trim() || null,
      body: d.segments,
      triggers,
      sensitive: d.sensitive,
      app_scope: d.appScope.trim() || null,
      content_type: d.contentType,
      case_mode: d.caseMode,
      tags: d.tagsText
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean),
    },
  };
}
