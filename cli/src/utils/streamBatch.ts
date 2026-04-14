import type { RunEvent } from '../native/index.js'

const IMMEDIATE_FLUSH_PATTERN = /\n|```/
const IMMEDIATE_FLUSH_TEXT_BUDGET = 160

export const STREAM_FLUSH_INTERVAL_MS = 120

export function coalesceStreamEvents(events: RunEvent[]): RunEvent[] {
  if (events.length <= 1) {
    return events
  }

  const merged: RunEvent[] = []

  for (const event of events) {
    const previous = merged[merged.length - 1]
    if (canMergeAssistantDelta(previous, event)) {
      const prevPayload = (previous.payload ?? {}) as Record<string, unknown>
      const nextPayload = (event.payload ?? {}) as Record<string, unknown>
      merged[merged.length - 1] = {
        ...previous,
        payload: {
          ...prevPayload,
          ...nextPayload,
          delta: `${String(prevPayload.delta ?? '')}${String(nextPayload.delta ?? '')}`,
          thinking_delta: `${String(prevPayload.thinking_delta ?? '')}${String(nextPayload.thinking_delta ?? '')}`,
        },
      }
      continue
    }
    merged.push(event)
  }

  return merged
}

function canMergeAssistantDelta(previous: RunEvent | undefined, current: RunEvent): boolean {
  return previous?.kind === 'assistant_delta' && current.kind === 'assistant_delta'
}

export function shouldFlushAssistantDeltaBatchImmediately(events: RunEvent[]): boolean {
  let bufferedChars = 0

  for (const event of events) {
    if (event.kind !== 'assistant_delta') {
      continue
    }

    const payload = (event.payload ?? {}) as Record<string, unknown>
    const delta = String(payload.delta ?? '')
    const thinkingDelta = String(payload.thinking_delta ?? '')
    bufferedChars += delta.length + thinkingDelta.length

    if (IMMEDIATE_FLUSH_PATTERN.test(delta) || IMMEDIATE_FLUSH_PATTERN.test(thinkingDelta)) {
      return true
    }
  }

  return bufferedChars >= IMMEDIATE_FLUSH_TEXT_BUDGET
}
