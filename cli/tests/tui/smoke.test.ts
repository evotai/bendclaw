import { describe, test, expect } from 'bun:test'
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, utimesSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { getTheme } from '../../src/render/theme.js'

const EVOT_BIN = process.env.EVOT_TEST_BIN || join(import.meta.dirname, '..', '..', 'dist', 'evot')
const canRun = process.platform !== 'win32' && existsSync(EVOT_BIN) && !!spawnSync('python3', ['--version']).stdout

function selectionBackgroundAnsi(): string {
  const [red, green, blue] = getTheme().selectionBgHex
    .slice(1)
    .match(/.{2}/g)!
    .map(part => Number.parseInt(part, 16))
  return `\x1b[48;2;${red};${green};${blue}m`
}

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
  checkpoint: () => void
  persistedSessionCount: () => number
  kill: () => Promise<void>
}

function stripAnsi(text: string): string {
  return text.replace(/\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\)|_.*?\x07)/g, '')
}

/** Generous by design: it only bounds failure, since a match returns early. */
const DEFAULT_WAIT_MS = 15_000
const POLL_INTERVAL_MS = 50

const SEEDED_SESSION_ID = '018f0000-0000-7000-8000-000000000001'
const OTHER_CWD_SESSION_ID = '028f0000-0000-7000-8000-000000000002'

function seedResumeSession(
  home: string,
  opts: { id?: string; cwd?: string; title?: string; updatedAt?: string } = {},
): void {
  const id = opts.id ?? SEEDED_SESSION_ID
  const sessionDir = join(home, 'sessions', id)
  mkdirSync(sessionDir, { recursive: true })
  const now = opts.updatedAt ?? new Date().toISOString()
  writeFileSync(join(sessionDir, 'session.json'), JSON.stringify({
    session_id: id,
    cwd: opts.cwd ?? process.cwd(),
    model: 'smoke-model',
    provider: 'smoke',
    thinking_level: null,
    title: opts.title ?? 'smoke resume fixture',
    source: 'repl',
    turns: 1,
    message_count: 1,
    context_tokens: 0,
    context_budget: 0,
    total_input_tokens: 0,
    total_output_tokens: 0,
    span_count: 0,
    created_at: now,
    updated_at: now,
  }))
  const transcriptPath = join(sessionDir, 'transcript.jsonl')
  writeFileSync(transcriptPath, `${JSON.stringify([{
    session_id: id,
    run_id: null,
    seq: 1,
    turn: 1,
    item: { type: 'user', text: 'smoke resume prompt', content: [] },
    created_at: now,
  }])}\n`)
  if (opts.updatedAt) {
    const timestamp = new Date(opts.updatedAt)
    utimesSync(transcriptPath, timestamp, timestamp)
  }
}

function seedResumePreviewCacheMiss(home: string): string[] {
  // Startup preloads the globally newest 20 sessions. Make all of those belong
  // to another project, while three older sessions belong to this cwd. The
  // live `/res` preview must therefore expand beyond its startup cache to agree
  // with the full selector opened by Enter.
  for (let index = 0; index < 20; index++) {
    const suffix = (index + 1).toString(16).padStart(12, '0')
    seedResumeSession(home, {
      id: `f00000${index.toString(16).padStart(2, '0')}-0000-7000-8000-${suffix}`,
      cwd: '/tmp/newer-other-project',
      title: `newer other session ${index + 1}`,
      updatedAt: `2026-08-${String(index + 1).padStart(2, '0')}T12:00:00Z`,
    })
  }

  const currentIds = [
    '11000000-0000-7000-8000-000000000001',
    '12000000-0000-7000-8000-000000000002',
    '13000000-0000-7000-8000-000000000003',
  ]
  currentIds.forEach((id, index) => {
    seedResumeSession(home, {
      id,
      title: `older current session ${index + 1}`,
      updatedAt: `2025-01-0${index + 1}T12:00:00Z`,
    })
  })
  return currentIds
}

