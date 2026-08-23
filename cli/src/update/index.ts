/**
 * Update module — public API.
 */

export type { CheckResult, RunResult, ReleaseInfo, InstallState } from './types.js'
export { checkForUpdate, fetchReleaseNotesFor, lastCheckError, selectRelease } from './check.js'
export { compareVersions, isNewer, isPrerelease, parseVersion } from './version.js'
export { executeInstall } from './install.js'
export { UpdateManager } from './manager.js'
export { parseReleaseNotes } from './notes.js'
export { checkInstallHealth, readInstallState } from './state.js'
export type { InstallHealth } from './state.js'

import type { RunResult } from './types.js'
import { checkForUpdate, lastCheckError } from './check.js'
import { executeInstall } from './install.js'
import { parseReleaseNotes } from './notes.js'

/**
 * Force-check for updates and install if available.
 * Used by `/update` and `evot update`.
 */
export async function runUpdate(currentVersion: string): Promise<RunResult> {
  const result = await checkForUpdate(currentVersion, { force: true })

  if (result.kind === 'error') {
    return { kind: 'error', message: result.message }
  }
  if (result.kind === 'up_to_date') {
    // Distinguish "confirmed current" from "could not reach GitHub, and the last
    // known release was not newer". Silently claiming the former would be wrong.
    if (!result.stale) return { kind: 'up_to_date' }
    return {
      kind: 'up_to_date',
      staleReason: lastCheckError()?.message ?? 'GitHub unreachable',
    }
  }

  const installResult = await executeInstall(result.latest.tag)
  if (installResult.success) {
    const notes = parseReleaseNotes(result.latest.body)
    return { kind: 'updated', from: currentVersion, to: result.latest.version, notes }
  }
  return { kind: 'error', message: installResult.output }
}
