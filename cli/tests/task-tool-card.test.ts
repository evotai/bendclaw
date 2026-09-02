import { describe, test, expect, beforeEach } from 'bun:test'
import { buildToolCard, resetIdCounter } from '../src/render/output.js'

beforeEach(() => {
  resetIdCounter()
})

const strip = (text: string): string => text.replace(/\x1b\[[0-9;]*m/g, '')

/** Render a card and return its lines with ANSI removed. */
function render(call: Record<string, unknown>, expanded = false): string[] {
  return buildToolCard(call as never, expanded).map(line => strip(line.text))
}

/** The `● · ...` / `✓ · ...` row, which is the line under test throughout. */
function statusLine(lines: string[]): string {
  return lines.find(line => /^ {2}[○●✓✗]/.test(line)) ?? ''
}

function poll(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'call-1',
    name: 'task_output',
    args: { task_id: 'task-abc' },
    previewCommand: 'cargo test -p evotengine',
    status: 'running',
    ...overrides,
  }
}

function settled(
  details: Record<string, unknown>,
  overrides: Record<string, unknown> = {},
): Record<string, unknown> {
  return poll({ status: 'done', result: 'body', details, ...overrides })
}

describe('task tool card identity', () => {
  test('the headline names the command, not the tool', () => {
    // The internal tool name is implementation detail; use the same
    // "background shell" term as the status row and management panel.
    expect(render(poll())[0]).toBe('◷ background shell  cargo test -p evotengine')
  })

  test('concurrent polls render distinguishable headlines', () => {
    const commands = ['cargo test -p evotengine', 'bun test', 'npm run build']
    const headlines = commands.map((previewCommand, index) =>
      render(poll({ id: `call-${index}`, previewCommand }))[0],
    )
    expect(new Set(headlines).size).toBe(3)
    for (const [index, headline] of headlines.entries()) {
      expect(headline).toContain(commands[index]!)
    }
  })

  test('task tools get their own glyphs, not the anonymous dot', () => {
    expect(render(poll())[0]?.startsWith('◷ ')).toBe(true)
    expect(render(poll({ name: 'task_stop' }))[0]?.startsWith('⊘ ')).toBe(true)
  })

  test('claude-style tool aliases render the same way', () => {
    // The engine exposes TaskOutput/TaskStop to some models; the card must not
    // fall through to generic rendering for those names.
    expect(render(poll({ name: 'TaskOutput' }))[0]).toBe('◷ background shell  cargo test -p evotengine')
    expect(render(poll({ name: 'TaskStop' }))[0]).toBe('⊘ stop background shell  cargo test -p evotengine')
  })

  test('a multi-line command collapses to one headline row', () => {
    const lines = render(poll({ previewCommand: 'cargo test \\\n  --workspace' }))
    expect(lines[0]).toContain('cargo test')
    expect(lines[0]).not.toContain('--workspace')
  })

  test('without a preview the raw task id still names the card', () => {
    // Resume from transcript has no previewCommand; an empty headline would
    // leave a nameless card.
    const lines = render(poll({ previewCommand: undefined }))
    expect(lines[0]).toBe('◷ background shell  task-abc')
  })
})

describe('task tool in-flight status', () => {
  test('a blocking poll says it is waiting, not merely running', () => {
    expect(statusLine(render(poll()))).toBe('  ● · waiting for task')
  })

  test('block defaults to true, matching the engine', () => {
    // The engine reads `block` with `is_none_or`, so an absent value waits.
    expect(statusLine(render(poll({ args: { task_id: 'x' } })))).toBe('  ● · waiting for task')
  })

  test('a non-blocking poll says it is only checking', () => {
    const card = poll({ args: { task_id: 'x', block: false } })
    expect(statusLine(render(card))).toBe('  ● · checking task')
  })

  test('a stop in flight says what it is doing', () => {
    expect(statusLine(render(poll({ name: 'task_stop' })))).toBe('  ● · stopping task')
  })

  test('other tools keep the plain running status', () => {
    const bash = { id: 'b', name: 'bash', args: { command: 'ls' }, status: 'running' }
    expect(statusLine(render(bash))).toBe('  ● · running')
  })
})

