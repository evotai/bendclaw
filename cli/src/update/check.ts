/**
 * GitHub release query, version comparison, and disk cache.
 */

import { join } from 'path'
import { homedir } from 'os'
import { readFileSync, writeFileSync, mkdirSync } from 'fs'
import type { ReleaseInfo, CheckResult } from './types.js'

const REPO = 'evotai/evot'
const RELEASES_URL = `https://api.github.com/repos/${REPO}/releases`
const CACHE_DIR = join(homedir(), '.evotai')
const CACHE_PATH = join(CACHE_DIR, 'update-check.json')
const CACHE_TTL = 24 * 60 * 60 * 1000 // 24 hours

interface CacheEntry {
  checked_at: number
  latest_tag: string
  latest_version: string
}

/**
 * Fetch the latest stable evot release from GitHub.
 * Filters: !draft && !prerelease && name starts with "evot".
 */
export async function fetchLatestStable(): Promise<ReleaseInfo | null> {
  const resp = await fetch(RELEASES_URL, {
    headers: { 'Accept': 'application/vnd.github+json' },
  })
  if (!resp.ok) return null

  const releases = await resp.json() as Array<{
    draft: boolean
    prerelease: boolean
    name: string
    tag_name: string
  }>

  const stable = releases.find(
    (r) => !r.draft && !r.prerelease && r.name.startsWith('evot')
  )
  if (!stable) return null

  const tag = stable.tag_name
  const version = tag.startsWith('v') ? tag.slice(1) : tag
  return { tag, version }
}

/**
 * Fetch recent release note titles from GitHub for the startup banner.
 */
export async function fetchRecentReleaseNotes(limit: number): Promise<string[]> {
  try {
    const resp = await fetch(RELEASES_URL, {
      headers: { 'Accept': 'application/vnd.github+json' },
    })
    if (!resp.ok) return []

    const releases = await resp.json() as Array<{
      draft: boolean
      prerelease: boolean
      name: string
      tag_name: string
    }>

    return releases
      .filter((r) => !r.draft && !r.prerelease && r.name.startsWith('evot'))
      .slice(0, limit)
      .map((r) => r.name)
  } catch {
    return []
  }
}

/**
 * Compare two version strings (e.g. "2026.4.13" vs "2026.4.15").
 * Splits on "." and compares each segment numerically.
 * Returns true if remote is newer than current.
 */
export function isNewer(current: string, remote: string): boolean {
  const c = current.split('.').map(Number)
  const r = remote.split('.').map(Number)
  const len = Math.max(c.length, r.length)
  for (let i = 0; i < len; i++) {
    const cv = c[i] ?? 0
    const rv = r[i] ?? 0
    if (rv > cv) return true
    if (rv < cv) return false
  }
  return false
}

function readCache(): CacheEntry | null {
  try {
    const raw = readFileSync(CACHE_PATH, 'utf-8')
    return JSON.parse(raw) as CacheEntry
  } catch {
    return null
  }
}

function writeCache(info: ReleaseInfo): void {
  try {
    mkdirSync(CACHE_DIR, { recursive: true })
    const entry: CacheEntry = {
      checked_at: Date.now(),
      latest_tag: info.tag,
      latest_version: info.version,
    }
    writeFileSync(CACHE_PATH, JSON.stringify(entry, null, 2))
  } catch { /* best effort */ }
}

/**
 * Check for updates. Uses disk cache (24h TTL) unless force is true.
 */
export async function checkForUpdate(
  currentVersion: string,
  opts?: { force?: boolean },
): Promise<CheckResult> {
  const force = opts?.force ?? false

  // Try cache first
  if (!force) {
    const cached = readCache()
    if (cached && Date.now() - cached.checked_at < CACHE_TTL) {
      if (isNewer(currentVersion, cached.latest_version)) {
        return { kind: 'available', latest: { tag: cached.latest_tag, version: cached.latest_version } }
      }
      return { kind: 'up_to_date' }
    }
  }

  // Fetch from GitHub
  try {
    const latest = await fetchLatestStable()
    if (!latest) {
      return { kind: 'error', message: 'failed to fetch release info' }
    }

    writeCache(latest)

    if (isNewer(currentVersion, latest.version)) {
      return { kind: 'available', latest }
    }
    return { kind: 'up_to_date' }
  } catch (err: any) {
    return { kind: 'error', message: err?.message ?? 'network error' }
  }
}
