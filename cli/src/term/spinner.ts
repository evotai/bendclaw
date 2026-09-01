/**
 * Spinner — ANSI-based animated loading indicator.
 * Pure logic: returns the string to display, no React.
 */

import { formatCacheHitPercent } from '../render/cache.js'

function getSpinnerChars(): string[] {
  if (process.env.TERM === 'xterm-ghostty') {
    return ['·', '✢', '✳', '✶', '✻', '*']
  }
  return process.platform === 'darwin'
    ? ['·', '✢', '✳', '✶', '✻', '✽']
    : ['·', '✢', '*', '✶', '✻', '✽']
}

const SPINNER_CHARS = getSpinnerChars()
const SPINNER_FRAMES = [...SPINNER_CHARS, ...[...SPINNER_CHARS].reverse()]
const SLOW_THRESHOLD_MS = 8000
// Actions that legitimately run long get wider thresholds so routine
// execution is not painted red as "slow": bash commands regularly outlive 8s
// (builds, test suites), and manual compaction includes an LLM summarization
// pass with a 30s budget before its deterministic fallback kicks in.
const SLOW_THRESHOLD_BY_TOOL_MS: Record<string, number> = {
  bash: 30_000,
  compact: 30_000,
  log_shot_render: 30_000,
  log_shot_chrome: 30_000,
  log_shot_capture: 30_000,
  log_shot_open: 30_000,
  ask_user: Number.POSITIVE_INFINITY,
  askuser: Number.POSITIVE_INFINITY,
}

function slowThresholdMs(state: SpinnerState): number {
  if (state.phase === 'executing' && state.toolName) {
    return SLOW_THRESHOLD_BY_TOOL_MS[state.toolName.toLowerCase()] ?? SLOW_THRESHOLD_MS
  }
  return SLOW_THRESHOLD_MS
}

/**
 * Run phases surfaced on the spinner line. `preparing` covers local work
 * (context assembly between tool results and the next provider request),
 * `waiting` is a request in flight with no content yet, `thinking` /
 * `responding` distinguish reasoning deltas from answer/tool-call deltas, and
 * `executing` is tool execution.
 */
export type SpinnerPhase = 'preparing' | 'waiting' | 'quota_waiting' | 'outage_waiting' | 'thinking' | 'responding' | 'executing'

/** Cancellable long-wait phases: quota exhaustion or a sustained upstream outage. */
export type LongWaitPhase = 'quota_waiting' | 'outage_waiting'

export function isLongWaitPhase(phase: SpinnerPhase): phase is LongWaitPhase {
  return phase === 'quota_waiting' || phase === 'outage_waiting'
}

/**
 * Map a tool name to a human action label shown on the spinner while it runs.
 * Grouped into verbs that mirror the tool-card glyphs (read/search/edit/...).
 * Unknown tools fall back to a generic "Working".
 */
export function toolActionLabel(toolName: string): string {
  switch (toolName.toLowerCase()) {
    case 'read': case 'read_code': return 'Reading'
    case 'grep': case 'glob': case 'find': case 'search': case 'semantic_code_search': return 'Searching'
    case 'edit': case 'file_edit': return 'Applying changes'
    case 'write': case 'file_write': return 'Writing file'
    case 'bash': return 'Running command'
    case 'web_fetch': case 'webfetch': return 'Fetching'
    case 'plan': return 'Planning'
    case 'skill': return 'Loading skill'
    case 'compact': return 'Compacting'
    case 'compact_remote': return 'Compacting remote'
    case 'compact_local': return 'Compacting local'
    case 'compact_local_fallback': return 'Compacting local fallback'
    case 'log_shot_render': return 'Rendering shot'
    case 'log_shot_chrome': return 'Starting Chrome'
    case 'log_shot_capture': return 'Capturing PNG'
    case 'log_shot_open': return 'Opening shot'
    case 'ask_user': case 'askuser': return 'Waiting for you'
    default: return 'Working'
  }
}

export interface SpinnerState {
  frame: number
  phase: SpinnerPhase
  phaseStartedAt: number
  lastTokenAt: number | null
  streaming: boolean
  toolName: string | null
  waitRetryAt: number | null
  tokenCount: number
  streamStartedAt: number | null
  glimmerPos: number
}

export function createSpinnerState(): SpinnerState {
  return {
    frame: 0,
    phase: 'preparing',
    phaseStartedAt: Date.now(),
    lastTokenAt: null,
    streaming: false,
    toolName: null,
    waitRetryAt: null,
    tokenCount: 0,
    streamStartedAt: null,
    glimmerPos: -2,
  }
}

export function advanceSpinner(state: SpinnerState): SpinnerState {
  return {
    ...state,
    frame: (state.frame + 1) % SPINNER_FRAMES.length,
    glimmerPos: state.glimmerPos + 1 > 30 ? -2 : state.glimmerPos + 1,
  }
}

