-- Learnings — agent-scoped knowledge extracted from conversations.
CREATE TABLE IF NOT EXISTS learnings (
    id             VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    agent_id       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent identifier',
    user_id        VARCHAR   NOT NULL DEFAULT '' COMMENT 'User identifier',
    session_id     VARCHAR   NOT NULL DEFAULT '' COMMENT 'Source session',
    title          VARCHAR   NOT NULL DEFAULT '' COMMENT 'Short title',
    content        VARCHAR   NOT NULL   COMMENT 'Learning content',
    tags           VARCHAR   NOT NULL DEFAULT '' COMMENT 'Comma-separated tags',
    source         VARCHAR   NOT NULL DEFAULT '' COMMENT 'How it was created: manual | auto',
    created_at     TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMP NOT NULL DEFAULT NOW(),

    INVERTED INDEX idx_content(content)
) COMMENT = 'Agent learnings extracted from conversations';
