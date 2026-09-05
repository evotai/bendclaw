import { afterEach, describe, expect, test } from 'bun:test'
import {
  getTheme,
  getThemeScheme,
  resetDetectedThemeScheme,
  setDetectedThemeScheme,
} from '../src/render/theme/index.js'

const prevTheme = process.env.EVOT_THEME
const prevColorfgbg = process.env.COLORFGBG

afterEach(() => {
  if (prevTheme === undefined) delete process.env.EVOT_THEME
  else process.env.EVOT_THEME = prevTheme
  if (prevColorfgbg === undefined) delete process.env.COLORFGBG
  else process.env.COLORFGBG = prevColorfgbg
  resetDetectedThemeScheme()
})

describe('theme detection priority', () => {
  test('EVOT_THEME override wins over detected scheme', () => {
    process.env.EVOT_THEME = 'dark'
    expect(setDetectedThemeScheme('light')).toBe(false)
    expect(getThemeScheme()).toBe('dark')
    // Dark theme headings use the gold accent.
    expect(getTheme().h1.paint('x')).toContain('x')
  })

  test('detected scheme applies when no override is set', () => {
    delete process.env.EVOT_THEME
    delete process.env.COLORFGBG
    expect(setDetectedThemeScheme('light')).toBe(true)
    expect(getThemeScheme()).toBe('light')
    expect(setDetectedThemeScheme('light')).toBe(false)
    expect(setDetectedThemeScheme('dark')).toBe(true)
    expect(getThemeScheme()).toBe('dark')
  })

  test('COLORFGBG is used when nothing else is available', () => {
    delete process.env.EVOT_THEME
    process.env.COLORFGBG = '0;15'
    resetDetectedThemeScheme()
    expect(getThemeScheme()).toBe('light')
  })
})
