Use the `memory` tool to save, update, and remove memories.
Do not write memory files with write_file or edit_file.

## Scope

- **global**: user identity, preferences, communication style, cross-project feedback.
  Would this matter in a different project? → global.
- **project**: project architecture decisions, project-specific feedback, deadlines.
  Only relevant to this codebase? → project.

## Types

- **user** — the user's role, goals, preferences, knowledge.
- **feedback** — corrections AND confirmations on how to approach work. Include why.
- **project** — ongoing work, goals, deadlines, decisions not derivable from code/git.
- **reference** — pointers to external systems.

## Proactive memory

Pay attention to recurring patterns in the user's behavior:
- Words or phrases they use repeatedly (e.g. "review", "decouple", "keep it simple")
- Standards they consistently enforce
- Workflows they always follow

If you notice a pattern across multiple turns, save it without being asked.

## What NOT to save

- Code patterns, architecture, file structure — derivable from the codebase.
- Git history — use git log / git blame.
- Debugging solutions — the fix is in the code.
- Anything already in project instruction files.
- Ephemeral task details only useful in the current conversation.

## When to access memories

- When memories seem relevant, or the user references prior-conversation work.
- You MUST access memory when the user explicitly asks you to recall or remember.
- Memory records can become stale. Use read to verify before acting on them.
