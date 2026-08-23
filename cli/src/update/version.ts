/**
 * Version parsing and comparison.
 *
 * evot ships CalVer (`2026.4.13`, `2026.4.13.2`) with optional semver-style
 * prereleases (`2026.4.13-beta.1`), so this handles an arbitrary number of
 * numeric segments plus a dot-separated prerelease tail.
 */

export interface ParsedVersion {
  /** Numeric release segments, e.g. 2026.4.13.2 -> [2026n, 4n, 13n, 2n] */
  release: bigint[]
  /** Dot-separated prerelease identifiers, empty for a stable release. */
  prerelease: string[]
}

/**
 * Parse a version string. Returns null when the input is not a version we can
 * order, which callers treat as "older than anything" so corrupt local state
 * gets repaired instead of trusted.
 */
export function parseVersion(version: string | null | undefined): ParsedVersion | null {
  if (typeof version !== 'string') return null

  const trimmed = version.trim().replace(/^v/, '')
  if (!trimmed) return null

  // Strip build metadata: it carries no ordering information in semver.
  const withoutBuild = trimmed.split('+')[0] ?? ''
  const dashIndex = withoutBuild.indexOf('-')
  const releasePart = dashIndex === -1 ? withoutBuild : withoutBuild.slice(0, dashIndex)
  const prereleasePart = dashIndex === -1 ? '' : withoutBuild.slice(dashIndex + 1)

  const segments = releasePart.split('.')
  if (segments.length === 0) return null

  const release: bigint[] = []
  for (const segment of segments) {
    // Reject empty and non-numeric segments outright rather than coercing to
    // NaN, which is what silently broke prerelease comparison before.
    if (!/^\d+$/.test(segment)) return null
    release.push(BigInt(segment))
  }

  if (dashIndex !== -1 && !prereleasePart) return null
  const prerelease = prereleasePart ? prereleasePart.split('.') : []
  for (const identifier of prerelease) {
    if (!identifier) return null
    if (!/^[0-9A-Za-z-]+$/.test(identifier)) return null
    // Numeric identifiers must not carry leading zeros (semver §9).
    if (/^0\d+$/.test(identifier)) return null
  }

  return { release, prerelease }
}

function compareParsed(a: ParsedVersion, b: ParsedVersion): number {
  const len = Math.max(a.release.length, b.release.length)
  for (let i = 0; i < len; i++) {
    // Missing trailing segments read as 0, so 2026.4.13 === 2026.4.13.0.
    const av = a.release[i] ?? 0n
    const bv = b.release[i] ?? 0n
    if (av < bv) return -1
    if (av > bv) return 1
  }

  // A prerelease sorts before its own release: 1.0.0-beta.1 < 1.0.0
  if (a.prerelease.length === 0) return b.prerelease.length === 0 ? 0 : 1
  if (b.prerelease.length === 0) return -1

  const preLen = Math.max(a.prerelease.length, b.prerelease.length)
  for (let i = 0; i < preLen; i++) {
    const ai = a.prerelease[i]
    const bi = b.prerelease[i]
    // A shorter prerelease chain sorts first: beta < beta.1
    if (ai === undefined) return -1
    if (bi === undefined) return 1
    if (ai === bi) continue

    const aNumeric = /^\d+$/.test(ai)
    const bNumeric = /^\d+$/.test(bi)
    // Numeric identifiers compare numerically (BigInt keeps long build
    // counters exact) and always sort before alphanumeric ones.
    if (aNumeric && bNumeric) return BigInt(ai) < BigInt(bi) ? -1 : 1
    if (aNumeric !== bNumeric) return aNumeric ? -1 : 1
    return ai < bi ? -1 : 1
  }

  return 0
}

/**
 * Order two version strings. Unparseable input is treated as the older side so
 * a damaged local version never suppresses a legitimate update.
 */
export function compareVersions(v1: string | null | undefined, v2: string | null | undefined): number {
  const p1 = parseVersion(v1)
  const p2 = parseVersion(v2)
  if (!p1 && !p2) return 0
  if (!p1) return -1
  if (!p2) return 1
  return compareParsed(p1, p2)
}

/** True when `remote` is strictly newer than `current`. */
export function isNewer(current: string | null | undefined, remote: string | null | undefined): boolean {
  // An unreadable remote version is never an upgrade, even against unreadable
  // local state — offering an update we cannot name would be worse than none.
  if (!parseVersion(remote)) return false
  return compareVersions(current, remote) < 0
}

/** True when `version` carries a prerelease tail (e.g. beta builds). */
export function isPrerelease(version: string | null | undefined): boolean {
  return (parseVersion(version)?.prerelease.length ?? 0) > 0
}
