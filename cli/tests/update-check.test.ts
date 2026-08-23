import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { checkForUpdate, fetchReleaseNotesFor, selectRelease } from '../src/update/check.js'

const originalFetch = globalThis.fetch
let home = ''

interface GhRelease {
  draft?: boolean
  prerelease?: boolean
  name?: string | null
  tag_name: string
  body?: string | null
}

function release(tag: string, opts: Partial<GhRelease> = {}): GhRelease {
  return {
    draft: false,
    prerelease: tag.includes('-beta.'),
    name: `evot ${tag.replace(/^v/, '')}`,
    tag_name: tag,
    body: null,
    ...opts,
  }
}

/** Records every request so cache behaviour is observable. */
function stubReleases(releases: GhRelease[], opts: { etag?: string } = {}) {
  const calls: Array<{ ifNoneMatch: string | null }> = []
  globalThis.fetch = (async (_url: string, init?: RequestInit) => {
    const headers = new Headers(init?.headers as HeadersInit)
    calls.push({ ifNoneMatch: headers.get('if-none-match') })
    return new Response(JSON.stringify(releases), {
      status: 200,
      headers: opts.etag ? { etag: opts.etag } : {},
    })
  }) as typeof globalThis.fetch
  return calls
}

function cachePath(): string {
  return join(home, 'update-check.json')
}

beforeEach(() => {
  home = mkdtempSync(join(tmpdir(), 'evot-check-test-'))
  process.env.EVOT_HOME = home
})

afterEach(() => {
  globalThis.fetch = originalFetch
  delete process.env.EVOT_HOME
  rmSync(home, { recursive: true, force: true })
})

describe('selectRelease', () => {
  const releases = [
    { tag: 'v2026.4.13', version: '2026.4.13', prerelease: false },
    { tag: 'v2026.4.20-beta.1', version: '2026.4.20-beta.1', prerelease: true },
  ]

  test('stable channel ignores prereleases', () => {
    expect(selectRelease(releases, { includePrerelease: false })?.version).toBe('2026.4.13')
  })

  test('prerelease channel sees the newest of either kind', () => {
    expect(selectRelease(releases, { includePrerelease: true })?.version).toBe('2026.4.20-beta.1')
  })

  test('picks the newest by version, not by list order', () => {
    const unordered = [
      { tag: 'v2026.4.13', version: '2026.4.13', prerelease: false },
      { tag: 'v2026.10.1', version: '2026.10.1', prerelease: false },
      { tag: 'v2026.5.9', version: '2026.5.9', prerelease: false },
    ]
    expect(selectRelease(unordered, { includePrerelease: false })?.version).toBe('2026.10.1')
  })

  test('returns null when nothing is eligible', () => {
    expect(selectRelease([], { includePrerelease: true })).toBeNull()
    expect(selectRelease(releases.slice(1), { includePrerelease: false })).toBeNull()
  })
})

