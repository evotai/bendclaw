# 2026-03-25 Schema Changelog

This change removes executable compatibility migrations.
`migrations/base/*.sql` is the only schema baseline.

External environments must be updated to the current schema before deploying this version of bendclaw.

## Schema Diff

### `runs`

```diff
 CREATE TABLE runs (
     id             VARCHAR   NOT NULL,
     session_id     VARCHAR   NOT NULL,
     agent_id       VARCHAR   NOT NULL DEFAULT '',
     user_id        VARCHAR   NOT NULL,
+    kind           VARCHAR   NOT NULL DEFAULT 'user_turn' COMMENT 'user_turn | session_checkpoint',
     parent_run_id  VARCHAR   NOT NULL DEFAULT '',
     node_id        VARCHAR   NOT NULL DEFAULT '',
     status         VARCHAR   NOT NULL DEFAULT 'RUNNING',
     input          VARCHAR   NOT NULL DEFAULT '',
     output         VARCHAR   NOT NULL DEFAULT '',
     error          VARCHAR   NOT NULL DEFAULT '',
     metrics        VARCHAR   NOT NULL DEFAULT '',
     stop_reason    VARCHAR   NOT NULL DEFAULT '',
+    checkpoint_through_run_id VARCHAR NOT NULL DEFAULT '' COMMENT 'For session_checkpoint rows: last user_turn run_id covered by the summary',
     iterations     UINT32    NOT NULL DEFAULT 0,
     created_at     TIMESTAMP NOT NULL DEFAULT NOW(),
     updated_at     TIMESTAMP NOT NULL DEFAULT NOW()
 )
```

### `sessions`

```diff
 CREATE TABLE sessions (
     id             VARCHAR   NOT NULL,
     agent_id       VARCHAR   NOT NULL DEFAULT '',
     user_id        VARCHAR   NOT NULL,
     title          VARCHAR   NOT NULL DEFAULT '',
     scope          VARCHAR   NOT NULL DEFAULT 'private',
+    base_key       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Conversation key shared by a session chain',
+    replaced_by_session_id VARCHAR NOT NULL DEFAULT '' COMMENT 'Next session in the chain and empty means active',
+    reset_reason   VARCHAR   NOT NULL DEFAULT '' COMMENT 'Why this session was replaced',
     session_state  VARIANT   NULL,
     meta           VARIANT   NULL,
     created_at     TIMESTAMP NOT NULL DEFAULT NOW(),
     updated_at     TIMESTAMP NOT NULL DEFAULT NOW()
 )
```

## Runtime Meaning

- `runs.kind`: distinguishes normal user turns from session checkpoints.
- `runs.checkpoint_through_run_id`: marks the last user turn covered by a checkpoint.
- `sessions.base_key`: shared logical conversation key for a session chain.
- `sessions.replaced_by_session_id`: explicit pointer to the next session; empty means active.
- `sessions.reset_reason`: records why a session was replaced, such as `/new` or `/clear`.
