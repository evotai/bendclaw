import { isKnownRunKind, validateRunPayload, type RunPayloads } from './run-payload.js'

export interface RunEvent {
  event_id: string
  run_id: string
  session_id: string
  turn: number
  kind: string
  payload: Record<string, unknown>
  created_at: string
}

export type KnownRunEvent = {
  [K in keyof RunPayloads]: Omit<RunEvent, 'kind' | 'payload'> & { kind: K; payload: RunPayloads[K] }
}[keyof RunPayloads]

/** Narrows decoded events without closing the envelope to future event kinds. */
export function isKnownRunEvent(event: RunEvent): event is KnownRunEvent {
  return isKnownRunKind(event.kind)
}

/** Host bridge messages intentionally have no synthetic run/session envelope. */
export interface HostToolEvent {
  kind: 'host_tool_call'
  payload: {
    tool_name: string
    tool_call_id: string
    arguments: Record<string, unknown>
  }
}

export type QueryEvent = RunEvent | HostToolEvent

export function isHostToolEvent(event: QueryEvent): event is HostToolEvent {
  return event.kind === 'host_tool_call'
}

function invalid(path: string): never {
  throw new Error(`Invalid query event at ${path}`)
}

function object(value: unknown, path: string): asserts value is Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) invalid(path)
}

function validate(value: unknown): asserts value is QueryEvent {
  object(value, '$')
  if (typeof value.kind !== 'string') invalid('$.kind')
  object(value.payload, '$.payload')
  if (value.kind === 'host_tool_call') {
    for (const key of ['tool_name', 'tool_call_id']) {
      if (typeof value.payload[key] !== 'string' || value.payload[key] === '') invalid(`$.payload.${key}`)
    }
    object(value.payload.arguments, '$.payload.arguments')
    return
  }
  for (const key of ['event_id', 'run_id', 'session_id', 'created_at']) {
    if (typeof value[key] !== 'string') invalid(`$.${key}`)
  }
  if (typeof value.turn !== 'number' || !Number.isInteger(value.turn) || value.turn < 0 || value.turn > 0xffff_ffff) invalid('$.turn')
  validateRunPayload(value.kind, value.payload)
}

/** Validate known payloads as well as the envelope. Unknown kinds stay
 * forward-readable. Diagnostics never echo tool arguments or credentials. */
export function decodeQueryEvent(json: string): QueryEvent {
  let value: unknown
  try { value = JSON.parse(json) } catch { invalid('$ (JSON)') }
  validate(value)
  return value
}
