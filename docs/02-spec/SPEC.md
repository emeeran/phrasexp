# SPEC.md — phrasexp (v1 / MVP)

> **SDD Stage:** P2 — Specification
> **Role:** Senior Architect (FastAPI / Python)
> **Input:** `docs/01-requirements/REQUIREMENTS.md`
> **Rule:** No placeholders. Every contract references its FR/NFR IDs.

Implements the **v1 MVP slice** (local-first, single-user, offline). Storage = SQLAlchemy 2.x +
SQLite (FR-11). All schemas are Pydantic v2 (NFR-1).

---

## 1. Technology & Layout

| Concern | Choice | FR/NFR |
|---|---|---|
| Web framework | FastAPI | NFR-2 |
| Validation | Pydantic v2 (discriminated unions) | NFR-1 |
| Persistence | SQLAlchemy 2.x (sync) + SQLite | FR-11, NFR-6 |
| Dependency mgmt | `uv` (add `sqlalchemy`) | p4_code |
| Server | uvicorn | NFR-2 |
| Docs | OpenAPI auto at `/docs` | NFR-2 |

Single module: **`backend/app.py`** (domain models, ORM, repository, routes). DB path via
`PHRASEXP_DB` env var, default `phrasexp.db`. Tables created on startup (`create_all`).

**Constants:** `CLIPBOARD_CAPACITY = 100` (FR-9 — oldest entry evicted by `captured_at` on overflow;
per P3 review O1).

---

## 2. Enums

```python
class ContentType(str, Enum):
    PLAIN = "plain"        # v1: only PLAIN is supported at delivery/creation
    RICHTEXT = "richtext"  # reserved — deferred (§9 of REQUIREMENTS.md)
    HTML = "html"          # reserved — deferred
    WORD = "word"          # reserved — deferred

class TriggerType(str, Enum):
    AUTOTEXT = "autotext"
    HOTKEY = "hotkey"

class MacroType(str, Enum):
    DATETIME = "datetime"
    DATEMATH = "datemath"
    INPUTFORM = "inputform"
```

**Content-type rule (v1):** creation/update with any type other than `PLAIN` is rejected with
`422` code `reserved_content_type`. The reserved values exist for forward-compatibility only. (FR-1)

---

## 3. Pydantic Schemas (domain + transport)

### 3.1 Macros — discriminated union on `type` (FR-5)

```python
class DateTimeMacro(BaseModel):
    type: Literal["datetime"] = "datetime"
    format: str = "%Y-%m-%d %H:%M"

class DateMathMacro(BaseModel):
    type: Literal["datemath"] = "datemath"
    days_offset: int = 0
    format: str = "%Y-%m-%d"

class InputFormMacro(BaseModel):
    type: Literal["inputform"] = "inputform"
    field: str

Macro = Annotated[Union[DateTimeMacro, DateMathMacro, InputFormMacro], Field(discriminator="type")]
```

### 3.2 Segments — discriminated union on `type` (FR-10)

```python
class TextSegment(BaseModel):
    type: Literal["text"] = "text"
    text: str

class MacroSegment(BaseModel):
    type: Literal["macro"] = "macro"
    macro: Macro

Segment = Annotated[Union[TextSegment, MacroSegment], Field(discriminator="type")]
```

### 3.3 Trigger binding (FR-3)

```python
class TriggerBinding(BaseModel):
    type: TriggerType
    abbreviation: str | None = None
    key: str | None = None
    # validator: autotext ⇒ abbreviation required; hotkey ⇒ key required
```

### 3.4 Aggregates

```python
class Folder(BaseModel):
    id: UUID
    name: str               # 1..120
    parent_id: UUID | None
    created_at: datetime

class Phrase(BaseModel):
    id: UUID
    folder_id: UUID
    name: str               # 1..120
    abbreviation: str | None  # 1..64
    content_type: ContentType = ContentType.PLAIN
    body: list[Segment]     # ≥ 1 (FR-10)
    triggers: list[TriggerBinding] = []
    sensitive: bool = False
    created_at: datetime
    updated_at: datetime
    deleted_at: datetime | None
```

### 3.5 Request DTOs

