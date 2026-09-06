import { describe, expect, test } from 'bun:test'
import { ResumeSessionCache } from '../src/term/app/resume-cache.js'
import type { SessionMeta, SessionWithText } from '../src/native/index.js'

function row(id: string): SessionMeta {
  return { session_id: id, title: id, model: 'model', thinking_level: null, cwd: '/work', source: 'test', turns: 1, created_at: '', updated_at: '' }
}
function fixture() {
  const metadata: Array<{ limit: number; result: ReturnType<typeof Promise.withResolvers<SessionMeta[]>> }> = []
  const text: Array<ReturnType<typeof Promise.withResolvers<SessionWithText[]>>> = []
  const published: SessionMeta[][] = []
  const cache = new ResumeSessionCache({
    listSessions: limit => {
      const result = Promise.withResolvers<SessionMeta[]>()
      metadata.push({ limit, result })
      return result.promise
    },
    listSessionsWithText: () => {
      const result = Promise.withResolvers<SessionWithText[]>()
      text.push(result)
      return result.promise
    },
  }, rows => published.push(rows))
  return { cache, metadata, text, published }
}

describe('resume cache ownership', () => {
  test('coalesces loads and does not replace full metadata with a late preview', async () => {
    const f = fixture()
    const preview = f.cache.preview()
    expect(f.cache.preview()).toBe(preview)
    const full = f.cache.all()
    expect(f.cache.all()).toBe(full)
    const rows = Array.from({ length: 30 }, (_, index) => row(String(index)))
    f.metadata[1]!.result.resolve(rows)
    await full
    f.metadata[0]!.result.resolve([row('stale-preview')])
    expect(await preview).toEqual(rows.slice(0, 20))
    expect(f.cache.metadata).toEqual(rows)
    expect(f.cache.complete).toBe(true)
  })

  test('late metadata cannot downgrade completed full-text enrichment', async () => {
    const f = fixture()
    const metadata = f.cache.all()
    const text = f.cache.text()
    const rich = [{ ...row('a'), search_text: 'body', user_prompts: ['prompt'] }]
    f.text[0]!.resolve(rich)
    await text
    f.metadata[0]!.result.resolve([row('old')])
    expect(await metadata).toEqual(rich)
    expect(f.cache.metadata).toEqual(rich)
    expect(f.cache.withText).toEqual(rich)
    expect(f.published).toEqual([rich])
  })

  test('invalidation rejects late results without publishing or clearing a newer load', async () => {
    const f = fixture()
    const old = f.cache.all()
    f.cache.invalidate()
    const current = f.cache.all()
    f.metadata[0]!.result.resolve([row('old')])
    expect(await old).toEqual([])
    expect(f.cache.all()).toBe(current)
    expect(f.published).toEqual([])
    f.metadata[1]!.result.resolve([row('new')])
    expect(await current).toEqual([row('new')])
  })

  test('empty preview is not authoritative while an empty full result is', async () => {
    const f = fixture()
    const first = f.cache.preview()
    f.metadata[0]!.result.resolve([])
    await first
    const second = f.cache.preview()
    expect(f.metadata).toHaveLength(2)
    f.metadata[1]!.result.resolve([])
    await second
    f.cache.replace([], true)
    expect(await f.cache.preview()).toEqual([])
    expect(f.metadata).toHaveLength(2)
  })

  test('removal preserves enriched text and completeness, invalidating old loads', async () => {
    const f = fixture()
    const text = f.cache.text()
    expect(f.cache.text()).toBe(text)
    const rows = ['a', 'b'].map(id => ({ ...row(id), search_text: id, user_prompts: [id] }))
    f.text[0]!.resolve(rows)
    await text
    f.cache.remove('a')
    expect(f.cache.metadata).toEqual([rows[1]])
    expect(f.cache.withText).toEqual([rows[1]])
    expect(f.cache.complete).toBe(true)
  })

  test('disposal rejects late results and prevents new requests or replacements', async () => {
    const f = fixture()
    const pending = f.cache.all()
    f.cache.dispose()
    f.metadata[0]!.result.resolve([row('late')])
    expect(await pending).toEqual([])
    expect(f.published).toEqual([])
    f.cache.replace([row('replaced')], true)
    f.cache.remove('missing', [row('fallback')])
    expect(await f.cache.preview()).toEqual([])
    expect(await f.cache.all()).toEqual([])
    expect(await f.cache.text()).toEqual([])
    expect(f.metadata).toHaveLength(1)
    expect(f.text).toHaveLength(0)
    expect(f.cache.metadata).toBeNull()
  })

  test('failed request releases ownership so a retry can proceed', async () => {
    const f = fixture()
    const first = f.cache.all()
    f.metadata[0]!.result.reject(new Error('offline'))
    await expect(first).rejects.toThrow('offline')
    const retry = f.cache.all()
    f.metadata[1]!.result.resolve([row('ok')])
    expect(await retry).toEqual([row('ok')])
  })
})
