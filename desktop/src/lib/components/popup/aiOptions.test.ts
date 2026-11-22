import { describe, expect, it } from "vitest";
import { splitAiOptions } from "./aiOptions";

describe("splitAiOptions", () => {
  it("keeps prose without option headings as one blob", () => {
    const text = "The corrected sentence is:\n\nShe walked to the store.";
    expect(splitAiOptions(text)).toEqual([{ label: "", text }]);
  });

  it("splits a pure numbered list into bare options", () => {
    const text = "1. First version\n2. Second version\n3. Third version";
    expect(splitAiOptions(text)).toEqual([
      { label: "", text: "First version" },
      { label: "", text: "Second version" },
      { label: "", text: "Third version" },
    ]);
  });

  it("extracts quoted responses from headed sections, dropping preamble and rationale", () => {
    const text = [
      "The original phrasing is grammatically incomplete and jargon-heavy.",
      "",
      "Here are a few improved versions depending on tone:",
      "",
      "### Option 1: Clear, Professional, & Complete (Recommended)",
      '> "We and our partners use cookies, tracking pixels, and SDKs **on this website/app.**"',
      ">",
      "> * **Why it works:** Connects the fragment to a logical conclusion and keeps a neutral tone.",
      "",
      "### Option 2: More User-Friendly & Less Technical",
      '> "We and our partners place cookies and SDKs **on your device.**"',
      ">",
      "> * **Why it works:** \"On your device\" is more concrete than \"use.\"",
    ].join("\n");

    expect(splitAiOptions(text)).toEqual([
      {
        label: "Clear, Professional, & Complete (Recommended)",
        text: "We and our partners use cookies, tracking pixels, and SDKs on this website/app.",
      },
      {
        label: "More User-Friendly & Less Technical",
        text: "We and our partners place cookies and SDKs on your device.",
      },
    ]);
  });

  it("falls back to the first paragraph when a section has no quotes", () => {
    const text = [
      "Two takes:",
      "",
      "**Option 1** — direct",
      "She went to the store.",
      "",
      "Rationale: shortest fix.",
      "",
      "**Option 2** — detailed",
      "She walked to the store to buy bread.",
    ].join("\n");

    expect(splitAiOptions(text)).toEqual([
      { label: "— direct", text: "She went to the store." },
      { label: "— detailed", text: "She walked to the store to buy bread." },
    ]);
  });

  it("drops sections that yielded no response instead of failing the whole split", () => {
    const text = [
      "### Option 1: Empty",
      "",
      "### Option 2: Real",
      '> "Keep this one."',
    ].join("\n");
    expect(splitAiOptions(text)).toEqual([{ label: "Real", text: "Keep this one." }]);
  });

  it("keeps a short bulleted list with prose around it as one blob", () => {
    // ai-bullets output: the bullets together ARE the response — splitting them
    // would make "Insert" paste a single bullet.
    const text = "Summary:\n\n- one point\n- another point\n\nHope this helps!";
    expect(splitAiOptions(text)).toEqual([{ label: "", text }]);
  });
});
