/**
 * Execute the install script to update evot.
 */

import { join } from 'path'
import { installBinDir, installRoot, runningInstallDir } from './paths.js'
import { applyProxyToEnv, resolveUpdateProxy } from './proxy.js'

const INSTALL_SCRIPT_BASE = 'https://raw.githubusercontent.com/evotai/evot'
const SCRIPT_FETCH_TIMEOUT = 30_000
const SCRIPT_FETCH_ATTEMPTS = 3
const SCRIPT_RETRY_BASE_DELAY = 1_000

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * Fetch install.sh with bounded retries.
 *
 * The install itself is not retried here — install.sh stages and validates
 * before replacing anything, and re-running it blindly would repeat a download
 * that failed for a non-transient reason. Only the script fetch, which is
 * cheap and idempotent, gets attempts.
 */
async function fetchInstallScript(tag?: string): Promise<{ script: string } | { error: string }> {
  // An update must use the installer committed with the release it selected.
  // Fetching main could apply newer install semantics to an older release asset.
  const ref = tag ? encodeURIComponent(tag) : 'main'
  const installScript = `${INSTALL_SCRIPT_BASE}/${ref}/install.sh`
  let lastError = ''
  for (let attempt = 1; attempt <= SCRIPT_FETCH_ATTEMPTS; attempt++) {
    try {
      const { fetchProxy } = await resolveUpdateProxy()
      const response = await fetch(installScript, {
        signal: AbortSignal.timeout(SCRIPT_FETCH_TIMEOUT),
        ...(fetchProxy ? { proxy: fetchProxy.url } : {}),
      })
      if (!response.ok) {
        lastError = `failed to download install script: HTTP ${response.status}`
        // 4xx will not fix itself; only retry server-side and throttling faults.
        const retryable = response.status === 408 || response.status === 429 || response.status >= 500
        if (!retryable) return { error: lastError }
      } else {
        const script = await response.text()
        if (script.trim()) return { script }
        lastError = 'failed to download install script: empty response'
      }
    } catch (err: unknown) {
      lastError = `failed to download install script: ${errorMessage(err)}`
    }

    if (attempt < SCRIPT_FETCH_ATTEMPTS) {
      await sleep(SCRIPT_RETRY_BASE_DELAY * 2 ** (attempt - 1))
    }
  }
  return { error: lastError || 'failed to download install script' }
}

async function verifyInstalledVersion(
  expectedVersion: string,
  env: Record<string, string>,
): Promise<{ success: boolean; output: string }> {
  const root = installRoot(env)
  const binary = join(installBinDir(env), 'evot')
  const proc = Bun.spawn([binary, '--version'], {
    stdout: 'pipe',
    stderr: 'pipe',
    env: { ...env, EVOT_HOME: root },
  })
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ])
  const output = (stderr || stdout).trim()
  if (exitCode !== 0) {
    return {
      success: false,
      output: `installed evot failed verification (exit code ${exitCode})${output ? `: ${output}` : ''}`,
    }
  }

  const actual = stdout.trim()
  const expected = `evot v${expectedVersion}`
  if (actual !== expected) {
    return {
      success: false,
      output: `installed version mismatch: expected ${expected}, got ${actual || '(empty output)'}`,
    }
  }
  return { success: true, output: actual }
}

export async function executeInstall(tag?: string): Promise<{ success: boolean; output: string }> {
  try {
    // The 37 MB release asset is downloaded by curl inside install.sh, so the
    // decision has to be pushed into the child's environment. Normalizing it
    // here also removes any unreachable value inherited from the parent shell.
    const selection = await resolveUpdateProxy()
    const env = applyProxyToEnv({ ...process.env as Record<string, string> }, selection)
    const inferredInstallDir = runningInstallDir()
    if (!env.EVOT_INSTALL_DIR && inferredInstallDir) {
      // Preserve custom installs after the one-shot installer environment is
      // gone: update the compiled evot that is actually running.
      env.EVOT_INSTALL_DIR = inferredInstallDir
    }
    if (tag) {
      env.EVOT_INSTALL_VERSION = tag
    }

    // Fetch first, then pass the complete script to sh. A `curl | sh` pipeline
    // can return success when curl fails because POSIX sh has no pipefail.
    const fetched = await fetchInstallScript(tag)
    if ('error' in fetched) {
      return { success: false, output: fetched.error }
    }

    const proc = Bun.spawn(['sh'], {
      stdin: new Blob([fetched.script]),
      stdout: 'pipe',
      stderr: 'pipe',
      env,
    })

    const [stdout, stderr, exitCode] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
      proc.exited,
    ])

    if (exitCode !== 0) {
      return { success: false, output: stderr || stdout || `exit code ${exitCode}` }
    }

    if (tag) {
      const verification = await verifyInstalledVersion(tag.replace(/^v/, ''), env)
      if (!verification.success) return verification
    }
    return { success: true, output: stdout }
  } catch (err: unknown) {
    return { success: false, output: errorMessage(err) || 'failed to run install script' }
  }
}
