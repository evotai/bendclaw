/**
 * Typed wrapper around the NAPI native addon.
 * All Rust types cross the boundary as JSON strings — this module
 * parses them into proper TS interfaces.
 */

// @ts-ignore — binding.js is generated
import { NapiAgent as RawAgent, version as rawVersion, startServer as rawStartServer, startServerBackground as rawStartServerBackground, fastExit as rawFastExit, authBegin as rawAuthBegin, authPoll as rawAuthPoll, authLogout as rawAuthLogout, authSyncModels as rawAuthSyncModels, authSyncNotices as rawAuthSyncNotices, authWhoami as rawAuthWhoami, authRefreshSession as rawAuthRefreshSession, authNotices as rawAuthNotices } from './binding.js'

type RawAgentType = any
type RawRunType = any
type RawForkedType = any

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
  /** Provider paired with model; absent on sessions saved before provider-aware selection. */
  provider?: string
  thinking_level: string | null
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
  /** Real user turns, oldest first. Compaction boilerplate is excluded. */
  user_prompts: string[]
}

export interface VariableInfo {
  key: string
  value: string
}

export interface BackgroundProcess {
  task_id: string
  command: string
  cwd: string
  output_path: string
  status: 'running_foreground' | 'running' | 'completed' | 'failed' | 'timed_out' | 'killed'
  exit_code: number | null
  elapsed_ms: number
  output_file_truncated: boolean
  /** Optional: absent on payloads written before stop attribution existed. */
  stopped_by_user?: boolean
}

export type SubmitOutcome =
  | { kind: 'run'; stream: QueryStream }
  | { kind: 'command'; message: string }

export interface QueuedPrompt {
  id: string
  version: number
  message: Record<string, unknown>
}

export type PromptQueueKind = 'steering' | 'follow_up'

export type ModelProtocol = 'anthropic' | 'openai' | 'openai_responses'

export interface ModelOption {
  provider: string
  /** Wire protocol configured for this provider. */
  protocol?: ModelProtocol
  model: string
  /** Provider-qualified value accepted by --model and the model setter. */
  spec: string
  /** Heading for this model's group, pushed by the server. Cloud models only. */
  group_label?: string
  /** Where this group sits relative to the others. Cloud models only. */
  group_order?: number
  /** Catalog rank inside the tier: higher shows earlier. Cloud models only. */
  sort_order?: number
  /** Present on evot cloud models; carries server-pushed metadata. */
  free?: {
    display_name?: string
    tagline?: string
    is_new?: boolean
    /** `base` (open to everyone) or `special` (granted per account). */
    tier?: string
  }
}

export interface ConfigInfo {
  provider: string
  protocol: ModelProtocol
  envPath: string
  hasApiKey: boolean
  baseUrl: string | null
  availableModels: ModelOption[]
  thinkingLevel: string
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

  steer(text: string, contentJson?: string): QueuedPrompt {
    return JSON.parse(this.raw.steer(text, contentJson ?? null)) as QueuedPrompt
  }

  followUp(text: string, contentJson?: string): QueuedPrompt {
    return JSON.parse(this.raw.followUp(text, contentJson ?? null)) as QueuedPrompt
  }

  queuedPrompts(queue: PromptQueueKind): QueuedPrompt[] {
    return JSON.parse(this.raw.queuedPrompts(queue)) as QueuedPrompt[]
  }

  updateQueuedPrompt(queue: PromptQueueKind, id: string, version: number, text: string): QueuedPrompt {
    return JSON.parse(this.raw.updateQueuedPrompt(queue, id, version, text)) as QueuedPrompt
  }

  removeQueuedPrompt(queue: PromptQueueKind, id: string, version?: number): QueuedPrompt {
    return JSON.parse(this.raw.removeQueuedPrompt(queue, id, version ?? null)) as QueuedPrompt
  }

