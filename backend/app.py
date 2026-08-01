"""phrasexp — text-expansion backend (FastAPI + Pydantic v2 + SQLAlchemy/SQLite).

Implements the approved v1 MVP spec (docs/02-spec/SPEC.md):
    Phrase Library (C2) + Trigger System: autotext & hotkey (C4)
    + Execution Pipeline (C1) + constrained Macro Engine: datetime/datemath/inputform (C3)
    + Clipboard Management (C7) + local persistence (FR-11) + sensitive masking (C11).

Local-first / single-user / offline. Delivery returns expanded text over HTTP; OS-level keystroke
injection (C15), sync/team/AI/telemetry, and formatted-content rendering are deferred (see
docs/01-requirements/REQUIREMENTS.md §5).

FR/NFR traceability (prompts/p4_code.txt):
  FR-1  Phrase CRUD (one canonical body)                    — C2 §5.1
  FR-2  Folder CRUD, acyclic tree + folder_not_empty guard  — C2 §5.2 (P3 O3)
  FR-3  Trigger bindings: autotext (abbreviation) / hotkey  — C4 §5.5
  FR-4  Execution pipeline: resolve -> expand -> deliver    — C1 §6
  FR-5  Macro evaluation: datetime/datemath/inputform       — C3 §5.3
  FR-6  Abbreviation unique within active scope (409)       — C2/C1 §5.1
  FR-7  Soft-delete to recycle bin (excluded from resolve)  — C2 §5.2
  FR-8  Sensitive phrases masked in non-delivery views      — C11 §7.3
  FR-9  Clipboard capture + bounded history (CLIPBOARD_CAPACITY) — C7 §5.4 (P3 O1)
  FR-10 Phrase body = ordered list of text/macro segments   — C3/C2 §5.1
  FR-11 Local persistence (SQLite, survives restart)        — local-first
  FR-12 Expansion inputs supplied per request               — C3 §5.3
  NFR-1 Pydantic v2 discriminated unions (no untyped dicts)
  NFR-2 Standard FastAPI patterns, OpenAPI at /docs, DI storage
  NFR-3 Deterministic expansion for fixed (phrase, inputs, now)
  NFR-4 Boundary validation -> 422
  NFR-5 Structured errors {code, message} with correct status
  NFR-6 Storage behind a Repository port (DB-pluggable)
  NFR-9/NFR-10 No secrets in logs; no network egress.
"""

from __future__ import annotations

import os
from datetime import datetime, timedelta, timezone
from enum import Enum
from typing import Annotated, Any, Literal, Protocol, Union
from uuid import UUID, uuid4

from fastapi import Depends, FastAPI, Query, Request, status
from fastapi.responses import JSONResponse
from pydantic import BaseModel, ConfigDict, Field, TypeAdapter, model_validator
from sqlalchemy import JSON, DateTime, ForeignKey, String, Text, create_engine, select
from sqlalchemy.orm import (
    DeclarativeBase,
    Mapped,
    Session,
    mapped_column,
    sessionmaker,
)

API_PREFIX = "/api/v1"
CLIPBOARD_CAPACITY = 100  # FR-9 / P3 review O1
PREVIEW_LIMIT = 60
DEFAULT_DB = "phrasexp.db"


# --------------------------------------------------------------------------- #
# 1. Enums (DOMAIN.md §4 ubiquitous language)                                 #
# --------------------------------------------------------------------------- #
class ContentType(str, Enum):
    PLAIN = "plain"  # v1: only PLAIN supported at creation/delivery
    RICHTEXT = "richtext"  # reserved — deferred
    HTML = "html"  # reserved — deferred
    WORD = "word"  # reserved — deferred


class TriggerType(str, Enum):
    AUTOTEXT = "autotext"
    HOTKEY = "hotkey"


class MacroType(str, Enum):
    DATETIME = "datetime"
    DATEMATH = "datemath"
    INPUTFORM = "inputform"


# --------------------------------------------------------------------------- #
# 2. Pydantic domain models (discriminated unions — NFR-1)                    #
# --------------------------------------------------------------------------- #
class DateTimeMacro(BaseModel):
    type: Literal[MacroType.DATETIME.value] = MacroType.DATETIME.value
    format: str = "%Y-%m-%d %H:%M"


