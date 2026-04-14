import { describe, expect, test } from 'bun:test'
import {
  coalesceStreamEvents,
  shouldFlushAssistantDeltaBatchImmediately,
} from '../src/utils/streamBatch.js'

describe('coalesceStreamEvents', () => {
  test('merges adjacent assistant deltas into a single event', () => {
    const events = [
      { kind: 'assistant_delta', payload: { delta: 'he' } },
      { kind: 'assistant_delta', payload: { delta: 'llo', thinking_delta: '...' } },
      { kind: 'tool_started', payload: { tool_call_id: '1', tool_name: 'bash' } },
    ] as any[]

    const result = coalesceStreamEvents(events)

    expect(result).toHaveLength(2)
    expect(result[0]).toEqual({
      kind: 'assistant_delta',
      payload: { delta: 'hello', thinking_delta: '...' },
    })
    expect(result[1]).toBe(events[2])
  })

  test('does not merge across non-delta boundaries', () => {
    const events = [
      { kind: 'assistant_delta', payload: { delta: 'a' } },
      { kind: 'tool_started', payload: { tool_call_id: '1', tool_name: 'bash' } },
      { kind: 'assistant_delta', payload: { delta: 'b' } },
    ] as any[]

    const result = coalesceStreamEvents(events)

    expect(result).toHaveLength(3)
    expect(result).toEqual(events)
  })
})

describe('shouldFlushAssistantDeltaBatchImmediately', () => {
  test('keeps short token dribbles buffered', () => {
    const events = [
      { kind: 'assistant_delta', payload: { delta: 'he' } },
      { kind: 'assistant_delta', payload: { delta: 'llo' } },
    ] as any[]

    expect(shouldFlushAssistantDeltaBatchImmediately(events)).toBe(false)
  })

  test('flushes immediately when a newline arrives', () => {
    const events = [
      { kind: 'assistant_delta', payload: { delta: 'hello' } },
      { kind: 'assistant_delta', payload: { delta: '\nworld' } },
    ] as any[]

    expect(shouldFlushAssistantDeltaBatchImmediately(events)).toBe(true)
  })

  test('flushes immediately when buffered text gets large', () => {
    const events = [
      { kind: 'assistant_delta', payload: { delta: 'a'.repeat(200) } },
    ] as any[]

    expect(shouldFlushAssistantDeltaBatchImmediately(events)).toBe(true)
  })
})