```python
class FolderCreate(BaseModel):
    name: str          # 1..120
    parent_id: UUID | None = None

class PhraseCreate(BaseModel):
    folder_id: UUID
    name: str
    abbreviation: str | None = None
    content_type: ContentType = ContentType.PLAIN
    body: list[Segment]
    triggers: list[TriggerBinding] = []
    sensitive: bool = False
    # validators: body non-empty; content_type == PLAIN;
    #             ≤1 autotext trigger; autotext abbreviation == phrase abbreviation

class PhraseUpdate(BaseModel):  # all optional
    name: str | None = None
    abbreviation: str | None = None
    content_type: ContentType | None = None
    body: list[Segment] | None = None
    triggers: list[TriggerBinding] | None = None
    sensitive: bool | None = None

class TriggerRequest(BaseModel):
    type: TriggerType
    abbreviation: str | None = None
    key: str | None = None

class ExpandRequest(BaseModel):              # FR-4, FR-12
    trigger: TriggerRequest
    inputs: dict[str, str] = {}

class ClipboardCreate(BaseModel):            # FR-9
    text: str   # min_length 1
```

### 3.6 Response DTOs

```python
class ExpandResponse(BaseModel):             # FR-4
    phrase_id: UUID
    abbreviation: str | None
    content_type: ContentType
    output: str

class PhraseSummary(BaseModel):              # FR-8 (masked)
    id: UUID
    folder_id: UUID
    name: str
    abbreviation: str | None
    content_type: ContentType
    sensitive: bool
    content_preview: str    # "***" when sensitive (FR-8)
    deleted: bool
```

---

## 4. SQLAlchemy ORM Models (FR-11, NFR-6)

```python
class Base(DeclarativeBase): ...

class FolderModel(Base):
    __tablename__ = "folders"
    id:        Mapped[str]         = mapped_column(String, primary_key=True)   # UUID hex
    name:      Mapped[str]         = mapped_column(String(120), nullable=False)
    parent_id: Mapped[str | None]  = mapped_column(ForeignKey("folders.id"), nullable=True)
    created_at:Mapped[datetime]    = mapped_column(default=utcnow)

class PhraseModel(Base):
    __tablename__ = "phrases"
    id:           Mapped[str]        = mapped_column(String, primary_key=True)
    folder_id:    Mapped[str]        = mapped_column(ForeignKey("folders.id"), nullable=False)
    name:         Mapped[str]        = mapped_column(String(120), nullable=False)
    abbreviation: Mapped[str | None] = mapped_column(String(64), nullable=True)
    content_type: Mapped[str]        = mapped_column(String(16), nullable=False, default="plain")
    body_json:    Mapped[list]       = mapped_column(JSON, nullable=False)          # segments
    triggers_json:Mapped[list]       = mapped_column(JSON, nullable=False, default=list)
    sensitive:    Mapped[bool]       = mapped_column(default=False)
    created_at:   Mapped[datetime]   = mapped_column(default=utcnow)
    updated_at:   Mapped[datetime]   = mapped_column(default=utcnow)
    deleted_at:   Mapped[datetime | None] = mapped_column(nullable=True)
    # Uniqueness of abbreviation among non-deleted rows enforced in the Repository (FR-6),
    # because SQLite partial unique indexes are awkward across the deleted_at boundary.

class ClipboardEntryModel(Base):
    __tablename__ = "clipboard_entries"
    id:         Mapped[str]      = mapped_column(String, primary_key=True)
    text:       Mapped[str]      = mapped_column(Text, nullable=False)
    captured_at:Mapped[datetime] = mapped_column(default=utcnow)
```

`body_json`/`triggers_json` are stored as JSON and re-validated into Pydantic models on read.

---

## 5. Repository Port (NFR-6)

```python
class Repository(Protocol):
    def create_folder(self, folder: Folder) -> Folder: ...
    def get_folder(self, folder_id: UUID) -> Folder | None: ...
    def list_folders(self) -> list[Folder]: ...
    def delete_folder(self, folder_id: UUID) -> bool: ...

    def create_phrase(self, phrase: Phrase) -> Phrase: ...
    def get_phrase(self, phrase_id: UUID) -> Phrase | None: ...
    def list_phrases(self, folder_id=None, q=None, include_deleted=False) -> list[Phrase]: ...
    def update_phrase(self, phrase: Phrase) -> Phrase: ...
    def soft_delete_phrase(self, phrase_id: UUID) -> Phrase | None: ...
    def resolve(self, trigger: TriggerRequest) -> Phrase | None: ...

    def add_clipboard(self, entry: ClipboardEntry) -> ClipboardEntry: ...
    def list_clipboard(self, limit: int) -> list[ClipboardEntry]: ...
```

v1 implementation `SqlAlchemyRepository` wraps a session. Abbreviation uniqueness (FR-6), folder
existence, and the acyclic-parent check (FR-2) are enforced here and raise domain errors.

---

## 6. API Contracts (OpenAPI-style)

