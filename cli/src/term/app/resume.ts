import { padRight, relativeTime } from '../../render/format.js'
import type { SessionMeta, SessionWithText } from '../../native/index.js'
import type { SelectorItem } from '../selector.js'

export const RESUME_SELECTOR_TITLE = 'Resume session  (Ctrl+D delete · twice)'

/**
 * Stem of the synthetic user message compaction injects ahead of a summary.
 * Shorter than the 40-character head budget used by session titles, so it also
 * matches titles an earlier release derived from that message before the
 * backend stopped doing so.
 */
export const COMPACT_SUMMARY_PREFIX = 'The conversation history before this'

/**
 * Display title for a session. Sessions compacted by an earlier release carry a
 * title built from the compaction summary boilerplate, which renders the resume
 * list as a wall of identical rows. Those read as `(compacted)` until the next
 * save recomputes them from real user turns.
 */
export function sanitizeSessionTitle(title?: string | null): string {
  if (title && title.startsWith(COMPACT_SUMMARY_PREFIX)) return '(compacted)'
  return title || '(untitled)'
}

export type SessionPrefixResolution =
  | { kind: 'matched'; session: SessionMeta }
  | { kind: 'none' }
  | { kind: 'ambiguous'; matches: SessionMeta[] }

export function shortenSessionCwd(cwd: string): string {
  const home = process.env.HOME || process.env.USERPROFILE || ''
  if (!home) return cwd
  if (cwd === home) return '~'
  return cwd.startsWith(`${home}/`) ? `~${cwd.slice(home.length)}` : cwd
}

function sessionHeader(label: string, group: string, searchOnly = false): SelectorItem {
  return { label, header: true, focusable: false, group, searchOnly }
}

function groupedSessionItems<T extends SessionMeta>(
  sessions: T[],
  currentCwd: string,
  format: (session: T, otherCwd: boolean) => SelectorItem,
): SelectorItem[] {
  const current = sessions.filter(session => session.cwd === currentCwd)
  const other = sessions.filter(session => session.cwd !== currentCwd)
  const items: SelectorItem[] = []

  if (current.length > 0) {
    items.push(sessionHeader(`Current cwd · ${shortenSessionCwd(currentCwd)}`, 'current-cwd'))
    items.push(...current.map(session => ({ ...format(session, false), group: 'current-cwd' })))
  }
  if (other.length > 0) {
    // Resume defaults to the project the user is in. Cross-project history
    // remains in the search pool, but never expands the initial picker into a
    // noisy global recents list — including when this cwd has no history yet.
    items.push(sessionHeader('Other cwd', 'other-cwd', true))
    items.push(...other.map(session => ({
      ...format(session, true),
      group: 'other-cwd',
      searchOnly: true,
    })))
  }
  return items
}

export function normalizeResumeQuery(value: string): string {
  const query = value.trim()
  const quotePairs = [["'", "'"], ['"', '"'], ['‘', '’'], ['“', '”']] as const
  for (const [open, close] of quotePairs) {
    if (query.startsWith(open) && query.endsWith(close)) {
      return query.slice(open.length, -close.length).trim()
    }
  }
  return query
}

export function isSessionIdPrefix(value: string): boolean {
  return /^[0-9a-f]{1,36}$/i.test(value)
}

export function resolveSessionByPrefix(sessions: SessionMeta[], prefix: string): SessionPrefixResolution {
  const matches = sessions.filter(s => s.session_id === prefix || s.session_id.startsWith(prefix))
  if (matches.length === 0) return { kind: 'none' }
  if (matches.length > 1) return { kind: 'ambiguous', matches }
  return { kind: 'matched', session: matches[0]! }
}

export function isResumeSelectorTitle(title: string): boolean {
  return title.startsWith('Resume session')
}

/**
 * Side-pane content for one session: a wall of near-identical titles is what
 * makes the resume list hard to read, so the pane shows the full title, one
 * compact identity line, and then the user's own turns — the fastest way to
 * recognize which conversation this was.
 *
 * `source` already appears in the row and `cwd` in the group heading, so
 * neither is repeated here; a session from another cwd is the exception, since
 * its path is the thing that distinguishes it.
 */
