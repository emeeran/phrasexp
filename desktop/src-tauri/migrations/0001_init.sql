-- phrasexp schema v1 — base tables; later columns come from the incremental
-- ALTERs in run_migrations (db/repo.rs).

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
    app_scope     TEXT,                 -- Phase 2: substring of focused window title; NULL = global
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

-- Phrase version history (Phase 1): snapshots of body+triggers prior to each update.
CREATE TABLE IF NOT EXISTS phrase_versions (
    id            TEXT PRIMARY KEY,
    phrase_id     TEXT NOT NULL,
    body_json     TEXT NOT NULL,
    triggers_json TEXT NOT NULL,
    saved_at      TEXT NOT NULL
);
