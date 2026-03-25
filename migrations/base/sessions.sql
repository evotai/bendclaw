-- Chat sessions per user.
CREATE TABLE IF NOT EXISTS sessions (
    id             VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    agent_id       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent identifier',
    user_id        VARCHAR   NOT NULL   COMMENT 'Owner',
    title          VARCHAR   NOT NULL DEFAULT '' COMMENT 'Display title',
    scope          VARCHAR   NOT NULL DEFAULT 'private' COMMENT 'private | shared',
    base_key       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Conversation key shared by a session chain',
    replaced_by_session_id VARCHAR NOT NULL DEFAULT '' COMMENT 'Next session in the chain and empty means active',
    reset_reason   VARCHAR   NOT NULL DEFAULT '' COMMENT 'Why this session was replaced',
    session_state  VARIANT   NULL       COMMENT 'Persistent session state (JSON)',
    meta           VARIANT   NULL       COMMENT 'Arbitrary session metadata (JSON)',
    created_at     TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMP NOT NULL DEFAULT NOW(),

    INVERTED INDEX idx_title(title)
) COMMENT = 'Chat sessions per user';
