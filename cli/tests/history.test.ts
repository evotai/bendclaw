import { describe, test, expect, beforeEach, afterEach } from 'bun:test'
import { HistoryManager, parseHistoryItems } from '../src/session/history.js'
import { mkdtempSync, rmSync, readFileSync } from 'fs'
import { join } from 'path'
import { tmpdir } from 'os'

let tempDir: string
let historyPath: string

beforeEach(() => {
  tempDir = mkdtempSync(join(tmpdir(), 'evot-history-test-'))
  historyPath = join(tempDir, 'history')
})

afterEach(() => {
  rmSync(tempDir, { recursive: true, force: true })
})

describe('HistoryManager', () => {
  test('load returns empty array when file does not exist', () => {
    const hm = new HistoryManager(historyPath)
    expect(hm.load()).toEqual([])
  })

  test('append + load round-trip', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('hello')
    hm.append('world')
    expect(hm.load()).toEqual(['hello', 'world'])
  })

  test('skips consecutive duplicates', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('hello')
    hm.append('hello')
    hm.append('hello')
    expect(hm.load()).toEqual(['hello'])
  })

  test('allows non-consecutive duplicates', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('a')
    hm.append('b')
    hm.append('a')
    expect(hm.load()).toEqual(['a', 'b', 'a'])
  })

  test('skips empty and whitespace-only entries', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('')
    hm.append('  ')
    hm.append('real')
    expect(hm.load()).toEqual(['real'])
  })

  test('trims entries', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('  hello  ')
    expect(hm.load()).toEqual(['hello'])
  })

  test('respects limit parameter', () => {
    const hm = new HistoryManager(historyPath)
    for (let i = 0; i < 10; i++) {
      hm.append(`entry-${i}`)
    }
    const last3 = hm.load(3)
    expect(last3).toEqual(['entry-7', 'entry-8', 'entry-9'])
  })

  test('persists across instances', () => {
    const hm1 = new HistoryManager(historyPath)
    hm1.append('from-first')

    const hm2 = new HistoryManager(historyPath)
    expect(hm2.load()).toEqual(['from-first'])
    hm2.append('from-second')

    const hm3 = new HistoryManager(historyPath)
    expect(hm3.load()).toEqual(['from-first', 'from-second'])
  })

  test('writes with newline-delimited format', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('one')
    hm.append('two')
    const raw = readFileSync(historyPath, 'utf-8')
    expect(raw).toBe('one\ntwo\n')
  })

  test('preserves multi-line entries via escaping', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('line1\nline2\nline3')
    hm.append('single')

    const hm2 = new HistoryManager(historyPath)
    const entries = hm2.load()
    expect(entries).toEqual(['line1\nline2\nline3', 'single'])
  })

  test('escapes backslashes in entries', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('path\\to\\file')

    const hm2 = new HistoryManager(historyPath)
    expect(hm2.load()).toEqual(['path\\to\\file'])
  })

  test('handles entry with both newlines and backslashes', () => {
    const hm = new HistoryManager(historyPath)
    hm.append('first\\nline\nsecond line')

    const hm2 = new HistoryManager(historyPath)
    expect(hm2.load()).toEqual(['first\\nline\nsecond line'])
  })
})

describe('parseHistoryItems', () => {
  test('parses typical history output', () => {
    const msg = [
      '    #1    user       hello world',
      '    …   assistant  I can help with that',
      '    #3    user       thanks',
    ].join('\n')
    expect(parseHistoryItems(msg)).toEqual([
      { label: '#1', detail: 'user  hello world', role: 'user' },
      { label: '…', detail: 'assistant  I can help with that', role: 'assistant' },
      { label: '#3', detail: 'user  thanks', role: 'user' },
    ])
  })

  test('returns empty array for no-messages text', () => {
    expect(parseHistoryItems('No messages in session.')).toEqual([])
  })

  test('skips snapshot lines with ellipsis', () => {
    const msg = [
      '    …   user       old message',
      '    #5    user       new message',
    ].join('\n')
    const items = parseHistoryItems(msg)
    expect(items).toEqual([
      { label: '…', detail: 'user  old message', role: 'user' },
      { label: '#5', detail: 'user  new message', role: 'user' },
    ])
  })

  test('handles single entry', () => {
    expect(parseHistoryItems('    …   assistant  done')).toEqual([
      { label: '…', detail: 'assistant  done', role: 'assistant' },
    ])
  })

  test('returns empty for empty string', () => {
    expect(parseHistoryItems('')).toEqual([])
  })

  test('parses all-snapshot entries after resume', () => {
    const msg = [
      '    …   user       hello world',
      '    …   assistant  I can help with that',
      '    …   user       thanks',
    ].join('\n')
    expect(parseHistoryItems(msg)).toEqual([
      { label: '…', detail: 'user  hello world', role: 'user' },
      { label: '…', detail: 'assistant  I can help with that', role: 'assistant' },
      { label: '…', detail: 'user  thanks', role: 'user' },
    ])
  })
})
