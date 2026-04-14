import type { RunEvent } from '../native/index.js'

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
