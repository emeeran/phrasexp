# DOMAIN.md — phrasexp

> **SDD Stage:** P0 — Domain Modeling (conceptual)
> **Inputs:** `raw_idea.txt`, PhraseExpress feature set (reference baseline)
> **Role:** Domain-Driven Design expert
> **Rule:** Strictly conceptual. No technology choices here — those are deferred to the architecture/ADR stage. Missing business logic is flagged ⚠️.

---

## 1. Problem Statement & Vision

**phrasexp** is a lightweight text-expander and automation utility modeled on PhraseExpress.
Its purpose is to eliminate repetitive typing: a user types a short **abbreviation** (or presses a
**hotkey**) and the app instantly **expands** it into a full phrase — plain text, formatted text,
or dynamically generated content produced by **macros**.

**Target users:** people already accustomed to PhraseExpress (currently Windows) who want a
complete, usable, lightweight alternative.

> ⚠️ *Business-logic gap — see §9.* The raw idea does not state a business model, platform
> scope, or whether cloud/team features are required. The domain model below captures the **full**
> conceptual space of a PhraseExpress-class product; which slices are in the first build is a
> requirements decision, not a domain decision.

---

## 2. Core Domain

> **Core domain = Text Expansion & Input Automation.**
> The single capability the product exists to deliver: *detect a user intent (trigger) and produce
> the right output, in the right place, at the right time.*

Everything else (organization, sync, teams, security) exists to make this core trustworthy and
scalable. If the core expansion pipeline is wrong, nothing else matters.

---

## 3. Subdomain Decomposition

Subdomains are classified **Core** (competitive differentiator), **Supporting** (necessary,
not differentiating), or **Generic** (solved problems, buy/standardize).

### 3.1 Core subdomain

| Subdomain | Responsibility |
|---|---|
| **Snippet Execution Pipeline** | The runtime heart. Listens for triggers, resolves the target phrase, expands content (running any macros), and delivers output (type into focused app / clipboard / specific program / file). Orchestrates all other subdomains at the moment of expansion. |

### 3.2 Supporting subdomains

| Subdomain | Responsibility |
|---|---|
| **Phrase Library Management** | Authoring, organizing, and persisting phrases and folders. Multi-level hierarchy, links, version history, recycle bin, bulk edit, duplicate detection, color tagging, sorting. The "editor" surface. |
| **Macro / Scripting Engine** | Produces dynamic content and runs automation logic: date/time, date math, input forms, conditional branching, loops, random values, external data lookups, program launching, keystroke simulation, string ops, math, message boxes. |
| **Trigger System** | Detects the user intent that fires an expansion: autotext abbreviation, hotkey, application-window appearance, clipboard-content change, time/date, or manual menu. Owns low-level input detection + matching. |
| **Autocomplete / Prediction** | Learns from usage to suggest single words and full sentences inline as the user types; can be trained from documents/email. Adaptive, frequency-based. |
| **Spelling Correction** | Auto-learns personal typos and corrects them via multi-language dictionary. Distinct from autotext (correction is unsolicited; autotext is user-defined). |
| **Clipboard Management** | Captures and retains clipboard history, strips formatting, infers context (email/URL/path), and feeds entries back as phrase content. |

### 3.3 Generic subdomains

| Subdomain | Responsibility | Notes |
|---|---|---|
| **Synchronization** | Keeps phrase libraries consistent across devices/cloud (Dropbox/GDrive/OneDrive/self-hosted) with multi-user change merging. | Conflict policy is the hard part. |
| **Collaboration / Team** | Shared phrase libraries via SQL Server / Citrix / central client-manager; personal folders per user. | Client-server shape. |
| **Identity & Access** | Users, roles, permissions; SSO (e.g. Azure Entra). Only meaningful when Collaboration is in scope. | |
| **Security** | AES encryption of phrase files, program digital signature, sensitive-data masking, UI restrictions. | Cross-cutting; shared kernel. |
| **Import / Export** | Converts external formats (DOC/DOCX/XLS/XLSX/CSV/TXT/RTF/PNG/JPG, Outlook) into and out of the library. | Anti-corruption layer. |
| **Telemetry / Usage Statistics** | Tracks expansion frequency to power usage-based sorting and team stats. | |
| **AI Integration** | Translation, shorten/expand/rewrite, embedded generated content, desktop AI surface for any app. | External LLM gateway. |
| **Host / Platform Integration** | OS-level concerns: global hotkeys, input injection, window detection, per-application context, portable mode. | Port behind an interface. |

---

## 4. Ubiquitous Language

