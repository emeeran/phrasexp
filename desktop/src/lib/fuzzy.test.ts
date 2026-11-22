import { describe, expect, it } from "vitest";
import { fuzzyScore, rankPhrases } from "./fuzzy";

describe("fuzzyScore", () => {
  it("rejects non-subsequences", () => {
    expect(fuzzyScore("xyz", "Address block")).toBe(-1);
    expect(fuzzyScore("blockk", "Address block")).toBe(-1);
  });

  it("returns 0 for empty query", () => {
    expect(fuzzyScore("", "anything")).toBe(0);
  });

  it("rewards exact whole-string match above everything", () => {
    const exact = fuzzyScore("sig", "sig");
    expect(exact).toBe(100);
  });

  it("consecutive beats scattered", () => {
    const consec = fuzzyScore("adr", "address");
    const scattered = fuzzyScore("adr", "a diffeRence");
    expect(consec).toBeGreaterThan(scattered);
  });

  it("word-start bonus: matching at word starts beats mid-word", () => {
    // "bl" in "block party": b+l consecutive at word start vs scattered mid-word in "bubble leaf"? use clean pair:
    const wordStart = fuzzyScore("bp", "block Party");
    const midWord = fuzzyScore("bp", "BumP");
    expect(wordStart).toBeGreaterThan(midWord);
  });

  it("is case-insensitive", () => {
    expect(fuzzyScore("ADR", "address")).toBe(fuzzyScore("adr", "address"));
  });
});

describe("rankPhrases", () => {
  const items = [
    { name: "Address block", abbreviation: "adr" },
    { name: "Adaptive design", abbreviation: null },
    { name: "Sign-off", abbreviation: "sig" },
  ];

  it("empty query returns items unchanged (up to limit)", () => {
    expect(rankPhrases("", items)).toEqual(items);
    expect(rankPhrases("   ", items, 2)).toHaveLength(2);
  });

  it("no match returns empty", () => {
    expect(rankPhrases("zzz", items)).toEqual([]);
  });

  it("abbreviation exact match outranks name fuzzy match", () => {
    const ranked = rankPhrases("adr", items);
    expect(ranked[0].name).toBe("Address block");
  });

  it("ranks subsequence matches on name", () => {
    const ranked = rankPhrases("sign", items);
    expect(ranked[0].name).toBe("Sign-off");
  });

  it("ties break by expanded_count desc", () => {
    const a = { name: "alpha thing", abbreviation: null, expanded_count: 5 };
    const b = { name: "aqua thing", abbreviation: null, expanded_count: 9 };
    // identical match shape ("thing" at word start, same distance) — usage decides
    const ranked = rankPhrases("thing", [a, b]);
    expect(ranked[0]).toBe(b);
  });

  it("respects the limit", () => {
    const many = Array.from({ length: 80 }, (_, i) => ({ name: `item ${i}`, abbreviation: null }));
    expect(rankPhrases("item", many, 50)).toHaveLength(50);
  });

  // -- #tag filters + multi-term AND (1.6) ---------------------------------- //
  const tagged = [
    { name: "Q4 followup", abbreviation: null, tags: ["sales", "q4"] },
    { name: "Invoice past due", abbreviation: "inv", tags: ["invoicing"] },
    { name: "Address block", abbreviation: "adr", tags: [] },
  ];

  it("#tag filters by tag (case-insensitive, without fuzzy pass)", () => {
    expect(rankPhrases("#sales", tagged).map((p) => p.name)).toEqual(["Q4 followup"]);
    expect(rankPhrases("#SALES", tagged).map((p) => p.name)).toEqual(["Q4 followup"]);
  });

  it("multiple #tags AND together", () => {
    expect(rankPhrases("#sales #q4", tagged).map((p) => p.name)).toEqual(["Q4 followup"]);
    expect(rankPhrases("#sales #invoicing", tagged)).toEqual([]);
  });

  it("#tag + free term combine (filter, then fuzzy AND)", () => {
    expect(rankPhrases("#invoicing due", tagged).map((p) => p.name)).toEqual([
      "Invoice past due",
    ]);
    // Free term that doesn't match the tagged survivor → empty.
    expect(rankPhrases("#invoicing zzz", tagged)).toEqual([]);
  });

  it("unknown tag yields nothing", () => {
    expect(rankPhrases("#nope", tagged)).toEqual([]);
  });

  it("multiple free terms AND (all must match name or abbr)", () => {
    expect(rankPhrases("invoice due", tagged).map((p) => p.name)).toEqual(["Invoice past due"]);
    expect(rankPhrases("invoice zzz", tagged)).toEqual([]);
  });
});
