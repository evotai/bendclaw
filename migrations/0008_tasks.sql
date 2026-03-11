-- Scheduled tasks for periodic agent execution.
CREATE TABLE IF NOT EXISTS tasks (
    id                    VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    executor_instance_id  VARCHAR   NOT NULL   COMMENT 'Executor instance ID that owns this task',
    name                  VARCHAR   NOT NULL   COMMENT 'Human-readable task name',
    prompt                TEXT      NOT NULL DEFAULT '' COMMENT 'Prompt to execute on schedule',
    enabled               BOOLEAN   NOT NULL DEFAULT true COMMENT 'Whether task is active',
    status                VARCHAR   NOT NULL DEFAULT 'idle' COMMENT 'idle | running | error',
    schedule              VARIANT   NOT NULL COMMENT 'Task schedule configuration',
    delivery              VARIANT   NOT NULL DEFAULT '{"kind":"none"}'::VARIANT COMMENT 'Task result delivery target',
    last_error            TEXT      NULL      COMMENT 'Last execution error message',
    delete_after_run      BOOLEAN   NOT NULL DEFAULT false COMMENT 'Auto-delete after execution',
    run_count             INT       NOT NULL DEFAULT 0 COMMENT 'Total execution count',
    last_run_at           TIMESTAMP NULL      COMMENT 'Last execution timestamp',
    next_run_at           TIMESTAMP NULL      COMMENT 'Next scheduled execution',
    lease_token           VARCHAR   NULL      COMMENT 'Execution lease token for concurrency control',
    created_at            TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at            TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Scheduled tasks for periodic agent execution';

-- Task execution history (snapshot of task at execution time).
CREATE TABLE IF NOT EXISTS task_history (
    id                       VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    task_id                  VARCHAR   NOT NULL   COMMENT 'FK to tasks.id',
    run_id                   VARCHAR   NULL       COMMENT 'Associated run ID',
    task_name                VARCHAR   NOT NULL   COMMENT 'Task name at execution time',
    schedule                 VARIANT   NOT NULL   COMMENT 'Schedule snapshot',
    prompt                   TEXT      NOT NULL   COMMENT 'Prompt that was executed',
    status                   VARCHAR   NOT NULL   COMMENT 'ok | error | skipped',
    output                   TEXT      NULL       COMMENT 'Execution output/result',
    error                    TEXT      NULL       COMMENT 'Error message if failed',
    duration_ms              INT       NULL       COMMENT 'Execution duration in ms',
    delivery                 VARIANT   NOT NULL DEFAULT '{"kind":"none"}'::VARIANT COMMENT 'Delivery snapshot for this execution',
    delivery_status          VARCHAR   NULL       COMMENT 'ok | failed',
    delivery_error           VARCHAR   NULL       COMMENT 'Delivery error',
    executed_by_instance_id  VARCHAR   NULL       COMMENT 'Instance that executed this run',
    created_at               TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Task execution history (snapshot of task at execution time)';
