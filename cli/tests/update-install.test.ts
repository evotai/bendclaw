import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { chmodSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { executeInstall } from '../src/update/install.js'

const originalFetch = globalThis.fetch
const originalExecPath = process.execPath
let root = ''

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'evot-update-test-'))
  process.env.EVOT_INSTALL_DIR = join(root, 'bin')
  delete process.env.EVOT_HOME
})

afterEach(() => {
  globalThis.fetch = originalFetch
  process.execPath = originalExecPath
  delete process.env.EVOT_INSTALL_DIR
  delete process.env.EVOT_HOME
  rmSync(root, { recursive: true, force: true })
})

function installScript(version: string): string {
  return `#!/bin/sh
set -e
mkdir -p "$EVOT_INSTALL_DIR"
printf '%s\\n' '#!/bin/sh' 'printf "evot v${version}\\n"' > "$EVOT_INSTALL_DIR/evot"
chmod +x "$EVOT_INSTALL_DIR/evot"
`
}

describe('executeInstall', () => {
  test('reports install-script download failures', async () => {
    globalThis.fetch = async () => new Response('unavailable', { status: 503 })

    const result = await executeInstall('v2026.7.19')

    expect(result).toEqual({
      success: false,
      output: 'failed to download install script: HTTP 503',
    })
    expect(existsSync(join(root, 'bin', 'evot'))).toBe(false)
  })

  test('rejects a successful script that did not install the requested version', async () => {
    globalThis.fetch = async () => new Response(installScript('2026.7.10.2'))

    const result = await executeInstall('v2026.7.19')

    expect(result.success).toBe(false)
    expect(result.output).toContain('installed version mismatch')
    expect(result.output).toContain('expected evot v2026.7.19')
    expect(result.output).toContain('got evot v2026.7.10.2')
  })

  test('accepts an installed binary with the requested version', async () => {
    globalThis.fetch = async () => new Response(installScript('2026.7.19'))

    const result = await executeInstall('v2026.7.19')

    expect(result.success).toBe(true)
    expect(readFileSync(join(root, 'bin', 'evot'), 'utf8')).toContain('2026.7.19')
  })

  test('fetches the installer from the selected release tag', async () => {
    let requestedUrl = ''
    globalThis.fetch = async (input) => {
      requestedUrl = String(input)
      return new Response(installScript('2026.7.19'))
    }

    const result = await executeInstall('v2026.7.19')

    expect(result.success).toBe(true)
    expect(requestedUrl).toBe(
      'https://raw.githubusercontent.com/evotai/evot/v2026.7.19/install.sh',
    )
  })

  test('targets the running compiled evot when no install override remains', async () => {
    delete process.env.EVOT_INSTALL_DIR
    const installDir = join(root, 'custom', 'bin')
    process.execPath = join(installDir, 'evot')
    globalThis.fetch = async () => new Response(installScript('2026.7.19'))

    const result = await executeInstall('v2026.7.19')

    expect(result.success).toBe(true)
    expect(readFileSync(join(installDir, 'evot'), 'utf8')).toContain('2026.7.19')
  })
})

