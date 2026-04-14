/**
 * AppState tests — verify applyEvent produces correct verbose text and stats.
 */

import { describe, test, expect } from 'bun:test'
import { createInitialState, applyEvent } from '../src/state/AppState.js'
import type { RunEvent } from '../src/native/index.js'

function makeEvent(kind: string, turn: number, payload: Record<string, any>): RunEvent {
  return { kind, turn, payload } as RunEvent
}

// ---------------------------------------------------------------------------
// LLM call started
// ---------------------------------------------------------------------------

describe('applyEvent llm_call_started', () => {
  test('generates verbose text with model and turn', () => {
    const state = createInitialState('claude-opus-4-6', '/tmp')
    const next = applyEvent(state, makeEvent('llm_call_started', 1, {
      model: 'claude-opus-4-6',
      message_count: 5,
      tools: [{}, {}, {}],
      system_prompt_tokens: 1200,
      attempt: 0,
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.kind).toBe('llm_call')
    expect(evt.text).toContain('[LLM] call')
    expect(evt.text).toContain('claude-opus-4-6')
    expect(evt.text).toContain('turn 1')
    expect(evt.text).toContain('5 messages')
    expect(evt.text).toContain('3 tools')
    expect(evt.text).not.toContain('retry')
  })

  test('shows retry when attempt > 0', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('llm_call_started', 2, {
      model: 'test-model',
      message_count: 10,
      tools: [],
      system_prompt_tokens: 500,
      attempt: 2,
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.text).toContain('retry 2')
  })
})

// ---------------------------------------------------------------------------
// LLM call completed
// ---------------------------------------------------------------------------

describe('applyEvent llm_call_completed', () => {
  test('generates completed verbose text with timing percentages', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('llm_call_completed', 1, {
      usage: { input: 4000, output: 120, cache_read: 0, cache_write: 0 },
      metrics: { duration_ms: 5000, ttfb_ms: 2000, ttft_ms: 2500, streaming_ms: 2500 },
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.kind).toBe('llm_completed')
    expect(evt.text).toContain('[LLM] completed')
    expect(evt.text).toContain('4k in')
    expect(evt.text).toContain('120 out')
    // Timing with percentages
    expect(evt.text).toContain('ttfb 2.0s (40%)')
    expect(evt.text).toContain('ttft 2.5s (50%)')
    expect(evt.text).toContain('stream 2.5s (50%)')
  })

  test('generates failed verbose text with error', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('llm_call_completed', 1, {
      usage: { input: 0, output: 0 },
      metrics: { duration_ms: 2100 },
      error: 'Rate limited: retry after 5s',
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.text).toContain('[LLM] failed')
    expect(evt.text).toContain('2.1s')
    expect(evt.text).toContain('Rate limited')
  })

  test('accumulates stats correctly', () => {
    let state = createInitialState('test-model', '/tmp')
    state = applyEvent(state, makeEvent('llm_call_completed', 1, {
      usage: { input: 3000, output: 100, cache_read: 500, cache_write: 200 },
      metrics: { duration_ms: 2000, ttfb_ms: 100, ttft_ms: 200, streaming_ms: 1800 },
    }))
    state = applyEvent(state, makeEvent('llm_call_completed', 1, {
      usage: { input: 4000, output: 200, cache_read: 1000, cache_write: 0 },
      metrics: { duration_ms: 3000, ttfb_ms: 150, ttft_ms: 300, streaming_ms: 2700 },
    }))
    expect(state.currentRunStats.llmCalls).toBe(2)
    expect(state.currentRunStats.inputTokens).toBe(7000)
    expect(state.currentRunStats.outputTokens).toBe(300)
    expect(state.currentRunStats.cacheReadTokens).toBe(1500)
    expect(state.currentRunStats.cacheWriteTokens).toBe(200)
    expect(state.currentRunStats.llmCallDetails).toHaveLength(2)
  })
})

// ---------------------------------------------------------------------------
// Context compaction
// ---------------------------------------------------------------------------

describe('applyEvent context_compaction', () => {
  test('compact started generates verbose text with budget bar', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('context_compaction_started', 1, {
      message_count: 68,
      estimated_tokens: 65000,
      budget_tokens: 156000,
      context_window: 160000,
      system_prompt_tokens: 4000,
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.kind).toBe('compact_call')
    expect(evt.text).toContain('[COMPACT] call')
    expect(evt.text).toContain('68 messages')
    expect(evt.text).toContain('65k')
    expect(evt.text).toContain('156k')
  })

  test('compact completed no-op', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('context_compaction_completed', 1, {
      result: { type: 'no_op' },
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.text).toContain('[COMPACT] · no-op')
  })

  test('compact completed level_compacted with details', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('context_compaction_completed', 1, {
      result: {
        type: 'level_compacted',
        level: 1,
        before_message_count: 72,
        after_message_count: 65,
        before_estimated_tokens: 74000,
        after_estimated_tokens: 65000,
      },
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.text).toContain('[COMPACT] · L1')
    expect(evt.text).toContain('72 messages')
    expect(evt.text).toContain('65 messages')
    expect(evt.text).toContain('saved 9k')
    expect(evt.text).toContain('12%')
  })

  test('compact completed tracks history for run summary', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('context_compaction_completed', 1, {
      result: {
        type: 'level_compacted',
        level: 2,
        before_message_count: 50,
        after_message_count: 40,
        before_estimated_tokens: 80000,
        after_estimated_tokens: 60000,
      },
    }))
    expect(next.currentRunStats.compactHistory).toHaveLength(1)
    expect(next.currentRunStats.compactHistory[0]).toEqual({
      level: 2,
      beforeTokens: 80000,
      afterTokens: 60000,
    })
  })

  test('compact completed run_once_cleared', () => {
    let state = createInitialState('test-model', '/tmp')
    // Set context tokens first
    state = applyEvent(state, makeEvent('context_compaction_started', 1, {
      message_count: 10,
      estimated_tokens: 50000,
      budget_tokens: 156000,
      context_window: 160000,
      system_prompt_tokens: 4000,
    }))
    const next = applyEvent(state, makeEvent('context_compaction_completed', 1, {
      result: { type: 'run_once_cleared', saved_tokens: 12000 },
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.text).toContain('cleared')
    expect(evt.text).toContain('saved 12k')
  })
})

// ---------------------------------------------------------------------------
// Run summary data
// ---------------------------------------------------------------------------

describe('applyEvent run_finished', () => {
  test('sets duration from server', () => {
    let state = createInitialState('test-model', '/tmp')
    state = { ...state, isLoading: true }
    const next = applyEvent(state, makeEvent('run_finished', 1, {
      duration_ms: 5000,
    }))
    expect(next.currentRunStats.durationMs).toBe(5000)
    expect(next.isLoading).toBe(false)
  })
})
