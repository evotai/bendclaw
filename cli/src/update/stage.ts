/**
 * Background download of a release into a staging directory.
 *
 * The goal is that `/update` and the next startup never wait on a 37 MB
 * transfer: by the time the user acts, the archive is already on disk,
 * checksum-verified, and its binary proven runnable. Staging is deliberately
 * side-effect-free with respect to the running install — the swap itself stays
 * in install.sh, which owns backup/rollback.
 *
 * Layout under stateDir()/staging/<version>/:
 *   evot-v<version>-<target>.tar.gz       verified archive
 *   evot-v<version>-<target>.tar.gz.sha256  sidecar used by install.sh
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'fs'
import { createWriteStream } from 'fs'
import { join } from 'path'
import type { ReleaseInfo } from './types.js'
import { currentTarget, stateDir } from './paths.js'
import { resolveUpdateProxy } from './proxy.js'

const DOWNLOAD_TIMEOUT_MS = 120_000
const MAX_ATTEMPTS = 3
/** Range requests only pay off when a partial actually exists. */
const MIN_RESUME_SIZE = 1024

export interface StagedUpdate {
  tag: string
  version: string
  target: string
  /** Archive path handed to install.sh via EVOT_INSTALL_ASSET. */
  assetPath: string
  stagedAt: number
}

interface Manifest {
  tag: string
  version: string
  target: string
  staged_at: number
}

export class StageAborted extends Error {}

function stagingRoot(): string {
  return join(stateDir(), 'staging')
}

/** Prune staging entries superseded by the currently staged version. */
function pruneSupersededVersions(current: string): void {
  try {
    for (const entry of readdirSync(stagingRoot())) {
      if (entry === current || entry.endsWith('.json')) continue
      rmSync(join(stagingRoot(), entry), { recursive: true, force: true })
    }
  } catch { /* best effort */ }
}

/** Drop whatever readStaged would not consider current, siblings included. */
export function pruneStaleStaging(): void {
  const staged = readStaged()
  if (staged) pruneSupersededVersions(staged.version)
}

function versionDir(version: string): string {
  return join(stagingRoot(), version)
}

function assetName(version: string, target: string): string {
  return `evot-v${version}-${target}.tar.gz`
}

/**
 * The staged update for this machine, if one is complete and still valid.
 *
 * Validation is cheap on purpose — manifest shape plus file presence — so the
 * startup path can call it unconditionally. Deeper checks (checksum ran during
 * staging; binary provenance was proven then too) are not repeated.
 */
export function readStaged(
  env: Record<string, string | undefined> = process.env,
): StagedUpdate | null {
  const target = currentTarget()
  if (!target) return null
  try {
    const parsed = JSON.parse(readFileSync(join(stagingRoot(), 'staged.json'), 'utf-8')) as Partial<Manifest>
    if (
      typeof parsed?.version !== 'string' || !parsed.version ||
      typeof parsed?.tag !== 'string' ||
      parsed.target !== target
    ) return null
    const assetPath = join(versionDir(parsed.version), assetName(parsed.version, target))
    const sidecar = `${assetPath}.sha256`
    if (!existsSync(assetPath) || !existsSync(sidecar)) return null
    // A manifest without a timestamp predates resume support; treat as absent
    // rather than guessing what is on disk.
    if (typeof parsed.staged_at !== 'number') return null
    return {
      tag: parsed.tag,
      version: parsed.version,
      target,
      assetPath,
      stagedAt: parsed.staged_at,
    }
  } catch {
    return null
  }
}

/** Remove any staged download. Called after a successful apply or a corrupt find. */
export function clearStaged(): void {
  try {
    rmSync(stagingRoot(), { recursive: true, force: true })
  } catch { /* best effort */ }
}

async function fetchWithResume(
  url: string,
  dest: string,
  signal: AbortSignal,
): Promise<void> {
  const { fetchProxy } = await resolveUpdateProxy()
  let offset = 0
  try {
    offset = statSync(dest).size
  } catch { /* no partial yet */ }

  const headers: Record<string, string> = {}
  if (offset >= MIN_RESUME_SIZE) headers['Range'] = `bytes=${offset}-`

  const response = await fetch(url, {
    headers,
    signal,
    redirect: 'follow',
    ...(fetchProxy ? { proxy: fetchProxy.url } : {}),
  })

  // A server that ignores Range answers 200 with the whole body; appending
  // would produce a corrupt hybrid, so restart from zero instead.
  const append = response.status === 206 && offset >= MIN_RESUME_SIZE
  if (!response.ok && response.status !== 206) {
    throw new Error(`HTTP ${response.status} fetching ${url}`)
  }

  if (!response.body) throw new Error('empty response body')
  const reader = response.body.getReader()
  const file = createWriteStream(dest, {
    // A server that ignored Range answered 200 with the whole body; appending
    // would produce a corrupt hybrid.
    flags: response.status === 206 && offset >= MIN_RESUME_SIZE ? 'a' : 'w',
  })
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      if (signal.aborted) throw new StageAborted('download aborted')
      if (!value) continue
      await new Promise<void>((resolve, reject) => {
        file.write(value, err => err ? reject(err) : resolve())
      })
    }
  } finally {
    await new Promise<void>(resolve => file.end(() => resolve()))
  }
}

