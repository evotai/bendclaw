import { describe, test, expect } from 'bun:test'
import { HINT_SEPARATOR, formatChord } from '../src/term/app/hint.js'

describe('formatChord', () => {
  test('named keys get their display spelling', () => {
    expect(formatChord('enter')).toBe('Enter')
    expect(formatChord('escape')).toBe('Esc')
    expect(formatChord('tab')).toBe('Tab')
  })

  test('arrows render as glyphs', () => {
    expect(formatChord('up')).toBe('↑')
    expect(formatChord('down')).toBe('↓')
  })

  test('interchangeable keys are joined with a slash', () => {
    expect(formatChord(['up', 'down'])).toBe('↑/↓')
  })

  test('bare characters are shown as typed, preserving case', () => {
    // `x` and `X` are different gestures, so the case must survive.
    expect(formatChord('x')).toBe('x')
    expect(formatChord('X')).toBe('X')
  })

  test('an unknown key name passes through rather than vanishing', () => {
    expect(formatChord('ctrl+t')).toBe('ctrl+t')
  })

  test('a single-element list reads like a bare key', () => {
    expect(formatChord(['enter'])).toBe('Enter')
  })
})

describe('HINT_SEPARATOR', () => {
  test('is a spaced middot, matching the rest of the status copy', () => {
    expect(HINT_SEPARATOR).toBe(' · ')
  })
})