export function setSpinnerPhase(state: SpinnerState, phase: SpinnerPhase, toolName?: string | null): SpinnerState {
  if (state.phase === phase && state.toolName === (toolName ?? null)) return state
  return {
    ...state,
    phase,
    phaseStartedAt: Date.now(),
    toolName: toolName ?? null,
    waitRetryAt: isLongWaitPhase(phase) ? state.waitRetryAt : null,
  }
}

export function setLongWait(state: SpinnerState, phase: LongWaitPhase, delayMs: number, now = Date.now()): SpinnerState {
  return {
    ...resetStreamStats(state),
    phase,
    phaseStartedAt: now,
    toolName: null,
    waitRetryAt: now + Math.max(0, delayMs),
  }
}

export function recordStreamDelta(state: SpinnerState, textDelta: string, now = Date.now()): SpinnerState {
  const tokens = estimateTokens(textDelta)
  return {
    ...state,
    lastTokenAt: now,
    streaming: true,
    tokenCount: state.tokenCount + tokens,
    streamStartedAt: state.streamStartedAt ?? now,
  }
}

export function resetStreamStats(state: SpinnerState): SpinnerState {
  return {
    ...state,
    lastTokenAt: null,
    streaming: false,
    tokenCount: 0,
    streamStartedAt: null,
  }
}

export function isSlow(state: SpinnerState, now: number): boolean {
  if (isLongWaitPhase(state.phase)) return false
  const threshold = slowThresholdMs(state)
  // While the model is emitting, health is measured from the last token:
  // a flowing stream is never slow, and a stalled one is surfaced instead of
  // being hidden behind the streaming flag forever.
  if ((state.phase === 'thinking' || state.phase === 'responding') && state.lastTokenAt != null) {
    return now - state.lastTokenAt > threshold
  }
  if (state.streaming) return false
  return now - state.phaseStartedAt > threshold
}

export interface SpinnerStats {
  /** Uncached prompt tokens from completed provider usage. */
  inputTokens?: number
  outputTokens?: number
  cacheReadTokens?: number
  cacheWriteTokens?: number
}

export interface SpinnerFormatOptions {
  /** Show the keyboard interrupt hint. Defaults to true for agent/tool runs. */
  interruptible?: boolean
  /** Requested model, used to identify long quota waits. */
  model?: string
  /**
   * Work is being waited on that esc can release without killing it: a shell
   * watched in the foreground, or a blocking `task_output` call holding the
   * turn. The hint has to say so — offering only "esc to interrupt" while the
   * softer gesture is the one bound would describe the wrong outcome. Worded as
   * "stop waiting" because a blocked task is already in the background; what
   * ends is the waiting, not the work.
   */
  backgroundable?: boolean
}

export function formatSpinnerLine(
  state: SpinnerState,
  now: number,
  stats?: SpinnerStats,
  options: SpinnerFormatOptions = {},
): string {
  const elapsed = now - state.phaseStartedAt
  const slow = isSlow(state, now)
  const char = SPINNER_FRAMES[state.frame]!

  const action = state.toolName ? toolActionLabel(state.toolName) : 'Working'
  let label: string
  switch (state.phase) {
    case 'executing':
      label = slow ? `${action} slow…` : `${action}…`
      break
    case 'waiting':
      label = slow ? 'LLM slow…' : 'Waiting for model…'
      break
    case 'quota_waiting': {
      const seconds = Math.max(0, Math.ceil(((state.waitRetryAt ?? now) - now) / 1000))
      const model = options.model?.replace(/\u001b(?:\[[0-9;?]*[ -/]*[@-~]|].*?(?:\u0007|\u001b\\)|.)/g, '').replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, '').trim()
      const subject = model ? `${model} quota unavailable` : 'Model quota unavailable'
      label = seconds > 0
        ? `${subject} · retrying in ${formatWaitCountdown(seconds)}`
        : `${subject} · retrying…`
      break
    }
    case 'outage_waiting': {
      const seconds = Math.max(0, Math.ceil(((state.waitRetryAt ?? now) - now) / 1000))
      label = `Upstream unavailable · retrying in ${seconds}s`
      break
    }
    case 'thinking':
      label = slow ? 'Stream stalled…' : 'Thinking…'
      break
    case 'responding':
      label = slow ? 'Stream stalled…' : 'Responding…'
      break
    default:
      label = slow ? 'Preparing slow…' : 'Preparing…'
  }

  const status = humanDuration(elapsed)
  const tokenSuffix = isLongWaitPhase(state.phase) ? '' : formatSpinnerTokenSuffix(state, now, stats)
  const interruptHint = options.interruptible === false
    ? ''
    : options.backgroundable
      ? ' · esc to stop waiting'
      : ' · esc to interrupt'

  if (slow) {
    return `\x1b[31m${char}\x1b[0m \x1b[31m${label}\x1b[0m\x1b[2m (${status}${tokenSuffix})${interruptHint}\x1b[0m`
  }

  const glimmerLabel = glimmerText(label, state.glimmerPos)
  return `\x1b[36m${char}\x1b[0m ${glimmerLabel}\x1b[2m (${status}${tokenSuffix})${interruptHint}\x1b[0m`
}

