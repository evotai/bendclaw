import { describe, test, expect, skipIf } from 'bun:test'
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
  kill: () => Promise<void>
}

function stripAnsi(text: string): string {
  return text.replace(/\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\)|_.*?\x07)/g, '')
}

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
  await wait(1500)
  return {
    write: data => { child.stdin!.write(data) },
    outputSince: () => all.slice(seen),
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
      expect(stripAnsi(session.outputSince())).toContain('Enter a coding task')

      session.write('hello smoke')
      await new Promise(resolve => setTimeout(resolve, 600))
      expect(stripAnsi(session.outputSince())).toContain('hello smoke')
    } finally {
      await session.kill()
    }
  }, 20_000)

  test('submitting a prompt commits it to the transcript', async () => {
    const session = await startEvot()
    try {
      session.write('echo smoke test\x0d')
      await new Promise(resolve => setTimeout(resolve, 1500))
      expect(stripAnsi(session.outputSince())).toContain('▍ echo smoke test')
    } finally {
      await session.kill()
    }
  }, 20_000)

  test('ctrl+c twice exits cleanly', async () => {
    const session = await startEvot()
    session.write('\x03')
    await new Promise(resolve => setTimeout(resolve, 400))
    session.write('\x03')
    await new Promise(resolve => setTimeout(resolve, 800))
    expect(stripAnsi(session.outputSince())).toMatch(/Press Ctrl\+C again|exited|Goodbye|bye/i)
    await session.kill()
  }, 20_000)
})
