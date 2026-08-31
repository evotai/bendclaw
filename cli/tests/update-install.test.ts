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
  // Collapse the install-script retry backoff, which is otherwise a fixed
  // 1s + 2s of pure waiting in the test that covers the retry path.
  process.env.EVOT_SCRIPT_RETRY_BASE_DELAY_MS = '1'
})

afterEach(() => {
  globalThis.fetch = originalFetch
  process.execPath = originalExecPath
  delete process.env.EVOT_INSTALL_DIR
  delete process.env.EVOT_HOME
  delete process.env.EVOT_SCRIPT_RETRY_BASE_DELAY_MS
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
      env: {
        ...process.env,
        PATH: `${env.FAKE_BIN}:/usr/bin:/bin`,
        // Collapse the retry backoff. Three attempts otherwise sleep a fixed
        // 1s + 2s, which dominated these tests' runtime and pushed the
        // retry-exercising ones past the default timeout.
        EVOT_DOWNLOAD_RETRY_BASE_DELAY: '0.01',
        ...env,
      },
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
    expect(stdout).toContain('installed evot to')
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

  /**
   * macOS caches a code-signature blob per path and invalidates it by mtime. A
   * path that has already hosted a differently-signed build keeps that stale
   * blob, so the kernel compares it against the new bytes, finds
   * cs_mtime != mtime, taints the mapped page and SIGKILLs on first fault.
   * Signing a staged copy refreshes the wrong path and leaves the destination
   * entry stale, so the signature has to be applied after the rename.
   */
  test('signs macOS artifacts at their destination, after the rename', () => {
    const script = readFileSync(installShPath, 'utf8')

    const binarySign = script.indexOf('codesign --force --sign - "$INSTALL_DIR/$BINARY"')
    const bindingSign = script.indexOf('codesign --force --sign - "$LIB_DIR/$BINDING"')
    expect(binarySign).toBeGreaterThan(-1)
    expect(bindingSign).toBeGreaterThan(-1)

    const binaryRename = script.indexOf('mv -f "$BINARY_STAGE" "$INSTALL_DIR/$BINARY"')
    const bindingRename = script.indexOf('mv -f "$BINDING_STAGE" "$LIB_DIR/$BINDING"')
    expect(binaryRename).toBeGreaterThan(-1)
    expect(bindingRename).toBeGreaterThan(-1)

    // Destination signing must follow both renames.
    expect(bindingSign).toBeGreaterThan(bindingRename)
    expect(binarySign).toBeGreaterThan(binaryRename)

    // The staged copies must never be signed: that refreshes the wrong path.
    expect(script).not.toContain('codesign --force --sign - "$BINARY_STAGE"')
    expect(script).not.toContain('codesign --force --sign - "$BINDING_STAGE"')

    // Signing has to happen before the binary is executed for verification,
    // otherwise the install fails on the very SIGKILL this prevents.
    const startupCheck = script.indexOf('INSTALLED_VERSION="$(EVOT_HOME=')
    expect(startupCheck).toBeGreaterThan(binarySign)
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

  /**
   * A release download can connect at TCP+TLS and then deliver nothing. With
   * only --connect-timeout, curl has no deadline for that state and waits
   * indefinitely, which is what a user experiences as "update just hangs".
   */
  test('bounds a stalled transfer instead of waiting indefinitely', () => {
    const script = readFileSync(installShPath, 'utf8')

    expect(script).toContain('--speed-limit "$STALL_MIN_BYTES_PER_SEC"')
    expect(script).toContain('--speed-time "$STALL_WINDOW_SECONDS"')
    // Both branches of download() need it, not just the resuming one.
    expect(script.match(/--speed-limit/g)).toHaveLength(2)
    // The small-response path is bounded too, so a stalled version or checksum
    // lookup cannot hang the install before the download starts.
    expect(script).toContain('--max-time')
  })

  /**
   * Retry belongs to download_verified, which owns backoff, resume and the
   * discard rules. An in-tool retry underneath it multiplies rather than bounds:
   * `--retry 2` turns a 20s stall limit into 60s, which the outer loop then
   * triples, so a stalled host hangs for minutes.
   */
  test('does not nest in-tool retries inside the download retry loop', () => {
    const script = readFileSync(installShPath, 'utf8')
    const downloadFn = script.slice(
      script.indexOf('\ndownload() {'),
      script.indexOf('\n# Download, verify checksum'),
    )

    expect(downloadFn).not.toContain('--retry')
    expect(downloadFn).not.toContain('--tries=3')
    expect(downloadFn).toContain('--tries=1')
    // The outer loop is what supplies the attempts.
    expect(script).toContain('DOWNLOAD_ATTEMPTS=3')
  })

  /**
   * `-s` hides curl's transfer meter, so a ~40 MB download printed one line and
   * then looked frozen with no bytes, rate or ETA. The meter is opt-in on a
   * terminal only: piping the installer into a log would otherwise accumulate
   * thousands of carriage-returned progress lines.
   */
  test('shows download progress on a terminal and stays quiet when piped', () => {
    const script = readFileSync(installShPath, 'utf8')
    const downloadFn = script.slice(
      script.indexOf('\ndownload() {'),
      script.indexOf('\n# Download, verify checksum'),
    )

    // The asset download must not hard-code -s; quietness is decided by the tty
    // check and passed in.
    expect(downloadFn).not.toContain('-fsSL')
    expect(downloadFn.match(/\$CURL_QUIET/g)).toHaveLength(2)
    expect(downloadFn.match(/\$WGET_PROGRESS/g)).toHaveLength(2)

    // stderr, not stdout: the meter is written to stderr, and stdout is piped
    // for the `curl | sh` invocation that is the documented entry point.
    expect(script).toContain('if [ -t 2 ]; then')
    expect(script).toContain('CURL_QUIET="-s"')

    // -S must survive in both branches so a hard failure is still reported when
    // the meter is off, rather than the install dying silently.
    expect(downloadFn.match(/-fL \$CURL_QUIET -S/g)).toHaveLength(2)

    // --show-progress predates neither wget nor every distro's build of it.
    expect(script).toContain("wget --help 2>&1 | grep -q -- '--show-progress'")
  })

  /**
   * The logo header is for humans at a terminal. `curl | sh` keeps stdout on
   * the tty, so they see it; the in-app updater captures stdout through a
   * pipe, where six lines of block letters would only be TUI log noise.
   */
  test('shows the logo banner only on a terminal', () => {
    const script = readFileSync(installShPath, 'utf8')
    expect(script).toContain('[ -t 1 ] || return 0')
  })

  test("prints a ready line when the installed binary is on PATH", async () => {
    const installDir = join(root, 'ready', 'bin')
    const archive = packArchive('2026.7.19')

    // FAKE_BIN is prepended to PATH wholesale, so a colon-separated value can
    // put the not-yet-existing install dir on PATH before install.sh runs.
    const { stdout, exitCode } = await runInstallSh({
      FAKE_BIN: `${fakeBin(CURL_SERVES_ARCHIVE)}:${installDir}`,
      TEST_ARCHIVE: archive,
      EVOT_INSTALL_DIR: installDir,
      EVOT_INSTALL_VERSION: 'v2026.7.19',
    })

    expect(exitCode).toBe(0)
    expect(stdout).toContain("ready. run 'evot' to get started")
  })
})
