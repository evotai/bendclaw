-- Scheduled tasks for periodic agent execution.
CREATE TABLE IF NOT EXISTS tasks (
    id               VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    agentos_id       VARCHAR   NOT NULL   COMMENT 'Target AgentOS instance ID',
    name             VARCHAR   NOT NULL   COMMENT 'Human-readable task name',
    cron_expr        VARCHAR   NOT NULL   COMMENT 'Cron expression for scheduling',
    prompt           TEXT      NOT NULL DEFAULT '' COMMENT 'Prompt to execute on schedule',
    enabled          BOOLEAN   NOT NULL DEFAULT true COMMENT 'Whether task is active',
    status           VARCHAR   NOT NULL DEFAULT 'idle' COMMENT 'idle | running | error',
    schedule_kind    VARCHAR   NOT NULL DEFAULT 'cron' COMMENT 'Schedule type: at | every | cron',
    every_seconds    INT       NULL      COMMENT 'Interval in seconds (kind=every)',
    at_time          TIMESTAMP NULL      COMMENT 'One-shot execution time (kind=at)',
    tz               VARCHAR   NULL      COMMENT 'Cron timezone (IANA format)',
    webhook_url      VARCHAR   NULL      COMMENT 'Webhook URL to call after execution',
    last_error       TEXT      NULL      COMMENT 'Last execution error message',
    delete_after_run BOOLEAN   NOT NULL DEFAULT false COMMENT 'Auto-delete after execution',
    run_count        INT       NOT NULL DEFAULT 0 COMMENT 'Total execution count',
    last_run_at      TIMESTAMP NULL      COMMENT 'Last execution timestamp',
    next_run_at      TIMESTAMP NULL      COMMENT 'Next scheduled execution',
    created_at       TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Scheduled tasks for periodic agent execution';
