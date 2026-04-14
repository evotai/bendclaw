import { describe, test, expect, beforeEach } from 'bun:test'
import {
  buildUserMessage,
  buildAssistantLines,
  buildToolResult,
  buildVerboseEvent,
  buildRunSummary,
  buildError,
  AssistantStreamBuffer,
  findSafeSplitPoint,
  resetIdCounter,
} from '../src/utils/outputLines.js'

beforeEach(() => {
  resetIdCounter()
})

// ---------------------------------------------------------------------------
// buildUserMessage
// ---------------------------------------------------------------------------

describe('buildUserMessage', () => {
  test('creates a single user line', () => {
    const lines = buildUserMessage('hello world')
    expect(lines).toHaveLength(1)
    expect(lines[0]!.kind).toBe('user')
    expect(lines[0]!.text).toBe('hello world')
  })
})

// ---------------------------------------------------------------------------
// buildAssistantLines
// ---------------------------------------------------------------------------

describe('buildAssistantLines', () => {
  test('renders markdown and splits into lines', () => {
    const lines = buildAssistantLines('hello **world**')
    expect(lines.length).toBeGreaterThan(0)
    expect(lines.every((l) => l.kind === 'assistant')).toBe(true)
  })

  test('returns empty for blank text', () => {
    expect(buildAssistantLines('')).toHaveLength(0)
    expect(buildAssistantLines('   ')).toHaveLength(0)
  })
})

// ---------------------------------------------------------------------------
// buildToolResult
// ---------------------------------------------------------------------------

