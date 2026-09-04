import { describe, expect, test } from 'bun:test'
import {
  RESUME_SELECTOR_TITLE,
  COMPACT_SUMMARY_PREFIX,
  formatSessionItems,
  formatSessionWithTextItems,
  isResumeSelectorTitle,
  isSessionIdPrefix,
  normalizeResumeQuery,
  resolveSessionByPrefix,
  sanitizeSessionTitle,
  sessionPreviewLines,
} from '../src/term/app/resume.js'
import { createSelectorState, selectorExpandItems, selectorType } from '../src/term/selector.js'
import type { SessionMeta, SessionWithText } from '../src/native/index.js'

describe('repl resume helpers', () => {
  const sessions: SessionMeta[] = [
    { session_id: 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa', title: 'cwd session', cwd: '/work', source: 'local', model: 'm1', turns: 3, updated_at: Date.now() } as any,
    { session_id: 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb', title: 'other session', cwd: '/other', model: 'm2', updated_at: Date.now() } as any,
  ]

  test('isSessionIdPrefix accepts hex prefix only', () => {
    expect(isSessionIdPrefix('abc123')).toBe(true)
    expect(isSessionIdPrefix('ABCDEF')).toBe(true)
    expect(isSessionIdPrefix('not-hex')).toBe(false)
    expect(isSessionIdPrefix('')).toBe(false)
  })

  test('normalizeResumeQuery accepts plain and quoted search terms', () => {
    expect(normalizeResumeQuery('NEBULA-4729')).toBe('NEBULA-4729')
    expect(normalizeResumeQuery(" 'NEBULA-4729' ")).toBe('NEBULA-4729')
    expect(normalizeResumeQuery('"NEBULA-4729"')).toBe('NEBULA-4729')
    expect(normalizeResumeQuery('“NEBULA-4729”')).toBe('NEBULA-4729')
    expect(normalizeResumeQuery("'unclosed")).toBe("'unclosed")
  })

  test('resolveSessionByPrefix matches unique prefix', () => {
    const resolved = resolveSessionByPrefix(sessions, 'aaaaaaaa')
    expect(resolved.kind).toBe('matched')
    if (resolved.kind === 'matched') expect(resolved.session).toBe(sessions[0])
  })

  test('resolveSessionByPrefix reports none', () => {
    expect(resolveSessionByPrefix(sessions, 'cccc').kind).toBe('none')
  })

  test('resolveSessionByPrefix reports ambiguous prefixes', () => {
    const ambiguous = [
      { session_id: 'abc11111-1111-4111-8111-aaaaaaaaaaaa' },
      { session_id: 'abc22222-2222-4222-8222-bbbbbbbbbbbb' },
    ] as SessionMeta[]
    const resolved = resolveSessionByPrefix(ambiguous, 'abc')
    expect(resolved.kind).toBe('ambiguous')
  })

  test('formatSessionItems groups current cwd before other cwd', () => {
    const items = formatSessionItems(sessions, '/work')
    expect(items.map(item => item.label)).toEqual([
      'Current cwd · /work',
      'aaaaaaaa',
      'Other cwd',
      'bbbbbbbb',
    ])
    expect(items[0]).toMatchObject({ header: true, focusable: false, group: 'current-cwd' })
    expect(items[1]).toMatchObject({ id: sessions[0]!.session_id, group: 'current-cwd' })
    expect(items[1]!.detail).toContain('local ')
    expect(items[1]!.detail).toContain('cwd session')
    expect(items[1]!.detail).toContain('[3 turns]')
    expect(items[1]!.searchText).toContain('/work')
    expect(items[3]).toMatchObject({ id: sessions[1]!.session_id, group: 'other-cwd' })
    expect(items[3]!.detail).toContain('/other')
  })

  test('formatSessionItems marks other cwd as search-only when current cwd has sessions', () => {
    const items = formatSessionItems(sessions, '/work')
    expect(items[0]!.searchOnly).toBeFalsy()
    expect(items[1]!.searchOnly).toBeFalsy()
    expect(items[2]).toMatchObject({ label: 'Other cwd', searchOnly: true })
    expect(items[3]).toMatchObject({ id: sessions[1]!.session_id, searchOnly: true })
  })

  test('resume defaults to current cwd but searches other cwd', () => {
    const items = formatSessionItems(sessions, '/work')
    let state = createSelectorState(RESUME_SELECTOR_TITLE, items, items)

    expect(state.items.map(item => item.label)).toEqual([
      'Current cwd · /work',
      'aaaaaaaa',
    ])
    for (const char of 'other session') state = selectorType(state, char)
    expect(state.items.map(item => item.label)).toEqual(['Other cwd', 'bbbbbbbb'])
  })

  test('formatSessionItems extends colliding UUIDv7 prefixes', () => {
    const colliding = [
      { ...sessions[0]!, session_id: '01a069e9-4c86-77c0-8578-0f959cb03b53' },
      { ...sessions[0]!, session_id: '01a069e9-4d40-7883-a488-480d11386234' },
      { ...sessions[0]!, session_id: '01a069f0-1234-7000-8000-000000000000' },
    ]
    const labels = formatSessionItems(colliding, '/work')
      .filter(item => !item.header)
      .map(item => item.label)

    expect(labels).toEqual(['01a069e9-4c', '01a069e9-4d', '01a069f0'])
    expect(new Set(labels).size).toBe(labels.length)
  })

  test('formatSessionItems keeps other cwd searchable when current cwd has no sessions', () => {
    const items = formatSessionItems(sessions, '/missing')
    expect(items[0]).toMatchObject({ label: 'Other cwd', header: true, searchOnly: true })
    expect(items.filter(item => !item.header)).toHaveLength(2)

    let state = createSelectorState(RESUME_SELECTOR_TITLE, items, items)
    expect(state.items).toHaveLength(0)
    for (const char of 'cwd session') state = selectorType(state, char)
    expect(state.items.map(item => item.label)).toEqual(['Other cwd', 'aaaaaaaa'])
  })

  test('formatSessionWithTextItems uses full search text and matching groups', () => {
    const withText = sessions.map((session, index) => ({
      ...session,
      search_text: index === 0 ? 'current full text body' : 'other full text body',
    })) as SessionWithText[]
    const items = formatSessionWithTextItems(withText, '/work')
    expect(items[1]!.searchText).toBe('current full text body')
    expect(items[3]!.searchText).toBe('other full text body')
    expect(items[3]!.contextPrefix).toBe('/other · ')
  })

  test('session rows carry a preview for the side pane', () => {
    const items = formatSessionItems(sessions, '/work')
    expect(items[1]!.preview?.[0]).toBe('cwd session')
    expect(items[0]!.preview).toBeUndefined()
  })

  test('sessionPreviewLines states identity on one line', () => {
    const lines = sessionPreviewLines(sessions[0]!)
    expect(lines[0]).toBe('cwd session')
    expect(lines[1]).toBe('m1 · 3 turns · just now')
    // cwd lives in the group heading and source in the row, so neither is
    // repeated for a session in the current directory.
    expect(lines.join('\n')).not.toContain('/work')
    expect(lines.join('\n')).not.toContain('local')
  })

  test('sessionPreviewLines adds cwd only for sessions from another directory', () => {
    expect(sessionPreviewLines(sessions[1]!, { showCwd: true })[1]).toEndWith(' · /other')
  })

  test('sessionPreviewLines lists user turns under a blank separator', () => {
    const lines = sessionPreviewLines(sessions[0]!, {
      userPrompts: ['fix the retry budget', 'now add a test'],
    })

    expect(lines[2]).toBe('')
    expect(lines.slice(3)).toEqual(['› fix the retry budget', '› now add a test'])
  })

  test('sessionPreviewLines omits the separator when no turns are loaded yet', () => {
    expect(sessionPreviewLines(sessions[0]!, { userPrompts: [] })).toHaveLength(2)
  })

  test('sessionPreviewLines drops the provider prefix from the model', () => {
    const session = { ...sessions[0]!, provider: 'anthropic', model: 'claude' }
    expect(sessionPreviewLines(session)[1]).toStartWith('claude · ')
  })

  test('sessionPreviewLines falls back to the provider when no model is recorded', () => {
    const session = { ...sessions[0]!, provider: 'anthropic', model: '' }
    expect(sessionPreviewLines(session)[1]).toBe('anthropic · 3 turns · just now')
  })

  test('resume title shows the portable Ctrl+D delete shortcut', () => {
    expect(RESUME_SELECTOR_TITLE).toBe('Resume session  (Ctrl+D delete · twice)')
    expect(isResumeSelectorTitle(RESUME_SELECTOR_TITLE)).toBe(true)
    expect(isResumeSelectorTitle('Select model')).toBe(false)
  })

  test('sanitizeSessionTitle hides compaction boilerplate titles', () => {
    // Full message form, as persisted by an older release.
    expect(sanitizeSessionTitle(
      'The conversation history before this point was compacted into the following summary:\n\nstuff',
    )).toBe('(compacted)')
    // Title already truncated mid-prefix by the 40-char head budget.
    expect(sanitizeSessionTitle('The conversation history before this poi.. … continue')).toBe('(compacted)')
    expect(COMPACT_SUMMARY_PREFIX).toBe('The conversation history before this')
  })

  test('sanitizeSessionTitle passes through real titles and fills empties', () => {
    expect(sanitizeSessionTitle('fix the resume selector')).toBe('fix the resume selector')
    expect(sanitizeSessionTitle('')).toBe('(untitled)')
    expect(sanitizeSessionTitle(undefined)).toBe('(untitled)')
    expect(sanitizeSessionTitle(null)).toBe('(untitled)')
  })
})