class DateMathMacro(BaseModel):
    type: Literal[MacroType.DATEMATH.value] = MacroType.DATEMATH.value
    days_offset: int = 0
    format: str = "%Y-%m-%d"


class InputFormMacro(BaseModel):
    type: Literal[MacroType.INPUTFORM.value] = MacroType.INPUTFORM.value
    field: str


Macro = Annotated[
    Union[DateTimeMacro, DateMathMacro, InputFormMacro],
    Field(discriminator="type"),
]


class TextSegment(BaseModel):
    type: Literal["text"] = "text"
    text: str


class MacroSegment(BaseModel):
    type: Literal["macro"] = "macro"
    macro: Macro


Segment = Annotated[Union[TextSegment, MacroSegment], Field(discriminator="type")]


class TriggerBinding(BaseModel):
    """FR-3. A trigger bound to a phrase. Exactly one payload applies per type."""

    type: TriggerType
    abbreviation: str | None = None
    key: str | None = None

    @model_validator(mode="after")
    def _check_payload(self) -> "TriggerBinding":
        if self.type is TriggerType.AUTOTEXT and not self.abbreviation:
            raise ValueError("autotext trigger requires 'abbreviation'")
        if self.type is TriggerType.HOTKEY and not self.key:
            raise ValueError("hotkey trigger requires 'key'")
        return self


class Folder(BaseModel):
    """FR-2. Container organizing phrases in an acyclic tree."""

    model_config = ConfigDict(from_attributes=True)

    id: UUID
    name: str
    parent_id: UUID | None = None
    created_at: datetime


class Phrase(BaseModel):
    """FR-1/FR-10. Aggregate root: one canonical body, ordered segments, triggers."""

    model_config = ConfigDict(from_attributes=True)

    id: UUID
    folder_id: UUID
    name: str
    abbreviation: str | None = None
    content_type: ContentType = ContentType.PLAIN
    body: list[Segment]
    triggers: list[TriggerBinding] = Field(default_factory=list)
    sensitive: bool = False
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    deleted_at: datetime | None = None

    @model_validator(mode="after")
    def _check_body(self) -> "Phrase":
        if not self.body:
            raise ValueError("phrase body must contain at least one segment")
        return self


class ClipboardEntry(BaseModel):
    """FR-9. One captured clipboard item."""

    model_config = ConfigDict(from_attributes=True)

    id: UUID
    text: str
    captured_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


# --------------------------------------------------------------------------- #
# 3. Request / response contracts                                             #
# --------------------------------------------------------------------------- #
class FolderCreate(BaseModel):
    name: str = Field(min_length=1, max_length=120)
    parent_id: UUID | None = None


class TriggerRequest(BaseModel):
    """FR-4 expansion request trigger."""

    type: TriggerType
    abbreviation: str | None = None
    key: str | None = None


class PhraseCreate(BaseModel):
    folder_id: UUID
    name: str = Field(min_length=1, max_length=120)
    abbreviation: str | None = Field(default=None, min_length=1, max_length=64)
    content_type: ContentType = ContentType.PLAIN
    body: list[Segment]
    triggers: list[TriggerBinding] = Field(default_factory=list)
    sensitive: bool = False

    @model_validator(mode="after")
    def _normalize(self) -> "PhraseCreate":
        autotext = [t for t in self.triggers if t.type is TriggerType.AUTOTEXT]
        if len(autotext) > 1:
            raise ValueError("at most one autotext trigger is allowed")
        if autotext:
            ta = autotext[0].abbreviation
            if self.abbreviation is not None and self.abbreviation != ta:
                raise ValueError(
                    "autotext trigger abbreviation must match phrase abbreviation"
                )
            self.abbreviation = ta  # canonical abbreviation lives on the phrase (FR-6)
        if not self.body:
            raise ValueError("phrase body must contain at least one segment")
        return self


class PhraseUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=120)
    abbreviation: str | None = Field(default=None, min_length=1, max_length=64)
    content_type: ContentType | None = None
    body: list[Segment] | None = None
    triggers: list[TriggerBinding] | None = None
    sensitive: bool | None = None