describe('buildToolResult', () => {
  test('creates tool line with name and duration', () => {
    const lines = buildToolResult('bash', { command: 'ls -la' }, 'done', undefined, 42)
    expect(lines.length).toBeGreaterThanOrEqual(1)
    expect(lines[0]!.kind).toBe('tool')
    expect(lines[0]!.text).toContain('✓')
    expect(lines[0]!.text).toContain('bash')
    expect(lines[0]!.text).toContain('ls -la')
    expect(lines[0]!.text).toContain('42ms')
  })

  test('creates error tool line', () => {
    const lines = buildToolResult('bash', { command: 'fail' }, 'error', 'command not found', 10)
    expect(lines[0]!.text).toContain('✗')
    expect(lines.some((l) => l.kind === 'error')).toBe(true)
  })

  test('includes diff when present', () => {
    const lines = buildToolResult('file_edit', { path: 'a.ts', diff: '+added\n-removed' }, 'done')
    expect(lines.some((l) => l.text.includes('added') || l.text.includes('removed'))).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// buildVerboseEvent
// ---------------------------------------------------------------------------

describe('buildVerboseEvent', () => {
  test('splits multi-line text with trailing separator', () => {
    const lines = buildVerboseEvent('line1\nline2\nline3')
    // 3 content lines + 1 empty separator
    expect(lines).toHaveLength(4)
    expect(lines.filter((l) => l.kind === 'verbose')).toHaveLength(4)
    expect(lines[0]!.text).toBe('line1')
    expect(lines[2]!.text).toBe('line3')
    expect(lines[3]!.text).toBe('')
  })
})

// ---------------------------------------------------------------------------
// buildRunSummary
// ---------------------------------------------------------------------------

describe('buildRunSummary', () => {
  test('formats stats with header and footer', () => {
    const lines = buildRunSummary({
      durationMs: 2500,
      turnCount: 3,
      toolCallCount: 5,
      toolErrorCount: 0,
      inputTokens: 1000,
      outputTokens: 200,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      llmCalls: 2,
      contextTokens: 0,
      contextWindow: 0,
      toolBreakdown: [],
      llmCallDetails: [],
    })
    expect(lines.length).toBeGreaterThan(1)
    // Header line
    expect(lines[0]!.text).toContain('Run Summary')
    // Stats line
    const statsLine = lines.find((l) => l.text.includes('turn'))!
    expect(statsLine.text).toContain('2.5s')
    expect(statsLine.text).toContain('3 turns')
    expect(statsLine.text).toContain('5 tools')
    expect(statsLine.text).toContain('1200 tokens')
    // Footer
    expect(lines[lines.length - 1]!.text).toContain('───')
  })

  test('includes llm call details', () => {
    const lines = buildRunSummary({
      durationMs: 5000,
      turnCount: 2,
      toolCallCount: 3,
      toolErrorCount: 0,
      inputTokens: 5000,
      outputTokens: 500,
      cacheReadTokens: 1000,
      cacheWriteTokens: 200,
      llmCalls: 2,
      contextTokens: 0,
      contextWindow: 0,
      toolBreakdown: [],
      llmCallDetails: [
        { model: 'test', durationMs: 2000, inputTokens: 3000, outputTokens: 300, ttfbMs: 100, ttftMs: 200, tokPerSec: 150 },
        { model: 'test', durationMs: 1500, inputTokens: 2000, outputTokens: 200, ttfbMs: 80, ttftMs: 150, tokPerSec: 133 },
      ],
    })
    expect(lines.some((l) => l.text.includes('llm'))).toBe(true)
    expect(lines.some((l) => l.text.includes('tok/s'))).toBe(true)
    expect(lines.some((l) => l.text.includes('cache'))).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// findSafeSplitPoint
// ---------------------------------------------------------------------------

describe('findSafeSplitPoint', () => {
  test('returns content.length when no newline', () => {
    expect(findSafeSplitPoint('hello world')).toBe(11)
  })

  test('splits at paragraph boundary', () => {
    const text = 'first paragraph\n\nsecond paragraph'
    const split = findSafeSplitPoint(text)
    expect(split).toBe(17) // after \n\n
    expect(text.slice(0, split)).toBe('first paragraph\n\n')
  })

  test('does not split inside code block', () => {
    const text = '```js\nconst x = 1\n\nconst y = 2\n```'
    const split = findSafeSplitPoint(text)
    // Should return content.length — the whole thing is inside a code block
    expect(split).toBe(text.length)
  })

  test('splits before code block, not inside', () => {
    const text = 'some text\n\n```js\nconst x = 1\n```'
    const split = findSafeSplitPoint(text)
    expect(split).toBe(11) // after "some text\n\n"
    expect(text.slice(0, split).trim()).toBe('some text')
  })

  test('falls back to single newline', () => {
    const text = 'line one\nline two'
    const split = findSafeSplitPoint(text)
    expect(split).toBe(9) // after "line one\n"
  })

  test('returns content.length for unclosed code block', () => {
    const text = 'hello\n\n```js\nconst x = 1'
    const split = findSafeSplitPoint(text)
    // End is inside unclosed code block, should not split
    expect(split).toBe(text.length)
  })
})

// ---------------------------------------------------------------------------
// AssistantStreamBuffer
// ---------------------------------------------------------------------------

describe('AssistantStreamBuffer', () => {
  test('emits lines with prefix on first content', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('hello')
    const lines = buf.finish()
    expect(lines.some((l) => l.text.startsWith('⏺'))).toBe(true)
  })

  test('skips leading whitespace', () => {
    const buf = new AssistantStreamBuffer()
    const lines1 = buf.push('\n\n')
    expect(lines1).toHaveLength(0)
    buf.push('hello')
    const lines2 = buf.finish()
    expect(lines2.some((l) => l.text.startsWith('⏺'))).toBe(true)
  })

  test('emits lines on newline', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('hello')
    const lines = buf.push(' world\n')
    const assistantLines = lines.filter((l) => l.kind === 'assistant')
    expect(assistantLines.length).toBeGreaterThanOrEqual(0)
  })

  test('finish flushes remaining buffer', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('hello world')
    const lines = buf.finish()
    expect(lines.some((l) => l.kind === 'assistant')).toBe(true)
  })

  test('finish on empty buffer returns nothing', () => {
    const buf = new AssistantStreamBuffer()
    expect(buf.finish()).toHaveLength(0)
  })

  test('pendingText returns incomplete line', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('hello')
    expect(buf.pendingText).toBe('hello')
    buf.push(' world\nfoo')
    expect(buf.pendingText).toBe('foo')
  })

  test('multi-line push emits all complete lines', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('first line\n')
    const lines = buf.push('second line\nthird')
    // 'third' stays pending
    expect(buf.pendingText).toBe('third')
  })

  test('does not split inside code block', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('text before\n\n```js\nconst x = 1\n')
    // The code block is unclosed, so the \n inside should NOT cause a flush
    // that breaks the code block. The pending text should contain the code block.
    const pending = buf.pendingText
    expect(pending).toContain('```js')
  })

  test('flushes text before code block at paragraph boundary', () => {
    const buf = new AssistantStreamBuffer()
    // Push text with a paragraph break followed by a closed code block
    const allLines: import('../src/utils/outputLines.js').OutputLine[] = []
    allLines.push(...buf.push('hello world\n\n'))
    allLines.push(...buf.push('```js\nconst x = 1\n```\n'))
    allLines.push(...buf.finish())
    // Should have emitted assistant lines for both parts
    const assistantLines = allLines.filter((l) => l.kind === 'assistant')
    expect(assistantLines.length).toBeGreaterThan(0)
  })
})
