import { describe, test, expect } from 'bun:test'
import { padRight, relativeTime, renderPositionBar } from '../src/render/format.js'

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

describe('renderPositionBar', () => {
  test('keeps unchanged marker consistent for L3', () => {
    const { bar, legend } = renderPositionBar(10, [{ index: 2, end_index: 4, method: 'Dropped' }], 3)
    expect(bar).toBe('[··DDD·····]')
    expect(legend).toBe('·=unchanged/kept  D=Dropped')
  })

  test('kept ranges visible when proportional mapping would hide them', () => {
    // 251 messages, indices 2–240 dropped, kept: [0,1] and [241,250]
    const actions = [{ index: 2, end_index: 240, method: 'Dropped' }]
    const { bar } = renderPositionBar(251, actions, 3)
    // Both kept ranges must have at least one '·'
    const chars = bar.slice(1, -1) // strip [ ]
    expect(chars.length).toBe(40)
    // First kept range [0,1] → slot 0 must be '·'
    expect(chars[0]).toBe('·')
    // Last kept range [241,250] → last slot(s) must include '·'
    const lastDot = chars.lastIndexOf('·')
    expect(lastDot).toBeGreaterThan(chars.length - 3) // near the end
  })

  test('no kept ranges means all action slots', () => {
    // Every message has an action — no gaps to preserve
    const actions = [{ index: 0, end_index: 99, method: 'Dropped' }]
    const { bar } = renderPositionBar(100, actions, 3)
    const chars = bar.slice(1, -1)
    expect(chars.length).toBe(40)
    expect(chars).not.toContain('·')
    expect(chars).toContain('─100─')
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
