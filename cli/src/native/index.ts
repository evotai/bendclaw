/**
 * Typed wrapper around the NAPI native addon.
 * All Rust types cross the boundary as JSON strings — this module
 * parses them into proper TS interfaces.
 */

// @ts-ignore — binding.js is generated
import { NapiAgent as RawAgent, version as rawVersion, startServer as rawStartServer, startServerBackground as rawStartServerBackground } from './binding.js'
import type { NapiAgent as RawAgentType, NapiRun as RawRunType, NapiForkedAgent as RawForkedType } from './binding.d.ts'

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
  source: string
  turns: number
  created_at: string
  updated_at: string
}

export interface TranscriptItem {
  [key: string]: unknown
}

export interface SessionWithText extends SessionMeta {
  search_text: string
}

export interface VariableInfo {
  key: string
  value: string
}

export type SubmitOutcome =
  | { kind: 'run'; stream: QueryStream }
  | { kind: 'command'; message: string }

export interface ConfigInfo {
  provider: string
  envPath: string
  hasApiKey: boolean
  baseUrl: string | null
  anthropicModel: string
  openaiModel: string
  availableModels: string[]
}

// ---------------------------------------------------------------------------
// QueryStream — async iterable over RunEvents
// ---------------------------------------------------------------------------

export class QueryStream {
  private raw: RawRunType

  constructor(raw: RawRunType) {
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

  steer(text: string, contentJson?: string): void {
    this.raw.steer(text, contentJson ?? null)
  }

  followUp(text: string): void {
    this.raw.followUp(text)
  }

  /** Respond to an ask_user event with a JSON-encoded AskUserResponse. */
  async respondAskUser(responseJson: string): Promise<void> {
    await this.raw.respondAskUser(responseJson)
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
// Content block types for multi-content queries
// ---------------------------------------------------------------------------

export interface TextContentBlock {
  type: 'text'
  text: string
}

export interface ImageContentBlock {
  type: 'image'
  data: string
  mimeType: string
  source?: string
}

export type ContentBlock = TextContentBlock | ImageContentBlock

// ---------------------------------------------------------------------------
// Agent — main entry point
// ---------------------------------------------------------------------------

export class Agent {
  private raw: RawAgentType

  private constructor(raw: RawAgentType) {
    this.raw = raw
  }

  static async create(model?: string, envFile?: string): Promise<Agent> {
    const raw = await RawAgent.create(model ?? null, envFile ?? null)
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

  async query(prompt: string, sessionId?: string, toolMode?: string, contentJson?: string): Promise<QueryStream> {
    const outcome = await this.raw.query(prompt, sessionId ?? null, toolMode ?? null, contentJson ?? null)
    if (outcome.kind !== 'run') {
      throw new Error(`Expected run, got command: ${outcome.message}`)
    }
    const run = outcome.takeRun()
    if (!run) {
      throw new Error('No run in submit outcome')
    }
    return new QueryStream(run)
  }

  /**
   * Unified submit — handles both commands and normal queries.
   * Commands return { kind: 'command', message }, queries return { kind: 'run', stream }.
   */
  async submit(
    prompt: string,
    sessionId?: string,
    toolMode?: string,
    contentJson?: string,
  ): Promise<SubmitOutcome> {
    const outcome = await this.raw.query(prompt, sessionId ?? null, toolMode ?? null, contentJson ?? null)
    if (outcome.kind === 'command') {
      return { kind: 'command', message: outcome.message ?? '' }
    }
    const run = outcome.takeRun()
    if (!run) {
      throw new Error('No run in submit outcome')
    }
    return { kind: 'run', stream: new QueryStream(run) }
  }

  async listSessions(limit?: number): Promise<SessionMeta[]> {
    const json = await this.raw.listSessions(limit ?? null)
    return JSON.parse(json) as SessionMeta[]
  }

  async deleteSession(sessionId: string): Promise<boolean> {
    return this.raw.deleteSession(sessionId)
  }

  async listSessionsWithText(limit?: number): Promise<SessionWithText[]> {
    const json = await this.raw.listSessionsWithText(limit ?? null)
    return JSON.parse(json) as SessionWithText[]
  }

  async loadTranscript(sessionId: string): Promise<TranscriptItem[]> {
    const json = await this.raw.loadTranscript(sessionId)
    return JSON.parse(json) as TranscriptItem[]
  }

  async findSession(sessionId: string): Promise<SessionMeta | null> {
    const json = await this.raw.findSession(sessionId)
    return json ? JSON.parse(json) as SessionMeta : null
  }

  fork(systemPrompt: string): ForkedAgent {
    const raw = this.raw.fork(systemPrompt)
    return new ForkedAgent(raw)
  }

  listVariables(): VariableInfo[] {
    return JSON.parse(this.raw.listVariables()) as VariableInfo[]
  }

  async setVariable(key: string, value: string): Promise<void> {
    await this.raw.setVariable(key, value)
  }

  async deleteVariable(key: string): Promise<boolean> {
    return this.raw.deleteVariable(key)
  }

  configInfo(): ConfigInfo {
    return JSON.parse(this.raw.configInfo()) as ConfigInfo
  }

  availableModels(): string[] {
    return this.raw.availableModels()
  }

  setProvider(provider: string): void {
    this.raw.setProvider(provider)
  }

  setLimits(maxTurns?: number, maxTokens?: number, maxDurationSecs?: number): void {
    this.raw.setLimits(maxTurns ?? null, maxTokens ?? null, maxDurationSecs ?? null)
  }

  appendSystemPrompt(extra: string): void {
    this.raw.appendSystemPrompt(extra)
  }

  addSkillsDirs(dirs: string[]): void {
    this.raw.addSkillsDirs(dirs)
  }

  steer(sessionId: string, text: string, contentJson?: string): void {
    this.raw.steer(sessionId, text, contentJson ?? null)
  }

  followUp(sessionId: string, text: string): void {
    this.raw.followUp(sessionId, text)
  }

  abortRun(sessionId: string): void {
    this.raw.abortRun(sessionId)
  }
}

// ---------------------------------------------------------------------------
// ForkedAgent — ephemeral readonly side conversation
// ---------------------------------------------------------------------------

export class ForkedAgent {
  private raw: RawForkedType

  constructor(raw: RawForkedType) {
    this.raw = raw
  }

  async query(prompt: string): Promise<QueryStream> {
    const raw = await this.raw.query(prompt)
    return new QueryStream(raw)
  }
}

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

export function version(): string {
  return rawVersion()
}

export async function startServer(port?: number, model?: string, envFile?: string): Promise<void> {
  return rawStartServer(port ?? null, model ?? null, envFile ?? null)
}

export interface ServerInfo {
  port: number
  address: string
  channels: string[]
  channelCount: number
}

export async function startServerBackground(port?: number, model?: string, envFile?: string): Promise<ServerInfo | null> {
  const json = await rawStartServerBackground(port ?? null, model ?? null, envFile ?? null)
  if (json === null) return null
  return JSON.parse(json) as ServerInfo
}
