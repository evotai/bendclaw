import { describe, expect, test } from 'bun:test'
import { appendFrozenEvents } from '../src/utils/frozenUpdates.js'

describe('appendFrozenEvents', () => {
  test('coalesces buffered assistant deltas while frozen', () => {
    const buffered = [
      { kind: 'assistant_delta', payload: { delta: 'hel' } },
    ] as any[]
    const incoming = [
      { kind: 'assistant_delta', payload: { delta: 'lo' } },
      { kind: 'tool_started', payload: { tool_call_id: '1', tool_name: 'bash' } },
    ] as any[]

    const result = appendFrozenEvents(buffered, incoming)

    expect(result).toEqual([
      { kind: 'assistant_delta', payload: { delta: 'hello', thinking_delta: '' } },
      { kind: 'tool_started', payload: { tool_call_id: '1', tool_name: 'bash' } },
    ])
  })
})
