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
    stopped_by_user: false,
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
  backgroundForeground?: () => number
  startingBlockingWaits?: number
  blockingWaits?: () => number
  releaseBlockingWaits?: () => number
  onList?: () => BackgroundProcess[]
  /** Queued completion notices the engine has not yet handed to a turn. */
  pendingNotifications?: () => number
  runInFlight?: () => boolean
  queuedMessages?: () => number
  overlayBlocking?: () => boolean
  /** Present by default so a wake is observable; pass null to omit the hook. */
  wake?: (() => void) | null
} = {}) {
  let processes = options.processes ?? []
  let panel: SelectorState | null = null
  const commits: Array<{ slot: string; text: string }> = []
  let renders = 0
  let messageDetaches = 0
  let wakes = 0
  let blockingWaits = options.startingBlockingWaits ?? 0

  // Shared by both detach entry points so a test cannot pass for one and fail
  // for the other. Mirrors the native side: only foreground shells move, and
  // they keep running.
  const detachForeground = (): number => {
    if (options.backgroundForeground) return options.backgroundForeground()
    const moved = processes.filter(candidate => candidate.status === 'running_foreground')
    processes = processes.map(candidate =>
      candidate.status === 'running_foreground'
        ? { ...candidate, status: 'running' as const }
        : candidate,
    )
    return moved.length
  }

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
      backgroundForegroundProcesses: () => detachForeground(),
      backgroundForegroundProcessesForMessage: () => {
        messageDetaches++
        return detachForeground()
      },
      blockingTaskWaits: () => {
        if (options.blockingWaits) return options.blockingWaits()
        return blockingWaits
      },
      releaseBlockingTaskWaits: () => {
        if (options.releaseBlockingWaits) return options.releaseBlockingWaits()
        const released = blockingWaits
        blockingWaits = 0
        return released
      },
      killAllBackgroundProcessesNow: () => processes.length,
      pendingProcessNotifications: () => options.pendingNotifications?.() ?? 0,
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
    runInFlight: () => options.runInFlight?.() ?? false,
    queuedMessages: () => options.queuedMessages?.() ?? 0,
    overlayBlocking: () => options.overlayBlocking?.() ?? false,
    wakeForNotifications: options.wake === null
      ? undefined
      : (options.wake ?? (() => { wakes++ })),
  })

  return {
    controller,
    commits,
    texts: () => commits.map(entry => entry.text),
    panel: () => panel,
    renders: () => renders,
    setProcesses: (next: BackgroundProcess[]) => { processes = next },
    processes: () => processes,
    messageDetaches: () => messageDetaches,
    wakes: () => wakes,
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

describe('BackgroundTerminals.backgroundForeground', () => {
  test('hands foreground shells to the background without killing them', () => {
    // The point of the gesture: reclaim the turn, keep the work. A killed task
    // would lose however far a build or test run had already got.
    const h = harness({ processes: [proc({ status: 'running_foreground' })] })
    h.controller.refresh()
    expect(h.controller.foregroundCount()).toBe(1)

    expect(h.controller.backgroundForeground()).toBe(1)

    expect(h.controller.foregroundCount()).toBe(0)
    // Still live, just no longer waited on, so the footer now counts it.
    expect(h.controller.runningCount()).toBe(1)
  })

  test('reports nothing moved when no shell is in the foreground', () => {
    // Lets the caller fall through to interrupting, so esc is never inert.
    const h = harness({ processes: [proc({ status: 'running' })] })
    expect(h.controller.foregroundCount()).toBe(0)
    expect(h.controller.backgroundForeground()).toBe(0)
  })

  test('a session that disappeared mid-gesture reports nothing moved', () => {
    const h = harness({ processes: [proc({ status: 'running_foreground' })], sessionId: null })
    expect(h.controller.backgroundForeground()).toBe(0)
  })

  test('a failing native call is swallowed rather than surfaced over a keypress', () => {
    const h = harness({
      processes: [proc({ status: 'running_foreground' })],
      backgroundForeground: () => { throw new Error('session vanished') },
    })
    expect(h.controller.backgroundForeground()).toBe(0)
    expect(h.texts()).toHaveLength(0)
  })

  test('foregroundCount ignores finished and already-background tasks', () => {
    const h = harness({
      processes: [
        proc({ task_id: 'a', status: 'running_foreground' }),
        proc({ task_id: 'b', status: 'running' }),
        proc({ task_id: 'c', status: 'completed', exit_code: 0 }),
      ],
    })
    h.controller.refresh()
    expect(h.controller.foregroundCount()).toBe(1)
  })

  test('the message-delivery detach frees the shell through its own entry point', () => {
    // Attribution matters: the model is told the shell moved so a queued message
    // could land, not that the user walked away from the result.
    const h = harness({ processes: [proc({ status: 'running_foreground' })] })
    h.controller.refresh()

    expect(h.controller.backgroundForegroundForMessage()).toBe(1)

    expect(h.messageDetaches()).toBe(1)
    expect(h.controller.foregroundCount()).toBe(0)
    // Still live: typing must not cost the user their build.
    expect(h.controller.runningCount()).toBe(1)
  })

  test('a message with nothing in the foreground detaches nothing', () => {
    // Steering already reaches the model between tool calls, so there is no
    // reason to disturb a task the model backgrounded itself.
    const h = harness({ processes: [proc({ status: 'running' })] })
    h.controller.refresh()
    expect(h.controller.backgroundForegroundForMessage()).toBe(0)
    expect(h.controller.runningCount()).toBe(1)
  })

  test('a message frees a blocking wait, not just a foreground shell', () => {
    // The reported case: `bash` had already detached and a blocking task_output
    // held the turn. Detaching shells alone left the message stuck behind it,
    // which is what made typing look inert.
    const h = harness({
      processes: [proc({ status: 'running' })],
      startingBlockingWaits: 1,
    })
    h.controller.refresh()
    expect(h.controller.foregroundCount()).toBe(0)

    expect(h.controller.reclaimTurnForMessage()).toBe(1)

    expect(h.controller.blockingWaitCount()).toBe(0)
    // Speaking up must never cost the user their build.
    expect(h.controller.runningCount()).toBe(1)
  })

  test('a message covers a shell and a wait together', () => {
    const h = harness({
      processes: [proc({ status: 'running_foreground' })],
      startingBlockingWaits: 1,
    })
    h.controller.refresh()
    expect(h.controller.reclaimTurnForMessage()).toBe(2)
    // Attributed to message delivery, so the model is told why the shell moved.
    expect(h.messageDetaches()).toBe(1)
    expect(h.controller.foregroundCount()).toBe(0)
    expect(h.controller.blockingWaitCount()).toBe(0)
  })

  test('a message with nothing holding the turn frees nothing', () => {
    // Steering already reaches the model between tool calls, so there is
    // nothing to disturb.
    const h = harness({ processes: [proc({ status: 'running' })] })
    h.controller.refresh()
    expect(h.controller.reclaimTurnForMessage()).toBe(0)
    expect(h.controller.runningCount()).toBe(1)
  })
})

describe('BackgroundTerminals.reclaimTurn', () => {
  test('releases a blocking wait even with no foreground shell', () => {
    // The state from the reported screenshot: `bash` had already detached, and a
    // blocking `task_output` call was what held the turn. No shell is in the
    // foreground, so the shell-only check saw nothing and esc fell through to
    // killing the run.
    const h = harness({
      processes: [proc({ status: 'running' })],
      startingBlockingWaits: 1,
    })
    h.controller.refresh()
    expect(h.controller.foregroundCount()).toBe(0)
    expect(h.controller.blockingWaitCount()).toBe(1)
    expect(h.controller.canReclaimTurn()).toBe(true)

    expect(h.controller.reclaimTurn()).toBe(1)

    // The watched task is untouched: only the waiting ended.
    expect(h.controller.blockingWaitCount()).toBe(0)
    expect(h.controller.runningCount()).toBe(1)
  })

  test('covers a foreground shell and a blocking wait in one gesture', () => {
    const h = harness({
      processes: [proc({ status: 'running_foreground' })],
      startingBlockingWaits: 2,
    })
    h.controller.refresh()
    expect(h.controller.reclaimTurn()).toBe(3)
    expect(h.controller.foregroundCount()).toBe(0)
    expect(h.controller.blockingWaitCount()).toBe(0)
  })

  test('reports nothing freed when neither is present', () => {
    // Lets the caller fall through to interrupting, so esc is never inert.
    const h = harness({ processes: [proc({ status: 'running' })] })
    h.controller.refresh()
    expect(h.controller.canReclaimTurn()).toBe(false)
    expect(h.controller.reclaimTurn()).toBe(0)
  })

  test('a failing release still counts the shell that was detached', () => {
    // Partial success must not read as total failure: reporting 0 here would
    // make the caller kill a run whose shell had in fact just been freed.
    const h = harness({
      processes: [proc({ status: 'running_foreground' })],
      releaseBlockingWaits: () => { throw new Error('session vanished') },
    })
    h.controller.refresh()
    expect(h.controller.reclaimTurn()).toBe(1)
    expect(h.texts()).toHaveLength(0)
  })

  test('a vanished session reclaims nothing', () => {
    const h = harness({
      processes: [proc({ status: 'running_foreground' })],
      sessionId: null,
      startingBlockingWaits: 1,
    })
    expect(h.controller.blockingWaitCount()).toBe(0)
    expect(h.controller.reclaimTurn()).toBe(0)
  })

  test('a failing wait probe reads as nothing waiting', () => {
    const h = harness({
      processes: [proc({ status: 'running' })],
      blockingWaits: () => { throw new Error('session vanished') },
    })
    h.controller.refresh()
    expect(h.controller.blockingWaitCount()).toBe(0)
    expect(h.controller.canReclaimTurn()).toBe(false)
  })

  test('a failing wait probe still lets the rest of the poll land', () => {
    // The probe only decides which of two esc gestures is offered. Letting it
    // abandon the poll would freeze the footer and panel over that.
    const h = harness({
      processes: [proc({ status: 'running' })],
      blockingWaits: () => { throw new Error('session vanished') },
    })
    h.controller.refresh()
    expect(h.controller.runningCount()).toBe(1)
  })

  test('the wait count comes from the poll, not a live read per frame', () => {
    // The spinner asks every frame to pick the esc hint; reading across the
    // native boundary that often would be wasteful, so the value is cached.
    const h = harness({
      processes: [proc({ status: 'running' })],
      startingBlockingWaits: 1,
    })
    expect(h.controller.blockingWaitCount()).toBe(0)
    h.controller.refresh()
    expect(h.controller.blockingWaitCount()).toBe(1)
  })

  test('a release clears the cached count without waiting for the next poll', () => {
    // Otherwise a second esc inside the 500ms window reads a stale count, thinks
    // there is still something to release, and does nothing visible.
    const h = harness({
      processes: [proc({ status: 'running' })],
      startingBlockingWaits: 1,
    })
    h.controller.refresh()
    expect(h.controller.reclaimTurn()).toBe(1)

    expect(h.controller.blockingWaitCount()).toBe(0)
    // So the next esc escalates to interrupting instead of silently repeating.
    expect(h.controller.canReclaimTurn()).toBe(false)
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

describe('BackgroundTerminals settled notices', () => {
  test('a task finishing while idle is reported in the transcript', () => {
    // The whole point: with no spinner and no turn in flight, the footer count
    // ticking down was the only trace a `make check` had ever finished.
    const h = harness({ processes: [proc()] })
    h.controller.refresh()
    expect(h.texts()).toEqual([])

    h.setProcesses([proc({ status: 'completed', exit_code: 0 })])
    h.controller.refresh()
    const notice = h.commits.find(entry => entry.slot === 'settled')
    expect(notice?.text).toBe('  ✓ completed in background · exit 0 · aaaaaaaa  sleep 30')
  })

  test('a failure is named and carries its exit code', () => {
    const h = harness({ processes: [proc()] })
    h.controller.refresh()
    h.setProcesses([proc({ status: 'failed', exit_code: 2 })])
    h.controller.refresh()
    expect(h.texts().join('\n')).toContain('✗ failed in background · exit 2')
  })

  test('the notice is emitted once, not on every poll', () => {
    // The poll runs twice a second; repeating the line would bury the session.
    const h = harness({ processes: [proc()] })
    h.controller.refresh()
    h.setProcesses([proc({ status: 'completed', exit_code: 0 })])
    h.controller.refresh()
    h.controller.refresh()
    h.controller.refresh()
    expect(h.commits.filter(entry => entry.slot === 'settled')).toHaveLength(1)
  })

  test('a task already settled when first seen is not announced', () => {
    // Otherwise resuming a session would replay outcomes the user has read.
    const h = harness({ processes: [proc({ status: 'completed', exit_code: 0 })] })
    h.controller.refresh()
    h.controller.refresh()
    expect(h.commits.filter(entry => entry.slot === 'settled')).toEqual([])
  })

  test('a panel stop is reported once, by the panel', async () => {
    // stopFromPanel already commits its own line; the poll that observes the
    // same transition must not say it a second time.
    const h = harness({ processes: [proc()] })
    h.controller.togglePanel()
    expect(h.controller.handlePanelKey({ type: 'char', char: 'x' })).toBe(true)
    await h.controller.settled()
    h.controller.refresh()
    expect(h.texts().join('\n')).toContain('Stopped aaaaaaaa')
    expect(h.commits.filter(entry => entry.slot === 'settled')).toEqual([])
  })

  test('a user cancellation is attributed, matching the panel and cards', () => {
    const h = harness({ processes: [proc()] })
    h.controller.refresh()
    h.setProcesses([proc({ status: 'killed', stopped_by_user: true })])
    h.controller.refresh()
    expect(h.texts().join('\n')).toContain('was cancelled by the user')
  })

  test('a task that vanishes between polls is not given an invented outcome', () => {
    // The engine reclaims entries. Its last observed state is all we ever saw.
    const h = harness({ processes: [proc()] })
    h.controller.refresh()
    h.setProcesses([])
    h.controller.refresh()
    expect(h.commits.filter(entry => entry.slot === 'settled')).toEqual([])
  })
})

describe('BackgroundTerminals wake on completion', () => {
  test('a queued notice while idle opens a turn to deliver it', () => {
    // The point of the whole mechanism: "run the build, then fix what breaks"
    // must survive a build that outlives its own turn.
    const h = harness({ processes: [proc()], pendingNotifications: () => 1 })
    h.controller.refresh()
    expect(h.wakes()).toBe(1)
  })

  test('nothing queued means no turn', () => {
    const h = harness({ processes: [proc()], pendingNotifications: () => 0 })
    h.controller.refresh()
    expect(h.wakes()).toBe(0)
  })

  test('a run in flight is left to drain the queue itself', () => {
    // The engine hands queued notices to the running turn between calls, so
    // waking here would deliver the same text twice.
    const h = harness({
      processes: [proc()],
      pendingNotifications: () => 1,
      runInFlight: () => true,
    })
    h.controller.refresh()
    expect(h.wakes()).toBe(0)
  })

  test('a queued user message carries the notices instead', () => {
    // It will submit on its own and pick them up, and it outranks a synthetic
    // turn the user did not ask for.
    const h = harness({
      processes: [proc()],
      pendingNotifications: () => 1,
      queuedMessages: () => 1,
    })
    h.controller.refresh()
    expect(h.wakes()).toBe(0)
  })

  test('an open ask is not answered by a synthetic turn', () => {
    const h = harness({
      processes: [proc()],
      pendingNotifications: () => 1,
      overlayBlocking: () => true,
    })
    h.controller.refresh()
    expect(h.wakes()).toBe(0)
  })

  test('no session means nothing to wake', () => {
    const h = harness({
      processes: [proc()],
      sessionId: null,
      pendingNotifications: () => 1,
    })
    h.controller.refresh()
    expect(h.wakes()).toBe(0)
  })

  test('a host without the hook never wakes', () => {
    const h = harness({
      processes: [proc()],
      pendingNotifications: () => 1,
      wake: null,
    })
    expect(() => h.controller.refresh()).not.toThrow()
    expect(h.wakes()).toBe(0)
  })

  test('a failing probe does not wake and does not abandon the poll', () => {
    // Reading it as "something is pending" would open turns in a loop.
    const h = harness({
      processes: [proc({ status: 'completed', exit_code: 0 })],
      pendingNotifications: () => { throw new Error('native boundary') },
    })
    expect(() => h.controller.refresh()).not.toThrow()
    expect(h.wakes()).toBe(0)
  })

  test('the queue draining stops the waking', () => {
    // build_turn consumes the queue, so the next poll sees zero and must not
    // open a second turn for the same batch.
    let pending = 1
    const h = harness({
      processes: [proc()],
      pendingNotifications: () => pending,
      wake: () => { pending = 0 },
    })
    h.controller.refresh()
    h.controller.refresh()
    h.controller.refresh()
    expect(pending).toBe(0)
  })
})

describe('BackgroundTerminals wake arming', () => {
  test('a wake that opens no turn is not retried every poll', () => {
    // A host can decline: a signed-out cloud session returns before starting a
    // run, leaving the queue pending. Retrying twice a second would hammer it.
    let pending = 1
    let attempts = 0
    const h = harness({
      processes: [proc()],
      pendingNotifications: () => pending,
      wake: () => { attempts++ },
    })
    h.controller.refresh()
    h.controller.refresh()
    h.controller.refresh()
    expect(attempts).toBe(1)
    expect(pending).toBe(1)
  })

  test('a drained queue re-arms the next batch', () => {
    // One refusal must not disable waking for the rest of the session.
    let pending = 1
    let attempts = 0
    const h = harness({
      processes: [proc()],
      pendingNotifications: () => pending,
      wake: () => { attempts++; pending = 0 },
    })
    h.controller.refresh()
    expect(attempts).toBe(1)
    // The turn took delivery, so the next poll observes an empty queue.
    h.controller.refresh()
    // A later task finishes.
    pending = 1
    h.controller.refresh()
    expect(attempts).toBe(2)
  })
})
