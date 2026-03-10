-- Request-level traces.
CREATE TABLE IF NOT EXISTS traces (
    trace_id     VARCHAR   NOT NULL   COMMENT 'Trace identifier (ULID)',
    run_id       VARCHAR   NOT NULL DEFAULT '' COMMENT 'FK → runs.id',
    session_id   VARCHAR   NOT NULL DEFAULT '' COMMENT 'FK → sessions.id',
    agent_id     VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent identifier',
    user_id      VARCHAR   NOT NULL DEFAULT '' COMMENT 'User identifier',
    name         VARCHAR   NOT NULL DEFAULT '' COMMENT 'Trace name (e.g. chat.turn)',
    status       VARCHAR   NOT NULL DEFAULT 'running' COMMENT 'running | completed | failed | cancelled',
    duration_ms  UINT64    NOT NULL DEFAULT 0 COMMENT 'Total trace duration in ms',
    input_tokens  UINT64   NOT NULL DEFAULT 0 COMMENT 'Total input tokens',
    output_tokens UINT64   NOT NULL DEFAULT 0 COMMENT 'Total output tokens',
    total_cost   DOUBLE    NOT NULL DEFAULT 0.0 COMMENT 'Total estimated cost',
    created_at   TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Request-level traces';

-- Individual operation spans within a trace.
CREATE TABLE IF NOT EXISTS spans (
    span_id        VARCHAR   NOT NULL   COMMENT 'Span identifier (ULID)',
    trace_id       VARCHAR   NOT NULL   COMMENT 'FK → traces.trace_id',
    parent_span_id VARCHAR   NOT NULL DEFAULT '' COMMENT 'Parent span for hierarchy',
    name           VARCHAR   NOT NULL DEFAULT '' COMMENT 'Operation name',
    kind           VARCHAR   NOT NULL DEFAULT '' COMMENT 'llm | tool | skill | compaction | checkpoint',
    model_role     VARCHAR   NOT NULL DEFAULT '' COMMENT 'reasoning | compaction | checkpoint',
    status         VARCHAR   NOT NULL DEFAULT '' COMMENT 'started | completed | failed | cancelled',
    duration_ms    UINT64    NOT NULL DEFAULT 0 COMMENT 'Span duration in ms',
    ttft_ms        UINT64    NOT NULL DEFAULT 0 COMMENT 'Time to first token in ms',
    input_tokens   UINT64    NOT NULL DEFAULT 0 COMMENT 'Input tokens',
    output_tokens  UINT64    NOT NULL DEFAULT 0 COMMENT 'Output tokens',
    reasoning_tokens UINT64  NOT NULL DEFAULT 0 COMMENT 'Reasoning tokens',
    cost           DOUBLE    NOT NULL DEFAULT 0.0 COMMENT 'Estimated cost in USD',
    error_code     VARCHAR   NOT NULL DEFAULT '' COMMENT 'Error code',
    error_message  VARCHAR   NOT NULL DEFAULT '' COMMENT 'Error message',
    summary        VARCHAR   NOT NULL DEFAULT '' COMMENT 'Human-readable summary',
    meta           VARCHAR   NOT NULL DEFAULT '' COMMENT 'JSON metadata',
    created_at     TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Operation spans within traces';

-- LLM token usage and cost tracking per request.
CREATE TABLE IF NOT EXISTS usage (
    id                 VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    agent_id           VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent identifier',
    user_id            VARCHAR   NOT NULL   COMMENT 'Who made the request',
    session_id         VARCHAR   NOT NULL DEFAULT '' COMMENT 'FK → sessions.id',
    run_id             VARCHAR   NOT NULL DEFAULT '' COMMENT 'FK → runs.id',
    provider           VARCHAR   NOT NULL DEFAULT '' COMMENT 'e.g. openai, anthropic',
    model              VARCHAR   NOT NULL DEFAULT '' COMMENT 'e.g. gpt-4.1-mini',
    model_role         VARCHAR   NOT NULL DEFAULT '' COMMENT 'reasoning | compaction | checkpoint',
    prompt_tokens      INT       NOT NULL DEFAULT 0  COMMENT 'Input tokens',
    completion_tokens  INT       NOT NULL DEFAULT 0  COMMENT 'Output tokens',
    reasoning_tokens   INT       NOT NULL DEFAULT 0  COMMENT 'Reasoning tokens (o1/o3)',
    total_tokens       INT       NOT NULL DEFAULT 0  COMMENT 'Prompt + completion tokens',
    cache_read_tokens  INT       NOT NULL DEFAULT 0  COMMENT 'Prompt cache hits',
    cache_write_tokens INT       NOT NULL DEFAULT 0  COMMENT 'Prompt cache writes',
    ttft_ms            INT       NOT NULL DEFAULT 0  COMMENT 'Time to first token in ms',
    cost               DOUBLE    NOT NULL DEFAULT 0.0 COMMENT 'Estimated cost in USD',
    created_at         TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'LLM token usage and cost tracking';