async function downloadAsset(
  release: ReleaseInfo,
  target: string,
  dest: string,
  signal: AbortSignal,
): Promise<void> {
  const url = `https://github.com/evotai/evot/releases/download/${release.tag}/${assetName(release.version, target)}`
  let lastError = ''
  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
    if (signal.aborted) throw new StageAborted('download aborted')
    try {
      await fetchWithResume(url, dest, signal)
      return
    } catch (err) {
      if (err instanceof StageAborted) throw err
      lastError = err instanceof Error ? err.message : String(err)
      // AbortSignal.timeout fires inside fetch and lands here as a plain error;
      // a partial from a timed-out attempt is still resumable, keep it.
    }
    if (attempt < MAX_ATTEMPTS) {
      await new Promise(resolve => setTimeout(resolve, attempt * 1000))
    }
  }
  throw new Error(`failed after ${MAX_ATTEMPTS} attempts: ${lastError}`)
}

function verifyChecksum(assetPath: string, sidecarPath: string): void {
  // Older releases published no checksum; skipping mirrors install.sh.
  if (!existsSync(sidecarPath)) return
  const expected = readFileSync(sidecarPath, 'utf-8').trim().split(/\s+/)[0]
  if (!expected) return
  const actual = new Bun.CryptoHasher('sha256')
    .update(readFileSync(assetPath))
    .digest('hex')
  if (actual !== expected) {
    throw new Error(`checksum mismatch (expected ${expected}, got ${actual})`)
  }
}

/**
 * Download, verify, and validate a release into staging.
 *
 * Resolves once the archive is proven installable; rejects (or throws
 * StageAborted) otherwise, leaving the previous staging contents untouched
 * unless they were themselves the problem.
 */
export async function stageUpdate(
  release: ReleaseInfo,
  signal: AbortSignal,
): Promise<StagedUpdate> {
  const target = currentTarget()
  if (!target) throw new Error('unsupported platform for auto-update')

  const dir = versionDir(release.version)
  const assetPath = join(dir, assetName(release.version, target))
  const sidecarPath = `${assetPath}.sha256`
  const shaUrl = `https://github.com/evotai/evot/releases/download/${release.tag}/${assetName(release.version, target)}.sha256`

  rmSync(dir, { recursive: true, force: true })
  mkdirSync(dir, { recursive: true })

  try {
    const { fetchProxy } = await resolveUpdateProxy()
    const sidecarResponse = await fetch(shaUrl, {
      signal,
      ...(fetchProxy ? { proxy: fetchProxy.url } : {}),
    })
    if (sidecarResponse.ok) {
      writeFileSync(sidecarPath, await sidecarResponse.text())
    }

    await downloadAsset(release, target, assetPath, signal)

    // A complete-but-corrupt archive must not survive to be resumed forever.
    try {
      verifyChecksum(assetPath, sidecarPath)
    } catch (err) {
      rmSync(assetPath, { force: true })
      throw err
    }

    const extracted = join(dir, 'package')
    rmSync(extracted, { recursive: true, force: true })
    mkdirSync(extracted, { recursive: true })
    const proc = Bun.spawn(['tar', '-xzf', assetPath, '-C', extracted], {
      stdout: 'ignore',
      stderr: 'pipe',
      env: { ...process.env },
    })
    await proc.exited
    if (proc.exitCode !== 0) {
      const stderr = await new Response(proc.stderr).text()
      throw new Error(`archive extraction failed: ${stderr.trim() || `exit ${proc.exitCode}`}`)
    }

    // Mirror install.sh's candidate check: the binary must run from inside the
    // extraction and report exactly this release's version.
    const binary = join(extracted, 'bin', 'evot')
    if (!existsSync(binary)) throw new Error('archive does not contain bin/evot')
    const check = Bun.spawn([binary, '--version'], {
      stdout: 'pipe',
      stderr: 'pipe',
      cwd: extracted,
      env: { ...process.env, EVOT_HOME: extracted },
    })
    const [stdout] = await Promise.all([new Response(check.stdout).text(), check.exited])
    if (check.exitCode !== 0 || stdout.trim() !== `evot v${release.version}`) {
      throw new Error(`staged binary failed verification (${stdout.trim() || `exit ${check.exitCode}`})`)
    }

    const manifest: Manifest = {
      tag: release.tag,
      version: release.version,
      target,
      staged_at: Date.now(),
    }
    writeFileSync(join(stagingRoot(), 'staged.json'), JSON.stringify(manifest))
    pruneSupersededVersions(release.version)

    return {
      tag: release.tag,
      version: release.version,
      target,
      assetPath,
      stagedAt: manifest.staged_at,
    }
  } catch (err) {
    // An aborted download keeps its partial for resume; anything else failed
    // validation and should not be found by the next startup.
    if (!(err instanceof StageAborted)) {
      rmSync(dir, { recursive: true, force: true })
    }
    throw err
  }
}

/** Size of the staged archive, for progress display. Zero when absent. */
export function stagedBytes(update: StagedUpdate): number {
  try {
    return statSync(update.assetPath).size
  } catch {
    return 0
  }
}
