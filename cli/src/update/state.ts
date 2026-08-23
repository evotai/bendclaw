/**
 * Install bookkeeping reader and health check.
 *
 * install.sh is the single writer for install-state.json because it handles
 * both fresh `curl | sh` installs and in-app updates. This module only reads
 * that record and compares it with the running binary and native bindings.
 */

import { existsSync, readFileSync } from 'fs'
import { join } from 'path'
import type { InstallState } from './types.js'
import { bindingFilenameForTarget, currentTarget, installRoot } from './paths.js'
import { compareVersions } from './version.js'

function statePath(env: Record<string, string | undefined> = process.env): string {
  return join(installRoot(env), 'install-state.json')
}

export function readInstallState(
  env: Record<string, string | undefined> = process.env,
): InstallState | null {
  try {
    const parsed = JSON.parse(readFileSync(statePath(env), 'utf-8')) as InstallState
    if (typeof parsed?.version !== 'string' || !parsed.version) return null
    if (typeof parsed?.target !== 'string' || !parsed.target) return null
    if (!Array.isArray(parsed?.lib) || parsed.lib.some((name) => typeof name !== 'string')) {
      return null
    }
    return parsed
  } catch {
    return null
  }
}

export type InstallHealth =
  | { kind: 'ok' }
  /** No bookkeeping yet: installed before this existed, or a source checkout. */
  | { kind: 'unknown' }
  | { kind: 'drift'; reason: string }

/**
 * Compare recorded install state against the running binary.
 *
 * Deliberately conservative: only a recorded state that contradicts reality is
 * reported as drift. A missing record is 'unknown', never a warning, so users
 * who installed before this shipped are not nagged.
 */
export function checkInstallHealth(
  runningVersion: string,
  env: Record<string, string | undefined> = process.env,
): InstallHealth {
  const state = readInstallState(env)
  if (!state) return { kind: 'unknown' }

  const target = currentTarget()
  if (target && state.target !== target) {
    return {
      kind: 'drift',
      reason: `installed for ${state.target}, running on ${target}`,
    }
  }

  if (compareVersions(state.version, runningVersion) !== 0) {
    return {
      kind: 'drift',
      reason: `install recorded v${state.version}, running v${runningVersion}`,
    }
  }

  const expectedBinding = bindingFilenameForTarget(state.target)
  if (!expectedBinding) {
    return { kind: 'drift', reason: `install recorded unsupported target ${state.target}` }
  }
  if (state.lib.length !== 1 || state.lib[0] !== expectedBinding) {
    return {
      kind: 'drift',
      reason: `install metadata expected lib/${expectedBinding}`,
    }
  }
  if (!existsSync(join(installRoot(env), 'lib', expectedBinding))) {
    return { kind: 'drift', reason: `missing native binding lib/${expectedBinding}` }
  }

  return { kind: 'ok' }
}
