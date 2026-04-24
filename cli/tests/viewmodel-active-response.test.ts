import { describe, test, expect } from 'bun:test'
import { buildActiveResponseBlocks, type ActiveResponseInput } from '../src/term/viewmodel/active-response.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import { createSpinnerState, setSpinnerPhase } from '../src/term/spinner.js'
import stripAnsi from 'strip-ansi'

function defaultInput(overrides?: Partial<ActiveResponseInput>): ActiveResponseInput {
  return {
    isLoading: true,
    pendingText: '',
    toolProgress: '',
    spinner: createSpinnerState(),
    termRows: 24,
    ...overrides,
  }
}

function render(input: ActiveResponseInput): string {
  return blocksToLines(buildActiveResponseBlocks(input)).join('\n')
}

function renderPlain(input: ActiveResponseInput): string {
  return stripAnsi(render(input))
}

describe('buildActiveResponseBlocks', () => {
  test('returns empty when not loading', () => {
    const blocks = buildActiveResponseBlocks(defaultInput({ isLoading: false }))
    expect(blocks).toEqual([])
  })

  test('shows spinner when loading', () => {
    const result = renderPlain(defaultInput())
    expect(result).toContain('Thinking')
  })

  test('shows pending text when streaming', () => {
    const result = renderPlain(defaultInput({ pendingText: 'hello world' }))
    expect(result).toContain('hello world')
  })

  test('truncates pending text to maxHeight', () => {
    const longText = Array.from({ length: 30 }, (_, i) => `line ${i}`).join('\n')
    const result = renderPlain(defaultInput({ pendingText: longText, termRows: 20 }))
    const lines = result.split('\n')
    const contentLines = lines.filter(l => l.includes('line '))
    expect(contentLines.length).toBeLessThanOrEqual(14) // termRows - RESERVED_LINES
  })

  test('shows tool progress with fixed height', () => {
    const result = renderPlain(defaultInput({ toolProgress: 'running...\noutput line 1\noutput line 2' }))
    expect(result).toContain('output line 2')
  })

  test('tool progress omits expand hint when all lines are visible', () => {
    const result = renderPlain(defaultInput({ toolProgress: 'single line' }))
    const lines = result.split('\n')
    expect(lines).toContain('  single line')
    expect(result).not.toContain('ctrl+o to expand')
  })

  test('tool progress shows expand hint only when truncated', () => {
    const progress = Array.from({ length: 7 }, (_, i) => `line ${i}`).join('\n')
    const result = renderPlain(defaultInput({ toolProgress: progress }))
    expect(result).toContain('  line 6')
    expect(result).toContain('  +2 lines  (ctrl+o to expand)')
    expect(result).not.toContain('  line 0')
  })

  test('shows Executing when tool phase', () => {
    const spinner = setSpinnerPhase(createSpinnerState(), 'executing', 'bash')
    const result = renderPlain(defaultInput({ spinner }))
    expect(result).toContain('Executing')
  })

  test('truncates long progress lines', () => {
    const longLine = 'x'.repeat(200)
    const result = renderPlain(defaultInput({ toolProgress: longLine }))
    const lines = result.split('\n')
    const progressLine = lines.find(l => l.includes('xxx'))
    expect(progressLine!.length).toBeLessThan(200)
    expect(progressLine).toContain('…')
  })
})
