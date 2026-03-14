CREATE TABLE IF NOT EXISTS knowledge (
    id VARCHAR NOT NULL,
    kind VARCHAR NOT NULL DEFAULT 'discovery',
    subject VARCHAR NOT NULL DEFAULT '',
    locator VARCHAR NOT NULL DEFAULT '',
    title VARCHAR NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    metadata VARIANT NULL,
    status VARCHAR NOT NULL DEFAULT 'active',
    confidence FLOAT NOT NULL DEFAULT 1.0,
    user_id VARCHAR NOT NULL DEFAULT '',
    first_run_id VARCHAR NOT NULL DEFAULT '',
    last_run_id VARCHAR NOT NULL DEFAULT '',
    first_seen_at TIMESTAMP NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMP NOT NULL DEFAULT NOW(),
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
) ENGINE = FUSE;

CREATE INVERTED INDEX IF NOT EXISTS idx_knowledge_kind ON knowledge(kind);
CREATE INVERTED INDEX IF NOT EXISTS idx_knowledge_subject ON knowledge(subject);
CREATE INVERTED INDEX IF NOT EXISTS idx_knowledge_locator ON knowledge(locator);
CREATE INVERTED INDEX IF NOT EXISTS idx_knowledge_status ON knowledge(status);
CREATE INVERTED INDEX IF NOT EXISTS idx_knowledge_summary ON knowledge(summary);

CREATE TABLE IF NOT EXISTS learnings (
    id VARCHAR NOT NULL,
    kind VARCHAR NOT NULL DEFAULT 'pattern',
    subject VARCHAR NOT NULL DEFAULT '',
    title VARCHAR NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    conditions VARIANT NULL,
    strategy VARIANT NULL,
    priority INT NOT NULL DEFAULT 0,
    confidence FLOAT NOT NULL DEFAULT 1.0,
    status VARCHAR NOT NULL DEFAULT 'active',
    supersedes_id VARCHAR NOT NULL DEFAULT '',
    user_id VARCHAR NOT NULL DEFAULT '',
    source_run_id VARCHAR NOT NULL DEFAULT '',
    success_count INT NOT NULL DEFAULT 0,
    failure_count INT NOT NULL DEFAULT 0,
    last_applied_at TIMESTAMP NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
) ENGINE = FUSE;

CREATE INVERTED INDEX IF NOT EXISTS idx_learnings_kind ON learnings(kind);
CREATE INVERTED INDEX IF NOT EXISTS idx_learnings_subject ON learnings(subject);
CREATE INVERTED INDEX IF NOT EXISTS idx_learnings_status ON learnings(status);
CREATE INVERTED INDEX IF NOT EXISTS idx_learnings_content ON learnings(content);
