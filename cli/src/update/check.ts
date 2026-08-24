/**
 * GitHub release query, channel selection, and disk cache.
 */

import { join } from 'path'
import { readFileSync, writeFileSync, mkdirSync } from 'fs'
import type { ReleaseInfo, CheckResult } from './types.js'
import { compareVersions, isNewer, isPrerelease } from './version.js'
import { stateDir } from './paths.js'
import { resolveUpdateProxy } from './proxy.js'

const REPO = 'evotai/evot'
const RELEASES_URL = `https://api.github.com/repos/${REPO}/releases?per_page=20`
/**
 * Unauthenticated GitHub allows 60 requests/hour/IP. A one-hour TTL keeps a
 * long-running session to roughly one request per hour while still noticing a
 * release the same day it ships.
 */
const CACHE_TTL = 60 * 60 * 1000
const REQUEST_TIMEOUT = 10_000

function cachePath(): string {
  return join(stateDir(), 'update-check.json')
}

interface CacheEntry {
  checked_at: number
  /** Serialized so a cache hit can surface release notes, not just a version. */
  releases: ReleaseInfo[]
  etag?: string
  /**
   * Last failed check, kept so `/update --status` can explain why a client is
   * not seeing new releases. Cleared on the next success.
   */
  last_error?: { at: number; message: string }
}

interface GithubRelease {
  draft: boolean
  prerelease: boolean
  name: string | null
  tag_name: string
  body: string | null
}

function toReleaseInfo(release: GithubRelease): ReleaseInfo {
  const tag = release.tag_name
  return {
    tag,
    version: tag.startsWith('v') ? tag.slice(1) : tag,
    body: release.body ?? undefined,
    prerelease: release.prerelease,
  }
}

/**
 * Newest release the given channel may install.
 *
 * Stable users only ever see stable releases. Prerelease users additionally see
 * prereleases, so a beta build is not stranded until the next stable cut, and
 * still move to a stable release once it overtakes their beta.
 */
export function selectRelease(
  releases: ReleaseInfo[],
  opts: { includePrerelease: boolean },
): ReleaseInfo | null {
  const eligible = releases.filter((r) => opts.includePrerelease || !r.prerelease)
  let best: ReleaseInfo | null = null
  for (const candidate of eligible) {
    if (!best || compareVersions(best.version, candidate.version) < 0) {
      best = candidate
    }
  }
  return best
}

async function fetchReleases(
  etag?: string,
): Promise<{ status: 'ok'; releases: ReleaseInfo[]; etag?: string } | { status: 'not_modified' } | null> {
  const headers: Record<string, string> = {
    Accept: 'application/vnd.github+json',
    'User-Agent': 'evot-cli',
    'X-GitHub-Api-Version': '2022-11-28',
  }
  if (etag) headers['If-None-Match'] = etag

  // An explicit `proxy` is required rather than relying on ambient variables:
  // Bun's fetch ignores ALL_PROXY, so the common socks-only setup would send
  // this request direct while install.sh proxied the download.
  const { fetchProxy } = await resolveUpdateProxy()
  const resp = await fetch(RELEASES_URL, {
    headers,
    signal: AbortSignal.timeout(REQUEST_TIMEOUT),
    ...(fetchProxy ? { proxy: fetchProxy.url } : {}),
  })

  if (resp.status === 304) return { status: 'not_modified' }
  if (!resp.ok) return null

  const releases = (await resp.json()) as GithubRelease[]
  if (!Array.isArray(releases)) return null

  // Draft releases are visible to authenticated maintainers and are never
  // installable: the workflow publishes assets before undrafting.
  const usable = releases
    .filter((r) => !r.draft && (r.name ?? '').startsWith('evot'))
    .map(toReleaseInfo)

  return { status: 'ok', releases: usable, etag: resp.headers.get('etag') ?? undefined }
}

/**
 * Release notes for one specific version.
 *
 * The "What's New" banner must describe the build that is actually running, not
 * whatever is newest: a beta user, or anyone who updated while a newer release
 * already existed, would otherwise be shown notes for a version they do not
 * have. Prefers the cache so a normal startup costs no network call.
 */
