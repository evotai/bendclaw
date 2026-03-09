-- Agent-level configuration: persona, behavior, and runtime settings.
CREATE TABLE IF NOT EXISTS agent_config (
    agent_id          VARCHAR   NOT NULL   COMMENT 'Agent identifier (unique per row)',
    system_prompt     TEXT      NOT NULL DEFAULT '' COMMENT 'Persistent system prompt',
    display_name      VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent display name',
    description       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Human-readable agent description',
    identity          TEXT      NOT NULL DEFAULT '' COMMENT 'Agent identity definition (PromptBuilder layer 1)',
    soul              TEXT      NOT NULL DEFAULT '' COMMENT 'Agent behavior/personality rules (PromptBuilder layer 2)',
    token_limit_total BIGINT UNSIGNED NULL COMMENT 'Total token usage limit',
    token_limit_daily BIGINT UNSIGNED NULL COMMENT 'Daily token usage limit',
    env               VARIANT   NOT NULL DEFAULT '{}'::VARIANT COMMENT 'Agent environment variables (JSON object)',
    created_at        TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Agent-level configuration and persona';