class ExpandRequest(BaseModel):
    """FR-4/FR-12."""

    trigger: TriggerRequest
    inputs: dict[str, str] = Field(default_factory=dict)


class ExpandResponse(BaseModel):
    phrase_id: UUID
    abbreviation: str | None
    content_type: ContentType
    output: str


class PhraseSummary(BaseModel):
    """FR-8 masked list view."""

    id: UUID
    folder_id: UUID
    name: str
    abbreviation: str | None
    content_type: ContentType
    sensitive: bool
    content_preview: str
    deleted: bool


class ClipboardCreate(BaseModel):
    text: str = Field(min_length=1)


# --------------------------------------------------------------------------- #
# 4. Domain exceptions (mapped to HTTP in §9)                                 #
# --------------------------------------------------------------------------- #
class DomainError(Exception):
    code: str = "domain_error"
    status_code: int = status.HTTP_400_BAD_REQUEST


class NotFoundError(DomainError):
    code = "not_found"
    status_code = status.HTTP_404_NOT_FOUND


class ConflictError(DomainError):
    code = "conflict"
    status_code = status.HTTP_409_CONFLICT


class FolderNotEmptyError(DomainError):  # P3 review O3
    code = "folder_not_empty"
    status_code = status.HTTP_409_CONFLICT


class MissingInputError(DomainError):
    code = "missing_input"
    status_code = status.HTTP_400_BAD_REQUEST


class CycleError(DomainError):
    code = "cycle"
    status_code = status.HTTP_400_BAD_REQUEST


class ReservedContentTypeError(DomainError):  # v1 non-PLAIN content type
    code = "reserved_content_type"
    status_code = status.HTTP_422_UNPROCESSABLE_CONTENT


# --------------------------------------------------------------------------- #
# 5. Pure macro evaluation (NFR-3 determinism)                                #
# --------------------------------------------------------------------------- #
def evaluate_macro(macro: Any, inputs: dict[str, str], now: datetime) -> str:
    """FR-5. Pure: identical (macro, inputs, now) -> identical output."""
    if isinstance(macro, DateTimeMacro):
        return now.strftime(macro.format)
    if isinstance(macro, DateMathMacro):
        return (now + timedelta(days=macro.days_offset)).strftime(macro.format)
    if isinstance(macro, InputFormMacro):
        if macro.field not in inputs:
            raise MissingInputError(f"input form field '{macro.field}' was not provided")
        return inputs[macro.field]
    raise ValueError(f"unsupported macro type: {macro!r}")


def expand_phrase(phrase: Phrase, inputs: dict[str, str], now: datetime) -> str:
    """FR-4/FR-10. Walk segments in order, concatenating literal text and macros."""
    parts: list[str] = []
    for segment in phrase.body:
        if isinstance(segment, TextSegment):
            parts.append(segment.text)
        else:  # MacroSegment
            parts.append(evaluate_macro(segment.macro, inputs, now))
    return "".join(parts)


def literal_preview(phrase: Phrase, limit: int = PREVIEW_LIMIT) -> str:
    """FR-8. Non-delivery preview: literal text only, truncated."""
    text = "".join(seg.text for seg in phrase.body if isinstance(seg, TextSegment))
    if len(text) > limit:
        return text[:limit].rstrip() + "…"
    return text


# --------------------------------------------------------------------------- #
# 6. SQLAlchemy ORM (FR-11, NFR-6)                                            #
# --------------------------------------------------------------------------- #
class Base(DeclarativeBase):
    pass


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


class FolderModel(Base):
    __tablename__ = "folders"

    id: Mapped[str] = mapped_column(String(36), primary_key=True)
    name: Mapped[str] = mapped_column(String(120), nullable=False)
    parent_id: Mapped[str | None] = mapped_column(
        ForeignKey("folders.id"), nullable=True
    )
    created_at: Mapped[datetime] = mapped_column(DateTime, default=_utcnow)


