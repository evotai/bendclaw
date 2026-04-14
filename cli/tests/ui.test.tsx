/**
 * UI component tests using ink-testing-library.
 * Renders components and asserts on the text output (lastFrame).
 */

import { describe, test, expect } from 'bun:test'
import React from 'react'
import { render } from 'ink-testing-library'
import { Text, Box } from 'ink'
import { OutputView } from '../src/components/OutputView.js'
import { StreamingMarkdown } from '../src/components/StreamingMarkdown.js'
import type { OutputLine } from '../src/utils/outputLines.js'

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

let idCounter = 0
function line(kind: OutputLine['kind'], text: string): OutputLine {
  return { id: `test-${++idCounter}`, kind, text }
}

// ---------------------------------------------------------------------------
// OutputView — line rendering
// ---------------------------------------------------------------------------

describe('OutputView', () => {
  test('renders user message with ❯ prefix', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>banner</Text>} lines={[line('user', 'hello world')]} />
    )
    const frame = lastFrame()
    expect(frame).toContain('❯')
    expect(frame).toContain('hello world')
  })

  test('renders assistant message with indentation', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('assistant', 'some response')]} />
    )
    expect(lastFrame()).toContain('some response')
  })

  test('renders error in red', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('error', 'something broke')]} />
    )
    expect(lastFrame()).toContain('something broke')
  })

  test('renders system message', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('system', 'info message')]} />
    )
    expect(lastFrame()).toContain('info message')
  })

  test('renders run_summary', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('run_summary', '─── This Run Summary')]} />
    )
    expect(lastFrame()).toContain('This Run Summary')
  })

  test('renders tool_result in green', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool_result', '  Result: completed')]} />
    )
    expect(lastFrame()).toContain('Result: completed')
  })
})

// ---------------------------------------------------------------------------
// OutputView — ToolLineView
// ---------------------------------------------------------------------------

describe('ToolLineView', () => {
  test('renders tool call badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool', '[BASH] call')]} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[BASH]')
    expect(frame).toContain('call')
  })

  test('renders tool completed badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool', '[BASH] completed · 120ms')]} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[BASH]')
    expect(frame).toContain('completed')
    expect(frame).toContain('120ms')
  })

  test('renders tool failed badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool', '[READ] failed · 50ms')]} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[READ]')
    expect(frame).toContain('failed')
  })

  test('renders tool detail line (indented)', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool', '  ❯ ls -la')]} />
    )
    expect(lastFrame()).toContain('❯ ls -la')
  })
})

// ---------------------------------------------------------------------------
// OutputView — VerboseLineView
// ---------------------------------------------------------------------------

describe('VerboseLineView', () => {
  test('renders LLM call badge in yellow', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[LLM] call · claude-opus-4-6 · turn 1')]} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[LLM]')
    expect(frame).toContain('call')
    expect(frame).toContain('turn 1')
  })

  test('renders LLM completed badge in green', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[LLM] completed')]} />
    )
    expect(lastFrame()).toContain('[LLM]')
    expect(lastFrame()).toContain('completed')
  })

  test('renders LLM failed badge in red', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[LLM] failed · 2.1s')]} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[LLM]')
    expect(frame).toContain('failed')
    expect(frame).toContain('2.1s')
  })

  test('renders LLM retry badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[LLM] call · claude-opus-4-6 · turn 2 · retry 1')]} />
    )
    expect(lastFrame()).toContain('retry 1')
  })

  test('renders COMPACT badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[COMPACT] · no-op')]} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[COMPACT]')
    expect(frame).toContain('no-op')
  })

  test('renders verbose detail line (indented)', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '  tokens   4k in · 12 out · 3 tok/s')]} />
    )
    expect(lastFrame()).toContain('tokens')
    expect(lastFrame()).toContain('4k in')
  })

  test('renders timing with percentages', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '  timing   4.3s · ttfb 3.9s (91%) · ttft 3.9s (91%) · stream 0.3s (8%)')]} />
    )
    const frame = lastFrame()
    expect(frame).toContain('ttfb 3.9s (91%)')
    expect(frame).toContain('stream 0.3s (8%)')
  })
})

// ---------------------------------------------------------------------------
// OutputView — mixed content ordering
// ---------------------------------------------------------------------------

describe('OutputView mixed content', () => {
  test('renders multiple line types in order', () => {
    const lines: OutputLine[] = [
      line('user', 'hello'),
      line('verbose', '[LLM] call · test · turn 1'),
      line('verbose', '  1 messages · 9 tools'),
      line('verbose', '[LLM] completed'),
      line('verbose', '  tokens   1k in · 50 out · 100 tok/s'),
      line('assistant', 'Hi there!'),
      line('run_summary', '─── This Run Summary ──────────────────────────────────'),
      line('run_summary', '2.5s · 1 turn · 1 llm call · 0 tool calls · 1k tokens'),
      line('run_summary', '────────────────────────────────────────────────────────'),
    ]
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} />
    )
    const frame = lastFrame()
    expect(frame).toContain('hello')
    expect(frame).toContain('[LLM]')
    expect(frame).toContain('Hi there!')
    expect(frame).toContain('This Run Summary')
    expect(frame).toContain('────────')
  })
})

// ---------------------------------------------------------------------------
// StreamingMarkdown
// ---------------------------------------------------------------------------

describe('StreamingMarkdown', () => {
  test('renders null for empty text', () => {
    const { lastFrame } = render(<StreamingMarkdown text="" maxHeight={10} />)
    expect(lastFrame()).toBe('')
  })

  test('renders markdown text', () => {
    const { lastFrame } = render(<StreamingMarkdown text="hello **world**" maxHeight={10} />)
    expect(lastFrame()).toContain('hello')
    expect(lastFrame()).toContain('world')
  })

  test('truncates to maxHeight lines', () => {
    const text = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`).join('\n\n')
    const { lastFrame } = render(<StreamingMarkdown text={text} maxHeight={5} />)
    const frame = lastFrame()
    // Should contain the last lines, not the first
    expect(frame).toContain('line 20')
    expect(frame).not.toContain('line 2\n')
  })
})