function glimmerText(text: string, pos: number): string {
  const start = pos - 1
  const end = pos + 1
  let result = ''
  for (let i = 0; i < text.length; i++) {
    if (i >= start && i <= end) {
      result += `\x1b[1;37m${text[i]}\x1b[0m`
    } else {
      result += `\x1b[2m${text[i]}\x1b[0m`
    }
  }
  return result
}

function formatWaitCountdown(totalSeconds: number): string {
  if (totalSeconds < 60) return `${totalSeconds}s`
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  if (hours > 0) return `${hours}h${minutes > 0 ? `${minutes}m` : ''}`
  return `${minutes}m${seconds > 0 ? `${seconds}s` : ''}`
}

function humanDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  const secs = Math.floor(ms / 100) / 10
  if (secs < 60) return `${secs.toFixed(1)}s`
  const totalSecs = Math.floor(ms / 1000)
  const mins = Math.floor(totalSecs / 60)
  const rem = totalSecs % 60
  return rem > 0 ? `${mins}m${rem}s` : `${mins}m`
}

function formatSpinnerTokenSuffix(state: SpinnerState, now: number, stats?: SpinnerStats): string {
  const inputTokens = stats?.inputTokens ?? 0
  const outputTokens = stats?.outputTokens ?? 0
  const cacheReadTokens = stats?.cacheReadTokens ?? 0
  const cacheWriteTokens = stats?.cacheWriteTokens ?? 0
  const liveRate = formatLiveTokPerSec(state, now)
  if (inputTokens > 0 || outputTokens > 0 || cacheReadTokens > 0 || cacheWriteTokens > 0) {
    const parts: string[] = []
    if (inputTokens > 0) parts.push(`↑${formatTokens(inputTokens)}`)
    if (outputTokens > 0) parts.push(`↓${formatTokens(outputTokens)}`)
    const cacheLabel = formatCacheLabel(inputTokens, cacheReadTokens, cacheWriteTokens)
    if (cacheLabel) parts.push(cacheLabel)
    return ` · ${parts.join(' ')}${liveRate ? ` · ${liveRate}` : ''}`
  }
  const tokenSuffix = state.tokenCount > 0 ? ` · ↓ ${formatTokens(state.tokenCount)} tokens` : ''
  return `${tokenSuffix}${liveRate ? ` · ${liveRate}` : ''}`
}

function formatLiveTokPerSec(state: SpinnerState, now: number): string {
  const emitting = state.phase === 'thinking' || state.phase === 'responding'
  if (!state.streaming || !emitting || !state.streamStartedAt || state.tokenCount <= 0) return ''
  const elapsedMs = Math.max(0, now - state.streamStartedAt)
  if (elapsedMs < 500) return ''
  const rate = state.tokenCount / (elapsedMs / 1000)
  if (!Number.isFinite(rate) || rate <= 0) return ''
  return `~${Math.round(rate)} tok/s`
}

/**
 * Compact spinner cache segment: read amount + hit percent + newly written
 * tokens (`+write` — the cache-billed part not served from cache this call).
 */
export function formatCacheLabel(
  inputTokens: number,
  cacheReadTokens: number,
  cacheWriteTokens = 0,
): string | null {
  if (cacheReadTokens <= 0 && cacheWriteTokens <= 0) return null
  if (cacheReadTokens <= 0) return `cache write ${formatTokens(cacheWriteTokens)}`
  const pct = formatCacheHitPercent(inputTokens, cacheReadTokens, cacheWriteTokens)
  const writeSuffix = cacheWriteTokens > 0 ? ` +${formatTokens(cacheWriteTokens)}` : ''
  return `cache ${formatTokens(cacheReadTokens)} ${pct}%${writeSuffix}`
}

/**
 * Pick spinner token stats from completed provider usage.
 * While an LLM call is active, its usage buckets are not known yet, so only
 * show the live output estimate instead of mixing it with the previous call.
 */
export function spinnerStatsFromLastUsage(
  last: {
    inputTokens: number
    outputTokens: number
    cacheReadTokens: number
    cacheWriteTokens: number
  } | null | undefined,
  liveOutputTokens = 0,
  activeLlmCall = false,
): SpinnerStats {
  const completedUsage = activeLlmCall ? null : last
  return {
    inputTokens: completedUsage?.inputTokens ?? 0,
    outputTokens: liveOutputTokens > 0 ? liveOutputTokens : (completedUsage?.outputTokens ?? 0),
    cacheReadTokens: completedUsage?.cacheReadTokens ?? 0,
    cacheWriteTokens: completedUsage?.cacheWriteTokens ?? 0,
  }
}

function estimateTokens(text: string): number {
  if (!text) return 0
  return Math.max(1, Math.round(text.length / 4))
}

function formatTokens(count: number): string {
  if (count < 1000) return `${count}`
  if (count < 10000) return `${(count / 1000).toFixed(1)}k`
  if (count < 1000000) return `${Math.round(count / 1000)}k`
  if (count < 10000000) return `${(count / 1000000).toFixed(1)}M`
  return `${Math.round(count / 1000000)}M`
}
