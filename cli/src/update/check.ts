/**
 * Latest-release lookup via auto.evot.ai, plus a disk cache.
 *
 * The curl|sh installer and the CLI share one endpoint so a NAT that 403s
 * GitHub still sees new releases. The server looks GitHub up with a token;
 * this client never talks to api.github.com.
 */

import { join } from 'path'
import { readFileSync, writeFileSync, mkdirSync } from 'fs'
import type { ReleaseInfo, CheckResult } from './types.js'
import { compareVersions, isNewer, isPrerelease } from './version.js'
import { stateDir } from './paths.js'

const LATEST_URL = 'https://auto.evot.ai/install/latest'
/**
 * A ten-minute TTL keeps checks responsive while coalescing requests from
 * multiple sessions through the shared on-disk cache.
 */
const CACHE_TTL = 10 * 60 * 1000
const REQUEST_TIMEOUT = 10_000

function cachePath(): string {
  return join(stateDir(), 'update-check.json')
}

interface CacheEntry {
  checked_at: number
  releases: ReleaseInfo[]
  last_error?: { at: number; message: string }
}

function parseTag(raw: string): ReleaseInfo | null {
  const tag = raw.trim()
  if (!/^v\d/.test(tag) || /\s/.test(tag)) return null
  const version = tag.slice(1)
  return { tag, version, prerelease: isPrerelease(version) }
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

async function fetchLatest(): Promise<ReleaseInfo> {
  const resp = await fetch(LATEST_URL, { signal: AbortSignal.timeout(REQUEST_TIMEOUT) })
  if (!resp.ok) throw new Error(`failed to fetch release info: HTTP ${resp.status}`)
  const info = parseTag(await resp.text())
  if (!info) throw new Error('failed to fetch release info: unexpected tag')
  return info
}

/**
 * Release notes for one specific version.
 *
 * The "What's New" banner must describe the build that is actually running, not
 * whatever is newest. The latest-tag endpoint does not carry notes, so this
 * only returns a cached body when one exists.
 */
export async function fetchReleaseNotesFor(version: string): Promise<ReleaseInfo | null> {
  const target = version.replace(/^v/, '')
  return readCache()?.releases.find((r) => r.version === target) ?? null
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
 * `force` skips the TTL so an explicit `/update` always reflects the registry.
 *
 * When the request fails but a cache exists, the cached answer is returned with
 * `stale: true`. That keeps a rate-limited background check from showing the
 * user an error, while still telling the scheduler the network attempt failed
 * so it can back off.
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
    writeCache({ ...cached, last_error: { at: Date.now(), message } })
  }

  try {
    const latest = await fetchLatest()
    writeCache({ checked_at: Date.now(), releases: [latest] })
    return decide(currentVersion, [latest])
  } catch (err: unknown) {
    const message = (err instanceof Error ? err.message : String(err)) || 'network error'
    if (cached) {
      recordFailure(message)
      return decide(currentVersion, cached.releases, { stale: true })
    }
    return { kind: 'error', message }
  }
}

/**
 * Last recorded check failure, or null when the most recent check succeeded.
 */
export function lastCheckError(): { at: number; message: string } | null {
  const recorded = readCache()?.last_error
  if (!recorded || typeof recorded.message !== 'string' || typeof recorded.at !== 'number') {
    return null
  }
  return recorded
}

export { compareVersions, isNewer, isPrerelease, parseVersion } from './version.js'