class PhraseModel(Base):
    __tablename__ = "phrases"

    id: Mapped[str] = mapped_column(String(36), primary_key=True)
    folder_id: Mapped[str] = mapped_column(
        ForeignKey("folders.id"), nullable=False
    )
    name: Mapped[str] = mapped_column(String(120), nullable=False)
    abbreviation: Mapped[str | None] = mapped_column(String(64), nullable=True)
    content_type: Mapped[str] = mapped_column(String(16), nullable=False, default="plain")
    body_json: Mapped[list] = mapped_column(JSON, nullable=False)
    triggers_json: Mapped[list] = mapped_column(JSON, nullable=False, default=list)
    sensitive: Mapped[bool] = mapped_column(default=False)
    created_at: Mapped[datetime] = mapped_column(DateTime, default=_utcnow)
    updated_at: Mapped[datetime] = mapped_column(DateTime, default=_utcnow)
    deleted_at: Mapped[datetime | None] = mapped_column(DateTime, nullable=True)


class ClipboardEntryModel(Base):
    __tablename__ = "clipboard_entries"

    id: Mapped[str] = mapped_column(String(36), primary_key=True)
    text: Mapped[str] = mapped_column(Text, nullable=False)
    captured_at: Mapped[datetime] = mapped_column(DateTime, default=_utcnow)


# Type adapters re-validate JSON columns back into Pydantic models on read (NFR-1).
_SEGMENT_LIST = TypeAdapter(list[Segment])
_TRIGGER_LIST = TypeAdapter(list[TriggerBinding])


def _phrase_from_model(row: PhraseModel) -> Phrase:
    return Phrase(
        id=UUID(row.id),
        folder_id=UUID(row.folder_id),
        name=row.name,
        abbreviation=row.abbreviation,
        content_type=ContentType(row.content_type),
        body=_SEGMENT_LIST.validate_python(row.body_json),
        triggers=_TRIGGER_LIST.validate_python(row.triggers_json),
        sensitive=row.sensitive,
        created_at=row.created_at,
        updated_at=row.updated_at,
        deleted_at=row.deleted_at,
    )


def _phrase_to_dict(phrase: Phrase) -> dict[str, Any]:
    return {
        "id": str(phrase.id),
        "folder_id": str(phrase.folder_id),
        "name": phrase.name,
        "abbreviation": phrase.abbreviation,
        "content_type": phrase.content_type.value,
        "body_json": [seg.model_dump(mode="json") for seg in phrase.body],
        "triggers_json": [t.model_dump(mode="json") for t in phrase.triggers],
        "sensitive": phrase.sensitive,
        "created_at": phrase.created_at,
        "updated_at": phrase.updated_at,
        "deleted_at": phrase.deleted_at,
    }


def _folder_from_model(row: FolderModel) -> Folder:
    return Folder(
        id=UUID(row.id),
        name=row.name,
        parent_id=UUID(row.parent_id) if row.parent_id else None,
        created_at=row.created_at,
    )


def _clipboard_from_model(row: ClipboardEntryModel) -> ClipboardEntry:
    return ClipboardEntry(
        id=UUID(row.id), text=row.text, captured_at=row.captured_at
    )


# --------------------------------------------------------------------------- #
# 7. Repository port + SQLAlchemy implementation (NFR-6)                      #
# --------------------------------------------------------------------------- #
class Repository(Protocol):
    def create_folder(self, folder: Folder) -> Folder: ...
    def get_folder(self, folder_id: UUID) -> Folder | None: ...
    def list_folders(self) -> list[Folder]: ...
    def delete_folder(self, folder_id: UUID) -> bool: ...

    def create_phrase(self, phrase: Phrase) -> Phrase: ...
    def get_phrase(self, phrase_id: UUID) -> Phrase | None: ...
    def list_phrases(
        self,
        folder_id: UUID | None = None,
        q: str | None = None,
        include_deleted: bool = False,
    ) -> list[Phrase]: ...
    def update_phrase(self, phrase: Phrase) -> Phrase: ...
    def soft_delete_phrase(self, phrase_id: UUID) -> Phrase | None: ...
    def resolve(self, trigger: TriggerRequest) -> Phrase | None: ...

    def add_clipboard(self, entry: ClipboardEntry) -> ClipboardEntry: ...
    def list_clipboard(self, limit: int) -> list[ClipboardEntry]: ...


