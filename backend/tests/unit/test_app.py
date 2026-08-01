"""End-to-end + unit tests for phrasexp v1 (docs/02-spec/SPEC.md).

Each test runs against an isolated SQLite file via the `client` fixture, which calls
`app.configure()` to re-point storage at a fresh temp DB.
"""

from datetime import datetime, timezone

import pytest
from fastapi.testclient import TestClient

import app as app_module
from app import (
    CLIPBOARD_CAPACITY,
    DateMathMacro,
    DateTimeMacro,
    Folder,
    InputFormMacro,
    MacroSegment,
    TextSegment,
    configure,
    expand_phrase,
    get_repository,
)
from app import app


# --------------------------------------------------------------------------- #
# Fixtures & helpers                                                          #
# --------------------------------------------------------------------------- #
@pytest.fixture
def client(tmp_path):
    configure(f"sqlite:///{tmp_path}/test.db")
    with TestClient(app) as c:
        yield c


def _folder(client, name="Inbox", parent=None) -> str:
    r = client.post("/api/v1/folders", json={"name": name, "parent_id": parent})
    assert r.status_code == 201, r.text
    return r.json()["id"]


def _phrase(client, folder_id: str, **overrides) -> dict:
    payload: dict = {
        "folder_id": folder_id,
        "name": "Greeting",
        "body": [{"type": "text", "text": "Hello"}],
    }
    payload.update(overrides)
    r = client.post("/api/v1/phrases", json=payload)
    assert r.status_code == 201, r.text
    return r.json()


# --------------------------------------------------------------------------- #
# FR-1 / health                                                               #
# --------------------------------------------------------------------------- #
def test_health(client):
    assert client.get("/health").json() == {"status": "ok"}


def test_phrase_crud(client):
    fid = _folder(client)
    created = _phrase(client, fid, name="Sig", abbreviation="sig")
    pid = created["id"]

    # read
    got = client.get(f"/api/v1/phrases/{pid}")
    assert got.status_code == 200
    assert got.json()["name"] == "Sig"

    # update
    upd = client.put(f"/api/v1/phrases/{pid}", json={"name": "Signature"})
    assert upd.status_code == 200
    assert upd.json()["name"] == "Signature"

    # list
    listed = client.get("/api/v1/phrases").json()
    assert any(p["id"] == pid for p in listed)


# --------------------------------------------------------------------------- #
# FR-2 folders (CRUD + folder_not_empty)                                      #
# --------------------------------------------------------------------------- #
def test_folder_crud_and_nesting(client):
    root = _folder(client, "Root")
    child = _folder(client, "Child", parent=root)
    assert client.get(f"/api/v1/folders/{child}").json()["parent_id"] == root


def test_folder_not_empty_blocks_delete(client):  # P3 review O3
    fid = _folder(client)
    _phrase(client, fid)
    r = client.delete(f"/api/v1/folders/{fid}")
    assert r.status_code == 409
    assert r.json()["code"] == "folder_not_empty"


def test_folder_delete_when_empty(client):
    fid = _folder(client)
    assert client.delete(f"/api/v1/folders/{fid}").status_code == 204
    assert client.get(f"/api/v1/folders/{fid}").status_code == 404


# --------------------------------------------------------------------------- #
# FR-3 / FR-4 triggers + execution                                            #
# --------------------------------------------------------------------------- #
def test_expand_autotext(client):
    fid = _folder(client)
    _phrase(
        client,
        fid,
        body=[{"type": "text", "text": "Best regards,"}],
        abbreviation="br,",
        triggers=[{"type": "autotext", "abbreviation": "br,"}],
    )
    r = client.post(
        "/api/v1/expand",
        json={"trigger": {"type": "autotext", "abbreviation": "br,"}, "inputs": {}},
    )
    assert r.status_code == 200
    assert r.json()["output"] == "Best regards,"


def test_expand_hotkey(client):
    fid = _folder(client)
    _phrase(
        client,
        fid,
        body=[{"type": "text", "text": "Hot!"}],
        triggers=[{"type": "hotkey", "key": "ctrl+shift+b"}],
    )
    r = client.post(
        "/api/v1/expand",
        json={"trigger": {"type": "hotkey", "key": "ctrl+shift+b"}, "inputs": {}},
    )
    assert r.json()["output"] == "Hot!"


