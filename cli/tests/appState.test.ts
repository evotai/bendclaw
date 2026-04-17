/**
 * AppState tests — verify applyEvent produces correct verbose text and stats.
 */

import { describe, test, expect } from 'bun:test'
import { createInitialState } from '../src/state/app.js'
import { applyEvent, countMessagesByRole } from '../src/state/reducer.js'
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
      message_stats: {
        user_count: 2,
        assistant_count: 2,
        tool_result_count: 1,
        image_count: 0,
        user_tokens: 100,
        assistant_tokens: 80,
        tool_result_tokens: 50,
        image_tokens: 0,
        tool_details: [['bash', 50]],
      },
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
    expect(evt.text).toContain('user 2')
    expect(evt.text).toContain('assistant 2')
    expect(evt.text).toContain('tool_result 1')
    expect(evt.text).toContain('system ~1k')
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

  test('shows per-tool breakdown when >= 2 tool types', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('llm_call_started', 1, {
      model: 'test-model',
      message_count: 6,
      message_stats: {
        user_count: 2,
        assistant_count: 1,
        tool_result_count: 3,
        image_count: 0,
        user_tokens: 50,
        assistant_tokens: 10,
        tool_result_tokens: 1400,
        image_tokens: 0,
        tool_details: [['bash', 600], ['read', 800]],
      },
      tools: [{}, {}],
      system_prompt_tokens: 1000,
      attempt: 0,
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    expect(evt.text).toContain('tool_result breakdown:')
    expect(evt.text).toContain('bash')
    expect(evt.text).toContain('read')
    expect(evt.text).toContain('%')
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
    expect(evt.text).toContain('5.0s')
    expect(evt.text).toContain('tok/s')
    expect(evt.text).toContain('4k in')
    expect(evt.text).toContain('120 out')
    // Timing with percentages
    expect(evt.text).toContain('ttfb 2.0s (40%)')
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
    expect(evt.text).toContain('72 msgs ~74k')
    expect(evt.text).toContain('65 msgs ~65k')
    expect(evt.text).toContain('saved ~9k')
  })

  test('compact actions with position bar, summary, and top/tail truncation', () => {
    const state = createInitialState('test-model', '/tmp')
    const next = applyEvent(state, makeEvent('context_compaction_completed', 1, {
      result: {
        type: 'level_compacted',
        level: 1,
        before_message_count: 100,
        after_message_count: 100,
        before_estimated_tokens: 50000,
        after_estimated_tokens: 45000,
        actions: [
          { index: 0, tool_name: 'read_file', method: 'Outline', before_tokens: 2000, after_tokens: 500, related_count: 3 },
          { index: 1, tool_name: 'bash', method: 'HeadTail', before_tokens: 1500, after_tokens: 800 },
          { index: 2, tool_name: 'search', method: 'Skipped', before_tokens: 100, after_tokens: 100 },
          { index: 3, tool_name: 'read_file', method: 'Outline', before_tokens: 500, after_tokens: 500 },
        ],
      },
    }))
    const evt = next.verboseEvents[next.verboseEvents.length - 1]!
    // Position bar present
    expect(evt.text).toContain('[')
    expect(evt.text).toContain(']')
    // Action summary
    expect(evt.text).toContain('outlined 2, head-tail 1')
    // After line with saved
    expect(evt.text).toContain('saved ~5k')
    // Actions header
    expect(evt.text).toContain('actions:')
    expect(evt.text).toContain('changed')
    // Skipped is filtered out
    expect(evt.text).not.toContain('Skipped')
    // Action lines show #index, tool_name, Method
    expect(evt.text).toContain('#0')
    expect(evt.text).toContain('read_file')
    expect(evt.text).toContain('Outline')
    expect(evt.text).toContain('#1')
    expect(evt.text).toContain('bash')
    expect(evt.text).toContain('HeadTail')
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
    expect(evt.text).toContain('12k')
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

// ---------------------------------------------------------------------------
// countMessagesByRole
// ---------------------------------------------------------------------------

describe('countMessagesByRole', () => {
  test('counts messages by role', () => {
    const stats = countMessagesByRole([
      { role: 'user', content: 'hello' },
      { role: 'assistant', content: 'hi' },
      { role: 'user', content: 'do something' },
    ])
    expect(stats.userCount).toBe(2)
    expect(stats.assistantCount).toBe(1)
    expect(stats.toolResultCount).toBe(0)
    expect(stats.userTokens).toBeGreaterThan(0)
    expect(stats.assistantTokens).toBeGreaterThan(0)
  })

  test('counts tool results and extracts tool names', () => {
    const stats = countMessagesByRole([
      { role: 'tool_result', toolName: 'bash', content: 'output' },
      { role: 'tool_result', toolName: 'read', content: 'file content here' },
      { role: 'tool_result', toolName: 'bash', content: 'more output' },
    ])
    expect(stats.toolResultCount).toBe(3)
    expect(stats.toolDetails).toHaveLength(3)
    // Sorted by tokens desc
    expect(stats.toolDetails[0]![0]).toBe('read') // longest content
  })

  test('handles empty messages', () => {
    const stats = countMessagesByRole([])
    expect(stats.userCount).toBe(0)
    expect(stats.assistantCount).toBe(0)
    expect(stats.toolResultCount).toBe(0)
    expect(stats.toolDetails).toHaveLength(0)
  })

  test('unknown roles count as user', () => {
    const stats = countMessagesByRole([
      { role: 'system', content: 'you are helpful' },
    ])
    expect(stats.userCount).toBe(1)
  })
})
