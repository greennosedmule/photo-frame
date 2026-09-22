-- rotation: authoritative curation (degrees clockwise, applied on top of EXIF
-- orientation when derivatives are generated). Must be in the curation export.
-- derivatives_rotation: derived; the rotation the derivatives on disk were
-- generated for. Differs from `rotation` while the indexer catches up.
ALTER TABLE photos ADD COLUMN IF NOT EXISTS rotation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE photos ADD COLUMN IF NOT EXISTS derivatives_rotation INTEGER NOT NULL DEFAULT 0;
