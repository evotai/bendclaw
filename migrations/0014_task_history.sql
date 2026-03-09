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
