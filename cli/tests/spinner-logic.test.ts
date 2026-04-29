import { describe, test, expect } from 'bun:test'
import {
  createSpinnerState,
  advanceSpinner,
  setSpinnerPhase,
  isSlow,
  formatSpinnerLine,
} from '../src/term/spinner.js'
import stripAnsi from 'strip-ansi'

describe('createSpinnerState', () => {
  test('creates initial state', () => {
    const state = createSpinnerState()
    expect(state.frame).toBe(0)
    expect(state.phase).toBe('thinking')
    expect(state.streaming).toBe(false)
    expect(state.toolName).toBeNull()
    expect(state.tokenCount).toBe(0)
  })
})

describe('advanceSpinner', () => {
  test('increments frame', () => {
    const state = createSpinnerState()
    const next = advanceSpinner(state)
    expect(next.frame).toBe(1)
  })

  test('wraps around at end of frames', () => {
    let state = createSpinnerState()
    // Advance through all frames (12 total: 6 + 6 reversed)
    for (let i = 0; i < 12; i++) {
      state = advanceSpinner(state)
    }
    expect(state.frame).toBe(0)
  })

  test('does not mutate other fields', () => {
    const state = { ...createSpinnerState(), tokenCount: 42 }
    const next = advanceSpinner(state)
    expect(next.tokenCount).toBe(42)
    expect(next.phase).toBe('thinking')
  })
})

describe('setSpinnerPhase', () => {
  test('changes phase to executing', () => {
    const state = createSpinnerState()
    const next = setSpinnerPhase(state, 'executing', 'bash')
    expect(next.phase).toBe('executing')
    expect(next.toolName).toBe('bash')
  })

  test('changes phase to thinking', () => {
    let state = createSpinnerState()
    state = setSpinnerPhase(state, 'executing', 'bash')
    const next = setSpinnerPhase(state, 'thinking')
    expect(next.phase).toBe('thinking')
    expect(next.toolName).toBeNull()
  })

  test('resets phaseStartedAt on change', () => {
    const state = { ...createSpinnerState(), phaseStartedAt: 1000 }
    const next = setSpinnerPhase(state, 'executing', 'read')
    expect(next.phaseStartedAt).toBeGreaterThan(1000)
  })

  test('returns same state if phase unchanged', () => {
    const state = createSpinnerState()
    const next = setSpinnerPhase(state, 'thinking')
    expect(next).toBe(state) // same reference
  })
})

describe('isSlow', () => {
  test('not slow when just started', () => {
    const state = createSpinnerState()
    expect(isSlow(state, Date.now())).toBe(false)
  })

  test('slow after threshold with no tokens', () => {
    const state = { ...createSpinnerState(), phaseStartedAt: Date.now() - 9000 }
    expect(isSlow(state, Date.now())).toBe(true)
  })

  test('not slow when streaming', () => {
    const state = {
      ...createSpinnerState(),
      phaseStartedAt: Date.now() - 9000,
      streaming: true,
    }
    expect(isSlow(state, Date.now())).toBe(false)
  })

  test('not slow when recent tokens received (thinking phase)', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phaseStartedAt: now - 9000,
      lastTokenAt: now - 1000, // 1s ago — recent
    }
    expect(isSlow(state, now)).toBe(false)
  })

  test('slow when tokens are stale (thinking phase)', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phaseStartedAt: now - 9000,
      lastTokenAt: now - 9000, // 9s ago — stale
    }
    expect(isSlow(state, now)).toBe(true)
  })

  test('slow in executing phase after threshold', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phase: 'executing' as const,
      phaseStartedAt: now - 9000,
      toolName: 'bash',
    }
    expect(isSlow(state, now)).toBe(true)
  })
})

describe('formatSpinnerLine', () => {
  test('contains Thinking label when thinking', () => {
    const state = createSpinnerState()
    const line = stripAnsi(formatSpinnerLine(state, Date.now()))
    expect(line).toContain('Thinking…')
  })

  test('contains Executing label when executing', () => {
    const state = setSpinnerPhase(createSpinnerState(), 'executing', 'bash')
    const line = stripAnsi(formatSpinnerLine(state, Date.now()))
    expect(line).toContain('Executing [BASH]…')
  })

  test('contains slow label after threshold', () => {
    const now = Date.now()
    const state = { ...createSpinnerState(), phaseStartedAt: now - 9000 }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('LLM slow…')
  })

  test('contains Executing slow label', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phase: 'executing' as const,
      phaseStartedAt: now - 9000,
      toolName: 'bash',
    }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('Executing [BASH] slow…')
  })

  test('contains duration', () => {
    const now = Date.now()
    const state = { ...createSpinnerState(), phaseStartedAt: now - 2500 }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('2.5s')
  })

  test('contains esc to interrupt hint', () => {
    const state = createSpinnerState()
    const line = stripAnsi(formatSpinnerLine(state, Date.now()))
    expect(line).toContain('esc to interrupt')
  })

  test('shows token count after 30s', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phaseStartedAt: now - 35000,
      tokenCount: 1500,
      streaming: true, // prevent slow
    }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('1.5k tokens')
  })

  test('shows token count with arrow even before 30s', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phaseStartedAt: now - 5000,
      tokenCount: 100,
    }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('↓ 100 tokens')
  })
})