A shared, unambiguous vocabulary. Every term below has one meaning across all contexts.

| Term | Definition |
|---|---|
| **Phrase** | A reusable unit of content that can be expanded. The central entity. |
| **Snippet** | Informal synonym for a small Phrase; treated as a Phrase in the model. |
| **Expansion** | The act (and result) of producing a Phrase's output at the cursor / target. |
| **Autotext** | A trigger method: typing an **abbreviation** that the app replaces with a Phrase. |
| **Abbreviation** | The short textual token that fires an autotext expansion (e.g. `br,` → "Best regards,"). |
| **Trigger** | Any condition that fires an expansion: autotext, hotkey, window, clipboard-change, time, or manual menu. |
| **Hotkey** | A keyboard-shortcut trigger. |
| **Folder** | A container organizing Phrases in a hierarchy (tree, no cycles). |
| **Phrase Link** | A reference from one Phrase to another, reusing content without duplication. |
| **Macro** | A dynamic, evaluated element embedded in a Phrase (date, input form, conditional, etc.). |
| **Input Form** | A macro that prompts the user for values (text/checkbox/radio/dropdown/grid/slider) at expansion time. |
| **Content Type** | The representation of a Phrase body: Plain, RichText, HTML, or Word. |
| **Output Target** | Where expansion goes: current program, specific program, clipboard, email, or file. |
| **Delivery** | The mechanism that injects expanded text (keystroke simulation, clipboard paste, API). |
| **Clipboard Entry** | One captured item in clipboard history. |
| **Autocomplete Suggestion** | A predictive inline offer of a word/sentence while typing. |
| **Correction** | An unsolicited fix of a typo (vs. user-defined Autotext). |
| **Sync Profile** | Configuration describing where and how libraries are synchronized. |
| **Team Space** | A shared library boundary with members and permissions. |
| **Version** | An immutable historical snapshot of a Phrase body (history/restore). |
| **Sensitive Phrase** | A Phrase flagged for masking, audit, or encryption (e.g. credentials). |
| **Application Context** | The currently focused app, used for per-app phrase sets and triggers. |

---

## 5. Aggregates

Aggregates are consistency boundaries. Each has one **Aggregate Root**, internal entities,
value objects, and invariants the root enforces.

### 5.1 Phrase (Aggregate Root) — *core*
- **Root:** `Phrase`
- **Entities:** `PhraseVersion` (immutable history), `PhraseLink` (reference)
- **Value Objects:** `ContentBody` (Plain/RichText/HTML/Word), `Abbreviation`, `TriggerBinding`, `BackgroundTag`, `CaseRule`, `RegexRule`, `OutputTarget`
- **Invariants:**
  - Exactly one canonical `ContentBody` per Phrase (active version).
  - An `Abbreviation` is unique within its resolution scope, *unless* multiple Phrases intentionally share one (then a selection menu is presented).
  - Past `PhraseVersion`s are immutable; only the active version is editable.
  - A Phrase belongs to exactly one Folder.
  - A Sensitive Phrase must never appear in telemetry or unmasked logs.

### 5.2 Folder / Library (Aggregate Root)
- **Root:** `Folder`
- **Entities:** nested `Folder`s, `Phrase` membership
- **Value Objects:** `FolderPath`, `LanguageTag`
- **Invariants:** folders form a tree (no cycles); deleted items move to a Recycle Bin, not hard-deleted immediately.

### 5.3 Macro / Script (Aggregate Root)
- **Root:** `Macro`
- **Entities:** nested `Macro` steps (loops, conditionals, linked macros)
- **Value Objects:** `MacroType` (Date/Time, DateMath, InputForm, Conditional, Loop, Random, ExternalData, ProgramLaunch, Keystroke, StringOp, Math, Notify), `Parameter`, `FormField`
- **Invariants:** parameters conform to the MacroType's schema; nesting depth is bounded (termination guarantee — no infinite recursion at expansion).

### 5.4 Clipboard Entry (Aggregate Root)
- **Root:** `ClipboardEntry`
- **Value Objects:** `Payload` (text/formatted/image), `CapturedAt`, `ContextHint`, `Format`
- **Invariants:** append-only; capacity bounded; entries can be masked/pinned/expired.

### 5.5 Trigger Binding (Value Object, modeled on Phrase)
- Most triggers live on a Phrase as a `TriggerBinding`. Global/unbound triggers (e.g. open the menu, toggle autocomplete) are registered in a **Trigger Registry**, which is a query model, not a transactional aggregate.

