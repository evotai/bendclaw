-- Scheduled tasks for periodic agent execution.
CREATE TABLE IF NOT EXISTS tasks (
    id            VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    agentos_id    VARCHAR   NOT NULL   COMMENT 'Target AgentOS instance ID',
    name          VARCHAR   NOT NULL   COMMENT 'Human-readable task name',
    cron_expr     VARCHAR   NOT NULL   COMMENT 'Cron expression for scheduling',
    prompt        TEXT      NOT NULL DEFAULT '' COMMENT 'Prompt to execute on schedule',
    enabled       BOOLEAN   NOT NULL DEFAULT true COMMENT 'Whether task is active',
    status        VARCHAR   NOT NULL DEFAULT 'idle' COMMENT 'idle | running | error',
    last_run_at   TIMESTAMP NULL      COMMENT 'Last execution timestamp',
    next_run_at   TIMESTAMP NULL      COMMENT 'Next scheduled execution',
    created_at    TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Scheduled tasks for periodic agent execution';
