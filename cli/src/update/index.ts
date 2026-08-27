/**
 * Update module — public API.
 */

export type { CheckResult, RunResult, ReleaseInfo, InstallState } from './types.js'
export { checkForUpdate, fetchReleaseNotesFor, lastCheckError, selectRelease } from './check.js'
export { compareVersions, isNewer, isPrerelease, parseVersion } from './version.js'
export { executeInstall } from './install.js'
export {
  applyProxyToEnv,
  collectCandidates,
  envCandidates,
  envExemptsAll,
  parseProxyUrl,
  probeReachable,
  resetUpdateProxy,
  resolveUpdateProxy,
  selectProxy,
  systemCandidates,
  UPDATE_URLS,
} from './proxy.js'
export type { ProxyCandidate, ProxyEnv, ProxySelection } from './proxy.js'
export { UpdateManager, type UpdateStatus } from './manager.js'
export { parseReleaseNotes } from './notes.js'
export { checkInstallHealth, readInstallState } from './state.js'
export type { InstallHealth } from './state.js'
export { readStaged, clearStaged } from './stage.js'
export type { StagedUpdate } from './stage.js'

import type { RunResult } from './types.js'
import { checkForUpdate, lastCheckError } from './check.js'
import { executeInstall } from './install.js'
import { parseReleaseNotes } from './notes.js'
import { clearStaged, readStaged } from './stage.js'
import { resolveUpdateProxy } from './proxy.js'

/**
 * How the update reached the network, for attaching to an outcome.
 *
 * The proxy is chosen automatically, so a failure that does not name the route
 * leaves the user unable to tell "my proxy was used and still failed" from "my
 * proxy was never consulted".
 */
async function proxyContext(): Promise<string | undefined> {
  try {
    return (await resolveUpdateProxy()).reason
  } catch {
    return undefined
  }
}

/**
 * Force-check for updates and install if available.
 * Used by `/update` and `evot update`.
 */
export async function runUpdate(currentVersion: string): Promise<RunResult> {
  const result = await checkForUpdate(currentVersion, { force: true })

  if (result.kind === 'error') {
    return { kind: 'error', message: result.message, proxy: await proxyContext() }
  }
  if (result.kind === 'up_to_date') {
    // Distinguish "confirmed current" from "could not reach GitHub, and the last
    // known release was not newer". Silently claiming the former would be wrong.
    if (!result.stale) return { kind: 'up_to_date' }
    return {
      kind: 'up_to_date',
      staleReason: lastCheckError()?.message ?? 'GitHub unreachable',
      proxy: await proxyContext(),
    }
  }

  const installResult = await executeInstall(result.latest.tag, stagedEnvFor(result.latest.tag))
  if (installResult.success) {
    const notes = parseReleaseNotes(result.latest.body)
    return { kind: 'updated', from: currentVersion, to: result.latest.version, notes }
  }
  return { kind: 'error', message: installResult.output, proxy: await proxyContext() }
}

/**
 * Hand install.sh the pre-downloaded archive when one matches this tag.
 *
 * A mismatch (the candidate moved on while a stale download sat in staging)
 * simply falls through to the normal network path.
 */
function stagedEnvFor(tag: string): Record<string, string> | undefined {
  const staged = readStaged()
  if (!staged || staged.tag !== tag) return undefined
  return { EVOT_INSTALL_ASSET: staged.assetPath }
}

/**
 * Apply a background-staged download at startup, before the REPL comes up.
 *
 * Returns the version now running when an apply happened, or null when there
 * was nothing to do. Deliberately quiet on failure: install.sh falls back to
 * downloading on its own, and a failed auto-apply must never block launch —
 * the next `/update` surfaces whatever went wrong with full context.
 */
export async function applyStagedOnStartup(currentVersion: string): Promise<string | null> {
  const staged = readStaged()
  if (!staged) return null

  // Stale against what is already running (a concurrent process applied it,
  // or the user updated manually): the download served its purpose or lost.
  const { compareVersions } = await import('./version.js')
  if (compareVersions(currentVersion, staged.version) >= 0) {
    clearStaged()
    return null
  }

  // Fast path: apply what is staged; background checks re-stage newer releases.
  const result = await executeInstall(staged.tag, { EVOT_INSTALL_ASSET: staged.assetPath })
  if (!result.success) return null
  clearStaged()
  return staged.version
}
