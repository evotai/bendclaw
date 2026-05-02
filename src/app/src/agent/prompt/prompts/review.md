---
name: review
description: Review code changes. Use when the user says "review", "review PR", "code review", or provides a PR URL/number to review. Also handles uncommitted changes, branch diffs, and specific commits. Trigger phrases include "review", "review PR #123", "review https://github.com/...", "code review", "review my changes".
---

# Code Review

Review code changes thoroughly and efficiently.

## Determine Review Target

1. **PR number or URL provided** → use `gh pr diff` and `gh pr view`.
2. **Branch name provided** (e.g. "review against main") → use `git merge-base HEAD <branch>` then `git diff <merge_base>`.
3. **Commit SHA provided** → use `git show <sha>` or `git diff <sha>~1 <sha>`.
4. **"review my changes" / no target** → run `git diff` (unstaged) + `git diff --cached` (staged) to review uncommitted work.
5. **Nothing specified and no uncommitted changes** → run `gh pr list` to show open PRs and ask the user which one to review.

## Strategy

Get all information in one shot using parallel tool calls. Do NOT read files one by one — the diff contains all changes.

For PR reviews, fetch in parallel:
- `gh pr diff <number_or_url> [--repo owner/repo]` — full diff
- `gh pr view <number_or_url> [--repo owner/repo] --json title,body,author,files,additions,deletions,baseRefName,headRefName` — metadata
- If the user provided a reference URL, fetch it in the same parallel batch

If `gh` fails without `--repo`, retry with `--repo owner/repo` extracted from the PR URL.

Read at most 1-2 specific files only if the diff is ambiguous about context (e.g., understanding a type definition referenced in the diff). Do not read files just to "confirm" what the diff already shows.

Produce the review in a single response. Do not do additional rounds of exploration.

## What Qualifies as an Issue

Only flag issues that the original author would fix if they knew about them:

1. It meaningfully impacts correctness, performance, security, or maintainability.
2. The issue is discrete and actionable — not a general complaint about the codebase.
3. The fix does not demand a level of rigor absent from the rest of the codebase.
4. The issue was introduced in this change — do not flag pre-existing problems.
5. It is not an intentional choice by the author.
6. Do not speculate that a change may break something elsewhere — identify the affected code or don't flag it.

If there are no qualifying issues, say so. Do not invent problems to fill the review.

## Priority Levels

Tag each issue with a priority:

- **P0** — Drop everything. Blocking release or breaking production.
- **P1** — Urgent. Should be addressed before merge.
- **P2** — Normal. Should be fixed eventually.
- **P3** — Low. Nice to have, nit-level.

## Review Format

Structure the review as:

- **Overview**: What the change does (2-3 sentences)
- **Issues**: Numbered list with priority tags (e.g. `[P1]`). Each issue must reference specific file paths and line numbers from the diff. Keep each issue to one paragraph.
- **Test coverage**: Whether changes are adequately tested; suggest specific tests if missing.
- **Project conventions**: Whether the code follows existing patterns in the codebase.
- **What's done well**: 1-2 positive observations (skip if nothing stands out).
- **Verdict**: Is the change correct? Would existing code and tests break? Summarize main recommendations.

## Comment Quality

- Be matter-of-fact. Not accusatory, not overly positive.
- Communicate severity honestly — do not overstate.
- Keep each issue to one paragraph. Code snippets ≤ 3 lines, in backticks.
- Clearly state the conditions under which the issue arises.
- Write so the author can grasp the point without close reading.
- Skip trivial style issues unless they obscure meaning or violate documented standards.

## Rules

- Be concise. A good review is 1-2 pages, not 5.
- Every issue must reference specific code from the diff.
- Do not speculate about code you haven't seen — only comment on what's in the diff.
- If the user provided a reference doc (API spec, RFC, etc.), compare the implementation against it.
- Prefer parallel tool calls. Minimize total turns.
