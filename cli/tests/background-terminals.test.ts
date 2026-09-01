import { describe, test, expect } from 'bun:test'
import { BackgroundTerminals } from '../src/term/app/background-terminals.js'
import { isBackgroundPanelTitle } from '../src/term/app/background-panel.js'
import type { SelectorState } from '../src/term/selector.js'
import type { BackgroundProcess } from '../src/native/index.js'

function proc(overrides: Partial<BackgroundProcess> = {}): BackgroundProcess {
  return {
    task_id: 'aaaaaaaa-1111',
    command: 'sleep 30',
    cwd: '/tmp',
    output_path: '/tmp/out.txt',
    status: 'running',
    exit_code: null,
    elapsed_ms: 1500,
    output_file_truncated: false,
    ...overrides,
  }
}

/**
 * Drives the controller with an in-memory overlay and client, mirroring how
 * `repl.ts` wires it: `panelOpen` is derived from the overlay's title, so the
 * tests exercise the same guard the REPL relies on.
 */
function harness(options: {
  processes?: BackgroundProcess[]
  sessionId?: string | null
  output?: string | (() => string)
  stopOne?: (taskId: string) => Promise<BackgroundProcess | null>
  stopAll?: () => Promise<BackgroundProcess[]>
  onList?: () => BackgroundProcess[]
} = {}) {
  let processes = options.processes ?? []
  let panel: SelectorState | null = null
  const commits: Array<{ slot: string; text: string }> = []
  let renders = 0

  const controller = new BackgroundTerminals({
    client: {
      backgroundProcesses: () => (options.onList ? options.onList() : processes),
      stopBackgroundProcess: async (_sessionId, taskId) => {
        if (options.stopOne) return options.stopOne(taskId)
        const target = processes.find(candidate => candidate.task_id === taskId)
        if (!target) return null
        const stopped = { ...target, status: 'killed' as const }
        processes = processes.map(candidate => candidate.task_id === taskId ? stopped : candidate)
        return stopped
      },
      stopAllBackgroundProcesses: async () => {
        if (options.stopAll) return options.stopAll()
        const live = processes.filter(candidate => candidate.status === 'running')
        processes = processes.map(candidate => ({ ...candidate, status: 'killed' as const }))
        return live
      },
      killAllBackgroundProcessesNow: () => processes.length,
    },
    sessionId: () => (options.sessionId === undefined ? 'session-1' : options.sessionId),
    commit: (slot, text) => commits.push({ slot, text }),
    requestRender: () => { renders++ },
    errorText: err => (err instanceof Error ? err.message : String(err)),
    readOutput: () => {
      if (typeof options.output === 'function') return options.output()
      return options.output ?? ''
    },
    openPanel: state => { panel = state },
    updatePanel: state => { panel = state },
    panelOpen: () => panel !== null && isBackgroundPanelTitle(panel.title),
    panelState: () => panel,
  })

  return {
    controller,
    commits,
    texts: () => commits.map(entry => entry.text),
    panel: () => panel,
    renders: () => renders,
    setProcesses: (next: BackgroundProcess[]) => { processes = next },
    processes: () => processes,
  }
}

