import { array, boolean, nullable, object, oneOf, optional, tagged, text, uint, type Schema } from './schema.js'

/** Published addon results. Optional legacy fields remain absent; opaque
 * transcript/queue content and unknown extension keys are never rewritten. */
export interface SessionMeta {
  session_id: string
  title?: string | null
  model: string
  provider?: string
  thinking_level?: string | null
  cwd: string
  source?: string
  turns: number
  created_at: string
  updated_at: string
}
export interface SessionWithText extends SessionMeta {
  search_text: string
  user_prompts: string[]
  /** First real user turn; absent from older addons and empty sessions. */
  first_prompt?: string
  /** Paths edited or written, first-seen order; absent from older addons. */
  changed_paths?: string[]
}
export interface TranscriptItem { [key: string]: unknown }
export interface VariableInfo { key: string; value: string; updated_at?: string }
export interface QueuedPrompt { id: string; version: number; message: Record<string, unknown> }
export interface BackgroundProcess {
  task_id: string
  command: string
  cwd: string
  output_path: string
  status: 'running_foreground' | 'running' | 'completed' | 'failed' | 'killed'
  exit_code: number | null
  elapsed_ms: number
  output_file_truncated: boolean
  stopped_by_user: boolean
}
export type ManualCompactionOutcome =
  | {
      status: 'compacted'; summary: string; tokens_before: number; tokens_after: number
      messages_before: number; messages_after: number; context_window: number
      messages_evicted: number; current_run_reclaimed: number; compaction_level: number
      used_fallback: boolean; method?: 'remote' | 'local' | 'remote_failed_local'
      remote_blob_bytes?: number; fallback_reason?: string
    }
  | { status: 'nothing_to_compact' }
  | { status: 'cancelled' }
export type CompactionPhase = 'planning' | 'remote' | 'local_fallback' | 'local' | 'complete'
export interface ServerInfo { port: number; address: string; channels: string[]; channelCount: number }
export interface LoginCodeResponse { code: string; login_url: string; expires_at: number; expires_in_ms: number; interval_ms: number }
export interface CloudUser { id: string; name: string; email: string }
export type AuthPollResult =
  | { status: 'pending' | 'expired' | 'denied' }
  | { status: 'success'; state: { user: CloudUser }; sync_error?: string }
export type AuthRefreshStatus = 'recovered' | 'login_required' | 'unavailable'
export interface AuthRefreshResult { status: AuthRefreshStatus; user: CloudUser | null; error?: string | null; cleanup_error?: string | null }
export interface CloudNotice { id: string; kind: string; priority?: number; title: string; body_md?: string }

const record: Schema<Record<string, unknown>> = object({})
const integer: Schema<number> = { read(value, path) {
  if (typeof value !== 'number' || !Number.isInteger(value)) throw new Error(`Invalid native result at ${path}`)
  return value
} }
const sessionFields = {
  session_id: text, title: optional(nullable(text)), model: text, provider: optional(text),
  thinking_level: optional(nullable(text)), cwd: text, source: optional(text), turns: uint,
  created_at: text, updated_at: text,
}
export const sessionMeta: Schema<SessionMeta> = object(sessionFields)
export const sessions = array(sessionMeta)
export const sessionWithText: Schema<SessionWithText> = object({
  ...sessionFields, search_text: text, user_prompts: array(text),
  first_prompt: optional(text), changed_paths: optional(array(text)),
})
export const sessionsWithText: Schema<SessionWithText[]> = array(sessionWithText)
export const transcript: Schema<TranscriptItem[]> = array(record)
export const variables: Schema<VariableInfo[]> = array(object({ key: text, value: text, updated_at: optional(text) }))
export const queuedPrompt: Schema<QueuedPrompt> = object({ id: text, version: uint, message: record })
export const queuedPrompts = array(queuedPrompt)
export const backgroundProcess: Schema<BackgroundProcess> = object({
  task_id: text, command: text, cwd: text, output_path: text,
  status: oneOf('running_foreground', 'running', 'completed', 'failed', 'killed'),
  exit_code: nullable(integer), elapsed_ms: uint, output_file_truncated: boolean, stopped_by_user: boolean,
})
export const backgroundProcesses = array(backgroundProcess)
export const compactionPhase: Schema<CompactionPhase> = oneOf('planning', 'remote', 'local_fallback', 'local', 'complete')
export const compactionOutcome: Schema<ManualCompactionOutcome> = tagged('status', {
  compacted: object({
    status: oneOf('compacted'), summary: text, tokens_before: uint, tokens_after: uint,
    messages_before: uint, messages_after: uint, context_window: uint, messages_evicted: uint,
    current_run_reclaimed: uint, compaction_level: uint, used_fallback: boolean,
    method: optional(oneOf('remote', 'local', 'remote_failed_local')), remote_blob_bytes: optional(uint), fallback_reason: optional(text),
  }),
  nothing_to_compact: object({ status: oneOf('nothing_to_compact') }),
  cancelled: object({ status: oneOf('cancelled') }),
})
export const serverInfo: Schema<ServerInfo> = object({ port: uint, address: text, channels: array(text), channelCount: uint })
export const loginCode: Schema<LoginCodeResponse> = object({ code: text, login_url: text, expires_at: integer, expires_in_ms: integer, interval_ms: integer })
export const cloudUser: Schema<CloudUser> = object({ id: text, name: text, email: text })
export const authPoll: Schema<AuthPollResult> = tagged('status', {
  pending: object({ status: oneOf('pending') }), expired: object({ status: oneOf('expired') }), denied: object({ status: oneOf('denied') }),
  success: object({ status: oneOf('success'), state: object({ user: cloudUser }), sync_error: optional(text) }),
})
export const authRefresh: Schema<AuthRefreshResult> = object({
  status: oneOf('recovered', 'login_required', 'unavailable'), user: nullable(cloudUser),
  error: optional(nullable(text)), cleanup_error: optional(nullable(text)),
})
export const cloudNotices: Schema<CloudNotice[]> = array(object({ id: text, kind: text, priority: optional(integer), title: text, body_md: optional(text) }))

/** Do not include JSON parser messages: they can quote private payload bytes. */
export function decodeResult<T>(raw: string, schema: Schema<T>): T {
  let value: unknown
  try { value = JSON.parse(raw) } catch { throw new Error('Invalid native result at $') }
  return schema.read(value, '$')
}