describe('install.sh', () => {
  const installShPath = join(import.meta.dir, '..', '..', 'install.sh')
  const testBinding = 'evot-napi.linux-x64-gnu.node'

  /**
   * Build a PATH shim so install.sh runs offline: `uname` reports a fixed
   * platform and `curl` serves a local archive. `curl` invocations with `-o`
   * are archive downloads; the rest are `fetch` calls (checksum, version).
   */
  function fakeBin(curlBody: string): string {
    const dir = mkdtempSync(join(root, 'fake-bin-'))
    writeFileSync(join(dir, 'uname'), `#!/bin/sh
if [ "\${1:-}" = "-s" ]; then printf 'Linux\\n'; else printf 'x86_64\\n'; fi
`)
    writeFileSync(join(dir, 'curl'), curlBody)
    chmodSync(join(dir, 'uname'), 0o755)
    chmodSync(join(dir, 'curl'), 0o755)
    return dir
  }

  /** curl shim that copies $TEST_ARCHIVE for downloads and fails fetches. */
  const CURL_SERVES_ARCHIVE = `#!/bin/sh
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then output="$2"; shift 2; continue; fi
  shift
done
if [ -n "$output" ]; then cp "$TEST_ARCHIVE" "$output"; exit 0; fi
exit 22
`

  function packArchive(
    version: string,
    opts: { libs?: string[]; binaryScript?: string } = {},
  ): string {
    const archiveRoot = mkdtempSync(join(root, 'archive-'))
    const archive = join(root, `release-${version}-${Math.random().toString(36).slice(2)}.tar.gz`)
    mkdirSync(join(archiveRoot, 'bin'), { recursive: true })
    writeFileSync(
      join(archiveRoot, 'bin', 'evot'),
      opts.binaryScript ?? `#!/bin/sh\nprintf "evot v${version}\\n"\n`,
    )
    chmodSync(join(archiveRoot, 'bin', 'evot'), 0o755)

    const libs = opts.libs ?? [testBinding]
    const members = ['bin']
    if (libs.length > 0) {
      mkdirSync(join(archiveRoot, 'lib'), { recursive: true })
      for (const lib of libs) writeFileSync(join(archiveRoot, 'lib', lib), 'new-binding')
      members.push('lib')
    }

    const tar = Bun.spawnSync(['tar', '-C', archiveRoot, '-czf', archive, ...members])
    expect(tar.exitCode).toBe(0)
    return archive
  }

  async function runInstallSh(env: Record<string, string>) {
    const proc = Bun.spawn(['sh', installShPath], {
      stdout: 'pipe',
      stderr: 'pipe',
      env: { ...process.env, PATH: `${env.FAKE_BIN}:/usr/bin:/bin`, ...env },
    })
    const [stdout, stderr, exitCode] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
      proc.exited,
    ])
    return { stdout, stderr, exitCode }
  }

  test('validates the candidate before replacing the installed binary', async () => {
    const installDir = join(root, 'installed', 'bin')
    mkdirSync(installDir, { recursive: true })
    writeFileSync(join(installDir, 'evot'), '#!/bin/sh\nprintf "evot vold\\n"\n')
    chmodSync(join(installDir, 'evot'), 0o755)

    const archive = packArchive('2026.7.18')
    const { stderr, exitCode } = await runInstallSh({
      FAKE_BIN: fakeBin(CURL_SERVES_ARCHIVE),
      TEST_ARCHIVE: archive,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).not.toBe(0)
    expect(stderr).toContain('Downloaded version mismatch')
    const current = Bun.spawnSync([join(installDir, 'evot'), '--version'])
    expect(current.stdout.toString().trim()).toBe('evot vold')
  })

  test('retries a failing download and succeeds', async () => {
    const installDir = join(root, 'retry', 'bin')
    mkdirSync(installDir, { recursive: true })
    const counter = join(root, 'curl-attempts')
    const archive = packArchive('2026.7.19')

    // Fail the first two download attempts, then serve the archive.
    const curl = fakeBin(`#!/bin/sh
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then output="$2"; shift 2; continue; fi
  shift
done
[ -n "$output" ] || exit 22
attempts=0
[ -f "$COUNTER" ] && attempts="$(cat "$COUNTER")"
attempts=$((attempts + 1))
printf '%s' "$attempts" > "$COUNTER"
if [ "$attempts" -lt 3 ]; then exit 7; fi
cp "$TEST_ARCHIVE" "$output"
`)

    const { stdout, exitCode } = await runInstallSh({
      FAKE_BIN: curl,
      TEST_ARCHIVE: archive,
      COUNTER: counter,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).toBe(0)
    expect(readFileSync(counter, 'utf8')).toBe('3')
    expect(stdout).toContain('Installed evot')
  })

  test('gives up after the attempt budget without touching the old binary', async () => {
    const installDir = join(root, 'exhausted', 'bin')
    mkdirSync(installDir, { recursive: true })
    writeFileSync(join(installDir, 'evot'), '#!/bin/sh\nprintf "evot vold\\n"\n')
    chmodSync(join(installDir, 'evot'), 0o755)
    const counter = join(root, 'always-fail-attempts')

    const curl = fakeBin(`#!/bin/sh
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then output="$2"; shift 2; continue; fi
  shift
done
[ -n "$output" ] || exit 22
attempts=0
[ -f "$COUNTER" ] && attempts="$(cat "$COUNTER")"
printf '%s' "$((attempts + 1))" > "$COUNTER"
exit 7
`)

    const { stderr, exitCode } = await runInstallSh({
      FAKE_BIN: curl,
      COUNTER: counter,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).not.toBe(0)
    expect(stderr).toContain('Failed to download')
    expect(readFileSync(counter, 'utf8')).toBe('3')
    const current = Bun.spawnSync([join(installDir, 'evot'), '--version'])
    expect(current.stdout.toString().trim()).toBe('evot vold')
  })

  test('discards a corrupt archive instead of resuming it', async () => {
    const installDir = join(root, 'corrupt', 'bin')
    mkdirSync(installDir, { recursive: true })
    const corrupt = join(root, 'corrupt.tar.gz')
    writeFileSync(corrupt, 'not a gzip stream')
    const sizes = join(root, 'observed-sizes')

    // Record the staged file size each attempt sees. A resumed corrupt payload
    // would grow; a discarded one is always downloaded from zero.
    const curl = fakeBin(`#!/bin/sh
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then output="$2"; shift 2; continue; fi
  shift
done
[ -n "$output" ] || exit 22
if [ -f "$output" ]; then wc -c < "$output" | tr -d ' ' >> "$SIZES"; else echo 0 >> "$SIZES"; fi
cp "$TEST_ARCHIVE" "$output"
`)

    const { stderr, exitCode } = await runInstallSh({
      FAKE_BIN: curl,
      TEST_ARCHIVE: corrupt,
      SIZES: sizes,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).not.toBe(0)
    expect(stderr).toContain('Failed to download')
    // Three attempts, none of which found a leftover partial to resume.
    expect(readFileSync(sizes, 'utf8').trim().split('\n')).toEqual(['0', '0', '0'])
  })

  test('cleans partial extraction output before retrying another archive', async () => {
    const installDir = join(root, 'clean-extract', 'bin')
    mkdirSync(installDir, { recursive: true })
    const archive = packArchive('2026.7.19', {
      libs: ['evot-napi.darwin-arm64.node'],
    })
    const tarAttempts = join(root, 'tar-attempts')
    const tools = fakeBin(CURL_SERVES_ARCHIVE)

    // Simulate tar leaving the required binding behind before failing. The next
    // archive is otherwise valid but contains only a binding for another target.
    // Reusing the partial extraction would incorrectly turn that pair into a
    // valid-looking package.
    writeFileSync(join(tools, 'tar'), `#!/bin/sh
attempts=0
[ -f "$TAR_ATTEMPTS" ] && attempts="$(cat "$TAR_ATTEMPTS")"
attempts=$((attempts + 1))
printf '%s' "$attempts" > "$TAR_ATTEMPTS"
if [ "$attempts" -eq 1 ]; then
  extract=''
  while [ "$#" -gt 0 ]; do
    if [ "$1" = '-C' ]; then extract="$2"; shift 2; continue; fi
    shift
  done
  mkdir -p "$extract/lib"
  printf stale > "$extract/lib/${testBinding}"
  exit 1
fi
exec /usr/bin/tar "$@"
`)
    chmodSync(join(tools, 'tar'), 0o755)

    const { stderr, exitCode } = await runInstallSh({
      FAKE_BIN: tools,
      TEST_ARCHIVE: archive,
      TAR_ATTEMPTS: tarAttempts,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).not.toBe(0)
    expect(readFileSync(tarAttempts, 'utf8')).toBe('2')
    expect(stderr).toContain(`Release archive does not contain lib/${testBinding}`)
    expect(existsSync(join(root, 'clean-extract', 'lib', testBinding))).toBe(false)
  })

  test('recovers when the host refuses byte ranges', async () => {
    const installDir = join(root, 'noresume', 'bin')
    mkdirSync(installDir, { recursive: true })
    const archive = packArchive('2026.7.19')
    const log = join(root, 'resume-log')

    // Mimic a host without Range support: curl exits 33 whenever --continue-at
    // is passed. The retry must drop the partial and refetch without resume.
    const curl = fakeBin(`#!/bin/sh
output=''
resume=no
for arg in "$@"; do
  [ "$arg" = '--continue-at' ] && resume=yes
done
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then output="$2"; shift 2; continue; fi
  shift
done
[ -n "$output" ] || exit 22
echo "$resume" >> "$LOG"
if [ "$resume" = yes ]; then exit 33; fi
# Leave a partial behind so the next attempt is tempted to resume it.
if [ ! -f "$STAGED" ]; then
  head -c 10 "$TEST_ARCHIVE" > "$output"
  : > "$STAGED"
  exit 18
fi
cp "$TEST_ARCHIVE" "$output"
`)

    const { exitCode } = await runInstallSh({
      FAKE_BIN: curl,
      TEST_ARCHIVE: archive,
      LOG: log,
      STAGED: join(root, 'staged-marker'),
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).toBe(0)
    // no (fresh, dies partway) → yes (rejected 33) → no (clean refetch, wins)
    expect(readFileSync(log, 'utf8').trim().split('\n')).toEqual(['no', 'yes', 'no'])
    const installed = Bun.spawnSync([join(installDir, 'evot'), '--version'])
    expect(installed.stdout.toString().trim()).toBe('evot v2026.7.19')
  })

  test('rejects an archive without the binding required by its target', async () => {
    const installRoot = join(root, 'wrong-binding')
    const installDir = join(installRoot, 'bin')
    const libDir = join(installRoot, 'lib')
    mkdirSync(installDir, { recursive: true })
    mkdirSync(libDir, { recursive: true })
    writeFileSync(join(installDir, 'evot'), '#!/bin/sh\nprintf "evot vold\\n"\n')
    chmodSync(join(installDir, 'evot'), 0o755)
    writeFileSync(join(libDir, testBinding), 'old-binding')

    const archive = packArchive('2026.7.19', {
      libs: ['evot-napi.darwin-arm64.node'],
    })
    const { stderr, exitCode } = await runInstallSh({
      FAKE_BIN: fakeBin(CURL_SERVES_ARCHIVE),
      TEST_ARCHIVE: archive,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).not.toBe(0)
    expect(stderr).toContain(`Release archive does not contain lib/${testBinding}`)
    const current = Bun.spawnSync([join(installDir, 'evot'), '--version'])
    expect(current.stdout.toString().trim()).toBe('evot vold')
    expect(readFileSync(join(libDir, testBinding), 'utf8')).toBe('old-binding')
  })

  test('records and installs only the binding selected for the target', async () => {
    const installDir = join(root, 'stateful', 'bin')
    mkdirSync(installDir, { recursive: true })
    const unrelated = 'evot-napi.darwin-arm64.node'
    const archive = packArchive('2026.7.19', { libs: [testBinding, unrelated] })

    const { exitCode } = await runInstallSh({
      FAKE_BIN: fakeBin(CURL_SERVES_ARCHIVE),
      TEST_ARCHIVE: archive,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).toBe(0)
    const state = JSON.parse(readFileSync(join(root, 'stateful', 'install-state.json'), 'utf8'))
    expect(state.version).toBe('2026.7.19')
    expect(state.target).toBe('x86_64-unknown-linux-gnu')
    expect(state.lib).toEqual([testBinding])
    expect(typeof state.installed_at).toBe('number')
    expect(readFileSync(join(root, 'stateful', 'lib', testBinding), 'utf8')).toBe('new-binding')
    expect(existsSync(join(root, 'stateful', 'lib', unrelated))).toBe(false)
  })

  test('rolls back binary, binding, and metadata when installed verification fails', async () => {
    const installRoot = join(root, 'rollback')
    const installDir = join(installRoot, 'bin')
    const libDir = join(installRoot, 'lib')
    const statePath = join(installRoot, 'install-state.json')
    mkdirSync(installDir, { recursive: true })
    mkdirSync(libDir, { recursive: true })

    writeFileSync(join(installDir, 'evot'), '#!/bin/sh\nprintf "evot vold\\n"\n')
    chmodSync(join(installDir, 'evot'), 0o755)
    writeFileSync(join(libDir, testBinding), 'old-binding')
    const oldState = JSON.stringify({
      version: '2026.7.18',
      target: 'x86_64-unknown-linux-gnu',
      lib: [testBinding],
      installed_at: 1,
    })
    writeFileSync(statePath, oldState)
    writeFileSync(join(installRoot, 'fail-installed-check'), '')

    // Candidate validation runs with EVOT_HOME set to the extraction dir and
    // succeeds. The same binary fails only after installation, when EVOT_HOME
    // points at installRoot, forcing the transaction rollback path.
    const archive = packArchive('2026.7.19', {
      binaryScript: `#!/bin/sh
if [ -f "$EVOT_HOME/fail-installed-check" ]; then
  echo 'post-install failure' >&2
  exit 1
fi
printf 'evot v2026.7.19\\n'
`,
    })
    const { stderr, exitCode } = await runInstallSh({
      FAKE_BIN: fakeBin(CURL_SERVES_ARCHIVE),
      TEST_ARCHIVE: archive,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).not.toBe(0)
    expect(stderr).toContain('Installed evot failed to start')
    const current = Bun.spawnSync([join(installDir, 'evot'), '--version'])
    expect(current.stdout.toString().trim()).toBe('evot vold')
    expect(readFileSync(join(libDir, testBinding), 'utf8')).toBe('old-binding')
    expect(readFileSync(statePath, 'utf8')).toBe(oldState)
  })

  test('writes no install state when the install fails', async () => {
    const installDir = join(root, 'nostate', 'bin')
    mkdirSync(installDir, { recursive: true })
    const archive = packArchive('2026.7.18')

    const { exitCode } = await runInstallSh({
      FAKE_BIN: fakeBin(CURL_SERVES_ARCHIVE),
      TEST_ARCHIVE: archive,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).not.toBe(0)
    expect(existsSync(join(root, 'nostate', 'install-state.json'))).toBe(false)
  })
})