  sendQueuedPromptNow(id: string, version?: number): QueuedPrompt {
    return JSON.parse(this.raw.sendQueuedPromptNow(id, version ?? null)) as QueuedPrompt
  }

  moveQueuedPrompt(queue: PromptQueueKind, id: string, version: number, direction: 'up' | 'down'): QueuedPrompt {
    return JSON.parse(this.raw.moveQueuedPrompt(queue, id, version, direction)) as QueuedPrompt
  }

  clearQueuedPrompts(queue: PromptQueueKind): void {
    this.raw.clearQueuedPrompts(queue)
  }

  /** Respond to a host_tool_call event with a JSON-encoded response.
   *  Payload shape: { tool_call_id, content, details?, is_error? }. */
  async respondHostTool(responseJson: string): Promise<void> {
    await this.raw.respondHostTool(responseJson)
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

export type ImageContentSource =
  | { type: 'path'; path: string }
  | { type: 'base64'; data: string; path?: string }

export interface ImageContentBlock {
  type: 'image'
  mimeType: string
  source: ImageContentSource
}

export type ContentBlock = TextContentBlock | ImageContentBlock

// ---------------------------------------------------------------------------
// Agent — main entry point
// ---------------------------------------------------------------------------

export type ManualCompactionOutcome =
  | {
      status: 'compacted'
      summary: string
      tokens_before: number
      tokens_after: number
      messages_before: number
      messages_after: number
      context_window: number
      messages_evicted: number
      current_run_reclaimed: number
      compaction_level: number
      used_fallback: boolean
      method?: 'remote' | 'local' | 'remote_failed_local'
      remote_blob_bytes?: number
      fallback_reason?: string
    }
  | { status: 'nothing_to_compact' }
  | { status: 'cancelled' }

export type CompactionPhase = 'planning' | 'remote' | 'local_fallback' | 'local' | 'complete'

export class CompactionTask {
  private raw: any

  constructor(raw: any) {
    this.raw = raw
  }

  get phase(): CompactionPhase {
    return this.raw.phase as CompactionPhase
  }

  async result(): Promise<ManualCompactionOutcome> {
    return JSON.parse(await this.raw.result()) as ManualCompactionOutcome
  }

  abort(): void {
    this.raw.abort()
  }
}

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

  async query(prompt: string, sessionId?: string, toolMode?: string, contentJson?: string, hostSpecsJson?: string): Promise<QueryStream> {
    const outcome = await this.raw.query(prompt, sessionId ?? null, toolMode ?? null, contentJson ?? null, hostSpecsJson ?? null)
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
    hostSpecsJson?: string,
  ): Promise<SubmitOutcome> {
    const outcome = await this.raw.query(prompt, sessionId ?? null, toolMode ?? null, contentJson ?? null, hostSpecsJson ?? null)
    if (outcome.kind === 'command') {
      return { kind: 'command', message: outcome.message ?? '' }
    }
    const run = outcome.takeRun()
    if (!run) {
      throw new Error('No run in submit outcome')
    }
    return { kind: 'run', stream: new QueryStream(run) }
  }

  async createSession(): Promise<SessionMeta> {
    const json = await this.raw.createSession()
    return JSON.parse(json) as SessionMeta
  }

  async listSessions(limit?: number): Promise<SessionMeta[]> {
    const json = await this.raw.listSessions(limit ?? null)
    return JSON.parse(json) as SessionMeta[]
  }

  async deleteSession(sessionId: string): Promise<boolean> {
    return this.raw.deleteSession(sessionId)
  }

  backgroundProcesses(sessionId: string): BackgroundProcess[] {
    return JSON.parse(this.raw.backgroundProcesses(sessionId)) as BackgroundProcess[]
  }

  async stopBackgroundProcess(sessionId: string, taskId: string): Promise<BackgroundProcess | null> {
    const json = await this.raw.stopBackgroundProcess(sessionId, taskId)
    return json ? JSON.parse(json) as BackgroundProcess : null
  }

  async stopAllBackgroundProcesses(sessionId: string): Promise<BackgroundProcess[]> {
    const json = await this.raw.stopAllBackgroundProcesses(sessionId)
    return JSON.parse(json) as BackgroundProcess[]
  }

  /** Detach every foreground shell so the turn can be reclaimed without
   *  discarding work. The processes keep running; only the waiting ends.
   *  Returns how many moved. */
  backgroundForegroundProcesses(sessionId: string): number {
    return this.raw.backgroundForegroundProcesses(sessionId)
  }

  /** Same detach, attributed to a queued message needing delivery. Steering is
   *  only inspected between tool calls, so a foreground shell would otherwise
   *  hold a typed message until it finished. */
  backgroundForegroundProcessesForMessage(sessionId: string): number {
    return this.raw.backgroundForegroundProcessesForMessage(sessionId)
  }

  /** Blocking `task_output` waits in flight. Such a wait holds the turn while
   *  the task it watches is already backgrounded, so no foreground shell exists
   *  to detach. */
  blockingTaskWaits(sessionId: string): number {
    return this.raw.blockingTaskWaits(sessionId)
  }

  /** End in-flight blocking waits, returning how many were released. The
   *  watched tasks keep running; only the waiting ends. */
  releaseBlockingTaskWaits(sessionId: string): number {
    return this.raw.releaseBlockingTaskWaits(sessionId)
  }

  /** Kill every background process synchronously. Safe to call before fastExit,
   *  which skips the async teardown that would otherwise stop them. */
  killAllBackgroundProcessesNow(): number {
    return this.raw.killAllBackgroundProcessesNow()
  }

  async listSessionsWithText(limit?: number): Promise<SessionWithText[]> {
    const json = await this.raw.listSessionsWithText(limit ?? null)
    return JSON.parse(json) as SessionWithText[]
  }

  async loadTranscript(sessionId: string): Promise<TranscriptItem[]> {
    const json = await this.raw.loadTranscript(sessionId)
    return JSON.parse(json) as TranscriptItem[]
  }

  async loadContextTranscript(sessionId: string): Promise<TranscriptItem[]> {
    const json = await this.raw.loadContextTranscript(sessionId)
    return JSON.parse(json) as TranscriptItem[]
  }

  async loadResumeTranscript(sessionId: string): Promise<TranscriptItem[]> {
    const json = await this.raw.loadResumeTranscript(sessionId)
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

  /**
   * Re-resolve the live model selection after login, logout, or key recovery.
   * A selection the fresh config still serves is kept, so recovering a scoped
   * key does not move this session onto a different model. Returns false only
   * when nothing is configured any more.
   */
  reloadSelection(): boolean {
    return this.raw.reloadSelection()
  }

  /**
   * Reload provider/model from disk, including its configured thinking level.
   * Returns false when the saved selection is unavailable and the current live
   * selection was refreshed instead.
   */
  reloadProvider(provider: string): boolean {
    return this.raw.reloadProvider(provider)
  }

  /**
   * Advance the thinking level to the next tier the current model supports,
   * wrapping around. Returns the new level's display label, or null when the
   * model has no selectable reasoning levels.
   */
  cycleThinkingLevel(): string | null {
    return this.raw.cycleThinkingLevel()
  }

  /** Apply an explicit live thinking level when supported by the active model. */
  restoreThinkingLevel(level: string): void {
    this.raw.restoreThinkingLevel(level)
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

  setSkillNames(names: string[]): void {
    this.raw.setSkillNames(names)
  }

  /**
   * The fully-resolved, ordered skills directories the agent scans (managed
   * builtins + global + EVOT_SKILLS_DIRS from config/env-file + claude).
   * Read this instead of re-deriving from process.env so `/skill list` and the
   * banner match what the agent actually loads (see issue #38).
   */
  skillsDirs(): string[] {
    return this.raw.skillsDirs()
  }

  compact(sessionId: string, customInstructions?: string): CompactionTask {
    return new CompactionTask(this.raw.compact(sessionId, customInstructions || null))
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

/**
 * Terminate the process immediately via `std::process::exit`, bypassing all
 * Rust `Drop` impls and async runtime shutdown. Use on user-triggered exit so
 * large sessions don't stall on tokio runtime teardown.
 * Callers must restore terminal state (raw mode, cursor, bracketed paste)
 * before invoking this.
 */
export function fastExit(code = 0): never {
  rawFastExit(code)
  // rawFastExit does not return; this satisfies the `never` type
  throw new Error('unreachable')
}

// ---------------------------------------------------------------------------
// Cloud auth (evot login)
// ---------------------------------------------------------------------------

export interface LoginCodeResponse {
  code: string
  login_url: string
  expires_at: number
  expires_in_ms: number
  interval_ms: number
}

export type AuthPollResult =
  | { status: 'pending' | 'expired' | 'denied' }
  | { status: 'success'; state: { user: { id: string; name: string; email: string } }; sync_error?: string }

function parseJsonOrThrow(raw: unknown, context: string): unknown {
  if (typeof raw !== 'string') return raw
  try {
    return JSON.parse(raw)
  } catch {
    // The addon surfaces Rust errors as plain text — rethrow with context.
    throw new Error(raw.startsWith('Error') ? `${context}: ${raw.replace(/^Error:\s*/, '')}` : `${context}: ${raw}`)
  }
}

export async function authBegin(serverUrl: string, fingerprintId: string): Promise<LoginCodeResponse> {
  return parseJsonOrThrow(await rawAuthBegin(serverUrl, fingerprintId), 'login failed') as LoginCodeResponse
}

export async function authPoll(serverUrl: string, code: string, expiresAt: number): Promise<AuthPollResult> {
  return parseJsonOrThrow(await rawAuthPoll(serverUrl, code, expiresAt), 'login polling failed') as AuthPollResult
}

export async function authSyncModels(): Promise<void> {
  await rawAuthSyncModels()
}

export async function authSyncNotices(): Promise<CloudNotice[]> {
  return parseJsonOrThrow(await rawAuthSyncNotices(), 'notice sync failed') as CloudNotice[]
}

export async function authLogout(): Promise<void> {
  await rawAuthLogout()
}

export async function authWhoami(): Promise<{ id: string; name: string; email: string } | null> {
  const raw = await rawAuthWhoami()
  if (!raw) return null
  try {
    return JSON.parse(raw)
  } catch {
    return null
  }
}

export interface CloudUser {
  id: string
  name: string
  email: string
}

/**
 * Outcome of repairing a cloud session the gateway rejected.
 * - `recovered`: a fresh scoped key was cached; retry the request.
 * - `login_required`: the CLI token was refused and has been cleared.
 * - `unavailable`: server unreachable; nothing was cleared.
 */
export type AuthRefreshStatus = 'recovered' | 'login_required' | 'unavailable'

export interface AuthRefreshResult {
  status: AuthRefreshStatus
  /** Null only when a new login is required. */
  user: CloudUser | null
  /** Why the server could not be reached, when status is `unavailable`. */
  error?: string | null
  /** Set when clearing a refused credential failed. */
  cleanup_error?: string | null
}

/** Re-mint the scoped LLM key after the gateway reported `session_revoked`. */
export async function authRefreshSession(): Promise<AuthRefreshResult> {
  return parseJsonOrThrow(await rawAuthRefreshSession(), 'session refresh failed') as AuthRefreshResult
}

export interface CloudNotice {
  id: string
  kind: 'notice' | 'ad'
  priority?: number
  title: string
  body_md?: string
}

export function authNotices(): CloudNotice[] {
  const raw = rawAuthNotices()
  if (!raw) return []
  try {
    return JSON.parse(raw) as CloudNotice[]
  } catch {
    return []
  }
}
