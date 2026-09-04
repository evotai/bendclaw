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
import {
  bindingFilenameForTarget,
  currentTarget,
  installRoot,
  runningInstallDir,
} from './paths.js'
import { compareVersions } from './version.js'

function statePath(env: Record<string, string | undefined> = process.env): string {
  return join(installRoot(env), 'install-state.json')
}

/**
 * Whether install bookkeeping describes the binary this process is running.
 *
 * A source checkout (`bun run src/index.ts`, `bun test`) reports its Cargo
 * version — 0.1.0 — while ~/.evotai/install-state.json describes a real
 * installed release. Comparing the two comes out as "the install is newer",
 * which is true and useless: the dev build is not that install, restarting
 * would not change what is running, and applying a staged download would
 * overwrite the user's release with a local build. An explicit
 * EVOT_INSTALL_DIR means the caller pointed us at an install on purpose.
 */
export function isManagedInstall(
  env: Record<string, string | undefined> = process.env,
): boolean {
  const runsFromInstall = !!env.EVOT_INSTALL_DIR || !!runningInstallDir()
  // A compiled binary can also be installed locally by `make install`. That
  // target deliberately removes install-state.json because the binary reports
  // the workspace version (0.1.0), not a published release. Treating its path
  // alone as a managed release lets a previously staged archive overwrite the
  // local build on its next startup. Only install.sh writes the state record,
  // so it is the authoritative marker for automatic staging and apply.
  return runsFromInstall && readInstallState(env) !== null
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

/**
 * Installed version, but only when the record describes the running binary.
 *
 * The single reader for "what is on disk for this install", so the
 * source-checkout guard cannot be forgotten at one call site: a dev build must
 * never be told to restart into an unrelated installed release.
 */
export function installedVersionForThisProcess(
  env: Record<string, string | undefined> = process.env,
): string | null {
  if (!isManagedInstall(env)) return null
  return readInstallState(env)?.version ?? null
}

export type InstallHealth =
  | { kind: 'ok' }
  /** No bookkeeping yet: installed before this existed, or a source checkout. */
  | { kind: 'unknown' }
  /**
   * A newer version is fully installed on disk while this process still runs
   * the image it started with. Nothing is broken and reinstalling would fix
   * nothing — the session just has to be restarted.
   */
  | { kind: 'restart_required'; installedVersion: string }
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
  if (!isManagedInstall(env)) return { kind: 'unknown' }
  const state = readInstallState(env)
  if (!state) return { kind: 'unknown' }

  const target = currentTarget()
  if (target && state.target !== target) {
    return {
      kind: 'drift',
      reason: `installed for ${state.target}, running on ${target}`,
    }
  }

  // Artifact integrity is checked before versions. A record that names a newer
  // release is still broken if its binding is missing or wrong, and that needs
  // a reinstall — a restart would only reload the same broken pair.
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

  const comparison = compareVersions(state.version, runningVersion)
  if (comparison > 0) {
    // A newer install completed while this process kept its already-mapped
    // image: either it applied the update itself, or another evot did. The
    // files on disk are consistent, so this is a restart, not a repair.
    return { kind: 'restart_required', installedVersion: state.version }
  }
  if (comparison !== 0) {
    return {
      kind: 'drift',
      reason: `install recorded v${state.version}, running v${runningVersion}`,
    }
  }

  return { kind: 'ok' }
}
