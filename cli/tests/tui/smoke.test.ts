import { describe, test, expect } from 'bun:test'
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const EVOT_BIN = join(import.meta.dirname, '..', '..', 'dist', 'evot')
const canRun = process.platform !== 'win32' && existsSync(EVOT_BIN) && !!spawnSync('python3', ['--version']).stdout

const PTY_RELAY = `
import os, pty, select, sys, signal
status = 0
pid, fd = pty.fork()
if pid == 0:
    os.environ['TERM'] = 'xterm-256color'
    os.execv(sys.argv[1], [sys.argv[1]])
try:
    while True:
        r, _, _ = select.select([sys.stdin.buffer, fd], [], [], 0.1)
        if sys.stdin.buffer in r:
            data = sys.stdin.buffer.read1(4096)
            if not data:
                break
            os.write(fd, data)
        if fd in r:
            try:
                data = os.read(fd, 65536)
            except OSError:
                break
            if not data:
                break
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
    _, status = os.waitpid(pid, 0)
except KeyboardInterrupt:
    pass
finally:
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
sys.exit(os.waitstatus_to_exitcode(status) if status else 0)
`

type Session = {
  write: (data: string) => void
  outputSince: () => string
  /**
   * Resolve once the output since the last checkpoint matches, or reject after
   * `timeoutMs`.
   *
   * Fixed sleeps made this suite flaky: a 600ms wait is ample on an idle
   * machine and far too short when 40+ test files compete for CPU, so the whole
   * file failed under load while passing in isolation. Polling makes the wait
   * proportional to how long the TUI actually takes.
   */
  waitFor: (match: string | RegExp, timeoutMs?: number) => Promise<string>
  kill: () => Promise<void>
}

function stripAnsi(text: string): string {
  return text.replace(/\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\)|_.*?\x07)/g, '')
}

/** Generous by design: it only bounds failure, since a match returns early. */
const DEFAULT_WAIT_MS = 15_000
const POLL_INTERVAL_MS = 50

async function startEvot(): Promise<Session> {
  // Isolated EVOT_HOME: a dev machine may hold a staged release newer than this binary.
  const isolatedHome = mkdtempSync(join(tmpdir(), 'evot-smoke-home-'))
  const child = spawn('python3', ['-c', PTY_RELAY, EVOT_BIN], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, TERM: 'xterm-256color', EVOT_MOUSE: '0', EVOT_HOME: isolatedHome },
  })
  let all = ''
  let seen = 0
  child.stdout!.on('data', (chunk: Buffer) => { all += chunk.toString('utf-8') })
  child.stderr!.on('data', (chunk: Buffer) => { all += chunk.toString('utf-8') })
  const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

  const outputSince = () => all.slice(seen)
  const waitFor = async (match: string | RegExp, timeoutMs = DEFAULT_WAIT_MS): Promise<string> => {
    const matches = (text: string) =>
      typeof match === 'string' ? text.includes(match) : match.test(text)
    const deadline = Date.now() + timeoutMs
    for (;;) {
      const text = stripAnsi(outputSince())
      if (matches(text)) return text
      if (Date.now() >= deadline) {
        // Surface what did arrive: a bare timeout says nothing about whether the
        // TUI was slow, crashed, or rendered something unexpected.
        const tail = text.slice(-800)
        throw new Error(
          `timed out after ${timeoutMs}ms waiting for ${match}\n--- output tail ---\n${tail}`,
        )
      }
      await wait(POLL_INTERVAL_MS)
    }
  }

  // The composer prompt is the readiness signal, so boot time is waited out
  // rather than guessed at.
  await waitFor('Enter a coding task')
  return {
    write: data => { child.stdin!.write(data) },
    outputSince,
    waitFor,
    kill: async () => {
      seen = all.length
      child.stdin!.write('\x03')
      await wait(300)
      child.stdin!.write('\x03')
      await wait(500)
      child.kill('SIGKILL')
      rmSync(isolatedHome, { recursive: true, force: true })
    },
  }
}

describe.skipIf(!canRun)('evot binary smoke (PTY)', () => {
  test('renders the composer and echoes typed text', async () => {
    const session = await startEvot()
    try {
      // startEvot already waited for the prompt; assert it explicitly so the
      // readiness contract is visible in the test body.
      expect(stripAnsi(session.outputSince())).toContain('Enter a coding task')

      session.write('hello smoke')
      await session.waitFor('hello smoke')
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('submitting a prompt commits it to the transcript', async () => {
    const session = await startEvot()
    try {
      session.write('echo smoke test\x0d')
      await session.waitFor('▍ echo smoke test')
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('ctrl+c twice exits cleanly', async () => {
    const session = await startEvot()
    try {
      session.write('\x03')
      await session.waitFor(/Press Ctrl\+C again|exited|Goodbye|bye/i)
      session.write('\x03')
    } finally {
      await session.kill()
    }
  }, 60_000)
})
