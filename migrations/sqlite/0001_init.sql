CREATE TABLE IF NOT EXISTS photos (
  hash            TEXT PRIMARY KEY,
  rel_path        TEXT NOT NULL UNIQUE,
  media_type      TEXT NOT NULL,
  mime            TEXT NOT NULL,
  byte_size       INTEGER NOT NULL,
  width           INTEGER NOT NULL,
  height          INTEGER NOT NULL,
  orientation     INTEGER NOT NULL DEFAULT 1,
  taken_at        TEXT,
  file_mtime      TEXT NOT NULL,
  date_override   TEXT,
  date_source     TEXT NOT NULL,
  favorite        INTEGER NOT NULL DEFAULT 0,
  derivatives_ok  INTEGER NOT NULL DEFAULT 0,
  indexed_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tags (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS photo_tags (
  photo_hash TEXT NOT NULL REFERENCES photos(hash) ON DELETE CASCADE,
  tag_id     INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (photo_hash, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_photos_effective_date
  ON photos(COALESCE(date_override, taken_at, file_mtime));
CREATE INDEX IF NOT EXISTS idx_photo_tags_tag ON photo_tags(tag_id);

-- meta: generation counter, schema version, scan-request flag.
-- Values are TEXT so new keys need no migration.
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('generation', '0');
INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1');
INSERT OR IGNORE INTO meta (key, value) VALUES ('scan_requested', '0');

-- Indexer singleton lock (SQLite topology).
CREATE TABLE IF NOT EXISTS singleton (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  holder      TEXT NOT NULL,
  acquired_at TEXT NOT NULL
);
