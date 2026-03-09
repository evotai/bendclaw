-- Agent config version history for rollback and audit.
CREATE TABLE IF NOT EXISTS agent_config_versions (
    id                VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    agent_id          VARCHAR   NOT NULL   COMMENT 'FK → agent_config.agent_id',
    version           UINT32    NOT NULL   COMMENT 'Monotonically increasing version number',
    label             VARCHAR   NOT NULL DEFAULT '' COMMENT 'Human label (e.g. stable, v1.2)',
    `stage`           VARCHAR   NOT NULL DEFAULT 'published' COMMENT 'draft | published',
    system_prompt     VARCHAR   NOT NULL DEFAULT '' COMMENT 'System prompt snapshot',
    display_name      VARCHAR   NOT NULL DEFAULT '' COMMENT 'Display name snapshot',
    description       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Description snapshot',
    identity          TEXT      NOT NULL DEFAULT '' COMMENT 'Identity snapshot',
    soul              TEXT      NOT NULL DEFAULT '' COMMENT 'Soul snapshot',
    token_limit_total BIGINT UNSIGNED NULL COMMENT 'Total token limit snapshot',
    token_limit_daily BIGINT UNSIGNED NULL COMMENT 'Daily token limit snapshot',
    notes             VARCHAR   NOT NULL DEFAULT '' COMMENT 'Change notes',
    created_at        TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Agent config version history';