describe('BackgroundTerminals.handlePromptDown', () => {
  test('↓ opens the panel on an empty composer with live work', () => {
    const h = harness({ processes: [proc()] })
    h.controller.refresh()
    expect(h.controller.handlePromptDown(true)).toBe(true)
    expect(h.panel()).not.toBeNull()
  })

  test('↓ is declined while the composer has text', () => {
    // Returning false leaves the key to the editor, which still moves the caret.
    const h = harness({ processes: [proc()] })
    h.controller.refresh()
    expect(h.controller.handlePromptDown(false)).toBe(false)
    expect(h.panel()).toBeNull()
  })

  test('↓ is declined when nothing is running', () => {
    const h = harness({ processes: [proc({ status: 'completed', exit_code: 0 })] })
    h.controller.refresh()
    expect(h.controller.handlePromptDown(true)).toBe(false)
    expect(h.panel()).toBeNull()
  })

  test('a foreground-only task does not arm ↓', () => {
    // It is already visible as a running tool card, so the prompt does not
    // advertise the panel for it and ↓ must keep its editor meaning.
    const h = harness({ processes: [proc({ status: 'running_foreground' })] })
    h.controller.refresh()
    expect(h.controller.handlePromptDown(true)).toBe(false)
  })

  test('the gesture and the prompt hint read the same polled snapshot', () => {
    // Both derive from `runningCount()`, so ↓ can never be live while the chip
    // above the composer is absent, or vice versa.
    const h = harness({ processes: [proc()] })
    expect(h.controller.hintVisible(true)).toBe(false)
    expect(h.controller.handlePromptDown(true)).toBe(false)

    h.controller.refresh()
    expect(h.controller.hintVisible(true)).toBe(true)
    expect(h.controller.hintVisible(false)).toBe(false)
  })

  test('an idle session never advertises the gesture', () => {
    const h = harness({ processes: [] })
    h.controller.refresh()
    expect(h.controller.hintVisible(true)).toBe(false)
  })
})

describe('BackgroundTerminals.togglePanel', () => {
  test('opens the panel over the current task list', () => {
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    expect(h.panel()).not.toBeNull()
    expect(h.panel()!.items).toHaveLength(1)
    expect(h.panel()!.subtitle).toBe('1 active shell')
  })

  test('a second press closes it, so one key round-trips', () => {
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    h.controller.togglePanel()
    expect(h.panel()).toBeNull()
  })

  test('opening refreshes first, so the list is never stale', () => {
    const h = harness({ processes: [] })
    h.setProcesses([proc(), proc({ task_id: 'bbbbbbbb-2222' })])
    h.controller.togglePanel()
    expect(h.panel()!.items).toHaveLength(2)
  })

  test('without a session it explains itself instead of opening empty', () => {
    const h = harness({ sessionId: null })
    h.controller.togglePanel()
    expect(h.panel()).toBeNull()
    expect(h.texts()[0]).toContain('No active session')
  })

  test('an empty list still opens the panel', () => {
    // The panel is the answer to "what is running?", including "nothing".
    const h = harness({ processes: [] })
    h.controller.togglePanel()
    expect(h.panel()).not.toBeNull()
    expect(h.panel()!.items).toHaveLength(0)
    expect(h.panel()!.emptyMessage).toBe('No tasks currently running')
  })
})

describe('BackgroundTerminals panel refresh', () => {
  test('a poll updates the open panel in place', () => {
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    h.setProcesses([proc({ status: 'completed', exit_code: 0 })])
    h.controller.refresh()
    expect(h.panel()!.items[0]!.detail).toBe('(exit 0 · 2s)')
    expect(h.panel()!.subtitle).toBe('1 finished')
  })

  test('a closed panel is not reopened by a poll', () => {
    const h = harness({ processes: [proc()] })
    h.controller.refresh()
    expect(h.panel()).toBeNull()
  })

  test('a failing list call keeps the last known state', () => {
    let fail = false
    const h = harness({
      processes: [proc()],
      onList: () => {
        if (fail) throw new Error('session gone')
        return [proc()]
      },
    })
    h.controller.refresh()
    expect(h.controller.runningCount()).toBe(1)
    fail = true
    h.controller.refresh()
    expect(h.controller.runningCount()).toBe(1)
  })
})

