/**
 * App state management for the CLI.
 */

import { type RunEvent } from '../native/index.js'
import { humanTokens as humanTokensInline, renderBar } from '../utils/format.js'

// ---------------------------------------------------------------------------
// Message types for the UI
// ---------------------------------------------------------------------------

export type MessageRole = 'user' | 'assistant'

export interface UIMessage {
  id: string
  role: MessageRole
  text: string
  timestamp: number
  toolCalls?: UIToolCall[]
  /** Run stats attached to the final assistant message of a run */
  runStats?: RunStats
  /** Verbose events that occurred before this message */
  verboseEvents?: VerboseEvent[]
  /** Text was already streamed to stdout — skip rendering in Message component */
  streamed?: boolean
}

export interface UIToolCall {
  id: string
  name: string
  args: Record<string, unknown>
  status: 'running' | 'done' | 'error'
  result?: string
  previewCommand?: string
  durationMs?: number
}

// ---------------------------------------------------------------------------
// Run stats — accumulated during a run, shown in verbose mode
// ---------------------------------------------------------------------------

export interface RunStats {
  durationMs: number
  turnCount: number
  toolCallCount: number
  toolErrorCount: number
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheWriteTokens: number
  llmCalls: number
  contextTokens: number
  contextWindow: number
  toolBreakdown: ToolBreakdownEntry[]
  llmCallDetails: LlmCallDetail[]
  compactHistory: CompactRecord[]
}

export interface LlmCallDetail {
  model: string
  durationMs: number
  inputTokens: number
  outputTokens: number
  ttfbMs: number
  ttftMs: number
  tokPerSec: number
}

export interface ToolBreakdownEntry {
  name: string
  count: number
  totalDurationMs: number
  errors: number
}

export interface CompactRecord {
  level: number
  beforeTokens: number
  afterTokens: number
}

