import { describe, test, expect } from 'bun:test'
import { buildOverlayBlocks } from '../src/term/viewmodel/overlays.js'
import { buildSelectorRegionLines } from '../src/term/viewmodel/selector.js'
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
  test('leads with the title and only the active count', () => {
    const lines = render([
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
    ])
    expect(lines[1]).toBe('Background')
    expect(lines[2]).toBe('1 active shell')
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

  test('navigation cannot select a finished task', () => {
    const processes = [
      proc({ task_id: 'a' }),
      proc({ task_id: 'b', command: 'finished command', status: 'completed', exit_code: 0 }),
    ]
    for (const moves of [0, 1, 2]) {
      const lines = render(processes, moves)
      expect(lines[lines.length - 1]).toContain('x to stop')
      expect(lines.join('\n')).not.toContain('finished command')
    }
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

  test('finished history does not add any rendered rows', () => {
    const active = proc({ task_id: 'a' })
    const processes = [
      active,
      proc({ task_id: 'b', status: 'completed', exit_code: 0 }),
      proc({ task_id: 'c', status: 'failed', exit_code: 1 }),
      proc({ task_id: 'd', status: 'killed' }),
    ]
    expect(render(processes)).toEqual(render([active]))
    expect(buildSelectorRegionLines(createBackgroundPanelState(processes), 100, 24))
      .toEqual(buildSelectorRegionLines(createBackgroundPanelState([active]), 100, 24))
  })

  test('finished-only history renders the same empty panel as no history', () => {
    const processes = [
      proc({ status: 'completed', exit_code: 0 }),
      proc({ task_id: 'failed', status: 'failed', exit_code: 1 }),
      proc({ task_id: 'killed', status: 'killed' }),
    ]
    expect(render(processes)).toEqual(render([]))
    expect(buildSelectorRegionLines(createBackgroundPanelState(processes), 100, 24))
      .toEqual(buildSelectorRegionLines(createBackgroundPanelState([]), 100, 24))
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

    expect(lines).toContain('⌘ bash  sleep 30')
    expect(lines.some(line => line.trim() === 'line 30')).toBe(true)
    expect(lines.some(line => line.trim() === 'line 1')).toBe(false)
    expect(lines.join('\n')).toContain('Esc to back')
    expect(lines.every(line => line.length <= 100)).toBe(true)

    const short = buildSelectorRegionLines(state, 100, 12).map(stripAnsi)
    expect(short.length).toBeLessThan(lines.length)
    expect(short.some(line => line.trim() === 'line 30')).toBe(true)
  })

  test('live output emphasizes activity without repeating the command', () => {
    const process = proc({ command: 'printf hello\nprintf world' })
    const state = createBackgroundOutputState(process, 'hello\nworld')
    const rendered = buildSelectorRegionLines(state, 80, 30)
    const plain = rendered.map(stripAnsi)
    expect(plain.filter(text => text.includes('running ·'))).toHaveLength(1)
    expect(plain).toContain('  hello')
    expect(plain).toContain('  world')
    expect(plain.join('\n')).not.toContain('Output:')
    expect(plain.join('\n')).not.toContain('ctrl+o')
  })

  test('huge command metadata never pushes activity or navigation off screen', () => {
    const command = 'echo ' + 'long-argument '.repeat(1200)
    for (const rows of [10, 12, 24, 40]) {
      const state = createBackgroundOutputState(proc({ command }), 'latest output')
      const lines = buildSelectorRegionLines(state, 60, rows).map(stripAnsi)
      expect(lines.length).toBeLessThanOrEqual(rows)
      expect(lines.every(text => text.length <= 60)).toBe(true)
      expect(lines).toContain('  latest output')
      expect(lines.join('\n')).toContain('Esc')
      const commandView = { ...state, outputView: { showCommand: true } }
      expect(buildSelectorRegionLines(commandView, 60, rows).length).toBeLessThanOrEqual(rows)
    }
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
