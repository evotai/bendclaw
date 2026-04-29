import { describe, test, expect, beforeAll } from 'bun:test'
import { buildOutputBlocks } from '../src/term/viewmodel/output.js'
import { blocksToLines, styledLineToAnsi, line, colored, dim } from '../src/term/viewmodel/types.js'
import type { OutputLine } from '../src/render/output.js'
import stripAnsi from 'strip-ansi'
import chalk from 'chalk'

beforeAll(() => {
  chalk.level = 3
})

function render(lines: OutputLine[]): string {
  return blocksToLines(buildOutputBlocks(lines)).join('\n')
}

function renderPlain(lines: OutputLine[]): string {
  return stripAnsi(render(lines))
}

describe('buildOutputBlocks', () => {
  test('user message has marginTop=1 and bold prefix', () => {
    const result = renderPlain([{ id: 'u1', kind: 'user', text: 'hello' }])
    expect(result).toContain('❯ hello')
    expect(result.startsWith('\n')).toBe(true)
  })

  test('assistant block starts with marginTop=1', () => {
    const result = renderPlain([
      { id: 'u1', kind: 'user', text: 'hi' },
      { id: 'a1', kind: 'assistant', text: 'response line 1' },
    ])
    const lines = result.split('\n')
    const assistantIdx = lines.findIndex(l => l.includes('response line 1'))
    expect(lines[assistantIdx - 1]).toBe('')
  })

  test('consecutive assistant lines have no margin', () => {
    const result = renderPlain([
      { id: 'a1', kind: 'assistant', text: 'line 1' },
      { id: 'a2', kind: 'assistant', text: 'line 2' },
      { id: 'a3', kind: 'assistant', text: 'line 3' },
    ])
    const lines = result.split('\n')
    const contentLines = lines.filter(l => l.includes('line'))
    expect(contentLines.length).toBe(3)
    const emptyBetween = lines.slice(
      lines.indexOf(contentLines[0]!),
      lines.indexOf(contentLines[2]!) + 1
    ).filter(l => l === '')
    expect(emptyBetween.length).toBe(0)
  })

  test('tool badge has marginTop=1', () => {
    const result = renderPlain([
      { id: 'a1', kind: 'assistant', text: 'text' },
      { id: 't1', kind: 'tool', text: '[bash] ●' },
    ])
    const lines = result.split('\n')
    const toolIdx = lines.findIndex(l => l.includes('[bash]'))
    expect(lines[toolIdx - 1]).toBe('')
  })

  test('tool detail lines have no margin', () => {
    const result = renderPlain([
      { id: 't1', kind: 'tool', text: '[bash] ●' },
      { id: 't2', kind: 'tool', text: '  ls -la' },
    ])
    const lines = result.split('\n')
    const detailIdx = lines.findIndex(l => l.includes('ls -la'))
    expect(lines[detailIdx - 1]).toContain('[bash]')
  })

  test('verbose badge has marginTop=1', () => {
    const result = renderPlain([
      { id: 'a1', kind: 'assistant', text: 'text' },
      { id: 'v1', kind: 'verbose', text: '[LLM] started model=gpt-4' },
    ])
    const lines = result.split('\n')
    const verboseIdx = lines.findIndex(l => l.includes('[LLM]'))
    expect(lines[verboseIdx - 1]).toBe('')
  })

  test('verbose status colors are unified', () => {
    const result = render([
      { id: 'v1', kind: 'verbose', text: '[COMPACT] ● compacting · 1 msgs' },
      { id: 'v2', kind: 'verbose', text: '[COMPACT] ✓ skipped · within budget' },
      { id: 'v3', kind: 'verbose', text: '[LLM] ✓ gpt-5.5 · turn 1 · 3.1s' },
    ])
    expect(result).toContain('\x1b[36m')
    expect(result).not.toContain('\x1b[32m')
    expect(result).not.toContain('\x1b[31m')
  })

  test('tool badge and status symbol use one unified color', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '[bash] ✓ · 1.2s' }])
    expect(result).toContain('\x1b[36m')
    expect(result).not.toContain('\x1b[32m')
  })

  test('tool failed symbol also uses unified color', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '[bash] ✗ · exit=1' }])
    expect(result).toContain('\x1b[36m')
    expect(result).not.toContain('\x1b[31m')
  })

  test('tool running symbol also uses unified color', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '[bash] ● · command' }])
    expect(result).toContain('\x1b[36m')
    expect(result).not.toContain('\x1b[33m')
  })

  test('tool skipped text no longer changes tool color', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '[bash] ● · skipped · cached' }])
    expect(result).toContain('\x1b[36m')
    expect(result).not.toContain('\x1b[33m')
    expect(result).not.toContain('\x1b[90m')
  })

  test('tool cancelled text no longer changes tool color', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '[bash] ● · cancelled' }])
    expect(result).toContain('\x1b[36m')
    expect(result).not.toContain('\x1b[33m')
  })

  test('JSON result body is not dimmed', () => {
    const result = render([{ id: 'r1', kind: 'tool_result', text: '  {"status":"ok"}' }])
    expect(result).not.toContain('\x1b[2m')
  })

  test('system lines are dim', () => {
    const result = render([{ id: 's1', kind: 'system', text: '  some info' }])
    expect(result).toContain('\x1b[2m')
  })

  test('error lines are red', () => {
    const result = render([{ id: 'e1', kind: 'error', text: 'something broke' }])
    expect(result).toContain('\x1b[31m')
  })

  test('run_summary is dim', () => {
    const result = render([{ id: 'r1', kind: 'run_summary', text: '  3 turns · 1.2k tokens' }])
    expect(result).toContain('\x1b[2m')
  })
})
