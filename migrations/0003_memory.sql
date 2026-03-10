-- Long-term memory store with vector + full-text search.
CREATE TABLE IF NOT EXISTS memories (
    id          VARCHAR      NOT NULL   COMMENT 'ULID primary key',
    user_id     VARCHAR      NOT NULL   COMMENT 'Owner',
    scope       VARCHAR      NOT NULL DEFAULT 'user' COMMENT 'user | shared | session',
    session_id  VARCHAR      NULL       COMMENT 'Set when scope = session',
    key         VARCHAR      NOT NULL   COMMENT 'Human-readable identifier',
    content     TEXT         NOT NULL   COMMENT 'Memory content (Markdown)',
    embedding   VECTOR(1024) NULL       COMMENT 'Vector embedding for semantic search',
    created_at  TIMESTAMP    NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMP    NOT NULL DEFAULT NOW(),

    VECTOR INDEX idx_embedding(embedding) DISTANCE='cosine',
    INVERTED INDEX idx_content(content)
) COMMENT = 'Long-term memory with vector + FTS search';

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