export function sessionPreviewLines(
  session: SessionMeta,
  opts: { userPrompts?: string[]; showCwd?: boolean } = {},
): string[] {
  const facts = [shortModel(session), `${session.turns || 0} turns`, relativeTime(session.updated_at)]
  if (opts.showCwd) facts.push(shortenSessionCwd(session.cwd))

  const lines = [sanitizeSessionTitle(session.title), facts.filter(Boolean).join(' · ')]
  // Prompts arrive with the async full-text load, so the pane starts as the
  // identity block and fills in once they land.
  const prompts = opts.userPrompts ?? []
  if (prompts.length > 0) {
    lines.push('')
    lines.push(...prompts.map(prompt => `› ${prompt}`))
  }
  return lines
}

/**
 * Model without its provider prefix. The pane has one line for identity and a
 * bare model name is what distinguishes sessions; `anthropic:` in front of
 * every Claude row spends columns without adding a distinction.
 */
function shortModel(session: SessionMeta): string {
  return session.model || session.provider || ''
}

/**
 * Columns the list spends on a title. The side pane carries the full title, so
 * the row only needs enough of it to tell neighbouring sessions apart — the
 * saved columns keep turn count and timestamp on screen next to the pane.
 */
const TITLE_COLUMN_WIDTH = 44

function commonPrefixLength(left: string, right: string): number {
  const end = Math.min(left.length, right.length)
  let index = 0
  while (index < end && left[index] === right[index]) index++
  return index
}

/**
 * UUIDv7 ids created close together often share their first eight characters.
 * Keep the familiar short label when it is unique, and extend only colliding
 * labels far enough to make every visible row distinguishable and resumable.
 */
function sessionIdLabels(sessions: SessionMeta[]): Map<string, string> {
  const sorted = sessions.map(session => session.session_id).sort()
  const labels = new Map<string, string>()
  for (let index = 0; index < sorted.length; index++) {
    const id = sorted[index]!
    const previous = sorted[index - 1] ?? ''
    const next = sorted[index + 1] ?? ''
    const uniqueLength = Math.max(
      8,
      commonPrefixLength(id, previous) + 1,
      commonPrefixLength(id, next) + 1,
    )
    labels.set(id, id.slice(0, uniqueLength))
  }
  return labels
}

function formatSessionItem(
  s: SessionMeta,
  label: string,
  otherCwd: boolean,
  searchText: string,
  preview: string[],
): SelectorItem {
  const source = padRight(s.source || '', 6)
  const title = padRight(sanitizeSessionTitle(s.title), TITLE_COLUMN_WIDTH)
  const turns = padRight(s.turns ? `[${s.turns} turns]` : '', 12)
  const time = relativeTime(s.updated_at)
  const cwd = otherCwd ? `  ${shortenSessionCwd(s.cwd)}` : ''
  return {
    label,
    id: s.session_id,
    detail: `${source} ${title} ${turns} ${time}${cwd}`,
    searchText,
    contextPrefix: otherCwd ? `${shortenSessionCwd(s.cwd)} · ` : undefined,
    preview,
  }
}

export function formatSessionItems(sessions: SessionMeta[], currentCwd: string): SelectorItem[] {
  const labels = sessionIdLabels(sessions)
  return groupedSessionItems(sessions, currentCwd, (session, otherCwd) =>
    formatSessionItem(
      session,
      labels.get(session.session_id) ?? session.session_id,
      otherCwd,
      `${session.session_id} ${session.title ?? ''} ${session.cwd} ${session.source} ${session.provider ?? ''} ${session.model}`,
      sessionPreviewLines(session, { showCwd: otherCwd }),
    ),
  )
}

export function formatSessionWithTextItems(items: SessionWithText[], currentCwd: string): SelectorItem[] {
  const labels = sessionIdLabels(items)
  return groupedSessionItems(items, currentCwd, (session, otherCwd) =>
    formatSessionItem(
      session,
      labels.get(session.session_id) ?? session.session_id,
      otherCwd,
      session.search_text,
      sessionPreviewLines(session, { userPrompts: session.user_prompts, showCwd: otherCwd }),
    ),
  )
}