export async function fetchReleaseNotesFor(version: string): Promise<ReleaseInfo | null> {
  const target = version.replace(/^v/, '')
  const fromCache = readCache()?.releases.find((r) => r.version === target)
  if (fromCache?.body) return fromCache

  try {
    const result = await fetchReleases()
    if (!result || result.status !== 'ok') return fromCache ?? null
    writeCache({ checked_at: Date.now(), releases: result.releases, etag: result.etag })
    return result.releases.find((r) => r.version === target) ?? null
  } catch {
    // Cosmetic banner content: never surface a network failure to the caller.
    return fromCache ?? null
  }
}

function readCache(): CacheEntry | null {
  try {
    const parsed = JSON.parse(readFileSync(cachePath(), 'utf-8')) as CacheEntry
    if (!Array.isArray(parsed?.releases) || typeof parsed?.checked_at !== 'number') {
      return null
    }
    return parsed
  } catch {
    return null
  }
}

function writeCache(entry: CacheEntry): void {
  try {
    mkdirSync(stateDir(), { recursive: true })
    writeFileSync(cachePath(), JSON.stringify(entry, null, 2))
  } catch { /* best effort */ }
}

function decide(
  currentVersion: string,
  releases: ReleaseInfo[],
  opts: { stale?: boolean } = {},
): CheckResult {
  const latest = selectRelease(releases, {
    includePrerelease: isPrerelease(currentVersion),
  })
  const stale = opts.stale === true ? { stale: true } : {}
  if (!latest) return { kind: 'up_to_date', ...stale }
  return isNewer(currentVersion, latest.version)
    ? { kind: 'available', latest, ...stale }
    : { kind: 'up_to_date', ...stale }
}

/**
 * Check for updates.
 *
 * `force` skips the TTL so an explicit `/update` always reflects the registry,
 * but it still sends the cached ETag: GitHub answers 304 for an unchanged
 * release list, which keeps the response cheap without going stale.
 *
 * When the request fails but a cache exists, the cached answer is returned with
 * `stale: true`. That keeps a rate-limited background check from showing the
 * user an error, while still telling the scheduler the network attempt failed
 * so it can back off — an unmarked success would reset its failure budget and
 * keep hammering an endpoint that is already refusing.
 */
export async function checkForUpdate(
  currentVersion: string,
  opts?: { force?: boolean },
): Promise<CheckResult> {
  const force = opts?.force ?? false
  const cached = readCache()

  if (!force && cached && Date.now() - cached.checked_at < CACHE_TTL) {
    return decide(currentVersion, cached.releases)
  }

  const recordFailure = (message: string): void => {
    if (!cached) return
    // Keep the release list and its checked_at (so the TTL still throttles a
    // failing endpoint), but note why the refresh did not happen.
    writeCache({ ...cached, last_error: { at: Date.now(), message } })
  }

  try {
    const result = await fetchReleases(cached?.etag)
    if (!result) {
      // A rejected request (rate limit, 5xx) is not a reason to forget what we
      // already know. Same treatment as a thrown error below.
      const message = 'failed to fetch release info'
      if (cached) {
        recordFailure(message)
        return decide(currentVersion, cached.releases, { stale: true })
      }
      return { kind: 'error', message }
    }

    if (result.status === 'not_modified') {
      if (!cached) return { kind: 'error', message: 'failed to fetch release info' }
      writeCache({ ...cached, checked_at: Date.now(), last_error: undefined })
      return decide(currentVersion, cached.releases)
    }

    writeCache({ checked_at: Date.now(), releases: result.releases, etag: result.etag })
    return decide(currentVersion, result.releases)
  } catch (err: unknown) {
    const message = (err instanceof Error ? err.message : String(err)) || 'network error'
    // A stale cache still answers the question better than an error banner.
    if (cached) {
      recordFailure(message)
      return decide(currentVersion, cached.releases, { stale: true })
    }
    return { kind: 'error', message }
  }
}

/**
 * Last recorded check failure, or null when the most recent check succeeded.
 *
 * Background checks deliberately stay quiet, so this is how "why am I not being
 * offered updates?" gets answered after the fact.
 */
export function lastCheckError(): { at: number; message: string } | null {
  const recorded = readCache()?.last_error
  if (!recorded || typeof recorded.message !== 'string' || typeof recorded.at !== 'number') {
    return null
  }
  return recorded
}

export { compareVersions, isNewer, isPrerelease, parseVersion } from './version.js'
