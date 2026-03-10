-- Agent runs — one per chat() invocation within a session.
CREATE TABLE IF NOT EXISTS runs (
    id             VARCHAR   NOT NULL   COMMENT 'ULID primary key (= run_id)',
    session_id     VARCHAR   NOT NULL   COMMENT 'FK → sessions.id',
    agent_id       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent identifier',
    user_id        VARCHAR   NOT NULL   COMMENT 'Who initiated the run',
    parent_run_id  VARCHAR   NOT NULL DEFAULT '' COMMENT 'Parent run id when continued/retried',
    status         VARCHAR   NOT NULL DEFAULT 'RUNNING' COMMENT 'PENDING | RUNNING | PAUSED | COMPLETED | CANCELLED | ERROR',
    input          VARCHAR   NOT NULL DEFAULT '' COMMENT 'User message that started this run',
    output         VARCHAR   NOT NULL DEFAULT '' COMMENT 'Final assistant response text',
    error          VARCHAR   NOT NULL DEFAULT '' COMMENT 'Final error message when status=ERROR',
    metrics        VARCHAR   NOT NULL DEFAULT '' COMMENT 'JSON RunMetrics: tokens, timing, cost',
    stop_reason    VARCHAR   NOT NULL DEFAULT '' COMMENT 'end_turn | max_iterations | timeout | aborted | error',
    iterations     UINT32    NOT NULL DEFAULT 0 COMMENT 'Number of LLM reasoning turns',
    created_at     TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Agent runs — one per chat() invocation';

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