class SqlAlchemyRepository:
    """v1 persistence. Uniqueness, existence, and acyclicity enforced here."""

    def __init__(self, sessionmaker_: sessionmaker[Session]) -> None:
        self._sessionmaker = sessionmaker_

    # -- folders ---------------------------------------------------------- #
    def create_folder(self, folder: Folder) -> Folder:
        with self._sessionmaker() as session:
            if folder.parent_id is not None:
                parent = session.get(FolderModel, str(folder.parent_id))
                if parent is None:
                    raise NotFoundError(f"parent folder {folder.parent_id} not found")
                self._guard_no_cycle(session, str(folder.id), str(folder.parent_id))
            session.add(
                FolderModel(
                    id=str(folder.id),
                    name=folder.name,
                    parent_id=str(folder.parent_id) if folder.parent_id else None,
                    created_at=folder.created_at,
                )
            )
            session.commit()
            return self.get_folder(folder.id) or folder  # type: ignore[return-value]

    def get_folder(self, folder_id: UUID) -> Folder | None:
        with self._sessionmaker() as session:
            row = session.get(FolderModel, str(folder_id))
            return _folder_from_model(row) if row else None

    def list_folders(self) -> list[Folder]:
        with self._sessionmaker() as session:
            rows = session.execute(select(FolderModel)).scalars().all()
            return [_folder_from_model(r) for r in rows]

    def delete_folder(self, folder_id: UUID) -> bool:
        with self._sessionmaker() as session:
            row = session.get(FolderModel, str(folder_id))
            if row is None:
                return False
            # P3 review O3: refuse if active (non-deleted) phrases reference it.
            active = session.execute(
                select(PhraseModel.id)
                .where(PhraseModel.folder_id == str(folder_id))
                .where(PhraseModel.deleted_at.is_(None))
                .limit(1)
            ).first()
            if active is not None:
                raise FolderNotEmptyError(
                    f"folder {folder_id} still contains active phrases"
                )
            session.delete(row)
            session.commit()
            return True

    def _guard_no_cycle(self, session: Session, folder_id: str, parent_id: str) -> None:
        """FR-2. Walk up the parent chain; reaching ourselves means a cycle."""
        cursor = parent_id
        seen: set[str] = set()
        while cursor is not None:
            if cursor == folder_id or cursor in seen:
                raise CycleError("folder parent_id would create a cycle")
            seen.add(cursor)
            parent = session.get(FolderModel, cursor)
            cursor = parent.parent_id if parent else None

    # -- phrases ---------------------------------------------------------- #
    def _assert_abbreviation_unique(self, phrase: Phrase) -> None:
        """FR-6. Abbreviation unique across non-deleted phrases."""
        if not phrase.abbreviation:
            return
        with self._sessionmaker() as session:
            clash = session.execute(
                select(PhraseModel.id)
                .where(PhraseModel.abbreviation == phrase.abbreviation)
                .where(PhraseModel.deleted_at.is_(None))
                .where(PhraseModel.id != str(phrase.id))
                .limit(1)
            ).first()
            if clash is not None:
                raise ConflictError(
                    f"abbreviation '{phrase.abbreviation}' already in use"
                )

    def create_phrase(self, phrase: Phrase) -> Phrase:
        with self._sessionmaker() as session:
            if session.get(FolderModel, str(phrase.folder_id)) is None:
                raise NotFoundError(f"folder {phrase.folder_id} not found")
        self._assert_abbreviation_unique(phrase)
        with self._sessionmaker() as session:
            session.add(PhraseModel(**_phrase_to_dict(phrase)))
            session.commit()
        return self.get_phrase(phrase.id) or phrase  # type: ignore[return-value]

    def get_phrase(self, phrase_id: UUID) -> Phrase | None:
        with self._sessionmaker() as session:
            row = session.get(PhraseModel, str(phrase_id))
            return _phrase_from_model(row) if row else None

    def list_phrases(
        self,
        folder_id: UUID | None = None,
        q: str | None = None,
        include_deleted: bool = False,
    ) -> list[Phrase]:
        stmt = select(PhraseModel)
        if not include_deleted:
            stmt = stmt.where(PhraseModel.deleted_at.is_(None))
        if folder_id is not None:
            stmt = stmt.where(PhraseModel.folder_id == str(folder_id))
        if q:
            needle = q.lower()
            stmt = stmt.where(
                PhraseModel.name.icontains(needle)
                | PhraseModel.abbreviation.icontains(needle)
            )
        with self._sessionmaker() as session:
            rows = session.execute(stmt).scalars().all()
            return [_phrase_from_model(r) for r in rows]

    def update_phrase(self, phrase: Phrase) -> Phrase:
        with self._sessionmaker() as session:
            if session.get(PhraseModel, str(phrase.id)) is None:
                raise NotFoundError(f"phrase {phrase.id} not found")
        self._assert_abbreviation_unique(phrase)
        with self._sessionmaker() as session:
            data = _phrase_to_dict(phrase)
            data["updated_at"] = _utcnow()
            session.merge(PhraseModel(**data))
            session.commit()
        return self.get_phrase(phrase.id) or phrase  # type: ignore[return-value]

    def soft_delete_phrase(self, phrase_id: UUID) -> Phrase | None:
        with self._sessionmaker() as session:
            row = session.get(PhraseModel, str(phrase_id))
            if row is None or row.deleted_at is not None:
                return None
            row.deleted_at = _utcnow()
            session.commit()
            return _phrase_from_model(row)

    def resolve(self, trigger: TriggerRequest) -> Phrase | None:
        """FR-4 resolution. Deleted phrases never resolve (FR-7).
        P3 review O2: first match in storage order wins (hotkey uniqueness not enforced in v1)."""
        with self._sessionmaker() as session:
            stmt = select(PhraseModel).where(PhraseModel.deleted_at.is_(None))
            rows = session.execute(stmt).scalars().all()
            for row in rows:
                phrase = _phrase_from_model(row)
                for binding in phrase.triggers:
                    if binding.type is not trigger.type:
                        continue
                    if (
                        trigger.type is TriggerType.AUTOTEXT
                        and binding.abbreviation == trigger.abbreviation
                    ):
                        return phrase
                    if trigger.type is TriggerType.HOTKEY and binding.key == trigger.key:
                        return phrase
            return None

    # -- clipboard -------------------------------------------------------- #
    def add_clipboard(self, entry: ClipboardEntry) -> ClipboardEntry:
        with self._sessionmaker() as session:
            session.add(
                ClipboardEntryModel(
                    id=str(entry.id),
                    text=entry.text,
                    captured_at=entry.captured_at,
                )
            )
            session.commit()
            # FR-9 / P3 O1: capacity-bounded — evict oldest on overflow.
            total = session.execute(select(ClipboardEntryModel.id).order_by(
                ClipboardEntryModel.captured_at.desc()
            )).scalars().all()
            if len(total) > CLIPBOARD_CAPACITY:
                for stale_id in total[CLIPBOARD_CAPACITY:]:
                    session.delete(session.get(ClipboardEntryModel, stale_id))
                session.commit()
        return entry

    def list_clipboard(self, limit: int) -> list[ClipboardEntry]:
        with self._sessionmaker() as session:
            rows = (
                session.execute(
                    select(ClipboardEntryModel)
                    .order_by(ClipboardEntryModel.captured_at.desc())
                    .limit(limit)
                )
                .scalars()
                .all()
            )
            return [_clipboard_from_model(r) for r in rows]


