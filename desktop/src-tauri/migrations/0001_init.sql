-- phrasexp schema — identical to the FastAPI/SQLAlchemy reference (backend/app.py)
-- so a phrasexp.db created by either app is usable by the other (FR-11).

CREATE TABLE IF NOT EXISTS folders (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    parent_id  TEXT REFERENCES folders(id),
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS phrases (
    id            TEXT PRIMARY KEY,
    folder_id     TEXT NOT NULL REFERENCES folders(id),
    name          TEXT NOT NULL,
    abbreviation  TEXT,
    content_type  TEXT NOT NULL DEFAULT 'plain',
    body_json     TEXT NOT NULL,        -- JSON array of segments
    triggers_json TEXT NOT NULL DEFAULT '[]',  -- JSON array of trigger bindings
    sensitive     INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    deleted_at    TEXT
);

CREATE TABLE IF NOT EXISTS clipboard_entries (
    id          TEXT PRIMARY KEY,
    text        TEXT NOT NULL,
    captured_at TEXT NOT NULL
);

-- App settings (M5): single row, id = 1.
CREATE TABLE IF NOT EXISTS settings (
    id                 INTEGER PRIMARY KEY CHECK (id = 1),
    popup_hotkey       TEXT    NOT NULL DEFAULT 'Control+Alt+Space',
    autotext_enabled   INTEGER NOT NULL DEFAULT 1,
    clipboard_enabled  INTEGER NOT NULL DEFAULT 1,
    autostart          INTEGER NOT NULL DEFAULT 0
);
