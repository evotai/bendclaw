-- Skills (tools) available to agents.
CREATE TABLE IF NOT EXISTS skills (
    name        VARCHAR      NOT NULL   COMMENT 'Unique skill identifier (slug)',
    version     VARCHAR      NOT NULL DEFAULT '0.0.0' COMMENT 'Semver',
    scope       VARCHAR      NOT NULL DEFAULT 'agent' COMMENT 'agent | user | global',
    source      VARCHAR      NOT NULL DEFAULT 'agent' COMMENT 'local | hub | github | agent',
    agent_id    VARCHAR      NULL       COMMENT 'Owning agent (NULL for global)',
    user_id     VARCHAR      NULL       COMMENT 'Owning user (NULL for global)',
    description VARCHAR      NOT NULL DEFAULT '' COMMENT 'Human-readable summary',
    timeout     INT UNSIGNED NOT NULL DEFAULT 30 COMMENT 'Execution timeout in seconds',
    executable  BOOLEAN      NOT NULL DEFAULT FALSE COMMENT 'Has a runnable script',
    enabled     BOOLEAN      NOT NULL DEFAULT TRUE COMMENT 'Soft-disable without deleting',
    content     VARCHAR      NOT NULL DEFAULT '' COMMENT 'SKILL.md body (prompt + docs)',
    sha256      VARCHAR      NOT NULL DEFAULT '' COMMENT 'Content checksum for sync',
    updated_at  TIMESTAMP    NOT NULL DEFAULT NOW()
) COMMENT = 'Skills available to agents';

-- Files bundled with a skill.
CREATE TABLE IF NOT EXISTS skill_files (
    skill_name VARCHAR   NOT NULL   COMMENT 'FK → skills.name',
    agent_id   VARCHAR   NULL       COMMENT 'Matches parent skill agent_id',
    user_id    VARCHAR   NULL       COMMENT 'Matches parent skill user_id',
    file_path  VARCHAR   NOT NULL   COMMENT 'Relative path within skill dir',
    file_body  VARCHAR   NOT NULL DEFAULT '' COMMENT 'File content',
    sha256     VARCHAR   NOT NULL DEFAULT '' COMMENT 'File checksum',
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Files bundled with a skill';