Base path `/api/v1`. All error bodies: `{"code": <str>, "message": <str>}` (NFR-5).

| # | Method | Path | Body | Success | Errors (code→status) | FR |
|---|---|---|---|---|---|---|
| 1 | GET | `/health` | — | `200 {"status":"ok"}` | — | — |
| 2 | POST | `/folders` | `FolderCreate` | `201 Folder` | `not_found→404` (bad parent), `cycle→400` | FR-2 |
| 3 | GET | `/folders` | — | `200 [Folder]` | — | FR-2 |
| 4 | GET | `/folders/{id}` | — | `200 Folder` | `not_found→404` | FR-2 |
| 5 | DELETE | `/folders/{id}` | — | `204` | `not_found→404`, `folder_not_empty→409` (has active phrases; per P3 review O3) | FR-2 |
| 6 | POST | `/phrases` | `PhraseCreate` | `201 Phrase` | `not_found→404` (folder), `conflict→409` (abbr), `reserved_content_type→422`, `422` (validation) | FR-1,6,10 |
| 7 | GET | `/phrases?folder_id&q&include_deleted` | — | `200 [PhraseSummary]` | — | FR-7,8 |
| 8 | GET | `/phrases/{id}` | — | `200 Phrase` | `not_found→404` (incl. deleted) | FR-1 |
| 9 | PUT | `/phrases/{id}` | `PhraseUpdate` | `200 Phrase` | `not_found→404`, `conflict→409`, `reserved_content_type→422`, `422` | FR-1,6 |
| 10 | DELETE | `/phrases/{id}` | — | `204` | `not_found→404` | FR-7 |
| 11 | POST | `/expand` | `ExpandRequest` | `200 ExpandResponse` | `not_found→404`, `missing_input→400`, `422` | FR-4,5,12 |
| 12 | POST | `/clipboard` | `ClipboardCreate` | `201 ClipboardEntry` | `422` | FR-9 |
| 13 | GET | `/clipboard?limit=` | — | `200 [ClipboardEntry]` | — | FR-9 |

### 6.1 Determinism note (NFR-3)
`/expand` resolves `now` server-side. Tests inject a fixed `now` via the `expand_phrase` helper
directly (pure function) so output is reproducible; the HTTP path documents this contract.

---

## 7. Domain Logic (pure, testable)

```python
def evaluate_macro(macro: Macro, inputs: dict[str, str], now: datetime) -> str:
    # datetime  -> now.strftime(format)
    # datemath  -> (now + timedelta(days=days_offset)).strftime(format)
    # inputform -> inputs[field]  (raise MissingInputError if absent)

def expand_phrase(phrase: Phrase, inputs: dict[str, str], now: datetime) -> str:
    # concatenate segment outputs in order (FR-10)
```

Both are pure (NFR-3).

---

## 8. Error Catalog (NFR-5)

| `code` | HTTP | Raised when |
|---|---|---|
| `not_found` | 404 | Folder/phrase/trigger target missing, or phrase is deleted |
| `conflict` | 409 | Abbreviation already used by another active phrase (FR-6) |
| `folder_not_empty` | 409 | Folder still contains non-deleted phrases (P3 review O3) |
| `missing_input` | 400 | An `inputform` field was not supplied (FR-5) |
| `cycle` | 400 | Folder `parent_id` would create a cycle (FR-2) |
| `reserved_content_type` | 422 | Non-`PLAIN` content type in v1 (FR-1) |
| *(FastAPI default)* | 422 | Schema/validation failure (NFR-4) |

---

## 9. Security & Operational (NFR-9, NFR-10)

- Sensitive phrases masked in every list/summary response; full body only via direct GET or `/expand` delivery (FR-8, NFR-9).
- No outbound network calls of any kind (NFR-10).
- DB file is a single local SQLite file; no credentials, no secrets in code (NFR-9).
- Logging must never include phrase bodies or input values.

---

## 10. Open Questions (non-blocking)
- **Q1:** Should abbreviation matching be case-sensitive? **Decision (v1): case-sensitive**, exact match (simplest, deterministic). Revisit if users expect `BR,`/`br,` parity.
- **Q2:** Hotkey normalization (e.g. `Ctrl+Shift+B` vs `ctrl+shift+b`)? **Decision (v1): exact string match**, caller-normalized. A future C15 host layer can canonicalize.
- **Q3:** Hotkey uniqueness (P3 review O2)? **Decision (v1): not enforced** — `resolve()` returns the first match in storage order. Deterministic and acceptable for single-user; a selection-menu feature (PhraseExpress parity) is deferred.
