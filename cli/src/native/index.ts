/**
 * Typed wrapper around the NAPI native addon.
 * All Rust types cross the boundary as JSON strings — this module
 * parses them into proper TS interfaces.
 */

// @ts-ignore — binding.js is generated
import { NapiAgent as RawAgent, version as rawVersion } from './binding.js'
import type { NapiAgent as RawAgentType, NapiQueryStream as RawStreamType } from './binding.d.ts'

// ---------------------------------------------------------------------------
// Event types (mirrors Rust RunEvent / RunEventPayload)
// ---------------------------------------------------------------------------

export interface RunEvent {
  event_id: string
  run_id: string
  session_id: string
  turn: number
  kind: string
  payload: Record<string, unknown>
  created_at: string
}

export interface SessionMeta {
  session_id: string
  title: string
  model: string
  cwd: string
  created_at: string
  updated_at: string
  turn_count: number
}

export interface TranscriptItem {
  [key: string]: unknown
}

// ---------------------------------------------------------------------------
// QueryStream — async iterable over RunEvents
// ---------------------------------------------------------------------------

export class QueryStream {
  private raw: RawStreamType

  constructor(raw: RawStreamType) {
    this.raw = raw
  }

  get sessionId(): string {
    return this.raw.sessionId
  }

  async next(): Promise<RunEvent | null> {
    const json = await this.raw.next()
    if (json === null) return null
    return JSON.parse(json) as RunEvent
  }

  abort(): void {
    this.raw.abort()
  }

  /** Async iterator support — `for await (const event of stream)` */
  async *[Symbol.asyncIterator](): AsyncIterableIterator<RunEvent> {
    let event: RunEvent | null
    while ((event = await this.next()) !== null) {
      yield event
    }
  }
}

// ---------------------------------------------------------------------------
// Agent — main entry point
// ---------------------------------------------------------------------------

export class Agent {
  private raw: RawAgentType

  private constructor(raw: RawAgentType) {
    this.raw = raw
  }

  static create(model?: string): Agent {
    const raw = RawAgent.create(model ?? null)
    return new Agent(raw)
  }

  get model(): string {
    return this.raw.model
  }

  set model(value: string) {
    this.raw.model = value
  }

  get cwd(): string {
    return this.raw.cwd
  }

  async query(prompt: string, sessionId?: string): Promise<QueryStream> {
    const raw = await this.raw.query(prompt, sessionId ?? null)
    return new QueryStream(raw)
  }

  async listSessions(limit?: number): Promise<SessionMeta[]> {
    const json = await this.raw.listSessions(limit ?? null)
    return JSON.parse(json) as SessionMeta[]
  }

  async loadTranscript(sessionId: string): Promise<TranscriptItem[]> {
    const json = await this.raw.loadTranscript(sessionId)
    return JSON.parse(json) as TranscriptItem[]
  }
}

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

export function version(): string {
  return rawVersion()
}
