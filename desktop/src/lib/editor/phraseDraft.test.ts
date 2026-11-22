import { describe, expect, it } from "vitest";
import { buildSavePayload, type PhraseDraft } from "./phraseDraft";
import type { Segment } from "$lib/types";

// Characterization tests for PhraseEditor's save payload (locked before the
// Phase-1 component split). The builder must keep producing exactly the payload
// create_phrase/update_phrase expect.
function draft(over: Partial<PhraseDraft> = {}): PhraseDraft {
  return {
    name: "Signature",
    abbreviation: "sig",
    hotkey: "",
    isRegex: false,
    execute: "immediate",
    appScope: "",
    tagsText: "",
    contentType: "plain",
    caseMode: "adapt",
    sensitive: false,
    segments: [{ type: "text", text: "Best regards" }],
    editFolderId: "f1",
    folderId: "f-prop",
    firstFolderId: "f-first",
    ...over,
  };
}

describe("buildSavePayload", () => {
  it("produces the full save shape", () => {
    const r = buildSavePayload(draft({ tagsText: "sales, work", appScope: "Firefox" }));
    if ("error" in r) throw new Error(r.error);
    expect(r.payload).toEqual({
      name: "Signature",
      folder_id: "f1",
      abbreviation: "sig",
      body: [{ type: "text", text: "Best regards" }],
      triggers: [{ type: "autotext", abbreviation: "sig", regex: false, execute: "immediate" }],
      sensitive: false,
      app_scope: "Firefox",
      content_type: "plain",
      case_mode: "adapt",
      tags: ["sales", "work"],
    });
  });

  it("requires a name", () => {
    expect(buildSavePayload(draft({ name: "   " }))).toEqual({ error: "Name is required" });
  });

  it("requires at least one segment", () => {
    expect(buildSavePayload(draft({ segments: [] as Segment[] }))).toEqual({
      error: "Body needs at least one segment",
    });
  });

  it("falls back through the folder chain and fails only when all are empty", () => {
    const r = buildSavePayload(draft({ editFolderId: "" }));
    expect("payload" in r && r.payload.folder_id).toBe("f-first");
    const r2 = buildSavePayload(draft({ editFolderId: "", firstFolderId: undefined }));
    expect("payload" in r2 && r2.payload.folder_id).toBe("f-prop");
    expect(
      buildSavePayload(draft({ editFolderId: "", firstFolderId: undefined, folderId: "" })),
    ).toEqual({ error: "Select a folder first" });
  });

  it("forces immediate execute for regex abbreviations", () => {
    const r = buildSavePayload(draft({ isRegex: true, execute: "after_delimiter" }));
    expect("payload" in r && r.payload.triggers[0]?.execute).toBe("immediate");
  });

  it("adds a hotkey trigger alongside autotext", () => {
    const r = buildSavePayload(draft({ hotkey: " ctrl+shift+b " }));
    expect("payload" in r && r.payload.triggers).toEqual([
      { type: "autotext", abbreviation: "sig", regex: false, execute: "immediate" },
      { type: "hotkey", key: "ctrl+shift+b" },
    ]);
  });

  it("omits the autotext trigger for a blank abbreviation", () => {
    const r = buildSavePayload(draft({ abbreviation: "" }));
    expect("payload" in r && r.payload.triggers).toEqual([]);
    expect("payload" in r && r.payload.abbreviation).toBeNull();
  });

  it("trims and drops empty tags", () => {
    const r = buildSavePayload(draft({ tagsText: " a , , b ,, " }));
    expect("payload" in r && r.payload.tags).toEqual(["a", "b"]);
  });

  it("nulls blank app_scope", () => {
    const r = buildSavePayload(draft({ appScope: "  " }));
    expect("payload" in r && r.payload.app_scope).toBeNull();
  });
});
