import { describe, expect, test } from 'bun:test'
import { formatEventForTest } from '../src/utils/transcriptLog.js'
import type { RunEvent } from '../src/native/index.js'

function makeEvent(kind: string, payload: Record<string, unknown>, turn = 1): RunEvent {
  return {
    event_id: 'evt-1',
    run_id: 'run-1',
    session_id: 'sess-1',
    turn,
    kind,
    payload,
    created_at: '2026-04-14T03:47:20.858030+00:00',
  }
}

describe('transcript log formatting', () => {
  test('omits run and turn start markers to match old log format', () => {
    expect(formatEventForTest(makeEvent('run_started', {}, 0))).toEqual([])
    expect(formatEventForTest(makeEvent('turn_started', {}, 1))).toEqual([])
  })

  test('formats compaction start and completion like the old log', () => {
    const started = makeEvent('context_compaction_started', {
      message_count: 3,
      estimated_tokens: 44,
      budget_tokens: 156000,
      system_prompt_tokens: 4000,
      context_window: 160000,
    })
    const completed = makeEvent('context_compaction_completed', {
      result: { type: 'no_op' },
    })

    expect(formatEventForTest(started)).toEqual([
      '[compact] 3 messages · ~44 tokens · 0% of budget',
    ])
    expect(formatEventForTest(completed)).toEqual([
      '[compact completed] no compaction needed',
    ])
  })

  test('formats llm call start and completion with old-style summary text', () => {
    const started = makeEvent('llm_call_started', {
      model: 'claude-opus-4-6',
      turn: 1,
      message_count: 1,
      message_bytes: 84,
      system_prompt_tokens: 744,
      tools: new Array(10).fill({}),
    })
    const completed = makeEvent('llm_call_completed', {
      usage: { input: 5779, output: 28, cache_read: 0, cache_write: 0 },
      metrics: { duration_ms: 3829, streaming_ms: 750, ttfb_ms: 3079, ttft_ms: 3079, chunk_count: 14 },
    })

    expect(formatEventForTest(started)).toEqual([
      '[llm call] claude-opus-4-6 · turn 1 · 1 messages · 10 tools · ~765 est tokens',
    ])
    expect(formatEventForTest(completed)).toEqual([
      '[llm completed] 5779 input · 28 output tokens · 3829ms · ttft 3079ms · 37 tok/s',
    ])
  })

  test('formats run_finished as the old two-line summary block', () => {
    const finished = makeEvent('run_finished', {
      duration_ms: 3833,
      turn_count: 1,
      usage: { input: 5779, output: 28, cache_read: 0, cache_write: 0 },
    })

    expect(formatEventForTest(finished)).toEqual([
      '---',
      'run 3.8s  ·  turns 1  ·  tokens 5807 (in 5779 · out 28)',
      '',
    ])
  })
})
