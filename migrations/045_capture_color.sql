-- A capture's paper colour (#273): the notes' palette keys, as a closed
-- set. Additive and non-destructive: every existing capture gets the
-- default (the sticky yellow it already shows). The list must match
-- `common::capture::CaptureColor::ALL`; the pg test checks they agree.
-- Idempotent: safe to re-run.

ALTER TABLE captures
    ADD COLUMN IF NOT EXISTS color TEXT NOT NULL DEFAULT 'default';

ALTER TABLE captures DROP CONSTRAINT IF EXISTS captures_color_check;
ALTER TABLE captures ADD CONSTRAINT captures_color_check
    CHECK (color IN ('default', 'amber', 'rose', 'lime', 'sky', 'violet'));