# --------------------------------------------------------------------------- #
# 8. Engine/session configuration + DI (NFR-2, NFR-6)                         #
# --------------------------------------------------------------------------- #
_engine: Any = None
_SessionLocal: sessionmaker[Session] | None = None
_repo: Repository | None = None


def _build_engine(db_url: str) -> Any:
    if db_url.startswith("sqlite"):
        return create_engine(db_url, connect_args={"check_same_thread": False})
    return create_engine(db_url)


def configure(db_url: str | None = None) -> None:
    """Create the engine, create tables, and wire the Repository.

    Called once on import with the PHRASEXP_DB env value; tests call it again
    against a fresh file to get hermetic, isolated state (and to exercise FR-11
    persistence by re-pointing at the same file).
    """
    global _engine, _SessionLocal, _repo
    url = db_url or f"sqlite:///{os.environ.get('PHRASEXP_DB', DEFAULT_DB)}"
    _engine = _build_engine(url)
    _SessionLocal = sessionmaker(bind=_engine, expire_on_commit=False)
    Base.metadata.create_all(_engine)
    _repo = SqlAlchemyRepository(_SessionLocal)


def get_repository() -> Repository:
    if _repo is None:
        configure()
    assert _repo is not None
    return _repo


RepoDep = Annotated[Repository, Depends(get_repository)]