### 5.6 Sync Profile (Aggregate Root)
- **Root:** `SyncProfile`
- **Value Objects:** `Endpoint`, `ConflictPolicy`, `LastSyncedAt`
- **Invariants:** changes are queued while offline and merged under the `ConflictPolicy` on reconnect.

### 5.7 Team Space (Aggregate Root)
- **Root:** `TeamSpace`
- **Entities:** `Member`, `SharedLibrary`
- **Value Objects:** `Role`, `Permission`
- **Invariants:** a Member's effective permission is the union of assigned Roles; personal folders are write-isolated per Member.

### 5.8 Sensitive Secret (Aggregate Root)
- **Root:** `Secret` (encrypted-at-rest)
- **Value Objects:** `CipherPayload`, `AccessAuditEntry`
- **Invariants:** never transmitted in clear text; every read is audited.

---

## 6. Domain Events (key)

`TriggerFired` → `PhraseResolved` → `ExpansionRequested` → `MacrosEvaluated` → `Delivered`
→ `ExpansionCompleted`.

Also: `PhraseCreated/Edited/Deleted`, `VersionRestored`, `ClipboardCaptured`, `LibrarySynced`,
`SyncConflictDetected`, `MemberAdded`, `SensitiveAccessed`.

---

## 7. Domain Invariants & Rules (cross-cutting)

1. **Idempotent delivery:** an expansion must produce the same output for the same trigger+inputs; re-firing must not duplicate or corrupt.
2. **Determinism vs. liveness:** macros with random/input values are non-deterministic by design; all others must be deterministic.
3. **Never leak sensitive data:** Sensitive Phrases/Secrets are masked everywhere except the delivery target.
4. **Offline-first:** the core expansion pipeline works with no network; sync/team/AI are strictly additive.
5. **Per-application scoping:** triggers and active phrase sets may depend on the focused Application Context.

---

## 8. Scope Guidance (conceptual only — *not* a commitment)

Mapping the full domain to realistic build slices (decided downstream in Requirements):

- **MVP (core must-haves):** Phrase Library, Trigger System (autotext + hotkey), Execution Pipeline, basic Macro Engine (date/time/input form), Clipboard Management.
- **Near-term:** Spelling Correction, Autocomplete, Import/Export, Host/Platform Integration, Security (encryption).
- **Deferred / optional:** Synchronization, Collaboration/Team, Identity, AI Integration, Telemetry, advanced scripting (loops/conditionals/external data).

---

## 9. ⚠️ Missing Business Logic & Gaps

The raw idea is a clone-and-replicate brief ("replicate all features"); several business decisions
that shape the model are **not specified** and must be resolved in Requirements/Spec:

| # | Gap | Why it matters |
|---|---|---|
| ⚠️1 | **Platform scope** | "Windows text expander" vs PhraseExpress's own Windows/Mac/iOS reach. Determines whether Host Integration is one port or many. |
| ⚠️2 | **Business / licensing model** | Free vs freemium vs paid tiers gate which subdomains ship. PhraseExpress itself has editions. |
| ⚠️3 | **Cloud vs local-only** | Sync/Team/Identity/AI require a backend; a pure-local app drops ~4 generic subdomains. A major architectural fork. |
| ⚠️4 | **Data persistence & location** | Where do Phrases live (local DB/file, cloud)? Affects Security and Sync aggregates. |
| ⚠️5 | **Privacy / compliance** | Expanders hold sensitive text (credentials, medical). GDPR/HIPAA posture unspecified. |
| ⚠️6 | **Sync conflict policy** | Last-write-wins? merge? — unspecified, yet it defines the SyncProfile invariant. |
| ⚠️7 | **Macro scripting power** | Full programming-language macros vs a constrained macro set. Affects safety (termination), sandboxing, and the Macro aggregate boundary. |
| ⚠️8 | **Localization (i18n)** | Multi-language UI/dictionaries required? PhraseExpress ships many languages. |
| ⚠️9 | **Identity model** | Local user only, or accounts for sync/team? |
| ⚠️10 | **AI dependency** | Is AI a first-class feature or optional? It implies external API keys/costs. |

---

## 10. Out of Scope for THIS Document

- **"Tech: provide three best options" (from `raw_idea.txt`):** this is a *technology / architecture*
  decision, not domain logic. It is intentionally excluded here per the conceptual rule and is
  deferred to the **ADR (architecture)** stage, where candidates (e.g. native vs cross-platform
  frameworks) can be evaluated against the chosen scope from §9.
- Concrete UI/UX layout, data schemas, and APIs — these belong to Requirements/Spec.
