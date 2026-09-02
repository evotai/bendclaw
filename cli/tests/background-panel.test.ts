import { describe, test, expect } from 'bun:test'
import {
  BACKGROUND_PANEL_TITLE,
  COMPLETED_GROUP,
  PANEL_EMPTY_MESSAGE,
  SHELLS_GROUP,
  backgroundPanelHints,
  createBackgroundPanelState,
  decideBackgroundPanelAction,
  focusedPanelTarget,
  formatCommandLabel,
  formatElapsed,
  formatOutputView,
  formatPanelItems,
  formatPanelSubtitle,
  formatStatusDetail,
  isBackgroundPanelShortcut,
  refreshBackgroundPanelState,
  shouldDownOpenPanel,
} from '../src/term/app/background-panel.js'
import { selectorDown } from '../src/term/selector.js'
import type { BackgroundProcess } from '../src/native/index.js'

function proc(overrides: Partial<BackgroundProcess> = {}): BackgroundProcess {
  return {
    task_id: '028deb34-db80-401f-8c65-7b13f92efb36',
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

describe('formatElapsed', () => {
  test('stays in seconds below a minute', () => {
    expect(formatElapsed(1500)).toBe('2s')
    expect(formatElapsed(59_000)).toBe('59s')
  })

  test('switches to minutes so a long run stays readable', () => {
    expect(formatElapsed(94_000)).toBe('1m 34s')
    expect(formatElapsed(120_000)).toBe('2m')
  })

  test('switches to hours for very long runs', () => {
    expect(formatElapsed(3_600_000)).toBe('1h')
    expect(formatElapsed(5_400_000)).toBe('1h 30m')
  })

  test('a negative clock reads as zero rather than a minus sign', () => {
    expect(formatElapsed(-500)).toBe('0s')
  })
})

describe('formatCommandLabel', () => {
  test('keeps a single-line command verbatim', () => {
    expect(formatCommandLabel('bun run dev')).toBe('bun run dev')
  })

  test('reports extra lines as a count so rows stay one line', () => {
    expect(formatCommandLabel('bun run dev\n--watch')).toBe('bun run dev (+1 line)')
    expect(formatCommandLabel('a\nb\nc')).toBe('a (+2 lines)')
  })

  test('truncates long commands while keeping the line count visible', () => {
    const label = formatCommandLabel(`${'x'.repeat(200)}\nsecond`, 40)
    expect(label).toEndWith('(+1 line)')
    expect(label.length).toBeLessThanOrEqual(40)
  })

  test('an empty command still yields a readable label', () => {
    expect(formatCommandLabel('')).toBe('(empty)')
  })
})

describe('formatStatusDetail', () => {
  test('a running task shows only its clock', () => {
    expect(formatStatusDetail(proc())).toBe('running · 2s')
  })

  test('a clean exit reports the code and elapsed time', () => {
    expect(formatStatusDetail(proc({ status: 'completed', exit_code: 0 })))
      .toBe('exit 0 · 2s')
  })

  test('a failure is named, not just numbered', () => {
    expect(formatStatusDetail(proc({ status: 'failed', exit_code: 7 })))
      .toBe('failed · exit 7 · 2s')
  })

  test('a model-initiated stop reads as stopped rather than killed', () => {
    expect(formatStatusDetail(proc({ status: 'killed' }))).toBe('stopped · 2s')
  })

  test('a user cancellation is named as such, matching the task cards', () => {
    // The panel and the tool cards must not spell the same outcome two ways.
    expect(formatStatusDetail(proc({ status: 'killed', stopped_by_user: true })))
      .toBe('cancelled by user · 2s')
  })
})

describe('formatPanelSubtitle', () => {
  test('an empty list has no subtitle, since the body states the empty case', () => {
    // Saying it twice, two rows apart, reads as two separate statements.
    expect(formatPanelSubtitle([])).toBeUndefined()
  })

  test('live tasks are counted as active shells', () => {
    expect(formatPanelSubtitle([proc(), proc({ task_id: 'b' })])).toBe('2 active shells')
    expect(formatPanelSubtitle([proc()])).toBe('1 active shell')
  })

  test('finished tasks are counted separately from live ones', () => {
    const subtitle = formatPanelSubtitle([
      proc(),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
      proc({ task_id: 'c', status: 'failed', exit_code: 1 }),
    ])
    expect(subtitle).toBe('1 active shell · 2 finished')
  })

  test('a list with no live work omits the active clause', () => {
    expect(formatPanelSubtitle([proc({ status: 'completed', exit_code: 0 })]))
      .toBe('1 finished')
  })
})

describe('formatPanelItems', () => {
  test('rows carry the command, a parenthesised status and the task id', () => {
    const items = formatPanelItems([proc({ command: 'bun run dev' })])
    expect(items).toHaveLength(1)
    expect(items[0]!.label).toBe('bun run dev')
    expect(items[0]!.detail).toBe('(running · 2s)')
    expect(items[0]!.id).toBe('028deb34-db80-401f-8c65-7b13f92efb36')
  })

  test('the full command stays searchable even when the row is truncated', () => {
    const items = formatPanelItems([proc({ command: 'bun run dev\n--watch --hot' })])
    expect(items[0]!.label).not.toContain('--hot')
    expect(items[0]!.searchText).toContain('--hot')
  })

  test('foreground tasks are listed too, since the panel is the whole picture', () => {
    // The footer count excludes them to avoid double-reporting a visible tool
    // card, but hiding them from the panel would make the list look wrong.
    const items = formatPanelItems([proc({ status: 'running_foreground' })])
    expect(items).toHaveLength(1)
  })

  test('a single group carries no heading', () => {
    // With only running shells on screen, a "Shells" label restates the title.
    const items = formatPanelItems([proc(), proc({ task_id: 'b' })])
    expect(items.some(item => item.header)).toBe(false)
  })

  test('a mixed list splits into Shells above Completed', () => {
    const items = formatPanelItems([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
    ])
    expect(items.map(item => item.header ? item.label : item.id))
      .toEqual([SHELLS_GROUP, 'a', COMPLETED_GROUP, 'b'])
  })

  test('group headings carry their row count and are not focusable', () => {
    const items = formatPanelItems([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b' }),
      proc({ task_id: 'c', status: 'killed' }),
    ])
    const header = items.find(item => item.label === SHELLS_GROUP)
    expect(header?.headerCount).toBe(2)
    expect(header?.focusable).toBe(false)
  })

  test('live work sorts above finished work regardless of input order', () => {
    const items = formatPanelItems([
      proc({ task_id: 'done', status: 'completed', exit_code: 0 }),
      proc({ task_id: 'live' }),
    ])
    const ids = items.filter(item => !item.header).map(item => item.id)
    expect(ids).toEqual(['live', 'done'])
  })

  test('a running row advertises stop, a finished row does not', () => {
    const items = formatPanelItems([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
    ])
    const hintFor = (id: string) =>
      (items.find(item => item.id === id)?.hints ?? []).map(hint => hint.action)
    expect(hintFor('a')).toContain('stop')
    expect(hintFor('b')).not.toContain('stop')
  })
})

describe('backgroundPanelHints', () => {
  test('an empty panel offers only close', () => {
    // Nothing to select, view or stop, so every other key would be a dead end.
    expect(backgroundPanelHints(null, 0).map(hint => hint.action)).toEqual(['close'])
  })

  test('a focused running task offers select, view and stop', () => {
    expect(backgroundPanelHints({ id: 'a', live: true }, 1).map(hint => hint.action))
      .toEqual(['select', 'view output', 'stop', 'close'])
  })

  test('a finished task keeps view but drops stop', () => {
    expect(backgroundPanelHints({ id: 'a', live: false }, 0).map(hint => hint.action))
      .toEqual(['select', 'view output', 'close'])
  })

  test('stop all appears only once it differs from stop', () => {
    const one = backgroundPanelHints({ id: 'a', live: true }, 1).map(hint => hint.action)
    const two = backgroundPanelHints({ id: 'a', live: true }, 2).map(hint => hint.action)
    expect(one).not.toContain('stop all')
    expect(two).toContain('stop all')
  })
})

describe('shouldDownOpenPanel', () => {
  test('↓ opens the panel on an empty composer with live work', () => {
    expect(shouldDownOpenPanel({ editorEmpty: true, running: 1 })).toBe(true)
  })

  test('↓ is left alone while there is text, so it still moves the caret', () => {
    expect(shouldDownOpenPanel({ editorEmpty: false, running: 1 })).toBe(false)
  })

  test('↓ does nothing when no background work is running', () => {
    // The prompt is not advertising the gesture, so it must not fire.
    expect(shouldDownOpenPanel({ editorEmpty: true, running: 0 })).toBe(false)
  })
})

describe('createBackgroundPanelState', () => {
  test('opens on the panel title with a count subtitle', () => {
    const state = createBackgroundPanelState([proc()])
    expect(state.title).toBe(BACKGROUND_PANEL_TITLE)
    expect(state.subtitle).toBe('1 active shell')
    expect(state.focusIndex).toBe(0)
  })

  test('an empty list still opens, so the panel can report having nothing', () => {
    const state = createBackgroundPanelState([])
    expect(state.items).toHaveLength(0)
    expect(state.emptyMessage).toBe(PANEL_EMPTY_MESSAGE)
    expect(state.subtitle).toBeUndefined()
  })

  test('the panel opts out of filtering, keeping bare letters for actions', () => {
    expect(createBackgroundPanelState([proc()]).noFilter).toBe(true)
  })
})

describe('refreshBackgroundPanelState', () => {
  test('focus follows the task id rather than the row index', () => {
    const first = proc({ task_id: 'a', command: 'first' })
    const second = proc({ task_id: 'b', command: 'second' })
    const opened = selectorDown(createBackgroundPanelState([first, second]))
    expect(opened.focusIndex).toBe(1)

    // The first task disappears: focus must stay on 'b', now at index 0.
    const refreshed = refreshBackgroundPanelState(opened, [second])
    expect(refreshed.focusIndex).toBe(0)
    expect(refreshed.items[refreshed.focusIndex]!.id).toBe('b')
  })

  test('a status change is reflected without moving focus', () => {
    const opened = createBackgroundPanelState([proc({ task_id: 'a' })])
    const refreshed = refreshBackgroundPanelState(
      opened,
      [proc({ task_id: 'a', status: 'completed', exit_code: 0 })],
    )
    expect(refreshed.items[0]!.detail).toBe('(exit 0 · 2s)')
    expect(refreshed.subtitle).toBe('1 finished')
    expect(refreshed.focusIndex).toBe(0)
  })

  test('losing the focused task clamps onto a surviving row', () => {
    const opened = selectorDown(createBackgroundPanelState([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b' }),
    ]))
    const refreshed = refreshBackgroundPanelState(opened, [proc({ task_id: 'a' })])
    expect(refreshed.focusIndex).toBe(0)
  })

  test('an emptied list leaves focus in range', () => {
    const opened = createBackgroundPanelState([proc({ task_id: 'a' })])
    const refreshed = refreshBackgroundPanelState(opened, [])
    expect(refreshed.items).toHaveLength(0)
    expect(refreshed.focusIndex).toBe(0)
  })
})

describe('isBackgroundPanelShortcut', () => {
  test('ctrl+t opens the panel', () => {
    expect(isBackgroundPanelShortcut({ type: 'ctrl', key: 't' })).toBe(true)
  })

  test('other control keys are left alone', () => {
    expect(isBackgroundPanelShortcut({ type: 'ctrl', key: 'b' })).toBe(false)
    expect(isBackgroundPanelShortcut({ type: 'char', char: 't' })).toBe(false)
  })
})

describe('focusedPanelTarget', () => {
  test('reports whether the focused task is still live', () => {
    const state = createBackgroundPanelState([proc({ task_id: 'a' })])
    expect(focusedPanelTarget(state, [proc({ task_id: 'a' })]))
      .toEqual({ id: 'a', live: true })
  })

  test('a finished task is not a stop target', () => {
    const done = proc({ task_id: 'a', status: 'completed', exit_code: 0 })
    const state = createBackgroundPanelState([done])
    expect(focusedPanelTarget(state, [done])).toEqual({ id: 'a', live: false })
  })

  test('an empty list has no target', () => {
    expect(focusedPanelTarget(createBackgroundPanelState([]), [])).toBeNull()
  })
})

describe('decideBackgroundPanelAction', () => {
  const live = { id: 'a', live: true }
  const done = { id: 'a', live: false }

  test('esc closes the panel', () => {
    expect(decideBackgroundPanelAction({ type: 'escape' }, live)).toEqual({ kind: 'close' })
  })

  test('enter views the focused task output', () => {
    expect(decideBackgroundPanelAction({ type: 'enter' }, done))
      .toEqual({ kind: 'view', taskId: 'a' })
  })

  test('x stops a live task', () => {
    expect(decideBackgroundPanelAction({ type: 'char', char: 'x' }, live))
      .toEqual({ kind: 'stop', taskId: 'a' })
  })

  test('x on a finished task does nothing rather than erroring', () => {
    // Its process is already gone, so there is nothing to signal.
    expect(decideBackgroundPanelAction({ type: 'char', char: 'x' }, done))
      .toEqual({ kind: 'none' })
  })

  test('shift+X stops everything in both terminal encodings', () => {
    // Kitty reports a lowercased shift-char; legacy terminals send bare 'X'.
    expect(decideBackgroundPanelAction({ type: 'shift-char', char: 'x' }, live))
      .toEqual({ kind: 'stop-all' })
    expect(decideBackgroundPanelAction({ type: 'char', char: 'X' }, live))
      .toEqual({ kind: 'stop-all' })
  })

  test('stop-all works with nothing focused, so an empty panel is harmless', () => {
    expect(decideBackgroundPanelAction({ type: 'char', char: 'X' }, null))
      .toEqual({ kind: 'stop-all' })
  })

  test('enter with nothing focused is inert', () => {
    expect(decideBackgroundPanelAction({ type: 'enter' }, null)).toEqual({ kind: 'none' })
  })

  test('navigation keys are left to the shared selector logic', () => {
    expect(decideBackgroundPanelAction({ type: 'up' }, live)).toEqual({ kind: 'none' })
    expect(decideBackgroundPanelAction({ type: 'down' }, live)).toEqual({ kind: 'none' })
  })

  test('other letters do not trigger destructive actions', () => {
    expect(decideBackgroundPanelAction({ type: 'char', char: 'k' }, live))
      .toEqual({ kind: 'none' })
  })
})

describe('formatOutputView', () => {
  test('leads with a status header naming the task and command', () => {
    const lines = formatOutputView(proc({ command: 'bun run dev' }), 'hello\n')
    expect(lines[0]).toBe('  ● 028deb34 · running · 2s  bun run dev')
    expect(lines[1]).toBe('    hello')
  })

  test('a trailing newline is a terminator, not an empty last line', () => {
    const lines = formatOutputView(proc(), 'one\ntwo\n')
    expect(lines.filter(line => line.trim() === '')).toHaveLength(0)
    expect(lines[2]).toBe('    two')
  })

  test('empty output says so and still points at the file', () => {
    const lines = formatOutputView(proc(), '')
    expect(lines[1]).toBe('    (no output yet)')
    expect(lines[2]).toBe('    /tmp/out.txt')
  })

  test('keeps the tail, since a running task’s latest output is what matters', () => {
    const output = Array.from({ length: 10 }, (_, index) => `line ${index}`).join('\n')
    const lines = formatOutputView(proc(), output, 3)
    expect(lines).toContain('    line 9')
    expect(lines).not.toContain('    line 0')
  })

  test('states how much was elided rather than silently cutting output', () => {
    const output = Array.from({ length: 10 }, (_, index) => `line ${index}`).join('\n')
    const lines = formatOutputView(proc(), output, 3)
    expect(lines[1]).toBe('    … 7 earlier lines')
  })

  test('a capped output file is called out so a partial view is not trusted', () => {
    const lines = formatOutputView(proc({ output_file_truncated: true }), 'tail\n')
    expect(lines[1]).toContain('output file was capped')
  })

  test('the output path closes the block so the full log stays reachable', () => {
    const lines = formatOutputView(proc(), 'a\n')
    expect(lines[lines.length - 1]).toBe('    /tmp/out.txt')
  })

  test('output shorter than the tail limit needs no elision notice', () => {
    const lines = formatOutputView(proc(), 'only\n', 40)
    expect(lines.some(line => line.includes('earlier'))).toBe(false)
  })
})