# --------------------------------------------------------------------------- #
# 9. FastAPI app, error mapping, routes                                       #
# --------------------------------------------------------------------------- #
app = FastAPI(
    title="phrasexp",
    version="0.1.0",
    description="Text-expansion backend (v1 MVP): phrase library, triggers, execution, clipboard.",
)


@app.exception_handler(DomainError)
async def _domain_error_handler(request: Request, exc: DomainError) -> JSONResponse:
    # NFR-5: structured {code, message}. message never includes full phrase bodies (NFR-9).
    return JSONResponse(status_code=exc.status_code, content={"code": exc.code, "message": str(exc)})


def _to_summary(phrase: Phrase) -> PhraseSummary:
    preview = "***" if phrase.sensitive else literal_preview(phrase)  # FR-8
    return PhraseSummary(
        id=phrase.id,
        folder_id=phrase.folder_id,
        name=phrase.name,
        abbreviation=phrase.abbreviation,
        content_type=phrase.content_type,
        sensitive=phrase.sensitive,
        content_preview=preview,
        deleted=phrase.deleted_at is not None,
    )


def _apply_update(phrase: Phrase, data: PhraseUpdate) -> Phrase:
    """Build the updated phrase; abbreviation is canonicalized from autotext trigger (FR-6)."""
    abbreviation = data.abbreviation if data.abbreviation is not None else phrase.abbreviation
    triggers = data.triggers if data.triggers is not None else phrase.triggers
    if triggers:
        autotext = [t for t in triggers if t.type is TriggerType.AUTOTEXT]
        if len(autotext) > 1:
            raise ValueError("at most one autotext trigger is allowed")
        if autotext:
            abbreviation = autotext[0].abbreviation
    content_type = data.content_type if data.content_type is not None else phrase.content_type
    if content_type is not ContentType.PLAIN:
        raise ReservedContentTypeError(
            f"content_type '{content_type.value}' is reserved (v1 supports plain only)"
        )
    return Phrase(
        id=phrase.id,
        folder_id=phrase.folder_id,
        name=data.name if data.name is not None else phrase.name,
        abbreviation=abbreviation,
        content_type=content_type,
        body=data.body if data.body is not None else phrase.body,
        triggers=triggers,
        sensitive=data.sensitive if data.sensitive is not None else phrase.sensitive,
        created_at=phrase.created_at,
        updated_at=phrase.updated_at,
        deleted_at=phrase.deleted_at,
    )


@app.get("/health")
def health() -> dict[str, str]:
    return {"status": "ok"}


# -- Folders (FR-2) -------------------------------------------------------- #
@app.post(f"{API_PREFIX}/folders", response_model=Folder, status_code=status.HTTP_201_CREATED)
def create_folder(payload: FolderCreate, repo: RepoDep) -> Folder:
    folder = Folder(
        id=uuid4(),
        name=payload.name,
        parent_id=payload.parent_id,
        created_at=_utcnow(),
    )
    return repo.create_folder(folder)


@app.get(f"{API_PREFIX}/folders", response_model=list[Folder])
def list_folders(repo: RepoDep) -> list[Folder]:
    return repo.list_folders()


@app.get(f"{API_PREFIX}/folders/{{folder_id}}", response_model=Folder)
def get_folder(folder_id: UUID, repo: RepoDep) -> Folder:
    folder = repo.get_folder(folder_id)
    if folder is None:
        raise NotFoundError(f"folder {folder_id} not found")
    return folder


@app.delete(f"{API_PREFIX}/folders/{{folder_id}}", status_code=status.HTTP_204_NO_CONTENT)
def delete_folder(folder_id: UUID, repo: RepoDep) -> None:
    if not repo.delete_folder(folder_id):
        raise NotFoundError(f"folder {folder_id} not found")


