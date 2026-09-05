import { describe, expect, test } from 'bun:test'
import { promptMode, promptModeLabels, promptModeStyle } from '../src/term/viewmodel/prompt-mode.js'
import { getTheme } from '../src/render/theme/index.js'

describe('promptMode', () => {
  test('defaults when no mode is active', () => {
    expect(promptMode({ planning: false, logMode: false })).toBe('default')
  })

  test('reports the single active mode', () => {
    expect(promptMode({ planning: true, logMode: false })).toBe('plan')
    expect(promptMode({ planning: false, logMode: true })).toBe('log')
  })

  test('log wins for hue and placeholder when both are set', () => {
    // Log forks a separate agent, so it is the narrower description of what
    // submitting will do.
    expect(promptMode({ planning: true, logMode: true })).toBe('log')
  })
})

describe('promptModeLabels', () => {
  test('lists every active mode, so neither is silently dropped', () => {
    expect(promptModeLabels({ planning: true, logMode: true })).toEqual(['log', 'plan'])
    expect(promptModeLabels({ planning: true, logMode: false })).toEqual(['plan'])
    expect(promptModeLabels({ planning: false, logMode: false })).toEqual([])
  })
})

describe('promptModeStyle', () => {
  test('recolours the frame away from brand only for real modes', () => {
    const { brandHex, accentHex } = getTheme()
    expect(promptModeStyle('default').hex).toBe(brandHex)
    expect(promptModeStyle('plan').hex).toBe(accentHex)
    expect(promptModeStyle('log').hex).toBe(accentHex)
  })

  test('every mode carries a short hint that is actually shorter', () => {
    for (const mode of ['default', 'plan', 'log'] as const) {
      const { hint, shortHint } = promptModeStyle(mode)
      expect(hint.length).toBeGreaterThan(0)
      expect(shortHint.length).toBeGreaterThan(0)
      expect(shortHint.length).toBeLessThanOrEqual(hint.length)
    }
  })

  test('plan says what it withholds, not just that it is plan mode', () => {
    expect(promptModeStyle('plan').hint).toContain('no edits')
  })
})
