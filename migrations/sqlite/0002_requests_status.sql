-- Web -> indexer requests for actions that touch library/ or exports/.
-- Web records a row; the indexer performs the work and updates state.
-- Operational state only: rebuilt from nothing, not part of the curation export.
CREATE TABLE IF NOT EXISTS requests (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL CHECK (kind IN ('delete', 'export')),
  target     TEXT,                      -- photo hash for 'delete'; NULL for 'export'
  state      TEXT NOT NULL DEFAULT 'pending'
             CHECK (state IN ('pending', 'running', 'done', 'failed')),
  result     TEXT,                      -- export file name when done, error text when failed
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_requests_state ON requests(state, id);
-- One in-flight delete per photo.
CREATE UNIQUE INDEX IF NOT EXISTS idx_requests_delete_inflight
  ON requests(target) WHERE kind = 'delete' AND state IN ('pending', 'running');

-- Per-photo derivative failure and backoff state. Derived; disposable.
CREATE TABLE IF NOT EXISTS derivative_failures (
  photo_hash    TEXT PRIMARY KEY REFERENCES photos(hash) ON DELETE CASCADE,
  attempts      INTEGER NOT NULL DEFAULT 0,
  last_error    TEXT NOT NULL,
  next_retry_at TEXT NOT NULL
);

-- Status keys, written by the indexer. Queue depth is not stored: it is
-- COUNT(*) of photos with derivatives_ok = 0, and failures are COUNT(*) of
-- derivative_failures.
INSERT OR IGNORE INTO meta (key, value) VALUES ('last_scan_at', '');
INSERT OR IGNORE INTO meta (key, value) VALUES ('indexing_state', 'idle');
INSERT OR IGNORE INTO meta (key, value) VALUES ('scan_total', '0');
INSERT OR IGNORE INTO meta (key, value) VALUES ('scan_done', '0');