describe('BackgroundTerminals.handlePanelKey', () => {
  test('returns false when the panel is closed, leaving keys to the REPL', () => {
    const h = harness({ processes: [proc()] })
    expect(h.controller.handlePanelKey({ type: 'enter' })).toBe(false)
  })

  test('esc closes the panel and consumes the key', () => {
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    expect(h.controller.handlePanelKey({ type: 'escape' })).toBe(true)
    expect(h.panel()).toBeNull()
  })

  test('navigation keys are not consumed, so the selector still moves', () => {
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    expect(h.controller.handlePanelKey({ type: 'down' })).toBe(false)
  })

  test('enter commits the task output into the transcript', () => {
    const h = harness({ processes: [proc()], output: 'building…\ndone\n' })
    h.controller.togglePanel()
    expect(h.controller.handlePanelKey({ type: 'enter' })).toBe(true)
    expect(h.texts()[0]).toContain('sleep 30')
    expect(h.texts()).toContain('    building…')
    expect(h.texts()).toContain('    done')
  })

  test('an unreadable output file reports the error instead of throwing', () => {
    const h = harness({
      processes: [proc()],
      output: () => { throw new Error('ENOENT') },
    })
    h.controller.togglePanel()
    h.controller.handlePanelKey({ type: 'enter' })
    expect(h.texts()[0]).toContain('Could not read output for aaaaaaa')
    expect(h.texts()[0]).toContain('ENOENT')
  })

  test('x stops the focused task and confirms it', async () => {
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    expect(h.controller.handlePanelKey({ type: 'char', char: 'x' })).toBe(true)
    await h.controller.settled()
    expect(h.texts().join('\n')).toContain('Stopped aaaaaaaa')
    expect(h.processes()[0]!.status).toBe('killed')
  })

  test('x on a finished task is inert and leaves keys to the REPL', () => {
    const h = harness({ processes: [proc({ status: 'completed', exit_code: 0 })] })
    h.controller.togglePanel()
    expect(h.controller.handlePanelKey({ type: 'char', char: 'x' })).toBe(false)
    expect(h.texts()).toHaveLength(0)
  })

  test('a task that outlives the stop timeout is not claimed as stopped', async () => {
    const h = harness({
      processes: [proc()],
      stopOne: async () => proc({ status: 'running' }),
    })
    h.controller.togglePanel()
    h.controller.handlePanelKey({ type: 'char', char: 'x' })
    await h.controller.settled()
    expect(h.texts().join('\n')).toContain('did not stop within the timeout')
  })

  test('a stop failure is surfaced rather than swallowed', async () => {
    const h = harness({
      processes: [proc()],
      stopOne: async () => { throw new Error('signal refused') },
    })
    h.controller.togglePanel()
    h.controller.handlePanelKey({ type: 'char', char: 'x' })
    await h.controller.settled()
    expect(h.texts().join('\n')).toContain('signal refused')
  })

  test('shift+X stops every task and reports the count', async () => {
    const h = harness({
      processes: [proc(), proc({ task_id: 'bbbbbbbb-2222' })],
    })
    h.controller.togglePanel()
    expect(h.controller.handlePanelKey({ type: 'shift-char', char: 'x' })).toBe(true)
    await h.controller.settled()
    expect(h.texts().join('\n')).toContain('Stopped 2 background terminals')
  })

  test('stopping refreshes the panel, so the row shows its new status', async () => {
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    h.controller.handlePanelKey({ type: 'char', char: 'x' })
    await h.controller.settled()
    expect(h.panel()!.items[0]!.detail).toContain('stopped')
  })
})

describe('BackgroundTerminals panel entry', () => {
  test('togglePanel opens the panel rather than dumping lines', async () => {
    // Background work is managed only through the panel: there are no slash
    // commands, so this is the single entry point.
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    expect(h.panel()).not.toBeNull()
    expect(h.texts()).toHaveLength(0)
  })

  test('X stops every task from the panel', async () => {
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    h.controller.handlePanelKey({ type: 'char', char: 'X' })
    await h.controller.settled()
    expect(h.texts().join('\n')).toContain('Stopped 1 background terminal')
  })
})

describe('BackgroundTerminals.guardSessionSwitch', () => {
  test('warns once while work is live, then lets the repeat through', () => {
    const h = harness({ processes: [proc()] })
    expect(h.controller.guardSessionSwitch('/clear')).toBe(true)
    expect(h.texts().join('\n')).toContain('ctrl+t to manage')
    expect(h.controller.guardSessionSwitch('/clear')).toBe(false)
  })

  test('an idle session is never gated', () => {
    const h = harness({ processes: [proc({ status: 'completed', exit_code: 0 })] })
    expect(h.controller.guardSessionSwitch('/clear')).toBe(false)
  })
})
