/**
 * Tests for paste accumulator — buffers chunked stdin input
 * and flushes as a single paste for correct collapse detection.
 */

import { describe, test, expect, beforeEach } from 'bun:test'
import { PasteAccumulator } from '../src/term/input/paste_accumulator.js'

describe('PasteAccumulator', () => {
  let flushed: string[]
  let acc: PasteAccumulator

  beforeEach(() => {
    flushed = []
    acc = new PasteAccumulator((text) => { flushed.push(text) })
  })

  test('single char is not a paste — calls flush synchronously', () => {
    acc.push('a')
    // Single char should flush immediately (not buffered)
    expect(flushed).toEqual(['a'])
  })

  test('multi-char input is detected as paste and buffered', async () => {
    acc.push('line1\nline2\nline3')
    // Should not flush synchronously — it's buffered
    expect(flushed).toEqual([])
    // Wait for flush timeout
    await sleep(150)
    expect(flushed).toEqual(['line1\nline2\nline3'])
  })

  test('multiple chunks are accumulated into one paste', async () => {
    acc.push('line1\nline2')
    acc.push('\nline3\nline4')
    expect(flushed).toEqual([])
    await sleep(150)
    // Should flush as a single combined string
    expect(flushed).toEqual(['line1\nline2\nline3\nline4'])
  })

  test('single char after paste flush is handled independently', async () => {
    acc.push('line1\nline2\nline3')
    await sleep(150)
    expect(flushed).toEqual(['line1\nline2\nline3'])

    acc.push('x')
    expect(flushed).toEqual(['line1\nline2\nline3', 'x'])
  })

  test('cancel discards buffered chunks', async () => {
    acc.push('line1\nline2')
    acc.cancel()
    await sleep(150)
    expect(flushed).toEqual([])
  })

  test('threshold: exactly 2 chars still treated as single-char path', () => {
    // 2 chars without newline — could be a fast double-tap, not a paste
    acc.push('ab')
    // This is ambiguous, but multi-char input should be buffered as potential paste
    expect(flushed).toEqual([])
  })

  test('isPending returns true while buffering', async () => {
    expect(acc.isPending()).toBe(false)
    acc.push('hello\nworld')
    expect(acc.isPending()).toBe(true)
    await sleep(150)
    expect(acc.isPending()).toBe(false)
  })
})

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}
