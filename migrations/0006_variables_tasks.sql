-- User-defined key-value variables, referenceable in prompt templates.
CREATE TABLE IF NOT EXISTS variables (
    id         VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    key        VARCHAR   NOT NULL   COMMENT 'Variable name (unique per agent db)',
    value      TEXT      NOT NULL DEFAULT '' COMMENT 'Variable value (masked if secret)',
    secret     BOOLEAN   NOT NULL DEFAULT false COMMENT 'Whether value should be masked in API responses',
    last_used_at TIMESTAMP NULL     COMMENT 'Last time this variable was referenced in a run',
    created_at   TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'User-defined key-value variables';

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

-- Task execution history (snapshot of task at execution time).
CREATE TABLE IF NOT EXISTS task_history (
    id              VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    task_id         VARCHAR   NOT NULL   COMMENT 'FK to tasks.id',
    run_id          VARCHAR   NULL       COMMENT 'Associated run ID',
    task_name       VARCHAR   NOT NULL   COMMENT 'Task name at execution time',
    schedule_kind   VARCHAR   NOT NULL   COMMENT 'Schedule kind: at | every | cron',
    cron_expr       VARCHAR   NULL       COMMENT 'Cron expression used',
    prompt          TEXT      NOT NULL   COMMENT 'Prompt that was executed',
    status          VARCHAR   NOT NULL   COMMENT 'ok | error | skipped',
    output          TEXT      NULL       COMMENT 'Execution output/result',
    error           TEXT      NULL       COMMENT 'Error message if failed',
    duration_ms     INT       NULL       COMMENT 'Execution duration in ms',
    webhook_url     VARCHAR   NULL       COMMENT 'Webhook URL called',
    webhook_status  VARCHAR   NULL       COMMENT 'ok | failed | skipped',
    webhook_error   VARCHAR   NULL       COMMENT 'Webhook delivery error',
    created_at      TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Task execution history (snapshot of task at execution time)';
