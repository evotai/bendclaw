-- Channel accounts linking agents to external messaging platforms.
CREATE TABLE IF NOT EXISTS channel_accounts (
    id              VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    channel_type    VARCHAR   NOT NULL   COMMENT 'Platform type (e.g. telegram, slack)',
    account_id      VARCHAR   NOT NULL   COMMENT 'Platform-specific account identifier',
    agent_id        VARCHAR   NOT NULL   COMMENT 'FK → agent_config.agent_id',
    user_id         VARCHAR   NOT NULL   COMMENT 'Owner',
    config          VARIANT   NOT NULL DEFAULT '{}'::VARIANT COMMENT 'Platform-specific config (JSON)',
    enabled         BOOLEAN   NOT NULL DEFAULT TRUE COMMENT 'Soft-disable without deleting',
    created_at      TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Channel accounts linking agents to external messaging platforms';

-- Inbound and outbound messages for channel integrations.
CREATE TABLE IF NOT EXISTS channel_messages (
    id                  VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    channel_type        VARCHAR   NOT NULL   COMMENT 'Platform type',
    account_id          VARCHAR   NOT NULL   COMMENT 'FK → channel_accounts.account_id',
    chat_id             VARCHAR   NOT NULL   COMMENT 'Platform chat/conversation identifier',
    session_id          VARCHAR   NOT NULL   COMMENT 'FK → sessions.id',
    direction           VARCHAR   NOT NULL   COMMENT 'inbound | outbound',
    sender_id           VARCHAR   NOT NULL   COMMENT 'Platform sender identifier',
    text                VARCHAR   NOT NULL DEFAULT '' COMMENT 'Message text',
    platform_message_id VARCHAR   NOT NULL DEFAULT '' COMMENT 'Platform-assigned message ID',
    run_id              VARCHAR   NOT NULL DEFAULT '' COMMENT 'FK → runs.id',
    attachments         VARCHAR   NOT NULL DEFAULT '[]' COMMENT 'JSON array of attachments',
    created_at          TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Inbound and outbound messages for channel integrations';
