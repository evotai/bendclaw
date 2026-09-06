import { padRight, relativeTime } from '../../render/format.js'
import type { SessionMeta, SessionWithText } from '../../native/index.js'
import { PREVIEW_SECTION_PREFIX, type SelectorItem } from '../selector.js'

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

/** Latest user turns shown after `Started with`. The rest stay behind `⋮`. */
const PREVIEW_LATEST_SHOWN = 3

/** Paths named in the `Changed` section before the rest become `+N`. */
const PREVIEW_CHANGED_PATHS_SHOWN = 3

/**
 * Side-pane content for one session, organised by what identifies it fastest:
 * the title and identity line, what it set out to do (`Started with`), where
 * it left off (`Latest`), and what it actually touched (`Changed`). Sections
 * are labelled entries so the pane can trim them by priority rather than by
 * position when space is short.
 *
 * `text` is absent until that session's transcript has been read, so the pane
 * starts as the identity block and fills in once it lands. Every field comes
 * from the engine's own extraction, so a row reads the same whether its text
 * arrived with the whole catalog or from focusing this one row.
 *
 * `source` already appears in the row and `cwd` in the group heading, so
 * neither is repeated here; a session from another cwd is the exception, since
 * its path is the thing that distinguishes it.
 */
export function sessionPreviewLines(
  session: SessionMeta,
  text?: SessionWithText,
  showCwd = false,
): string[] {
  const facts = [shortModel(session), `${session.turns || 0} turns`, sessionSpan(session)]
  if (showCwd) facts.push(shortenSessionCwd(session.cwd))
  const lines = [sanitizeSessionTitle(session.title), facts.filter(Boolean).join(' · ')]
  if (!text) return lines

  const prompts = text.user_prompts
  const first = text.first_prompt ?? prompts[0]
  // The opening turn is its own section; `Latest` is every turn after it so a
  // one-turn session does not say the same thing twice.
  const latest = first !== undefined && prompts[0] === first ? prompts.slice(1) : prompts
  const latestShown = latest.slice(-PREVIEW_LATEST_SHOWN)
  const sections: string[][] = []
  if (first) sections.push([`${PREVIEW_SECTION_PREFIX}Started with`, `› ${first}`])
  if (latestShown.length > 0) {
    sections.push([
      `${PREVIEW_SECTION_PREFIX}Latest`,
      ...(latest.length > latestShown.length ? ['  ⋮'] : []),
      ...latestShown.map(prompt => `› ${prompt}`),
    ])
  }
  const changed = text.changed_paths ?? []
  if (changed.length > 0) {
    const shown = changed.slice(0, PREVIEW_CHANGED_PATHS_SHOWN)
    const more = changed.length - shown.length
    const names = shown.map(path => path.split('/').pop() || path)
    sections.push([
      `${PREVIEW_SECTION_PREFIX}Changed ${changed.length} ${changed.length === 1 ? 'file' : 'files'}`,
      `  ${names.join(' · ')}${more > 0 ? ` · +${more}` : ''}`,
    ])
  }
  if (sections.length > 0) lines.push('', ...sections.flatMap((section, index) => index === 0 ? section : ['', ...section]))
  return lines
}

/**
 * `2d ago → 1h ago` for a session that ran across time, or just the last
 * activity when it was a single sitting: the span is what separates a long
 * task from a quick question.
 */
function sessionSpan(session: SessionMeta): string {
  const updated = relativeTime(session.updated_at)
  const created = relativeTime(session.created_at)
  return created && created !== updated ? `${created} → ${updated}` : updated
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
  showSource: boolean,
  text: SessionWithText | undefined,
): SelectorItem {
  // The source column only earns its space when it tells rows apart.
  const source = showSource ? `${padRight(s.source || '', 6)} ` : ''
  const title = padRight(sanitizeSessionTitle(s.title), TITLE_COLUMN_WIDTH)
  const turns = padRight(s.turns ? `${s.turns} turns` : '', 10)
  const time = relativeTime(s.updated_at)
  const cwd = otherCwd ? `  ${shortenSessionCwd(s.cwd)}` : ''
  return {
    label,
    id: s.session_id,
    detail: `${source}${title} ${turns} ${time}${cwd}`,
    // Transcript text is searchable once loaded; until then a row still matches
    // on the metadata the list already displays.
    searchText: text?.search_text
      ?? `${s.session_id} ${s.title ?? ''} ${s.cwd} ${s.source} ${s.provider ?? ''} ${s.model}`,
    contextPrefix: otherCwd ? `${shortenSessionCwd(s.cwd)} · ` : undefined,
    preview: sessionPreviewLines(s, text, otherCwd),
  }
}

/** True when rows differ by source, so the column carries information. */
function mixedSources(sessions: SessionMeta[]): boolean {
  return new Set(sessions.map(session => session.source || '')).size > 1
}

/**
 * The resume list. `sessionText` supplies whatever text has been loaded, so
 * one formatter covers the metadata-only first paint, the focused row whose
 * transcript just arrived, and the fully loaded catalog behind a typed filter.
 */
export function formatSessionItems(
  sessions: SessionMeta[],
  currentCwd: string,
  sessionText: (sessionId: string) => SessionWithText | undefined = () => undefined,
): SelectorItem[] {
  const labels = sessionIdLabels(sessions)
  const showSource = mixedSources(sessions)
  return groupedSessionItems(sessions, currentCwd, (session, otherCwd) =>
    formatSessionItem(
      session,
      labels.get(session.session_id) ?? session.session_id,
      otherCwd,
      showSource,
      sessionText(session.session_id),
    ),
  )
}

/**
 * Re-render one row against newly loaded text, preserving its identity.
 *
 * Patching the single row keeps the other rows' object identity, which the
 * selector's lowercased-search cache is keyed on: rebuilding the whole list
 * would throw that away on every focus move.
 */
export function applySessionText(item: SelectorItem, text: SessionWithText, currentCwd: string): SelectorItem {
  return {
    ...item,
    searchText: text.search_text,
    preview: sessionPreviewLines(text, text, text.cwd !== currentCwd),
  }
}
