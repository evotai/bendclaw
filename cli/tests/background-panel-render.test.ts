import { describe, test, expect } from 'bun:test'
import { buildOverlayBlocks, buildSelectorRegionLines } from '../src/term/viewmodel/overlays.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import { createBackgroundOutputState, createBackgroundPanelState } from '../src/term/app/background-panel.js'
import { selectorDown } from '../src/term/selector.js'
import type { BackgroundProcess } from '../src/native/index.js'

function stripAnsi(text: string): string {
  return text.replace(/\x1b\[[0-9;]*m/g, '')
}

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

function render(processes: BackgroundProcess[], moves = 0): string[] {
  let state = createBackgroundPanelState(processes)
  for (let i = 0; i < moves; i++) state = selectorDown(state)
  return blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 100)).map(stripAnsi)
}

describe('background panel rendering', () => {
  test('leads with the title and the active/finished counts', () => {
    const lines = render([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
    ])
    expect(lines[1]).toBe('Background')
    expect(lines[2]).toBe('1 active shell · 1 finished')
  })

  test('the title carries no generic row tally', () => {
    // The subtitle and group headings already count things; "Background  2"
    // beside the title would be a third count of the same list.
    const lines = render([proc(), proc({ task_id: 'b' })])
    expect(lines[1]).toBe('Background')
  })

  test('no filter line is offered, since bare letters are actions', () => {
    const text = render([proc()]).join('\n')
    expect(text).not.toContain('Filter')
    expect(text).not.toContain('type to search')
  })

  test('hints read as "<key> to <action>" joined by a middot', () => {
    const lines = render([proc()])
    expect(lines[lines.length - 1])
      .toBe('↑/↓ to select · Enter to view output · x to stop · Esc to close')
  })

  test('a focused running row offers stop; a finished row does not', () => {
    const running = render([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
    ])
    expect(running[running.length - 1]).toContain('x to stop')

    // Rows: Shells header, a, Completed header, b — two moves lands on b.
    const finished = render([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
    ], 1)
    expect(finished[finished.length - 1]).not.toContain('x to stop')
    expect(finished[finished.length - 1]).toContain('Enter to view output')
  })

  test('stop all is only advertised with more than one live shell', () => {
    const one = render([proc()])
    const two = render([proc({ task_id: 'a' }), proc({ task_id: 'b' })])
    expect(one[one.length - 1]).not.toContain('X to stop all')
    expect(two[two.length - 1]).toContain('X to stop all')
  })

  test('a single group renders without a heading', () => {
    const text = render([proc(), proc({ task_id: 'b' })]).join('\n')
    expect(text).not.toContain('Shells')
    expect(text).not.toContain('Completed')
  })

  test('a mixed list shows bold group headings with counts', () => {
    const lines = render([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
      proc({ task_id: 'c', status: 'failed', exit_code: 1 }),
    ])
    expect(lines).toContain('  Shells (1)')
    expect(lines).toContain('  Completed (2)')
  })

  test('groups are separated by a blank line, not a divider rule', () => {
    const lines = render([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
    ])
    const completedAt = lines.indexOf('  Completed (1)')
    expect(completedAt).toBeGreaterThan(0)
    expect(lines[completedAt - 1]).toBe('')
    expect(lines.join('\n')).not.toContain('──')
  })

  test('the focused row is marked and rows carry a parenthesised status', () => {
    const lines = render([proc({ command: 'bun run dev' })])
    expect(lines.some(l => l.startsWith('❯ bun run dev'))).toBe(true)
    expect(lines.find(l => l.includes('bun run dev'))).toContain('(running · 2s)')
  })

  test('an empty panel states it in the body and offers only close', () => {
    const lines = render([])
    expect(lines).toContain('  No tasks currently running')
    expect(lines[lines.length - 1]).toBe('Esc to close')
    // No count line above an empty body: the body is the whole message.
    expect(lines.join('\n')).not.toContain('active shell')
  })

  test('live output uses a bounded full-width tail with a back hint', () => {
    const output = Array.from({ length: 30 }, (_, index) => `line ${index + 1}`).join('\n')
    const state = createBackgroundOutputState(proc(), output)
    const lines = buildSelectorRegionLines(state, 100, 24).map(stripAnsi)

    expect(lines).toContain('Background output')
    expect(lines.some(line => line.trim() === 'line 30')).toBe(true)
    expect(lines.some(line => line.trim() === 'line 1')).toBe(false)
    expect(lines).toContain('Esc to back')
    expect(lines.every(line => line.length <= 100)).toBe(true)

    const short = buildSelectorRegionLines(state, 100, 12).map(stripAnsi)
    expect(short.length).toBeLessThan(lines.length)
    expect(short.some(line => line.trim() === 'line 30')).toBe(true)
  })

  test('capped-output warning stays pinned above a noisy tail', () => {
    const output = Array.from({ length: 30 }, (_, index) => `line ${index + 1}`).join('\n')
    const state = createBackgroundOutputState(proc({ output_file_truncated: true }), output)
    const lines = buildSelectorRegionLines(state, 100, 12).map(stripAnsi)

    expect(lines.some(line => line.includes('output file was capped'))).toBe(true)
    expect(lines.some(line => line.trim() === 'line 30')).toBe(true)
  })

  test('a multi-line command stays on one row', () => {
    const lines = render([proc({ command: 'tail -f log\n| grep err' })])
    const row = lines.find(l => l.includes('tail -f log'))
    expect(row).toContain('(+1 line)')
    expect(lines.some(l => l.includes('grep err'))).toBe(false)
  })
})