describe('task tool settled status', () => {
  test('a completed task leads with its outcome and exit code', () => {
    const lines = render(settled({
      retrieval_status: 'success',
      status: 'completed',
      exit_code: 0,
      elapsed_ms: 107_000,
      total_lines: 412,
    }))
    expect(statusLine(lines)).toBe('  ✓ · completed · exit 0 · 1m 47s · 412 lines')
  })

  test('a failed task is marked failed, not as a successful poll', () => {
    // The poll succeeded but the build did not. Marking this ✓ would report a
    // broken build as a success.
    const lines = render(settled({
      retrieval_status: 'success',
      status: 'failed',
      exit_code: 1,
      elapsed_ms: 8_000,
      total_lines: 30,
    }))
    expect(statusLine(lines)).toBe('  ✗ · failed · exit 1 · 8s · 30 lines')
  })

  test('a stopped task reads as stopped and stays non-failing', () => {
    // A stop is a deliberate outcome, not an error.
    const lines = render(settled({ status: 'killed', elapsed_ms: 300_000 }))
    expect(statusLine(lines)).toBe('  ✓ · stopped · 5m')
  })

  test('a user cancellation is named as cancelled, not merely stopped', () => {
    // "cancelled" carries that the work is void; "stopped" only describes the
    // action, which left room to re-run the same work another way.
    const lines = render(settled({
      status: 'killed',
      elapsed_ms: 21_000,
      stopped_by_user: true,
    }))
    expect(statusLine(lines)).toBe('  ✓ · cancelled by user · 21s')
  })

  test('a model stop is not attributed to the user', () => {
    const lines = render(settled({
      status: 'killed',
      elapsed_ms: 21_000,
      stopped_by_user: false,
    }))
    expect(statusLine(lines)).toBe('  ✓ · stopped · 21s')
  })

  test('a missing attribution flag degrades to a plain stop', () => {
    // Unlike the panel's typed BackgroundProcess, a card reads untyped tool-result
    // details, so a missing key is a real parse case rather than a type error.
    // Attributing an unattributed stop to the user would be the worse guess.
    const lines = render(settled({ status: 'killed', elapsed_ms: 21_000 }))
    expect(statusLine(lines)).toBe('  ✓ · stopped · 21s')
  })

  test('a legacy expired wait still renders from a replayed transcript', () => {
    // Nothing writes `timeout` any more: a blocking wait now ends only when the
    // task finishes or the user reclaims the turn. Tool-result details are
    // persisted per transcript, so sessions recorded before that change replay
    // through this branch and must keep explaining themselves.
    const lines = render(settled({
      retrieval_status: 'timeout',
      status: 'running',
      elapsed_ms: 45_000,
      total_lines: 12,
    }))
    expect(statusLine(lines)).toBe('  ● · running in background · wait timed out · 45s · 12 lines')
  })

  test('a wait the user ended reads as plainly detached, not as a timeout', () => {
    // Nothing went wrong and no deadline was hit: the user reclaimed the turn
    // and the task is still running. Reporting a timeout here would suggest the
    // task was slow, which is a different problem with a different fix. It also
    // earns no clause of its own — "stopped waiting" only repeated what
    // "running in background" already says.
    const lines = render(settled({
      retrieval_status: 'released',
      status: 'running',
      elapsed_ms: 45_000,
      total_lines: 12,
    }))
    expect(statusLine(lines)).toBe('  ● · running in background · 45s · 12 lines')
  })

  test('a non-blocking check that found work in progress omits the timeout note', () => {
    const lines = render(settled({
      retrieval_status: 'not_ready',
      status: 'running',
      elapsed_ms: 3_000,
    }))
    expect(statusLine(lines)).toBe('  ● · running in background · 3s')
  })

  test('a backgrounded task names the background, a foreground one does not', () => {
    // `running` is RunningBackground, `running_foreground` is a shell still being
    // waited on. Both used to render as a bare "still running", so the card could
    // not tell a user whether the turn was already free. This is the pair that
    // distinction rests on, so it is asserted directly rather than inferred from
    // the retrieval-status cases above.
    const background = render(settled({
      retrieval_status: 'not_ready',
      status: 'running',
      elapsed_ms: 3_000,
    }))
    expect(statusLine(background)).toBe('  ● · running in background · 3s')

    const foreground = render(settled({
      retrieval_status: 'not_ready',
      status: 'running_foreground',
      elapsed_ms: 3_000,
    }))
    expect(statusLine(foreground)).toBe('  ● · still running · 3s')
  })

  test('an engine-side error still renders as a failure', () => {
    const lines = render(settled({ error: true }, { status: 'error', result: 'boom' }))
    expect(statusLine(lines).startsWith('  ✗')).toBe(true)
  })

  test('a task with no captured output omits the line count', () => {
    const lines = render(settled({ status: 'completed', exit_code: 0, elapsed_ms: 2_000, total_lines: 0 }))
    expect(statusLine(lines)).toBe('  ✓ · completed · exit 0 · 2s')
  })

  test('missing elapsed time degrades instead of printing NaN', () => {
    const lines = render(settled({ status: 'completed', exit_code: 0 }))
    expect(statusLine(lines)).toBe('  ✓ · completed · exit 0')
  })

  test('an unrecognized status is shown verbatim rather than dropped', () => {
    // Forward compatibility: a status added engine-side should still surface.
    const lines = render(settled({ status: 'paused', elapsed_ms: 1_000 }))
    expect(statusLine(lines)).toBe('  ✓ · paused · 1s')
  })

  test('elapsed formatting matches the background panel spelling', () => {
    const cases: [number, string][] = [
      [1_500, '2s'],
      [59_000, '59s'],
      [94_000, '1m 34s'],
      [120_000, '2m'],
      [3_600_000, '1h'],
      [5_400_000, '1h 30m'],
    ]
    for (const [elapsed_ms, expected] of cases) {
      const lines = render(settled({ status: 'completed', exit_code: 0, elapsed_ms }))
      expect(statusLine(lines)).toBe(`  ✓ · completed · exit 0 · ${expected}`)
    }
  })

  test('the card keeps headline-then-status geometry', () => {
    // Every lifecycle state shares this shape, so the transcript does not
    // reflow when a card settles.
    const lines = render(settled({ status: 'completed', exit_code: 0, elapsed_ms: 1_000 }))
    expect(lines[0]?.startsWith('◷ background shell')).toBe(true)
    expect(lines[1]).toBe('  ✓ · completed · exit 0 · 1s')
  })
})