def test_expand_not_found(client):
    r = client.post(
        "/api/v1/expand",
        json={"trigger": {"type": "autotext", "abbreviation": "nope"}, "inputs": {}},
    )
    assert r.status_code == 404
    assert r.json()["code"] == "not_found"


# --------------------------------------------------------------------------- #
# FR-5 / FR-10 / FR-12 macros + ordered segments + inputs                     #
# --------------------------------------------------------------------------- #
def test_macro_datetime():
    phrase = _phrase_domain(body=[MacroSegment(macro=DateTimeMacro(format="%Y-%m-%d"))])
    out = expand_phrase(phrase, {}, datetime(2026, 8, 1, 9, 30, tzinfo=timezone.utc))
    assert out == "2026-08-01"


def test_macro_datemath():
    phrase = _phrase_domain(
        body=[MacroSegment(macro=DateMathMacro(days_offset=1, format="%Y-%m-%d"))]
    )
    out = expand_phrase(phrase, {}, datetime(2026, 8, 1, tzinfo=timezone.utc))
    assert out == "2026-08-02"


def test_inputform_missing_via_http(client):  # FR-5 error path
    fid = _folder(client)
    _phrase(
        client,
        fid,
        abbreviation="hi",
        triggers=[{"type": "autotext", "abbreviation": "hi"}],
        body=[
            {"type": "macro", "macro": {"type": "inputform", "field": "name"}},
        ],
    )
    r = client.post(
        "/api/v1/expand",
        json={"trigger": {"type": "autotext", "abbreviation": "hi"}, "inputs": {}},
    )
    assert r.status_code == 400
    assert r.json()["code"] == "missing_input"


def test_ordered_segments_with_input(client):  # FR-10 + FR-12
    fid = _folder(client)
    _phrase(
        client,
        fid,
        abbreviation="hi",
        triggers=[{"type": "autotext", "abbreviation": "hi"}],
        body=[
            {"type": "text", "text": "Hello "},
            {"type": "macro", "macro": {"type": "inputform", "field": "name"}},
            {"type": "text", "text": "!"},
        ],
    )
    r = client.post(
        "/api/v1/expand",
        json={
            "trigger": {"type": "autotext", "abbreviation": "hi"},
            "inputs": {"name": "Sam"},
        },
    )
    assert r.json()["output"] == "Hello Sam!"


# --------------------------------------------------------------------------- #
# FR-6 abbreviation uniqueness                                                #
# --------------------------------------------------------------------------- #
def test_abbreviation_conflict(client):
    fid = _folder(client)
    _phrase(client, fid, abbreviation="sig", triggers=[{"type": "autotext", "abbreviation": "sig"}])
    r = _phrase_raw(client, fid, abbreviation="sig")
    assert r.status_code == 409
    assert r.json()["code"] == "conflict"


# --------------------------------------------------------------------------- #
# FR-7 soft-delete                                                            #
# --------------------------------------------------------------------------- #
def test_soft_delete(client):
    fid = _folder(client)
    p = _phrase(client, fid, abbreviation="sig", triggers=[{"type": "autotext", "abbreviation": "sig"}])
    pid = p["id"]

    assert client.delete(f"/api/v1/phrases/{pid}").status_code == 204

    # not in default list
    assert not any(x["id"] == pid for x in client.get("/api/v1/phrases").json())
    # not resolvable
    exp = client.post(
        "/api/v1/expand",
        json={"trigger": {"type": "autotext", "abbreviation": "sig"}, "inputs": {}},
    )
    assert exp.status_code == 404
    # visible with include_deleted, and direct GET 404s
    assert any(x["id"] == pid for x in client.get("/api/v1/phrases?include_deleted=true").json())
    assert client.get(f"/api/v1/phrases/{pid}").status_code == 404
    # double delete -> 404
    assert client.delete(f"/api/v1/phrases/{pid}").status_code == 404


# --------------------------------------------------------------------------- #
# FR-8 sensitive masking                                                      #
# --------------------------------------------------------------------------- #
def test_sensitive_masking(client):
    fid = _folder(client)
    p = _phrase(client, fid, sensitive=True, body=[{"type": "text", "text": "top-secret-value"}])
    summary = next(s for s in client.get("/api/v1/phrases").json() if s["id"] == p["id"])
    assert summary["content_preview"] == "***"
    assert summary["sensitive"] is True
    # full body only on direct GET
    assert client.get(f"/api/v1/phrases/{p['id']}").json()["body"][0]["text"] == "top-secret-value"


