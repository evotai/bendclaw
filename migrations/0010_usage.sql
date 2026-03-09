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
