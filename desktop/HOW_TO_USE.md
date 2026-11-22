# PhraseXp — How to Use Guide

A lightweight, local-first text expander. Type an abbreviation and it expands into a full phrase;
press a hotkey to search all phrases; or use macros, Markdown, AI, and spellcheck to automate your
typing. Everything runs locally on your machine — no cloud, no account.

---

## 1. Installation

### From the .deb (Ubuntu/Debian)
```bash
sudo apt install ./PhraseXp_1.4.1_amd64.deb
```
Then launch **PhraseXp** from your application menu, or run `phrasexp` in a terminal.

### Runtime dependencies (Linux)
The installer pulls these automatically: `libwebkit2gtk-4.1-0`, `libgtk-3-0`,
`libayatana-appindicator3-1`. For app-scoping (per-app phrases), also install:
```bash
sudo apt install xdotool
```

### First launch
On first launch, PhraseXp creates its database in
`~/.local/share/com.phrasexp.app/phrasexp.db` and starts the system tray icon.
The main window opens with the **Library** view.

### Platform notes
- **Linux** (primary): X11 and Wayland. Wayland needs `ydotool` + `ydotoold`
  running; app-scoping needs `xdotool`. Add yourself to the `input` group for
  the listener (`sudo usermod -aG input $USER`, then re-login).
- **Windows**: no extra packages. A second launch focuses the first (single
  instance). Admin/elevated windows don't receive synthetic input.
- **macOS**: grant **Accessibility** and **Input Monitoring** on first run
  (System Settings → Privacy & Security) — without them the listener runs but
  macOS silently drops every keystroke (the header dot shows **blocked**).
  Password fields (Secure Input) pause global listening temporarily. There is
  no keyboard grab on macOS: offer/chooser popups take app focus, and focus
  returns to your text field when the popup resolves.
- The keystroke decoder assumes a **US keyboard layout** on every platform.
- Encryption keys live in the OS keychain: Secret Service (Linux, e.g.
  gnome-keyring), Credential Manager (Windows), Keychain (macOS).

---

## 2. The App Shell

The main window is a **sidebar + page** layout:

| Part | What it does |
|---|---|
| **Left rail** (sidebar) | Navigate between pages: **Library**, **Clipboard**, **Macros**, **AI**, **Settings** |
| **Header** | Status dots (`autotext live/blocked/off`, `spell`), theme toggle (dark → light → sepia) |
| **Page area** | The selected page |
| **Status bar** (bottom) | Phrase/folder/clip counts + listener state |

The app remembers which page you were on and reopens there.

### Library page
Three panes:
- **Folder drawer** (left, collapsible) — filter phrases by folder; disabled (muted) folders
  are dimmed. Collapse it with the chevron in its header (or expand from the rail button) to
  give the list and detail pane the full width; the state is remembered.
- **Phrase list** (center) — search (fuzzy — subsequence matches work, e.g. `adr` finds
  "Address block"), sort (Name / Recently edited / Newest / **Most used**), and the phrase
  cards with a `×N` usage badge. Keyboard: **↑↓** move, **Enter** edit, **Delete** removes.
- **Detail pane** (right, resizable by dragging its left edge) — the phrase editor when
  editing, otherwise the test-expander. All three columns are drag-resizable.

### Clipboard page
A full-width clipboard history: list on the left, preview on the right, with **Copy back**
and **Save as phrase** actions.

### Macros page
A gallery of all 11 macro kinds with examples — reference for building phrase bodies.

### AI page
Provider presets (Groq / DeepSeek / OpenAI / Custom), API key, model list, and connection
test — formerly the Settings → AI tab.

### Settings page
Section navigation (Features / Spellcheck / Data / Folders / Shortcuts / About) — no more
modal. Edit, then **Save**.

---

## 3. Creating Phrases

### Quick Add (fastest)
At the top of the center panel, the Quick Add bar:

```
[>emg]  →  [emeeranjp@gmail.com]  [Add]
```