async function startEvot(
  seedSession = false,
  seedOtherCwd = false,
  seedPreviewCacheMiss = false,
): Promise<Session> {
  // Isolated EVOT_HOME: a dev machine may hold a staged release newer than this binary.
  const isolatedHome = mkdtempSync(join(tmpdir(), 'evot-smoke-home-'))
  if (seedSession) seedResumeSession(isolatedHome)
  if (seedOtherCwd) {
    seedResumeSession(isolatedHome, {
      id: OTHER_CWD_SESSION_ID,
      cwd: '/tmp/other-project',
      title: 'other cwd fixture',
    })
  }
  if (seedPreviewCacheMiss) seedResumePreviewCacheMiss(isolatedHome)
  const child = spawn('python3', ['-c', PTY_RELAY, EVOT_BIN], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: {
      ...process.env,
      TERM: 'xterm-256color',
      COLORTERM: 'truecolor',
      EVOT_THEME: 'dark',
      EVOT_MOUSE: '0',
      EVOT_HOME: isolatedHome,
      EVOT_STORAGE_FS_ROOT_DIR: isolatedHome,
    },
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
    checkpoint: () => { seen = all.length },
    persistedSessionCount: () => {
      const sessionsDir = join(isolatedHome, 'sessions')
      return existsSync(sessionsDir) ? readdirSync(sessionsDir).length : 0
    },
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

  test('/new stays unbound until the first real prompt', async () => {
    const session = await startEvot()
    try {
      expect(session.persistedSessionCount()).toBe(0)
      session.write('/new\x0d')
      await session.waitFor('new session')
      expect(session.persistedSessionCount()).toBe(0)
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('a command window previews live, then arrows transfer focus', async () => {
    const session = await startEvot()
    try {
      // A unique prefix immediately renders the formal model window above the
      // composer, but the highlighted row already uses the selector's complete
      // current-row treatment while keyboard focus remains in the composer.
      session.checkpoint()
      session.write('/mo')
      const preview = await session.waitFor('Only showing models from configured providers')
      expect(preview).toContain('Model Name:')
      expect(preview).toContain('/mo')
      expect(preview).toMatch(/❯\s+GPT 5\.6 Sol/)
      expect(session.outputSince()).toContain(selectionBackgroundAnsi())

      // Continued typing still belongs to the composer, not the model filter.
      session.checkpoint()
      session.write('del')
      const continued = await session.waitFor('/model')
      expect(continued).toContain('/model')
      expect(continued).not.toContain('> del')

      // Argument entry hides the no-argument command window immediately. When
      // the command becomes argument-free again, the same preview returns.
      session.checkpoint()
      session.write(' ')
      await session.waitFor('/model ')
      session.checkpoint()
      session.write('\x7f')
      const restored = await session.waitFor('Only showing models from configured providers')
      expect(restored).toContain('/model')

      // The first arrow transfers focus without replacing the layout. The
      // blurred composer stays in the same frame below the active selector.
      // Promotion lands on the model the preview already highlighted, so its
      // unified current-row appearance does not need to be repainted.
      session.checkpoint()
      session.write('\x1b[B')
      await Bun.sleep(100)
      expect(session.outputSince()).not.toContain('\x1b[2J')

      // Once promoted, subsequent arrows belong to the formal selector.
      session.checkpoint()
      session.write('\x1b[B')
      const navigated = await session.waitFor('Model Name:')
      expect(navigated).toContain('Model Name: Claude Opus 5')

      session.checkpoint()
      session.write('\x1b')
      await session.waitFor('Enter a coding task')

      // Enter keeps its original immediate-submit behavior; arrows are only
      // needed to transfer focus without submitting the command.
      session.checkpoint()
      session.write('/model\x0d')
      const submitted = await session.waitFor('Only showing models from configured providers')
      expect(submitted).toContain('Model Name:')
      session.write('\x1b')
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('skill command window opens live and stays stable through focus', async () => {
    const session = await startEvot()
    try {
      // `/ski` uniquely identifies `/skill`; no Enter is needed to open the
      // installed-skill inventory and the composer keeps keyboard focus.
      session.checkpoint()
      session.write('/ski')
      const preview = await session.waitFor('installed · /skill list for sources and management')
      expect(preview).toContain('Skills')
      expect(preview).toContain('/ski')
      expect(preview).not.toContain('\x1b[2J')

      // The first arrow transfers focus without replacing or clearing the
      // selector/composer frame. The current-row marker is already present;
      // promotion changes keyboard ownership only. Enter is intentionally inert
      // in this read-only inventory; management stays explicit via `/skill ...`.
      session.checkpoint()
      session.write('\x1b[B')
      await Bun.sleep(100)
      expect(session.outputSince()).not.toContain('\x1b[2J')

      session.checkpoint()
      session.write('\x0d')
      await Bun.sleep(100)
      expect(session.outputSince()).not.toContain('\x1b[2J')

      session.checkpoint()
      session.write('\x1b')
      await session.waitFor('Enter a coding task')
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('resume preview stays responsive through rapid typing and deletion', async () => {
    const session = await startEvot(true)
    try {
      const createdSessionId = SEEDED_SESSION_ID.slice(0, 8)

      // The whole command can arrive in one input burst. Delete it completely
      // and retype the /re prefix immediately; input must remain responsive.
      session.checkpoint()
      session.write('/resume\x7f\x7f\x7f\x7f\x7f\x7f\x7f/re')
      const preview = await session.waitFor('/re', 1_000)
      expect(preview).toContain('Resume session')
      expect(preview).toContain('type to search titles, prompts and transcript text')

      // A unique prefix is enough: the existing session appears without
      // completing /resume or pressing an arrow. Like `/mo`, the preview uses
      // the complete shared current-row treatment before keyboard promotion.
      const populated = await session.waitFor(createdSessionId!)
      expect(populated).toContain('Resume session')
      expect(populated).toMatch(new RegExp(`❯\\s+${createdSessionId}`))
      expect(session.outputSince()).toContain(selectionBackgroundAnsi())

      // An ambiguous bare slash is a bridge between command windows. Keep the
      // session window mounted while `/re` is erased, then replace it in place
      // once `/mo` identifies the model window. Wait for the initial async
      // session paint first so each checkpoint observes one editor transition.
      await Bun.sleep(200)
      session.checkpoint()
      session.write('\x7f')
      const shortened = await session.waitFor('│  /r')
      expect(shortened).not.toContain('\x1b[2J')
      session.checkpoint()
      session.write('\x7f')
      const bridged = await session.waitFor('│  /')
      expect(bridged).not.toContain('\x1b[2J')
      session.checkpoint()
      session.write('mo')
      const switched = await session.waitFor('Model Name:')
      expect(switched).toContain('/mo')
      expect(switched).not.toContain('\x1b[2J')

      // Return to resume so the rest of this test exercises its focus path.
      session.checkpoint()
      session.write('\x7f\x7fre')
      await session.waitFor(createdSessionId!)

      // The first arrow transfers focus directly to the first session. The /re
      // composer remains in the same frame while metadata expansion waits for
      // keyboard idle; no intermediate Filter-focused keypress is required.
      session.checkpoint()
      session.write('\x1b[A')
      await Bun.sleep(100)
      expect(session.outputSince()).not.toContain('\x1b[2J')

      session.checkpoint()
      session.write('\x1b')
      await session.waitFor('Enter a coding task')

      // Closing immediately after focus promotion must cancel deferred metadata
      // enrichment. A late native result may fill caches, but must never reopen
      // the selector after Esc returned to the empty composer.
      session.checkpoint()
      session.write('/re\x1b[B\x1b')
      await session.waitFor('Enter a coding task')
      await Bun.sleep(250)
      const afterQuickClose = stripAnsi(session.outputSince())
      expect(afterQuickClose).not.toContain('Loading sessions…')
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('submitted resume stays closed when Esc wins the async load', async () => {
    const session = await startEvot(true, false, true)
    try {
      // Submit the command rather than using the live command window, then
      // cancel in the same input burst. Any metadata/text result that resolves
      // later belongs to the cancelled generation and must not reopen it.
      session.checkpoint()
      session.write('/resume\x0d\x1b')
      await session.waitFor('Enter a coding task')
      session.checkpoint()
      await Bun.sleep(300)
      const afterLoad = stripAnsi(session.outputSince())
      expect(afterLoad).not.toContain('Loading sessions…')
      expect(afterLoad).not.toContain('Resume session')
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('a first prompt invalidates resume caches and exposes its new session', async () => {
    const session = await startEvot()
    try {
      // Opening and closing first creates a complete empty cache. The prompt
      // then persists a formerly unbound session; reopening must not reuse the
      // empty snapshot.
      session.write('/resume\x0d')
      await session.waitFor('No sessions found')
      session.checkpoint()
      session.write('cache refresh smoke\x0d')
      await session.waitFor('▍ cache refresh smoke')
      for (let i = 0; i < 100 && session.persistedSessionCount() !== 1; i++) {
        await Bun.sleep(20)
      }
      expect(session.persistedSessionCount()).toBe(1)

      // Persistence happens before the provider necessarily finishes. Return
      // to an idle composer so the next command is executed, not queued.
      session.checkpoint()
      session.write('\x1b')
      await session.waitFor('Enter a coding task')

      session.checkpoint()
      session.write('/resume\x0d')
      const reopened = await session.waitFor('Resume session')
      expect(reopened).toMatch(/Resume session.*\s1(?:\r|\n)/)
      expect(reopened).toContain('Current cwd')
      expect(reopened).toContain('(untitled)')
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('resume defaults to current cwd and searches other cwd on demand', async () => {
    const session = await startEvot(true, true)
    try {
      session.checkpoint()
      session.write('/re')
      const current = await session.waitFor(SEEDED_SESSION_ID.slice(0, 8))
      expect(current).toContain('Resume session')
      expect(current).toContain('smoke resume fixture')
      expect(current).not.toContain('Other cwd')
      expect(current).not.toContain('other cwd fixture')

      // The first arrow lands on the first current-project session instead of
      // stopping in the filter input or skipping to the second row.
      session.checkpoint()
      session.write('\x1b[B')
      await Bun.sleep(100)
      expect(session.outputSince()).not.toContain('\x1b[2J')

      // Typing after activation returns focus to Filter and expands search to
      // cross-project history without making it part of the default recents.
      session.checkpoint()
      session.write('other cwd fixture')
      const searched = await session.waitFor(OTHER_CWD_SESSION_ID.slice(0, 8))
      expect(searched).toContain('Other cwd')
      expect(searched).toContain('other cwd fixture')
    } finally {
      await session.kill()
    }
  }, 60_000)

  test('resume preview expands beyond the global startup cache without Enter', async () => {
    const session = await startEvot(false, false, true)
    try {
      session.checkpoint()
      session.write('/res')

      // The globally newest 20 fixtures all belong to another cwd, so none of
      // these current-project rows can come from the bounded startup cache.
      // Seeing all three proves the live preview automatically loaded the same
      // complete metadata catalog used by submitted `/resume`.
      const expanded = await session.waitFor('11000000')
      await session.waitFor('12000000')
      await session.waitFor('13000000')
      expect(expanded).toMatch(/Resume session.*\s3(?:\r|\n)/)
      expect(expanded).toContain('older current session 1')
      expect(expanded).not.toContain('Other cwd')
      expect(expanded).not.toContain('newer other session')
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
