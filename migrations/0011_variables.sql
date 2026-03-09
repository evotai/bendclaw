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
