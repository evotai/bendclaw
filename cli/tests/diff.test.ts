import { describe, test, expect } from 'bun:test'
import { formatDiff, colorizeUnifiedDiff } from '../src/utils/diff.js'
import stripAnsi from 'strip-ansi'

describe('formatDiff', () => {
  test('detects added lines', () => {
    const result = formatDiff('a\nb\n', 'a\nb\nc\n')
    expect(result.linesAdded).toBe(1)
    expect(result.linesRemoved).toBe(0)
    expect(stripAnsi(result.text)).toContain('+c')
  })

  test('detects removed lines', () => {
    const result = formatDiff('a\nb\nc\n', 'a\nc\n')
    expect(result.linesRemoved).toBe(1)
    expect(stripAnsi(result.text)).toContain('-b')
  })

  test('detects changed lines', () => {
    const result = formatDiff('hello\n', 'world\n')
    expect(result.linesAdded).toBe(1)
    expect(result.linesRemoved).toBe(1)
    expect(stripAnsi(result.text)).toContain('-hello')
    expect(stripAnsi(result.text)).toContain('+world')
  })

  test('returns empty for identical text', () => {
    const result = formatDiff('same\n', 'same\n')
    expect(result.linesAdded).toBe(0)
    expect(result.linesRemoved).toBe(0)
  })

  test('includes @@ hunk headers', () => {
    const result = formatDiff('a\n', 'b\n')
    expect(stripAnsi(result.text)).toContain('@@')
  })
})

describe('colorizeUnifiedDiff', () => {
  test('colorizes diff lines', () => {
    const diff = '--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new\n context'
    const result = colorizeUnifiedDiff(diff)
    const plain = stripAnsi(result)
    expect(plain).toContain('--- a/file')
    expect(plain).toContain('+new')
    expect(plain).toContain('-old')
  })
})
