import { describe, expect, test } from 'bun:test'
import { ResumeItemCache } from '../src/term/app/resume-items.js'
import type { SessionMeta, SessionWithText } from '../src/native/index.js'

function row(id: string): SessionMeta {
  return {
    session_id: id, title: `session ${id}`, model: 'model', thinking_level: null,
    cwd: '/work', source: 'test', turns: 1, created_at: '', updated_at: '',
  }
}

function withText(id: string, text: string): SessionWithText {
  return { ...row(id), search_text: text, user_prompts: [text] }
}

const none = () => undefined

describe('resume item memo', () => {
  test('reuses row identity when nothing changed', () => {
    const cache = new ResumeItemCache()
    const sessions = [row('a'), row('b')]
    const inputs = { sessions, cwd: '/work', textVersion: 0 }

    const first = cache.format(inputs, none)
    const second = cache.format(inputs, none)

    // Identity is the point: the selector's lowercase cache is keyed on it.
    expect(second).toBe(first)
    expect(second[0]).toBe(first[0])
  })

  test('rebuilds when the catalog snapshot is replaced', () => {
    const cache = new ResumeItemCache()
    const first = cache.format({ sessions: [row('a')], cwd: '/work', textVersion: 0 }, none)
    const second = cache.format({ sessions: [row('a')], cwd: '/work', textVersion: 0 }, none)
    expect(second).not.toBe(first)
  })

  test('rebuilds when session text arrives', () => {
    const cache = new ResumeItemCache()
    const sessions = [row('a')]
    const text = new Map([['a', withText('a', 'refactor the storage layer')]])

    const before = cache.format({ sessions, cwd: '/work', textVersion: 0 }, none)
    const after = cache.format(
      { sessions, cwd: '/work', textVersion: 1 },
      id => text.get(id),
    )

    expect(after).not.toBe(before)
    // Index 0 is the "Current cwd" group heading, so assert on the session row.
    expect(after.find(item => !item.header)?.searchText).toContain('refactor the storage layer')
  })

  test('rebuilds when the cwd changes, since grouping depends on it', () => {
    const cache = new ResumeItemCache()
    const sessions = [row('a')]
    const here = cache.format({ sessions, cwd: '/work', textVersion: 0 }, none)
    const elsewhere = cache.format({ sessions, cwd: '/other', textVersion: 0 }, none)
    expect(elsewhere).not.toBe(here)
  })

  test('the bounded preview and the full catalog do not evict each other', () => {
    // The hot path applies limit=20 and then the whole catalog on every
    // keystroke. A single-slot memo would miss on both.
    const cache = new ResumeItemCache()
    const sessions = Array.from({ length: 50 }, (_, i) => row(String(i)))
    const base = { sessions, cwd: '/work', textVersion: 0 }

    const bounded1 = cache.format({ ...base, limit: 20 }, none)
    const all1 = cache.format(base, none)
    const bounded2 = cache.format({ ...base, limit: 20 }, none)
    const all2 = cache.format(base, none)

    expect(bounded2).toBe(bounded1)
    expect(all2).toBe(all1)
    expect(bounded1).not.toBe(all1)
  })

  test('the bounded slot honours its row cap', () => {
    const cache = new ResumeItemCache()
    const sessions = Array.from({ length: 50 }, (_, i) => row(String(i)))
    const bounded = cache.format({ sessions, cwd: '/work', textVersion: 0, limit: 20 }, none)
    const all = cache.format({ sessions, cwd: '/work', textVersion: 0 }, none)
    expect(bounded.filter(item => !item.header)).toHaveLength(20)
    expect(all.filter(item => !item.header)).toHaveLength(50)
  })

  test('clear forces a rebuild', () => {
    const cache = new ResumeItemCache()
    const sessions = [row('a')]
    const inputs = { sessions, cwd: '/work', textVersion: 0 }
    const first = cache.format(inputs, none)
    cache.clear()
    expect(cache.format(inputs, none)).not.toBe(first)
  })
})