# -- Phrases (FR-1, FR-6, FR-7, FR-8) -------------------------------------- #
@app.post(f"{API_PREFIX}/phrases", response_model=Phrase, status_code=status.HTTP_201_CREATED)
def create_phrase(payload: PhraseCreate, repo: RepoDep) -> Phrase:
    if payload.content_type is not ContentType.PLAIN:
        raise ReservedContentTypeError(
            f"content_type '{payload.content_type.value}' is reserved (v1 supports plain only)"
        )
    phrase = Phrase(
        id=uuid4(),
        folder_id=payload.folder_id,
        name=payload.name,
        abbreviation=payload.abbreviation,
        content_type=payload.content_type,
        body=payload.body,
        triggers=payload.triggers,
        sensitive=payload.sensitive,
    )
    return repo.create_phrase(phrase)


@app.get(f"{API_PREFIX}/phrases", response_model=list[PhraseSummary])
def list_phrases(
    repo: RepoDep,
    folder_id: UUID | None = Query(default=None),
    q: str | None = Query(default=None),
    include_deleted: bool = Query(default=False),
) -> list[PhraseSummary]:
    phrases = repo.list_phrases(folder_id=folder_id, q=q, include_deleted=include_deleted)
    return [_to_summary(p) for p in phrases]


@app.get(f"{API_PREFIX}/phrases/{{phrase_id}}", response_model=Phrase)
def get_phrase(phrase_id: UUID, repo: RepoDep) -> Phrase:
    phrase = repo.get_phrase(phrase_id)
    if phrase is None or phrase.deleted_at is not None:
        raise NotFoundError(f"phrase {phrase_id} not found")
    return phrase


@app.put(f"{API_PREFIX}/phrases/{{phrase_id}}", response_model=Phrase)
def update_phrase(phrase_id: UUID, payload: PhraseUpdate, repo: RepoDep) -> Phrase:
    phrase = repo.get_phrase(phrase_id)
    if phrase is None or phrase.deleted_at is not None:
        raise NotFoundError(f"phrase {phrase_id} not found")
    updated = _apply_update(phrase, payload)
    return repo.update_phrase(updated)


@app.delete(f"{API_PREFIX}/phrases/{{phrase_id}}", status_code=status.HTTP_204_NO_CONTENT)
def delete_phrase(phrase_id: UUID, repo: RepoDep) -> None:
    if repo.soft_delete_phrase(phrase_id) is None:  # FR-7 recycle bin
        raise NotFoundError(f"phrase {phrase_id} not found")


# -- Execution pipeline (FR-4, FR-5, FR-12) -------------------------------- #
@app.post(f"{API_PREFIX}/expand", response_model=ExpandResponse)
def expand(payload: ExpandRequest, repo: RepoDep) -> ExpandResponse:
    phrase = repo.resolve(payload.trigger)
    if phrase is None:
        raise NotFoundError("no phrase matches the given trigger")
    output = expand_phrase(phrase, payload.inputs, _utcnow())  # NFR-3 via pure helper
    return ExpandResponse(
        phrase_id=phrase.id,
        abbreviation=phrase.abbreviation,
        content_type=phrase.content_type,
        output=output,
    )


# -- Clipboard (FR-9) ------------------------------------------------------ #
@app.post(
    f"{API_PREFIX}/clipboard",
    response_model=ClipboardEntry,
    status_code=status.HTTP_201_CREATED,
)
def add_clipboard(payload: ClipboardCreate, repo: RepoDep) -> ClipboardEntry:
    return repo.add_clipboard(
        ClipboardEntry(id=uuid4(), text=payload.text, captured_at=_utcnow())
    )


@app.get(f"{API_PREFIX}/clipboard", response_model=list[ClipboardEntry])
def list_clipboard(
    repo: RepoDep, limit: int = Query(default=20, ge=1, le=CLIPBOARD_CAPACITY)
) -> list[ClipboardEntry]:
    return repo.list_clipboard(limit)


def main() -> None:
    import uvicorn

    configure()
    uvicorn.run("app:app", host="127.0.0.1", port=8000, reload=True)


# NOTE: the DB is configured lazily on first request (get_repository) or explicitly
# in main(), so importing this module does not create phrasexp.db on disk. Tests call
# configure(<tmp file>) for hermetic isolation.

if __name__ == "__main__":
    main()