function emptyRunStats(): RunStats {
  return {
    durationMs: 0,
    turnCount: 0,
    toolCallCount: 0,
    toolErrorCount: 0,
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    llmCalls: 0,
    contextTokens: 0,
    contextWindow: 0,
    toolBreakdown: [],
    llmCallDetails: [],
    compactHistory: [],
  }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

export interface VerboseEvent {
  kind: 'llm_call' | 'llm_completed' | 'compact_call' | 'compact_done'
  text: string
}

export interface AppState {
  messages: UIMessage[]
  isLoading: boolean
  sessionId: string | null
  model: string
  cwd: string
  error: string | null
  verbose: boolean
  currentStreamText: string
  currentThinkingText: string
  activeToolCalls: Map<string, UIToolCall>
  /** Accumulated tool calls for the current turn, merged into assistant_completed */
  turnToolCalls: UIToolCall[]
  /** Stats accumulated during the current run */
  currentRunStats: RunStats
  /** Start time of the current run */
  runStartTime: number
  /** Verbose inline events (LLM calls, compaction) shown during streaming */
  verboseEvents: VerboseEvent[]
  /** Timestamp of the last received token (for stall detection) */
  lastTokenAt: number
}

export function createInitialState(model: string, cwd: string): AppState {
  return {
    messages: [],
    isLoading: false,
    sessionId: null,
    model,
    cwd,
    error: null,
    verbose: true,
    currentStreamText: '',
    currentThinkingText: '',
    activeToolCalls: new Map(),
    turnToolCalls: [],
    currentRunStats: emptyRunStats(),
    runStartTime: 0,
    verboseEvents: [],
    lastTokenAt: 0,
  }
}

// ---------------------------------------------------------------------------
// Reducer-style state updates from RunEvents
// ---------------------------------------------------------------------------

export function applyEvent(state: AppState, event: RunEvent): AppState {
  const kind = event.kind
  const p = event.payload as Record<string, any>

  switch (kind) {
    case 'run_started':
      return {
        ...state,
        isLoading: true,
        sessionId: event.session_id,
        error: null,
        currentStreamText: '',
        currentThinkingText: '',
        activeToolCalls: new Map(),
        turnToolCalls: [],
        currentRunStats: emptyRunStats(),
        runStartTime: Date.now(),
        verboseEvents: [],
      }

    case 'turn_started':
      return {
        ...state,
        currentStreamText: '',
        currentThinkingText: '',
        turnToolCalls: [],
        currentRunStats: {
          ...state.currentRunStats,
          turnCount: state.currentRunStats.turnCount + 1,
        },
      }

    case 'assistant_delta': {
      const delta = p.delta as string | undefined
      const thinkingDelta = p.thinking_delta as string | undefined
      return {
        ...state,
        currentStreamText: state.currentStreamText + (delta ?? ''),
        currentThinkingText: state.currentThinkingText + (thinkingDelta ?? ''),
        lastTokenAt: Date.now(),
      }
    }

    case 'assistant_completed': {
      const content = p.content as any[] | undefined
      const textParts = (content ?? [])
        .filter((b: any) => b.type === 'text')
        .map((b: any) => b.text)
      const text = textParts.join('') || state.currentStreamText

      // Extract tool calls from content blocks (these are the LLM's requests)
      // and merge with any already-finished tool results from turnToolCalls
      const contentToolCalls = (content ?? [])
        .filter((b: any) => b.type === 'tool_call')
        .map((b: any) => {
          // Check if we already have a finished result for this tool call
          const finished = state.turnToolCalls.find((tc) => tc.id === b.id)
          if (finished) return finished
          return {
            id: b.id as string,
            name: b.name as string,
            args: (b.input ?? {}) as Record<string, unknown>,
            status: 'running' as const,
          }
        })

      // Merge: content tool calls + any turnToolCalls not in content
      const contentIds = new Set(contentToolCalls.map((tc) => tc.id))
      const extraToolCalls = state.turnToolCalls.filter((tc) => !contentIds.has(tc.id))
      const allToolCalls = [...contentToolCalls, ...extraToolCalls]

      const msg: UIMessage = {
        id: event.event_id,
        role: 'assistant',
        text,
        timestamp: Date.now(),
        toolCalls: allToolCalls.length > 0 ? allToolCalls : undefined,
        verboseEvents: state.verboseEvents.length > 0 ? [...state.verboseEvents] : undefined,
        streamed: state.currentStreamText.length > 0,
      }

      return {
        ...state,
        messages: [...state.messages, msg],
        currentStreamText: '',
        currentThinkingText: '',
        activeToolCalls: new Map(),
        turnToolCalls: [],
        verboseEvents: [],
      }
    }

    case 'tool_started': {
      const tc: UIToolCall = {
        id: p.tool_call_id,
        name: p.tool_name,
        args: p.args ?? {},
        status: 'running',
        previewCommand: p.preview_command,
      }
      const newMap = new Map(state.activeToolCalls)
      newMap.set(tc.id, tc)
      return { ...state, activeToolCalls: newMap }
    }

    case 'tool_finished': {
      const id = p.tool_call_id as string
      const isError = !!p.is_error
      const toolName = p.tool_name ?? state.activeToolCalls.get(id)?.name ?? 'unknown'
      const durationMs = (p.duration_ms as number) ?? 0

      const finished: UIToolCall = {
        id,
        name: toolName,
        args: state.activeToolCalls.get(id)?.args ?? {},
        status: isError ? 'error' : 'done',
        result: p.content,
        previewCommand: state.activeToolCalls.get(id)?.previewCommand,
        durationMs,
      }

      // Extract diff from details if present (file edit tools)
      const details = p.details as Record<string, any> | undefined
      if (details?.diff && typeof details.diff === 'string') {
        finished.args = { ...finished.args, diff: details.diff }
      }

      const newMap = new Map(state.activeToolCalls)
      newMap.delete(id)

      // Update run stats
      const stats = { ...state.currentRunStats }
      stats.toolCallCount++
      if (isError) stats.toolErrorCount++

      // Update tool breakdown (immutable)
      const breakdown = stats.toolBreakdown.map((e) =>
        e.name === toolName
          ? { ...e, count: e.count + 1, totalDurationMs: e.totalDurationMs + durationMs, errors: e.errors + (isError ? 1 : 0) }
          : e
      )
      if (!breakdown.some((e) => e.name === toolName)) {
        breakdown.push({
          name: toolName,
          count: 1,
          totalDurationMs: durationMs,
          errors: isError ? 1 : 0,
        })
      }
      stats.toolBreakdown = breakdown

      return {
        ...state,
        activeToolCalls: newMap,
        turnToolCalls: [...state.turnToolCalls, finished],
        // Update tool call status in existing messages (tool_finished fires after assistant_completed)
        messages: updateToolCallInMessages(state.messages, id, finished),
        currentRunStats: stats,
      }
    }

    case 'tool_progress': {
      const id = p.tool_call_id as string
      const existing = state.activeToolCalls.get(id)
      if (!existing) return state
      const newMap = new Map(state.activeToolCalls)
      newMap.set(id, { ...existing, previewCommand: p.text })
      return { ...state, activeToolCalls: newMap }
    }

    case 'llm_call_started': {
      const model = (p.model as string) ?? state.model
      const turn = event.turn
      const attempt = (p.attempt as number) ?? 0
      const msgCount = (p.message_count as number) ?? 0
      const tools = p.tools as any[] | undefined
      const toolCount = tools?.length ?? 0
      const sysTok = (p.system_prompt_tokens as number) ?? 0
      const retryStr = attempt > 0 ? ` · retry ${attempt}` : ''
      const text = `[LLM] call · ${model} · turn ${turn}${retryStr}\n  ${msgCount} messages · ${toolCount} tools\n  ~${humanTokensInline(sysTok)} sys tokens`
      return {
        ...state,
        verboseEvents: [...state.verboseEvents, { kind: 'llm_call', text }],
      }
    }

    case 'llm_call_completed': {
      const usage = p.usage as Record<string, any> | undefined
      const metrics = p.metrics as Record<string, any> | undefined
      const error = p.error as string | undefined
      const stats = { ...state.currentRunStats }
      stats.llmCalls++
      const inputTok = (usage?.input as number) ?? 0
      const outputTok = (usage?.output as number) ?? 0
      const durationMs = (metrics?.duration_ms as number) ?? 0
      const ttfbMs = (metrics?.ttfb_ms as number) ?? 0
      const ttftMs = (metrics?.ttft_ms as number) ?? 0
      const streamingMs = (metrics?.streaming_ms as number) ?? 0
      const tokPerSec = durationMs > 0 ? (outputTok / (durationMs / 1000)) : 0

      if (usage) {
        stats.inputTokens += inputTok
        stats.outputTokens += outputTok
        stats.cacheReadTokens += (usage.cache_read as number) ?? 0
        stats.cacheWriteTokens += (usage.cache_write as number) ?? 0
      }

      stats.llmCallDetails = [...stats.llmCallDetails, {
        model: (p.model as string) ?? state.model,
        durationMs,
        inputTokens: inputTok,
        outputTokens: outputTok,
        ttfbMs,
        ttftMs,
        tokPerSec,
      }]

      let text: string
      if (error) {
        const durSec = (durationMs / 1000).toFixed(1)
        text = `[LLM] failed · ${durSec}s\n  ${error}`
      } else {
        const durSec = (durationMs / 1000).toFixed(1)
        const dur = durationMs || 1
        const ttfbPct = (ttfbMs / dur * 100).toFixed(0)
        const ttftPct = (ttftMs / dur * 100).toFixed(0)
        const streamPct = (streamingMs / dur * 100).toFixed(0)
        text = `[LLM] completed\n  tokens   ${humanTokensInline(inputTok)} in · ${outputTok} out · ${tokPerSec.toFixed(0)} tok/s\n  timing   ${durSec}s · ttfb ${(ttfbMs / 1000).toFixed(1)}s (${ttfbPct}%) · ttft ${(ttftMs / 1000).toFixed(1)}s (${ttftPct}%) · stream ${(streamingMs / 1000).toFixed(1)}s (${streamPct}%)`
      }

      return {
        ...state,
        currentRunStats: stats,
        verboseEvents: [...state.verboseEvents, { kind: 'llm_completed', text }],
      }
    }

    case 'context_compaction_started': {
      const msgCount = (p.message_count as number) ?? 0
      const estTokens = (p.estimated_tokens as number) ?? 0
      const budget = (p.budget_tokens as number) ?? 0
      const window = (p.context_window as number) ?? 0
      const sysTok = (p.system_prompt_tokens as number) ?? 0
      const pct = budget > 0 ? ((estTokens / budget) * 100).toFixed(0) : '0'
      const bar = renderBar(estTokens, budget, 40)
      const text = `[COMPACT] call\n  ${msgCount} messages · ~${humanTokensInline(estTokens)} tokens\n  ${bar}  ${pct}%(${humanTokensInline(estTokens)}) of budget(${humanTokensInline(budget)})\n  budget ${humanTokensInline(budget)} (window ${humanTokensInline(window)} − sys ${humanTokensInline(sysTok)})`
      return {
        ...state,
        currentRunStats: { ...state.currentRunStats, contextTokens: estTokens, contextWindow: window },
        verboseEvents: [...state.verboseEvents, { kind: 'compact_call', text }],
      }
    }

    case 'context_compaction_completed': {
      const result = p.result as Record<string, any> | undefined
      const type = (result?.type as string) ?? 'done'
      let action = type
      if (type === 'no_op') {
        action = 'no-op'
      } else if (type === 'run_once_cleared') {
        const saved = (result?.saved_tokens as number) ?? 0
        action = `cleared · saved ${humanTokensInline(saved)} tokens`
      } else if (type === 'level_compacted') {
        const level = (result?.level as number) ?? 0
        const beforeMsgs = (result?.before_message_count as number) ?? 0
        const afterMsgs = (result?.after_message_count as number) ?? 0
        const before = (result?.before_estimated_tokens as number) ?? 0
        const after = (result?.after_estimated_tokens as number) ?? 0
        const saved = before - after
        const savedPct = before > 0 ? (saved / before * 100).toFixed(0) : '0'
        action = `L${level}\n  ${beforeMsgs} messages ~${humanTokensInline(before)} tok\n  → ${afterMsgs} messages ~${humanTokensInline(after)} tok\n  saved ${humanTokensInline(saved)} (${savedPct}%)`
        // Add actions detail if available
        const actions = result?.actions as any[] | undefined
        if (actions && actions.length > 0) {
          const sorted = [...actions].filter((a: any) => a.method !== 'Skipped').sort((a: any, b: any) => {
            const sa = (a.before_tokens ?? 0) - (a.after_tokens ?? 0)
            const sb = (b.before_tokens ?? 0) - (b.after_tokens ?? 0)
            return sb - sa
          })
          for (const a of sorted) {
            const method = (a.method as string) ?? 'unknown'
            const msgs = (a.message_count as number) ?? 0
            const bTok = (a.before_tokens as number) ?? 0
            const aTok = (a.after_tokens as number) ?? 0
            const aSaved = bTok - aTok
            const aPct = before > 0 ? (aSaved / before * 100).toFixed(0) : '0'
            const bar = renderBar(aSaved, saved || 1, 12)
            action += `\n    ${method.toLowerCase()}  ${msgs} msgs  ${humanTokensInline(bTok)}→${humanTokensInline(aTok)}  saved ${humanTokensInline(aSaved)}  ${bar} ${aPct}%`
          }
        }
      }
      // Track compact history for run summary
      const compactRecord: import('./AppState.js').CompactRecord | null =
        type === 'level_compacted' ? {
          level: (result?.level as number) ?? 0,
          beforeTokens: (result?.before_estimated_tokens as number) ?? 0,
          afterTokens: (result?.after_estimated_tokens as number) ?? 0,
        } : type === 'run_once_cleared' ? {
          level: 0,
          beforeTokens: state.currentRunStats.contextTokens,
          afterTokens: state.currentRunStats.contextTokens - ((result?.saved_tokens as number) ?? 0),
        } : null

      const updatedStats = compactRecord
        ? { ...state.currentRunStats, compactHistory: [...state.currentRunStats.compactHistory, compactRecord] }
        : state.currentRunStats

      return {
        ...state,
        currentRunStats: updatedStats,
        verboseEvents: [...state.verboseEvents, { kind: 'compact_done', text: `[COMPACT] · ${action}` }],
      }
    }

    case 'run_finished': {
      const serverDuration = (p.duration_ms as number) ?? 0
      const stats = {
        ...state.currentRunStats,
        durationMs: serverDuration || (Date.now() - state.runStartTime),
        turnCount: (p.turn_count as number) ?? state.currentRunStats.turnCount,
      }

      // Attach stats to the last assistant message
      const messages = [...state.messages]
      for (let i = messages.length - 1; i >= 0; i--) {
        if (messages[i]!.role === 'assistant') {
          messages[i] = { ...messages[i]!, runStats: stats }
          break
        }
      }

      return {
        ...state,
        messages,
        isLoading: false,
        currentStreamText: '',
        currentThinkingText: '',
        activeToolCalls: new Map(),
        currentRunStats: stats,
      }
    }

    case 'error':
      return {
        ...state,
        isLoading: false,
        error: p.message ?? 'Unknown error',
      }

    default:
      return state
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Update a tool call's status in the message history (search from the end). */
function updateToolCallInMessages(
  messages: UIMessage[],
  toolCallId: string,
  finished: UIToolCall,
): UIMessage[] {
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i]!
    if (!msg.toolCalls) continue
    const idx = msg.toolCalls.findIndex((tc) => tc.id === toolCallId)
    if (idx >= 0) {
      const newMessages = [...messages]
      const newToolCalls = [...msg.toolCalls]
      newToolCalls[idx] = finished
      newMessages[i] = { ...msg, toolCalls: newToolCalls }
      return newMessages
    }
  }
  return messages
}
