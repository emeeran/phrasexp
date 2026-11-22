import { describe, expect, it } from "vitest";
import { argPromptFor, paramTokens } from "./aiArgs";

// Characterization tests: the multi-token frontend mirror of Rust's param_tokens.
// Known divergence vs Rust's param_token (kept for required-param checks): Rust's
// scanner rejects "a {b {weird} c" outright (it sees "{b {weird" first and gives
// up), while this regex mirror finds "{weird}" — behavior locked here is the JS one.
describe("paramTokens", () => {
  it("finds the interactive placeholder", () => {
    expect(paramTokens("Translate to {language}:\n{clipboard}")).toEqual(["language"]);
    expect(paramTokens("Summarize — {length}:\n{clipboard}")).toEqual(["length"]);
  });

  it("ignores clipboard/selection placeholders", () => {
    expect(paramTokens("Fix grammar:\n{clipboard}")).toEqual([]);
    expect(paramTokens("Rewrite:\n{selection}")).toEqual([]);
  });

  it("lists every distinct interactive token in template order", () => {
    expect(paramTokens("{clipboard} to {language}, {length} as {format}")).toEqual([
      "language",
      "length",
      "format",
    ]);
    expect(paramTokens("{a} {a} {b}")).toEqual(["a", "b"]);
  });

  it("accepts underscores in token names", () => {
    expect(paramTokens("Tone: {target_tone}\n{clipboard}")).toEqual(["target_tone"]);
  });

  it("rejects tokens with non-word characters (over-strict, like Rust)", () => {
    expect(paramTokens("keep {not a param} and {x-y} here")).toEqual([]);
  });

  it("handles unclosed and nested braces (see header note re: Rust divergence)", () => {
    expect(paramTokens("a {b {weird} c")).toEqual(["weird"]);
    expect(paramTokens("50% off {")).toEqual([]);
    expect(paramTokens("{}")).toEqual([]);
  });
});

describe("argPromptFor", () => {
  it("has curated copy + chips for known tokens", () => {
    expect(argPromptFor("language").label).toBe("translate to");
    expect(argPromptFor("mode").chips).toContain("academic");
    expect(argPromptFor("length").placeholder).toContain("30 words");
  });

  it("falls back to a generic ask for unknown tokens", () => {
    const p = argPromptFor("audience");
    expect(p.label).toBe("audience");
    expect(p.placeholder).toBe("value for {audience}…");
    expect(p.chips).toBeUndefined();
  });
});

describe("argPromptFor", () => {
  it("has curated copy for known tokens", () => {
    expect(argPromptFor("language").label).toBe("translate to");
    expect(argPromptFor("length").placeholder).toContain("30 words");
  });

  it("falls back to a generic ask for unknown tokens", () => {
    const p = argPromptFor("audience");
    expect(p.label).toBe("audience");
    expect(p.placeholder).toBe("value for {audience}…");
  });
});
