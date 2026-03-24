ALTER TABLE runs ADD COLUMN IF NOT EXISTS kind VARCHAR NOT NULL DEFAULT 'user_turn' COMMENT 'user_turn | session_checkpoint';
ALTER TABLE runs ADD COLUMN IF NOT EXISTS checkpoint_through_run_id VARCHAR NOT NULL DEFAULT '' COMMENT 'For session_checkpoint rows: last user_turn run_id covered by the summary';