# --------------------------------------------------------------------------- #
# FR-9 clipboard + capacity eviction (P3 O1)                                  #
# --------------------------------------------------------------------------- #
def test_clipboard_capture_and_list(client):
    client.post("/api/v1/clipboard", json={"text": "first"})
    client.post("/api/v1/clipboard", json={"text": "second"})
    items = client.get("/api/v1/clipboard?limit=10").json()
    assert [i["text"] for i in items] == ["second", "first"]  # newest first


def test_clipboard_capacity_eviction(client):
    from datetime import timedelta

    repo = get_repository()
    base = datetime(2026, 8, 1, tzinfo=timezone.utc)
    # distinct, increasing timestamps so "oldest by captured_at" is well-defined
    for i in range(CLIPBOARD_CAPACITY + 1):
        repo.add_clipboard(_clipboard_domain(f"item-{i}", base + timedelta(seconds=i)))
    listed = repo.list_clipboard(CLIPBOARD_CAPACITY + 1)
    assert len(listed) == CLIPBOARD_CAPACITY
    assert all(e.text != "item-0" for e in listed)  # oldest evicted
    assert any(e.text == f"item-{CLIPBOARD_CAPACITY}" for e in listed)  # newest kept


# --------------------------------------------------------------------------- #
# FR-11 persistence across "restart"                                          #
# --------------------------------------------------------------------------- #
def test_persistence_across_reconfigure(client, tmp_path):
    fid = _folder(client)
    p = _phrase(client, fid, name="Persisted", abbreviation="ps")
    db_file = f"sqlite:///{tmp_path}/test.db"

    # simulate restart: re-point at the SAME file with a fresh engine/session
    configure(db_file)
    assert get_repository().get_phrase(p["id"]) is not None
    got = get_repository().get_phrase(p["id"])
    assert got.name == "Persisted"


# --------------------------------------------------------------------------- #
# NFR-4 / reserved content type                                               #
# --------------------------------------------------------------------------- #
def test_reserved_content_type_on_create(client):
    fid = _folder(client)
    r = client.post(
        "/api/v1/phrases",
        json={
            "folder_id": fid,
            "name": "X",
            "content_type": "html",
            "body": [{"type": "text", "text": "x"}],
        },
    )
    assert r.status_code == 422
    assert r.json()["code"] == "reserved_content_type"


def test_reserved_content_type_on_update(client):
    fid = _folder(client)
    p = _phrase(client, fid)
    r = client.put(f"/api/v1/phrases/{p['id']}", json={"content_type": "richtext"})
    assert r.status_code == 422
    assert r.json()["code"] == "reserved_content_type"


def test_invalid_payload_422(client):
    fid = _folder(client)
    r = client.post(
        "/api/v1/phrases",
        json={"folder_id": fid, "name": "X", "body": []},  # empty body
    )
    assert r.status_code == 422


# --------------------------------------------------------------------------- #
# NFR-3 determinism (pure helper)                                             #
# --------------------------------------------------------------------------- #
def test_expand_deterministic():
    phrase = _phrase_domain(body=[MacroSegment(macro=DateTimeMacro(format="%Y"))])
    now = datetime(2026, 8, 1, tzinfo=timezone.utc)
    assert expand_phrase(phrase, {}, now) == expand_phrase(phrase, {}, now) == "2026"


# --------------------------------------------------------------------------- #
# helpers (domain object construction)                                        #
# --------------------------------------------------------------------------- #
def _phrase_domain(body) -> app_module.Phrase:
    from uuid import uuid4

    return app_module.Phrase(
        id=uuid4(),
        folder_id=uuid4(),
        name="t",
        body=body,
        created_at=datetime(2026, 8, 1, tzinfo=timezone.utc),
        updated_at=datetime(2026, 8, 1, tzinfo=timezone.utc),
    )


def _clipboard_domain(text: str, captured_at: datetime | None = None):
    from uuid import uuid4

    return app_module.ClipboardEntry(
        id=uuid4(),
        text=text,
        captured_at=captured_at or datetime(2026, 8, 1, tzinfo=timezone.utc),
    )


def _phrase_raw(client, folder_id: str, **overrides):
    payload: dict = {
        "folder_id": folder_id,
        "name": "Greeting",
        "body": [{"type": "text", "text": "Hello"}],
    }
    payload.update(overrides)
    return client.post("/api/v1/phrases", json=payload)
