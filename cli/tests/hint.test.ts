import { describe, test, expect } from 'bun:test'
import { HINT_SEPARATOR, backgroundChord, backgroundChordLabel, formatChord } from '../src/term/app/hint.js'

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

describe('backgroundChord', () => {
  // Ctrl+B is tmux's own default prefix, so inside tmux a single press never
  // reaches us -- tmux keeps it. Advertising the plain chord there is worse than
  // showing nothing: the user presses once, sees no effect, and concludes
  // backgrounding is broken.
  test('is a single press outside tmux', () => {
    expect(backgroundChord({})).toBe('ctrl+b')
    expect(backgroundChordLabel({})).toBe('Ctrl+B')
  })

  test('asks for a double press inside tmux', () => {
    const env = { TMUX: '/private/tmp/tmux-501/default,12345,0' }
    expect(backgroundChord(env)).toBe('ctrl+b ctrl+b (twice)')
    expect(backgroundChordLabel(env)).toBe('Ctrl+B Ctrl+B (twice)')
  })

  // TMUX is set to the socket path, so an empty value means no tmux. Treating
  // presence alone as truth would advertise the double press everywhere the
  // variable is exported but blank.
  test('an empty TMUX is not tmux', () => {
    expect(backgroundChord({ TMUX: '' })).toBe('ctrl+b')
    expect(backgroundChordLabel({ TMUX: '' })).toBe('Ctrl+B')
  })

  test('the two spellings differ only in case', () => {
    const env = { TMUX: 'socket,1,0' }
    expect(backgroundChordLabel(env).toLowerCase()).toBe(backgroundChord(env))
    expect(backgroundChordLabel({}).toLowerCase()).toBe(backgroundChord({}))
  })
})
