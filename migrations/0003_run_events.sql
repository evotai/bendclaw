-- Run-level event log (append-only).
CREATE TABLE IF NOT EXISTS run_events (
    id             VARCHAR   NOT NULL COMMENT 'ULID primary key',
    run_id         VARCHAR   NOT NULL COMMENT 'FK -> runs.id',
    session_id     VARCHAR   NOT NULL COMMENT 'FK -> sessions.id',
    agent_id       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent identifier',
    user_id        VARCHAR   NOT NULL COMMENT 'Owner of the run',
    seq            UINT32    NOT NULL DEFAULT 0 COMMENT 'Monotonic event sequence within a run',
    event          VARCHAR   NOT NULL COMMENT 'Event type name',
    payload        VARCHAR   NOT NULL DEFAULT '' COMMENT 'JSON payload of the event',
    created_at     TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Append-only event timeline for runs';