describe('checkForUpdate', () => {
  test('reports an available stable update', async () => {
    stubReleases([release('v2026.4.20'), release('v2026.4.13')])

    const result = await checkForUpdate('2026.4.13')

    expect(result.kind).toBe('available')
    if (result.kind === 'available') expect(result.latest.version).toBe('2026.4.20')
  })

  test('skips draft releases even when they are newest', async () => {
    stubReleases([release('v2026.4.20', { draft: true }), release('v2026.4.13')])

    expect(await checkForUpdate('2026.4.13')).toEqual({ kind: 'up_to_date' })
  })

  test('keeps a stable install off prereleases', async () => {
    stubReleases([release('v2026.4.20-beta.1'), release('v2026.4.13')])

    expect(await checkForUpdate('2026.4.13')).toEqual({ kind: 'up_to_date' })
  })

  test('offers prereleases to a beta install', async () => {
    stubReleases([release('v2026.4.20-beta.1'), release('v2026.4.13')])

    const result = await checkForUpdate('2026.4.13-beta.1')

    expect(result.kind).toBe('available')
    if (result.kind === 'available') expect(result.latest.version).toBe('2026.4.20-beta.1')
  })

  test('moves a beta install onto a newer stable release', async () => {
    stubReleases([release('v2026.4.20')])

    const result = await checkForUpdate('2026.4.13-beta.1')

    expect(result.kind).toBe('available')
    if (result.kind === 'available') expect(result.latest.version).toBe('2026.4.20')
  })

  test('serves a fresh cache without a network call', async () => {
    const calls = stubReleases([release('v2026.4.20')])

    await checkForUpdate('2026.4.13')
    expect(calls).toHaveLength(1)

    const second = await checkForUpdate('2026.4.13')
    expect(calls).toHaveLength(1)
    expect(second.kind).toBe('available')
  })

  test('cache hits still carry release notes', async () => {
    stubReleases([release('v2026.4.20', { body: '### Changelog\n* faster startup' })])

    await checkForUpdate('2026.4.13')
    const cached = await checkForUpdate('2026.4.13')

    expect(cached.kind).toBe('available')
    if (cached.kind === 'available') {
      expect(cached.latest.body).toContain('faster startup')
    }
  })

  test('force bypasses the TTL but revalidates with the stored ETag', async () => {
    const calls = stubReleases([release('v2026.4.20')], { etag: 'W/"abc"' })

    await checkForUpdate('2026.4.13')
    await checkForUpdate('2026.4.13', { force: true })

    expect(calls).toHaveLength(2)
    expect(calls[0]?.ifNoneMatch).toBeNull()
    expect(calls[1]?.ifNoneMatch).toBe('W/"abc"')
  })

  test('a 304 reuses cached releases and refreshes the TTL', async () => {
    stubReleases([release('v2026.4.20')], { etag: 'W/"abc"' })
    await checkForUpdate('2026.4.13')
    const before = JSON.parse(readFileSync(cachePath(), 'utf8')) as { checked_at: number }

    globalThis.fetch = (async () => new Response(null, { status: 304 })) as typeof globalThis.fetch
    const result = await checkForUpdate('2026.4.13', { force: true })

    expect(result.kind).toBe('available')
    const after = JSON.parse(readFileSync(cachePath(), 'utf8')) as { checked_at: number }
    expect(after.checked_at).toBeGreaterThanOrEqual(before.checked_at)
  })

  test('falls back to a stale cache when the network fails', async () => {
    stubReleases([release('v2026.4.20')])
    await checkForUpdate('2026.4.13')

    // Expire the cache so the fetch path is taken, then make it fail.
    const cached = JSON.parse(readFileSync(cachePath(), 'utf8')) as Record<string, unknown>
    cached.checked_at = 0
    writeFileSync(cachePath(), JSON.stringify(cached))
    globalThis.fetch = (async () => { throw new Error('offline') }) as typeof globalThis.fetch

    const result = await checkForUpdate('2026.4.13')

    expect(result.kind).toBe('available')
    if (result.kind === 'available') {
      expect(result.latest.version).toBe('2026.4.20')
      // Marked stale so the scheduler counts the failed attempt and backs off.
      expect(result.stale).toBe(true)
    }
  })

  test('marks a rate-limited fallback stale too', async () => {
    stubReleases([release('v2026.4.20')])
    await checkForUpdate('2026.4.13')

    globalThis.fetch = (async () => new Response('rate limited', { status: 403 })) as typeof globalThis.fetch
    const result = await checkForUpdate('2026.4.13', { force: true })

    expect(result.kind).toBe('available')
    if (result.kind === 'available') expect(result.stale).toBe(true)
  })

  test('a fresh answer is never marked stale', async () => {
    stubReleases([release('v2026.4.20')])

    const network = await checkForUpdate('2026.4.13')
    const fromCache = await checkForUpdate('2026.4.13')

    expect(network.kind === 'available' && network.stale).toBeUndefined()
    // A cache hit within the TTL is current, not a degraded answer.
    expect(fromCache.kind === 'available' && fromCache.stale).toBeUndefined()
  })

  test('reports an error when there is no cache to fall back on', async () => {
    globalThis.fetch = (async () => { throw new Error('offline') }) as typeof globalThis.fetch

    const result = await checkForUpdate('2026.4.13')

    expect(result).toEqual({ kind: 'error', message: 'offline' })
  })

  test('ignores a corrupt cache file instead of throwing', async () => {
    mkdirSync(home, { recursive: true })
    writeFileSync(cachePath(), 'not json')
    stubReleases([release('v2026.4.20')])

    const result = await checkForUpdate('2026.4.13')

    expect(result.kind).toBe('available')
    expect(existsSync(cachePath())).toBe(true)
  })

  test('writes cache under EVOT_HOME, not the real home directory', async () => {
    stubReleases([release('v2026.4.20')])

    await checkForUpdate('2026.4.13')

    expect(existsSync(cachePath())).toBe(true)
  })
})

describe('fetchReleaseNotesFor', () => {
  test('returns notes for the requested version, not the newest one', async () => {
    stubReleases([
      release('v2026.4.20', { body: '### Changelog\n* newest thing' }),
      release('v2026.4.13', { body: '### Changelog\n* the running build' }),
    ])

    const info = await fetchReleaseNotesFor('2026.4.13')

    expect(info?.version).toBe('2026.4.13')
    expect(info?.body).toContain('the running build')
  })

  test('finds notes for a prerelease build', async () => {
    stubReleases([
      release('v2026.4.20'),
      release('v2026.4.13-beta.2', { body: '### Changelog\n* beta fix' }),
    ])

    const info = await fetchReleaseNotesFor('2026.4.13-beta.2')

    expect(info?.body).toContain('beta fix')
  })

  test('tolerates a leading v in the requested version', async () => {
    stubReleases([release('v2026.4.13', { body: '### Changelog\n* tagged' })])

    expect((await fetchReleaseNotesFor('v2026.4.13'))?.body).toContain('tagged')
  })

  test('serves a cached body without a network call', async () => {
    const calls = stubReleases([release('v2026.4.13', { body: '### Changelog\n* cached' })])
    await checkForUpdate('2026.4.13')
    expect(calls).toHaveLength(1)

    const info = await fetchReleaseNotesFor('2026.4.13')

    expect(calls).toHaveLength(1)
    expect(info?.body).toContain('cached')
  })

  test('returns null when the version is unknown', async () => {
    stubReleases([release('v2026.4.20')])

    expect(await fetchReleaseNotesFor('2019.1.1')).toBeNull()
  })

  test('returns null instead of throwing when offline with no cache', async () => {
    globalThis.fetch = (async () => { throw new Error('offline') }) as typeof globalThis.fetch

    expect(await fetchReleaseNotesFor('2026.4.13').catch(() => 'threw')).toBeNull()
  })
})
