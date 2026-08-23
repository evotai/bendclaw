/**
 * Filesystem locations for update state.
 *
 * Two distinct roots, deliberately:
 *   - stateDir()    user state that follows the user (update cache, seen version)
 *   - installRoot() the directory install.sh writes bin/ and lib/ into
 *
 * They differ when EVOT_INSTALL_DIR points somewhere outside EVOT_HOME (e.g.
 * /usr/local/bin), so install bookkeeping must not be written to stateDir().
 */

import { realpathSync } from 'fs'
import { homedir } from 'os'
import { basename, dirname, join } from 'path'

/** Root for user-scoped evot state. Mirrors the default install home. */
export function stateDir(env: Record<string, string | undefined> = process.env): string {
  return env.EVOT_HOME || join(homedir(), '.evotai')
}

/**
 * Directory containing a running installed evot binary.
 *
 * Bun's compiled executable reports itself through process.execPath. Ignore
 * ordinary `bun`/test processes so source checkouts still use the default
 * install location rather than Bun's own directory.
 */
export function runningInstallDir(executablePath: string = process.execPath): string | null {
  if (basename(executablePath) !== 'evot') return null
  try {
    // Updating a symlink path would replace the link and split bin/ from the
    // binding beside its real target. Follow it so self-update preserves the
    // install layout the running binary actually came from.
    return dirname(realpathSync(executablePath))
  } catch {
    // The path can disappear during a concurrent replacement, and tests use
    // synthetic paths. Its lexical directory is still the best available root.
    return dirname(executablePath)
  }
}

function rootForInstallDir(installDir: string): string {
  return basename(installDir) === 'bin' ? dirname(installDir) : installDir
}

/**
 * Root that contains the installed executable and lib/.
 *
 * An explicit EVOT_INSTALL_DIR wins. Otherwise a compiled evot infers its root
 * from process.execPath, which keeps custom installs discoverable after the
 * one-shot installer environment is gone. Source/test processes fall back to
 * ~/.evotai. EVOT_HOME remains user state only and is deliberately ignored.
 */
export function installRoot(
  env: Record<string, string | undefined> = process.env,
  executablePath: string = process.execPath,
): string {
  const installDir = env.EVOT_INSTALL_DIR || runningInstallDir(executablePath)
  return installDir ? rootForInstallDir(installDir) : join(homedir(), '.evotai')
}

/** Directory holding the installed executable. */
export function installBinDir(
  env: Record<string, string | undefined> = process.env,
  executablePath: string = process.execPath,
): string {
  return env.EVOT_INSTALL_DIR
    || runningInstallDir(executablePath)
    || join(installRoot(env, executablePath), 'bin')
}

/** Release target triple for the machine running this process. */
export function currentTarget(): string | null {
  switch (`${process.platform}-${process.arch}`) {
    case 'darwin-arm64': return 'aarch64-apple-darwin'
    case 'darwin-x64': return 'x86_64-apple-darwin'
    case 'linux-x64': return 'x86_64-unknown-linux-gnu'
    case 'linux-arm64': return 'aarch64-unknown-linux-gnu'
    default: return null
  }
}

/** Exact napi binding shipped for a release target. */
export function bindingFilenameForTarget(target: string): string | null {
  switch (target) {
    case 'aarch64-apple-darwin': return 'evot-napi.darwin-arm64.node'
    case 'x86_64-apple-darwin': return 'evot-napi.darwin-x64.node'
    case 'aarch64-unknown-linux-gnu': return 'evot-napi.linux-arm64-gnu.node'
    case 'x86_64-unknown-linux-gnu': return 'evot-napi.linux-x64-gnu.node'
    default: return null
  }
}
