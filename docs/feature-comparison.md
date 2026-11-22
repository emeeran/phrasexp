# Feature Comparison: PhraseXp vs PhraseExpress Pro vs QuillBot

Positioning first: PhraseXp and PhraseExpress are text expanders (they type for you); QuillBot is a web writing assistant (it rewrites text you paste/select). Overlap is only in the AI/paraphrase and spellcheck corners.

## Feature matrix

| Feature | PhraseXp 1.4.1 | PhraseExpress Pro | QuillBot (Premium) |
|---|---|---|---|
| **Abbreviation autotext** (type `br,` → expands) | ✅ silent, case-adaptive, after-delimiter mode, regex triggers | ✅ its core feature, very mature | ❌ |
| **Triggering** | autotext, global popup hotkey, per-phrase hotkeys, multi-match chooser, regex | autotext, menu popup, hotkeys, autocomplete-as-you-type | ❌ |
| **Case adaptation** (`BR,` → ALL CAPS) | ✅ global + per-phrase override | ✅ | ❌ |
| **App-scoped phrases** | ✅ (window-title substring) | ✅ (per-app, more granular) | ❌ |
| **Macros / dynamic content** | ✅ 11 kinds: datetime, datemath, input forms (text/select/checkbox), random, math, clipboard, nested phrases, env vars, pick, file, delay, cursor-position | ✅ far richer: forms, program launch, mouse automation, scripting language, file operations, email automation, calculations | ❌ |
| **AI macros / rewrite** | ✅ `ai` + `aiaction` macros, streaming results, replace/insert/retry/discard, bring-your-own key (Groq/DeepSeek/OpenAI/custom), ✨variations generator | ⚠️ limited (no native LLM integration; third-party glue needed) | ✅ its core: paraphraser with modes/tones, grammar fix, translator |
| **Grammar/spellcheck while typing** | ✅ built-in: 65K-word dict, SymSpell + Damerau-Levenshtein + Soundex, learns accepted corrections, offer-popup or silent, per-app exclusions | ⚠️ basic; relies on OS/Office spellcheck | ✅ strong (its other core feature) |
| **Clipboard history** | ✅ text-only, 100 entries, copy-back, save-as-phrase | ✅ multi-format, longer history, search | ❌ |
| **Rich text / formatted snippets** | ⚠️ Markdown → HTML paste (works in rich-paste apps) | ✅ full RTF/HTML/images formatting | ❌ |
| **Inline input forms** (fill-in blanks at trigger time) | ✅ via `input` macro + `{param}` prompts | ✅ very mature (forms with dropdowns, date pickers) | ❌ |
| **Caret repositioning after expansion** | ✅ `cursor` macro | ✅ | ❌ |
| **Nested phrases** (reuse inside phrases) | ✅ `nested` macro | ✅ | ❌ |
| **Version history / restore** | ✅ per-phrase snapshots | ✅ | ⚠️ web app revision history |
| **Sync / sharing** | ✅ file-based (Syncthing/WebDAV/git folder), last-writer-wins + tombstones | ✅ Pro tier: SQL/SCP server, cloud sync, team sharing — enterprise-grade | ✅ cloud-native (account required) |
| **Import** | ✅ CSV, JSON, AutoHotKey, aText, TextExpander, Espanso | ✅ imports from most expanders | ❌ |
| **Export / backup** | ✅ JSON | ✅ | ⚠️ limited |
| **Privacy / offline** | ✅ 100% local; network only for AI you enable; AES at rest + OS keychain | ✅ local, optional cloud | ❌ cloud-only — everything you write is sent to their servers |
| **Platform** | Linux (primary), Windows, macOS | Windows (primary, deep Office integration), macOS, iOS/Android | Web + browser extension + Word/Google Docs plugins |
| **License model** | Local-first, your build | Commercial, per-seat, subscription or perpetual | Freemium, subscription |
| **Team features** | ❌ (single-user, sync by shared folder) | ✅ shared phrase libraries, permissions | ⚠️ team plans, but no shared snippet library |

## Where each wins

**PhraseXp wins on:** privacy (nothing leaves the machine except AI you explicitly enable), cost, Linux/Wayland support, built-in typo correction that learns, regex triggers, import breadth, no account. It's the only one of the three with a real local spellchecker.

**PhraseExpress Pro wins on:** depth — macro scripting language, program/mouse automation, full rich-text + image snippets, enterprise team sharing via SQL server, two decades of edge-case polish, native Windows reliability. If you need forms-with-logic, email automation, or managed team libraries, it's ahead.

**QuillBot wins on:** writing quality — paraphrase modes (formal/fluent/academic/creative), grammar checking at the sentence level, summarizer, plagiarism + AI detection, citations. It does nothing at the keyboard level; it's not an expander at all.

**Not covered by any competitor that PhraseXp has:** delimiter-gated expansion of plain words (`sig` only fires on `sig␣`, not inside "signature"), portable USB mode, and pick-random "variations" as a first-class macro.

## Gaps vs PhraseExpress (roadmap candidates)

- **Script/executable macros** (run a command, insert its output) — PhraseXp has no equivalent yet.
- **Image snippets** — PhraseExpress supports rich image content; PhraseXp is text-only.
