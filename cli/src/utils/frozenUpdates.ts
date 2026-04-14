import type { RunEvent } from '../native/index.js'
import { coalesceStreamEvents } from './streamBatch.js'

export function appendFrozenEvents(buffer: RunEvent[], incoming: RunEvent[]): RunEvent[] {
  return coalesceStreamEvents([...buffer, ...incoming])
}
