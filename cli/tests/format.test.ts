import { describe, test, expect } from 'bun:test'
import { padRight, relativeTime } from '../src/render/format.js'

describe('padRight', () => {
  test('pads short string with spaces', () => {
    expect(padRight('hi', 6)).toBe('hi    ')
  })

  test('returns string as-is when exact length', () => {
    expect(padRight('hello', 5)).toBe('hello')
  })

  test('truncates with ellipsis when too long', () => {
    expect(padRight('hello world', 8)).toBe('hello w…')
  })

  test('handles empty string', () => {
    expect(padRight('', 4)).toBe('    ')
  })

  test('handles n=0', () => {
    expect(padRight('hi', 0)).toBe('…')
  })

  test('handles n=1 with long string', () => {
    expect(padRight('hello', 1)).toBe('…')
  })
})

describe('relativeTime', () => {
  test('returns "just now" for recent timestamps', () => {
    const now = new Date().toISOString()
    expect(relativeTime(now)).toBe('just now')
  })

  test('returns minutes ago', () => {
    const fiveMinAgo = new Date(Date.now() - 5 * 60 * 1000).toISOString()
    expect(relativeTime(fiveMinAgo)).toBe('5m ago')
  })

  test('returns hours ago', () => {
    const twoHoursAgo = new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString()
    expect(relativeTime(twoHoursAgo)).toBe('2h ago')
  })

  test('returns days ago', () => {
    const threeDaysAgo = new Date(Date.now() - 3 * 24 * 60 * 60 * 1000).toISOString()
    expect(relativeTime(threeDaysAgo)).toBe('3d ago')
  })

  test('returns raw string on invalid input', () => {
    expect(relativeTime('not-a-date')).toBe('not-a-date')
  })
})
