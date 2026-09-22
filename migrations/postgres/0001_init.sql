CREATE TABLE IF NOT EXISTS photos (
  hash            TEXT PRIMARY KEY,
  rel_path        TEXT NOT NULL UNIQUE,
  media_type      TEXT NOT NULL,
  mime            TEXT NOT NULL,
  byte_size       BIGINT NOT NULL,
  width           INTEGER NOT NULL,
  height          INTEGER NOT NULL,
  orientation     INTEGER NOT NULL DEFAULT 1,
  taken_at        TIMESTAMPTZ,
  file_mtime      TIMESTAMPTZ NOT NULL,
  date_override   TIMESTAMPTZ,
  date_source     TEXT NOT NULL,
  favorite        BOOLEAN NOT NULL DEFAULT FALSE,
  derivatives_ok  BOOLEAN NOT NULL DEFAULT FALSE,
  indexed_at      TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS tags (
  id         INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  name       TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL
);
-- Case-insensitive uniqueness without requiring the citext extension.
CREATE UNIQUE INDEX IF NOT EXISTS idx_tags_name_lower ON tags (lower(name));

CREATE TABLE IF NOT EXISTS photo_tags (
  photo_hash TEXT NOT NULL REFERENCES photos(hash) ON DELETE CASCADE,
  tag_id     INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (photo_hash, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_photos_effective_date
  ON photos (COALESCE(date_override, taken_at, file_mtime));
CREATE INDEX IF NOT EXISTS idx_photo_tags_tag ON photo_tags(tag_id);

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT INTO meta (key, value) VALUES ('generation', '0') ON CONFLICT DO NOTHING;
INSERT INTO meta (key, value) VALUES ('schema_version', '1') ON CONFLICT DO NOTHING;
INSERT INTO meta (key, value) VALUES ('scan_requested', '0') ON CONFLICT DO NOTHING;

-- Unused on Postgres (pg_try_advisory_lock is used instead); kept so both
-- backends carry the same tables, per the spec.
CREATE TABLE IF NOT EXISTS singleton (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  holder      TEXT NOT NULL,
  acquired_at TIMESTAMPTZ NOT NULL
);
