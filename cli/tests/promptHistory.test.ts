import { describe, expect, test } from 'bun:test'
import {
  getHistoryCursorPlacement,
  isHistoryUpShortcut,
  isHistoryDownShortcut,
  resolveUpArrowAction,
  resolveDownArrowAction,
} from '../src/utils/promptHistory.js'

describe('prompt history shortcuts', () => {
  test('uses ctrl-p for history up', () => {
    expect(isHistoryUpShortcut('p', { ctrl: true } as any)).toBe(true)
    expect(isHistoryUpShortcut('', { upArrow: true } as any)).toBe(false)
  })

  test('uses ctrl-n for history down', () => {
    expect(isHistoryDownShortcut('n', { ctrl: true } as any)).toBe(true)
    expect(isHistoryDownShortcut('', { downArrow: true } as any)).toBe(false)
  })

  test('up moves to line start before opening previous history item', () => {
    expect(resolveUpArrowAction({ linesLength: 1, cursorLine: 0, cursorCol: 4 })).toBe('move_to_line_start')
    expect(resolveUpArrowAction({ linesLength: 1, cursorLine: 0, cursorCol: 0 })).toBe('history_up')
  })

  test('down moves to line end before opening next history item', () => {
    expect(resolveDownArrowAction({ linesLength: 1, cursorLine: 0, cursorCol: 2, lineLength: 5 })).toBe('move_to_line_end')
    expect(resolveDownArrowAction({ linesLength: 1, cursorLine: 0, cursorCol: 5, lineLength: 5 })).toBe('history_down')
  })

  test('history navigation uses directional cursor placement', () => {
    expect(getHistoryCursorPlacement('history_up')).toBe('start')
    expect(getHistoryCursorPlacement('history_down')).toBe('end')
  })
})
