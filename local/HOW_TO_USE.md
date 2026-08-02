# PhraseXp — How to Use Guide

A lightweight, local-first text expander. Type an abbreviation and it expands into a full phrase;
press a hotkey to search all phrases; or use macros, Markdown, AI, and spellcheck to automate your
typing. Everything runs locally on your machine — no cloud, no account.

---

## 1. Installation

### From the .deb (Ubuntu/Debian)
```bash
sudo apt install ./PhraseXp_0.1.0_amd64.deb
```
Then launch **PhraseXp** from your application menu, or run `phraseexp` in a terminal.

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

---

## 2. The Library View

The main window has three panels:

| Panel | What it does |
|---|---|
| **Left** (sidebar) | Folder list + clipboard history |
| **Center** | Quick-add bar + search + phrase list |
| **Right** | Test-expander (or the phrase editor when editing) |

### Header bar
- `N phrases · N folders` — library stats
- `autotext live · N evts` — listener health (N = keystrokes captured; if 0 or "blocked", autotext isn't receiving keys)
- **⭳ Export** — download the entire library as JSON (backup)
- **⭱ Import** — bulk-import phrases from a CSV file (`abbreviation,text` per line)
- **⧉ Dupes** — find and display duplicate phrases
- **⚙ Settings** — configure the app

---

## 3. Creating Phrases

### Quick Add (fastest)
At the top of the center panel, the Quick Add bar:

```
[/emg]  →  [emeeranjp@gmail.com]  [Add]
```

- **Left box**: the trigger (what you'll type). Use a distinctive prefix like `/` or `;` to avoid
  mid-word triggers — e.g. `/emg`, `;sig`, `//addr`.
- **Right box**: the text to expand to.
- Click **Add** (or press Enter in the right box).

The phrase appears in the list immediately and is ready to use. If you don't have any folders yet,
Quick Add auto-creates a "Snippets" folder.

### Full Editor (advanced)
Click **+ New phrase** (or click any existing phrase to edit it). The editor opens in the right panel:

- **Name** — a label for the phrase (shown in the list).
- **Content type** — `Plain text` (typed) or `Markdown` (pasted as formatted HTML).
- **Autotext abbreviation** — the trigger word (e.g. `br,` or `/sig`).
- **Regex** checkbox — treat the abbreviation as a regular expression.
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

> **Tip**: use a prefix (`/`, `;`, or `:`) so the trigger only fires when you mean it. Without a
> prefix, `sig` would also fire inside "signature".

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

Macros are inserted into a phrase body alongside literal text. Click the macro buttons in the editor:

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

### AI (Phase 7)
- **+ ai** — sends a prompt to your configured AI endpoint and inserts the response. Example prompt:
  `Translate to French: Hello world`. Requires AI to be enabled in Settings (see §8).

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

## 7. Spellcheck (Phase 5)

PhraseXp auto-corrects typos as you type. It builds a **dictionary from your phrase bodies** (all
words ≥ 3 characters) and corrects misspelled words using Levenshtein edit distance (≤ 2).

How it works: type a word + space. If the word isn't in the dictionary but a close match exists, it's
auto-corrected.

Example: your phrases contain "hello" → typing "helo " + space → corrected to "hello ".

> The more phrases you have, the better the dictionary. The dictionary rebuilds automatically when
> you create or edit phrases.

---

## 8. Settings (⚙)

Click the **⚙** button in the header to open the Settings modal:

### General
- **Silent autotext** — enable/disable abbreviation expansion as you type.
- **Capture clipboard history** — auto-record text copies.
- **Launch on system login** — start PhraseXp automatically.

### Popup Hotkey
- Change the global hotkey that summons the search popup. Format: `Control+Alt+Space`,
  `Shift+Alt+P`, `Meta+Space`, etc. Keys: a–z, 0–9, Space, F1–F12.

### AI Integration
- **Enable AI macros** — turn on the AI gateway.
- **API base URL** — your OpenAI-compatible endpoint (default: `https://api.openai.com/v1`).
  Works with OpenAI, local LLMs (Ollama, LM Studio), Azure OpenAI, etc.
- **API key** — your secret key (stored locally in the phrase DB).
- **Model** — e.g. `gpt-4o-mini`, `gpt-3.5-turbo`, `llama3` (for local).

Click **Save settings** to apply.

---

## 9. Clipboard History

The **left sidebar** shows your clipboard history (most recent first). PhraseXp auto-captures text
copies (up to 100 entries). You can also manually add entries via the "Capture text…" input.

---

## 10. Import / Export

### Export (backup)
Click **⭳ Export** → downloads `phrasexp-library.json` containing all folders + phrases. Use this
for backups or migration to another machine.

### Import (bulk add)
Click **⭱ Import** → select a `.csv` file. Format: one phrase per line, `abbreviation,text`.
The text field may contain commas (split on the first comma only). Existing abbreviations are
skipped. Imported phrases go into the selected folder (or a new "Imported" folder).

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

## 12. Duplicate Finder

Click **⧉ Dupes** to scan for phrases with identical first-text content. A modal groups them so you
can review and clean up.

---

## 13. Copy to Clipboard

Each phrase card has a **Copy** button — expands the phrase and copies the result to the clipboard
(without typing it). Useful for apps that block synthetic input.

---

## 14. System Tray & Background

- Closing the main window **hides it to the system tray** (the app keeps running).
- Click the **tray icon** (or "Show phrasexp" in the tray menu) to bring the window back.
- **Quit** in the tray menu exits the app.

---

## 15. Portable Mode

To run PhraseXp from a USB stick (database next to the executable):

1. Install or copy the `phraseexp` binary to a folder.
2. Create an empty marker file next to it:
   ```bash
   touch /path/to/folder/phrasexp.portable
   ```
3. Run `phraseexp` from that folder. The database (`phrasexp.db`) is created there instead of the
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

### App crashes on launch
- If you see "no such column: app_scope", your database is from an older version. Run:
  ```bash
  sqlite3 ~/.local/share/com.phrasexp.app/phrasexp.db "ALTER TABLE phrases ADD COLUMN app_scope TEXT;"
  ```

### AI macros return an error
- Ensure AI is **enabled** in Settings with a valid **API key** and **base URL**. Check that your
  endpoint is reachable. AI macros require network access (unlike everything else in PhraseXp).

---

## 17. Privacy & Security

- **All data is local.** No network calls except AI macros (which you explicitly enable).
- The silent-autotext listener is a global key handler by construction. It matches **only registered
  abbreviations** and **never logs, stores, or transmits** raw keystrokes.
- The AI API key is stored in the local phrase database (not yet encrypted — see the encryption
  roadmap in `docs/04-design/ROADMAP.md`).
- Clipboard capture stores copied text locally (up to 100 entries).

---

## Quick Reference

| Action | How |
|---|---|
| Add a quick snippet | Quick Add bar: `/trigger` → `expansion` → Add |
| Add a full phrase | + New phrase → editor |
| Trigger by typing | Type the abbreviation in any app |
| Trigger by hotkey | Ctrl+Alt+Space → popup → search → Enter |
| Copy (don't type) | Copy button on the phrase card |
| Restore old version | History → Restore |
| Find duplicates | ⧉ Dupes |
| Backup library | ⭳ Export → JSON |
| Bulk import | ⭱ Import → CSV (`abbr,text`) |
| Change hotkey | ⚙ Settings → Popup hotkey |
| Enable AI | ⚙ Settings → Enable AI → key + model |
| Run from USB | `touch phrasexp.portable` next to the binary |
| Quit | Tray icon → Quit |