- **Left box**: the trigger (what you'll type). Use a distinctive prefix like `>` or `;` to avoid
  mid-word triggers — e.g. `>emg`, `;sig`, `>>addr`.
- **Right box**: the text to expand to.
- Click **Add** (or press Enter in the right box).

The phrase appears in the list immediately and is ready to use. If you don't have any folders yet,
Quick Add auto-creates a "Snippets" folder.

### Full Editor (advanced)
Click **+ New phrase** (or click any existing phrase to edit it). The editor opens in the detail pane:

- **Name** — a label for the phrase (shown in the list).
- **Content type** — `Plain text` (typed) or `Markdown` (pasted as formatted HTML).
- **Case adaptation** — per-phrase override: **Match typed** (follow the global setting),
  **Always** (adapt even when the global toggle is off), or **Never** (output the body
  exactly as written).
- **Autotext abbreviation** — the trigger word (e.g. `br,` or `/sig`).
- **Regex** checkbox — treat the abbreviation as a regular expression.
- **Expand** — when a literal abbreviation fires: **Immediately** (as soon as typed) or
  **After Space or punctuation** (see §4).
- **App scope** — restrict this phrase to a specific app (substring of the window title).
- **Hotkey** — a global keyboard shortcut that directly injects this phrase.
- **Body** — an ordered list of segments (text and macros). Add/remove/reorder with the buttons.

Click **Save** when done.

---

## 4. Triggering Phrases

### Silent Autotext (type an abbreviation)
Type the abbreviation in **any** application (browser, editor, email, terminal). PhraseXp detects it
and instantly replaces it with the expansion.

Example: type `br,` → it becomes `Best regards, Meeran`.

**Case adaptation**: the expansion adapts to how you typed the abbreviation:
- `br,` → `best regards` (lowercase)
- `Br,` → `Best regards` (Title case)
- `BR,` → `BEST REGARDS` (ALL CAPS)

### After-delimiter expansion (1.4)
Don't want `sig` to fire inside "signature"? Set the phrase's **Expand** option to
**After Space or punctuation**. The abbreviation is then parked when typed and only fires
when the next key is a delimiter: **Space, Enter, Tab, `,` `.` `;` `:` `!` `?**. Any other
key cancels it — so `signature` types through untouched, while `sig␣` expands.

How it lands: the delimiter is erased along with the abbreviation, the expansion is typed,
and the delimiter is re-typed after it — so `Best regards, ` continues seamlessly.

> **Tip**: the prefix trick (`/`, `;`, `:`) still works for immediate triggers —
> after-delimiter mode is the alternative that keeps plain words usable.

### Hotkey → Popup (Ctrl+Alt+Space)
Press **Ctrl+Alt+Space** anywhere → a search popup appears near your cursor:
- Type to filter phrases.
- **↑/↓** to navigate.
- **Enter** to insert the selected phrase.
- **Esc** to dismiss.

### Per-phrase Hotkeys
In the phrase editor, set the **Hotkey** field (e.g. `ctrl+shift+b`). That shortcut then directly
injects the phrase — no popup needed. The hotkey works system-wide, in any app.

### Multi-match Selection Menu
If multiple phrases share the same abbreviation, typing it shows a **chooser popup** listing all
candidates. Pick one with ↑/↓ + Enter.

### Regex Triggers
Tick the **Regex** checkbox in the editor and write a regex pattern instead of a literal abbreviation.
The pattern triggers when it matches the **end** of what you typed. Examples:
- `:[a-z]+` → matches `:wave`, `:smile`, etc.
- `\d{4}` → matches any 4-digit number

---

## 5. Macros (Dynamic Content)

Macros are inserted into a phrase body alongside literal text. In the phrase editor, click
**+ Segment** and pick from the grouped menu (Basics / Date & numbers / Context / Interactive /
AI presets):

### Date & Time
- **+ datetime** — inserts the current date/time. Format: strftime/chrono (e.g. `%Y-%m-%d %H:%M`).
- **+ datemath** — date with an offset (e.g. `days_offset: 1` = tomorrow).

### Input Forms
- **+ input** — prompts for a value when the phrase is triggered (via the popup). Choose a type:
  - **text** — free-form text field.
  - **select** — dropdown (comma-separated options).
  - **checkbox** — true/false toggle.

  Example body: `[text "Hello "][input name][text "!"]` → popup asks for "name", you type "Sam", output is `Hello Sam!`.

### Random & Math
- **+ random** — a random integer in [min, max].
- **+ math** — evaluates an arithmetic expression (e.g. `100*1.2`, `round(10/3)`).

### Data & Reuse
- **+ clipboard** — inserts the current clipboard text.
- **+ nested** — inserts another phrase's expansion inline (linked/reused).
- **+ env** — inserts an environment variable (e.g. `$HOME`, `$USER`).
- **+ pick** — picks one option randomly from a comma-separated list (e.g. `yes, no, maybe`).
- **+ file** — inserts the contents of a file at a given path.
- **+ cite** — inserts a formatted citation (APA, MLA, or Chicago) for a DOI, URL, or raw
  title. Metadata is looked up on Crossref (free, no key) at expansion time. Leave the source
  blank to be asked for it whenever the phrase expands (the same prompt-once popup as input
  forms). Formatting is template code — deterministic and style-correct — not AI. Requires
  network access for the lookup (unlike everything else except AI).

### AI (Phase 7)
- **+ ai** — sends a prompt to your configured AI endpoint and inserts the response. Example prompt:
  `Translate to French: Hello world`. Requires AI to be enabled in Settings (see §8).
- **+ aiaction** — runs one of your **AI actions** (Settings → AI → Actions) against the clipboard
  text at expansion. Editing an action in Settings updates every phrase that references it. The
  built-ins (Translate, Make formal, Shorten, Fix grammar, Detect tone, …) are editable; custom
  actions can be created and deleted.
- **AI action templates** may reference `{clipboard}` / `{selection}` (the captured text) and any
  number of interactive `{token}`s — other placeholders (e.g. `{language}`, or `{length}` +
  `{format}` together) prompt once per token when the action runs. Known tokens get one-tap
  preset chips (paraphrase styles, target lengths, output formats), and your last answers are
  remembered per action, so re-running with the same choices is one Enter. Results stream in
  live; Replace / Insert / Copy / Retry / Discard work the same while streaming (Discard cancels
  the request).
- **Paraphrase** — select text → Control+Alt+A → **Paraphrase** picks a style chip (formal /
  fluent / academic / creative), or use the dedicated one-tap actions **Make formal**,
  **Improve fluency**, **Academic tone**, **Creative rewrite**.
- **Summarize** — prompts for a target length (`30 words`, `3 bullets`, `one paragraph`, …) and
  an output format (paragraphs or bullet points); long results scroll inside the result popup.
- **✨ Variations** — editor button: generates AI rewrites of the first text segment; applying
  them swaps the segment for a pick-random macro so every expansion sounds a little different.

### Timing & Caret
- **+ delay** — pauses the expansion for N milliseconds (0–5000) before continuing — for slow
  apps that need to catch up mid-template.
- **+ cursor** — after the expansion is delivered, the caret moves back to this position — put
  your fill-in blank *after* this segment (exact on plain text, approximate on Markdown).

---

## 6. Markdown Phrases

Set **Content type** to `Markdown` in the editor. Author your phrase body in Markdown:

```markdown
# Meeting Notes
- **Date**: today
- *Attendees*: team
- [ ] Action item
```

On expansion, PhraseXp renders the Markdown to **HTML** and **pastes it as formatted content**
(via clipboard HTML + Ctrl+V). This works in apps that accept rich paste (browsers, email, docs,
Slack, Notion). In plain-text-only apps, the plain-text fallback is used.

> **Note**: Markdown delivery overwrites your clipboard (it sets the HTML format). This is expected.

---

## 7. Spellcheck

PhraseXp auto-corrects typos as you type. The dictionary is a **65K-word bundled English list**
(two tiers: the 25K most common words are fuzzy-indexed; the other 40K are recognized so rare
but valid words are never flagged) **plus your phrase bodies** (all words ≥ 3 characters and
their common inflections). Corrections use **Damerau-Levenshtein** distance
(transposition-aware), looked up via a **SymSpell** symmetric-delete index for speed, with a
**Soundex** phonetic fallback for longer words. Candidates are ranked by real word frequency
(Google unigram counts), boosted when they fit the previous word (`a lto` → `a lot`), prefer
keyboard-adjacent typos, and are adapted to match the case you typed.

How it works: type a word + space. If the word isn't known but a close match exists, it's
auto-corrected. Example: typing `helo ` → corrected to `hello `.

**It learns your typos.** When you accept a correction (or pick one from the offer popup), the
typo → correction pair is remembered: the next time you make the same typo it's fixed instantly.
Undoing a correction (Left arrow) forgets the pair and never "fixes" that word again — you'll
see a `📝 Spellcheck will now ignore "…"` banner.

**What it leaves alone:** URLs, emails, file paths, and anything containing digits (`a16z`,
`ISO9001`, `x86_64`) are never corrected. Contractions are dictionary words (`dont` → `don't`),
and a possessive `'s` is kept (`recieve's` → `receive's`). Punctuation around a word is
preserved (`"recieve,` → `"receive,`).

**Undo a correction:** press **← Left arrow** immediately after a correction and PhraseXp restores
your original spelling (and adds that word to the ignore list so it won't be "fixed" again). It
only works on the first Left-arrow after a correction, before you move the cursor.

**Toggle spellcheck on/off:** press **Control+Shift+Space** (configurable in Settings → Features).
A banner confirms `✓ Spellcheck ON` / `✗ Spellcheck OFF`.

**Aggressiveness** (Settings → Spellcheck): *Conservative* (distance 1 only), *Normal* (distance 2
+ Soundex for long words), or *Aggressive* (distance 3 + Soundex for all). You can also list words
to never correct in the **Ignore words** field.

---

## 8. Settings

Open the **Settings** page from the sidebar. Sections run down the left; pick one, edit,
then **Save** (top right).

### Features section

PhraseXp runs as three independent modules plus app-wide general settings. Each module has a
master switch; the sub-settings below it dim when the module is off.

| Module | Master switch | Sub-settings | What "off" means |
|---|---|---|---|
| **Text Expander** | Enabled | Case adaptation; popup hotkey | Abbreviations stop expanding; spellcheck and clipboard keep working |
| **Spellchecker** | Enabled | Toggle hotkey (mode/delivery/exclusions live in the Spellcheck tab) | No corrections offered or applied; expansions keep working |
| **Clipboard Capture** | Enabled | Max entry length (chars), max entries | New copies aren't recorded; existing history is kept |

Modules are independent — any combination works, and with all three off the app does
essentially no background work (the keyboard listener stays alive but inert).

General settings (same page): **Launch on system login**, **Start minimized** (hidden to the
tray), and **Encrypt sensitive data at rest** (AES-encrypts sensitive phrases, the AI key, and
clipboard history in the DB; key kept in the OS keychain).

### Spellcheck section
- **Spellcheck** on/off, **Aggressiveness** (Conservative / Normal / Aggressive), **Delivery**
  (silent auto-correct vs offer popup), **Ignore words**, and **Exclude apps**. See §7.

### Data section
- **Top phrases** — your 5 most-expanded phrases with counts (usage stats).
- **Export library as JSON** (backup).
- **Import** — a wizard with a preview: pick a PhraseXp JSON / CSV / AutoHotKey / aText file,
  see what will be imported (CSV/JSON), choose a target folder, confirm. AHK/aText import
  directly (format auto-detected).
- **Reset database** — deletes all phrases/folders/clipboard/history.

### Folders section
- Create folders, mute (disable) them, delete them (must be empty), or **merge** them.
- **⇄ Merge** moves every snippet from one folder into another (a single transaction —
  soft-deleted snippets included) and deletes the now-empty source folder. The abbreviation
  index rebuilds immediately, so triggers keep working.
- Muting stops a folder's snippets from triggering without deleting them.

### Shortcuts section
- A reference table of every keyboard shortcut (reflects your configured popup + spellcheck hotkeys).

### About section
- Version, stack, and credits. Developed by **Dawnstar**.

> AI configuration lives on its own **AI page** (sidebar), not in Settings.

---

## 9. Clipboard History

Open the **Clipboard** page from the sidebar. PhraseXp auto-captures text copies (default 100
entries; configurable in Settings → Features, including a per-entry length cap). The left column
lists entries (most recent first); selecting one shows a full preview with **Copy back** (re-copy
to the system clipboard) and **Save as phrase** (open the editor with the text pre-filled).
You can also manually add entries via the "Capture text…" input at the top.

---

## 10. Import / Export

### Sync (multi-device, local-first)
Settings → Data → **Sync** points PhraseXp at a folder you own — a Syncthing/WebDAV mount, a git
repo, or a USB drive. Every change is written to `phrasexp-library.json` there, and on startup the
folder is pulled and merged back: per-phrase **last-writer-wins** on edit time, deletions propagate
as tombstones, and an overwritten phrase is recoverable via version history. Settings, counters, and
clipboard history never leave the machine. Note the snapshot file is plaintext — the same caveat as
the JSON export below.

### Export (backup)
Settings → Data → **Export library as JSON** → downloads `phrasexp-library.json` containing all
folders + phrases. Use this for backups or migration to another machine.

### Import (bulk add)
Settings → Data → **Import** opens a wizard: pick a format (PhraseXp JSON / CSV / AutoHotKey /
aText / TextExpander / Espanso), choose the file, review the preview (CSV and PhraseXp JSON show
a table of what will be imported), optionally pick a target folder, and confirm. CSV format: one
phrase per line, `abbreviation,text` (split on the first comma only; existing abbreviations are
skipped).

- **TextExpander**: use TextExpander's CSV export (`abbreviation,snippet,label` columns).
- **Espanso**: Espanso matches YAML — literal `trigger`/`replace` pairs are imported; forms and
  script/clipboard variables are skipped (nothing executable is ever imported).

Example CSV:
```
/emg,emeeranjp@gmail.com
/sig,Best regards, Meeran
/addr,123 Main St, Anytown
```

---

## 11. Version History & Restore

Every time you edit a phrase, PhraseXp saves a **snapshot** of the previous version. To view/restore:

1. Click **History** on any phrase card.
2. A modal lists prior versions (with a text preview + timestamp).
3. Click **Restore** to roll back (the current version is also saved as a snapshot, so restores are
   undoable).

---

## 12. Duplicate Finder & Bulk Cleanup

**Settings → Data → Find duplicates** scans for phrases with identical first-text content and
groups them in a modal. The first phrase in each group is tagged **keep**; from there you can:
- **Delete all but first** — one click removes every duplicate across all groups,
- **keep first, delete N** — clean up a single group,
- or delete individual phrases.

---

## 13. Copy to Clipboard

Each phrase card has a **Copy** button — expands the phrase and copies the result to the clipboard
(without typing it). Useful for apps that block synthetic input.

---

## 14. System Tray & Background

- Closing the main window **hides it to the system tray** (the app keeps running).
- Click the **tray icon** (or "Show PhraseXp" in the tray menu) to bring the window back.
- **Quit** in the tray menu exits the app.

---

## 15. Portable Mode

To run PhraseXp from a USB stick (database next to the executable):

1. Install or copy the `phrasexp` binary to a folder.
2. Create an empty marker file next to it:
   ```bash
   touch /path/to/folder/phrasexp.portable
   ```
3. Run `phrasexp` from that folder. The database (`phrasexp.db`) is created there instead of the
   OS app-data directory.

---

## 16. Troubleshooting

### Autotext doesn't fire
- Check the header: `autotext live · N evts`. If **N stays at 0** or says **blocked**:
  - On **Wayland** (Linux): silent autotext is unsupported. Log into an **Xorg/X11** session.
  - On **macOS**: grant **Accessibility** permission (System Settings → Privacy & Security).
  - On **X11**: rdev uses XRecord (no extra permissions needed). If it still doesn't work, some
    compositors may block XRecord.

### Hotkey popup types into the popup, not my app
- The popup now hides before injecting (120ms settle delay). If you still see this, the target app
  may grab focus aggressively. Try the **Copy** button instead, then paste manually (Ctrl+V).

### Per-app phrases don't work
- Install `xdotool`: `sudo apt install xdotool`. Without it, app detection is unavailable and
  app-scoped phrases fire everywhere.

### App crashes on launch / "no such column" errors
- PhraseXp runs automatic schema migrations on startup (adding any new columns your older database
  is missing). If you still see a "no such column" error, your database predates a migration — run
  PhraseXp once from a terminal to see the migration log, or reset from Settings → Data → Reset
  database (this deletes all phrases/history, so export a backup first).

### AI macros return an error
- Ensure AI is **enabled** in Settings with a valid **API key** and **base URL**. Check that your
  endpoint is reachable. AI macros require network access (unlike everything else in PhraseXp).

---

## 17. Privacy & Security

- **All data is local.** No network calls except AI macros (which you explicitly enable).
- The silent-autotext listener is a global key handler by construction. It matches **only registered
  abbreviations** and **never logs, stores, or transmits** raw keystrokes.
- The AI API key is stored in the local phrase database, encrypted at rest when **Encrypt
  sensitive data at rest** is enabled (Settings → Features; key held in the OS keychain).
- Clipboard capture stores copied text locally (up to 100 entries).

---

## Quick Reference

| Action | How |
|---|---|
| Add a quick snippet | Library → Quick Add bar: `/trigger` → `expansion` → Add |
| Add a full phrase | Library → + New phrase → editor |
| Trigger by typing | Type the abbreviation in any app |
| Trigger by hotkey | Ctrl+Alt+Space → popup → search → Enter |
| Copy (don't type) | Copy button on the phrase card |
| Restore old version | History → Restore |
| Find duplicates | Settings → Data → Find duplicates |
| Merge folders | Settings → Folders → ⇄ → pick target |
| Most-used phrases | Library sort → Most used; top list in Settings → Data |
| Backup library | Settings → Data → Export library |
| Bulk import | Settings → Data → Import wizard (CSV/JSON/AHK/aText/TextExpander/Espanso) |
| Tag phrases | Phrase editor → Tags; find with `#tag` in search |
| AI-rewrite selected text | Select text anywhere → Control+Alt+A → pick action → **Replace / Insert / Retry / Discard** (needs AI enabled) |
| Repeat last expansion | Control+Alt+R (re-emits the last output into the focused app) |
| Disable expansion per app | Settings → Features → Never expand in |
| Re-copy a clip | Clipboard page → select → Copy back |
| Turn a clip into a phrase | Clipboard page → select → Save as phrase |
| Browse macros | Macros page (gallery + examples) |
| Change popup hotkey | Settings → Features → Popup hotkey |
| Expand after a delimiter | Phrase editor → Expand → After Space or punctuation |
| Per-phrase case override | Phrase editor → Case adaptation |
| Collapse folder panel | Library → chevron in the folder header |
| Toggle spellcheck | Control+Shift+Space (configurable in Features) |
| Undo a correction | ← Left arrow, right after the correction |
| Enable AI | AI page → provider + key + model |
| Run from USB | `touch phrasexp.portable` next to the binary |
| Quit | Tray icon → Quit |
