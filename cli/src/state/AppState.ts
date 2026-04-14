/**
 * App state management for the CLI.
 */

import { type RunEvent } from '../native/index.js'
import { humanTokens as humanTokensInline, renderBar, formatMsDynamic } from '../utils/format.js'

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function padToolName(name: string, width = 18): string {
  if (name.length >= width) return name + ' '
  return name + ' '.repeat(width - name.length)
}

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

export interface CompactionEntry {
  kind: 'run_once' | 'level'
  level?: number
  beforeTokens: number
  afterTokens: number
  savedTokens: number
}

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
  systemPromptTokens: number
  toolBreakdown: ToolBreakdownEntry[]
  llmCallDetails: LlmCallDetail[]
  compactionHistory: CompactionEntry[]
}

export interface LlmCallDetail {
  model: string
  durationMs: number
  inputTokens: number
  outputTokens: number
  ttfbMs: number
  ttftMs: number
  streamingMs: number
  tokPerSec: number
}

export interface ToolBreakdownEntry {
  name: string
  count: number
  totalDurationMs: number
  totalResultTokens: number
  errors: number
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
    systemPromptTokens: 0,
    toolBreakdown: [],
    llmCallDetails: [],
    compactionHistory: [],
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
}

export function createInitialState(model: string, cwd: string): AppState {
  return {
    messages: [],
    isLoading: false,
    sessionId: null,
    model,
    cwd,
    error: null,
    verbose: false,
    currentStreamText: '',
    currentThinkingText: '',
    activeToolCalls: new Map(),
    turnToolCalls: [],
    currentRunStats: emptyRunStats(),
    runStartTime: 0,
    verboseEvents: [],
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

      const resultTokens = (p.result_tokens as number) ?? 0

      // Update tool breakdown (immutable)
      const breakdown = stats.toolBreakdown.map((e) =>
        e.name === toolName
          ? { ...e, count: e.count + 1, totalDurationMs: e.totalDurationMs + durationMs, totalResultTokens: e.totalResultTokens + resultTokens, errors: e.errors + (isError ? 1 : 0) }
          : e
      )
      if (!breakdown.some((e) => e.name === toolName)) {
        breakdown.push({
          name: toolName,
          count: 1,
          totalDurationMs: durationMs,
          totalResultTokens: resultTokens,
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
      const msgCount = (p.message_count as number) ?? 0
      const tools = p.tools as any[] | undefined
      const toolCount = tools?.length ?? 0
      const sysTok = (p.system_prompt_tokens as number) ?? 0
      const msgBytes = (p.message_bytes as number) ?? 0
      const estMsgTokens = Math.floor(msgBytes / 4)
      const estTotal = sysTok + estMsgTokens

      // Build per-role message counts from message_role_counts if available
      const roleCounts = p.message_role_counts as Record<string, number> | undefined
      let roleStr = ''
      if (roleCounts) {
        const parts: string[] = []
        for (const [role, count] of Object.entries(roleCounts)) {
          parts.push(`${role} ${count}`)
        }
        roleStr = ` (${parts.join(' · ')})`
      }

      const lines = [
        `[LLM] call · ${model} · turn ${turn}`,
        `  ${msgCount} messages${roleStr} · ${toolCount} tools`,
        `  ~${humanTokensInline(estTotal)} est tokens (sys ~${humanTokensInline(sysTok)} · msgs ~${humanTokensInline(estMsgTokens)})`,
      ]

      // Per-tool token breakdown from accumulated tool results
      const toolTokens = state.currentRunStats.toolBreakdown
        .filter((t) => t.totalResultTokens > 0)
        .sort((a, b) => b.totalResultTokens - a.totalResultTokens)
      if (toolTokens.length > 0) {
        const totalToolTok = toolTokens.reduce((s, t) => s + t.totalResultTokens, 0)
        lines.push('  tool results:')
        for (const t of toolTokens) {
          const pctVal = totalToolTok > 0 ? ((t.totalResultTokens / totalToolTok) * 100) : 0
          const bar = renderBar(t.totalResultTokens, totalToolTok, 20)
          lines.push(`    ${padToolName(t.name)}~${humanTokensInline(t.totalResultTokens).padStart(5)}     (${pctVal.toFixed(1).padStart(5)}%)  ${bar}`)
        }
      }

      return {
        ...state,
        currentRunStats: { ...state.currentRunStats, systemPromptTokens: sysTok },
        verboseEvents: [...state.verboseEvents, { kind: 'llm_call', text: lines.join('\n') }],
      }
    }

    case 'llm_call_completed': {
      const usage = p.usage as Record<string, any> | undefined
      const metrics = p.metrics as Record<string, any> | undefined
      const stats = { ...state.currentRunStats }
      stats.llmCalls++
      const inputTok = (usage?.input as number) ?? 0
      const outputTok = (usage?.output as number) ?? 0
      const durationMs = (metrics?.duration_ms as number) ?? 0
      const ttfbMs = (metrics?.ttfb_ms as number) ?? 0
      const ttftMs = (metrics?.ttft_ms as number) ?? 0
      const streamingMs = (metrics?.streaming_ms as number) ?? 0
      const tokPerSec = streamingMs > 0 ? (outputTok / (streamingMs / 1000)) : 0
      const model = (p.model as string) ?? state.model

      // Check for error
      const error = p.error as string | undefined

      if (usage) {
        stats.inputTokens += inputTok
        stats.outputTokens += outputTok
        stats.cacheReadTokens += (usage.cache_read as number) ?? 0
        stats.cacheWriteTokens += (usage.cache_write as number) ?? 0
      }

      stats.llmCallDetails = [...stats.llmCallDetails, {
        model,
        durationMs,
        inputTokens: inputTok,
        outputTokens: outputTok,
        ttfbMs,
        ttftMs,
        streamingMs,
        tokPerSec,
      }]

      // Timing percentages
      const ttfbPct = durationMs > 0 ? ((ttfbMs / durationMs) * 100).toFixed(0) : '0'
      const ttftPct = durationMs > 0 ? ((ttftMs / durationMs) * 100).toFixed(0) : '0'
      const streamPct = durationMs > 0 ? ((streamingMs / durationMs) * 100).toFixed(0) : '0'
      const durSec = (durationMs / 1000).toFixed(1)

      let text: string
      if (error) {
        text = `[LLM] error · ${model}\n  ${error}`
      } else {
        text = `[LLM] completed · ${model}\n  tokens   ${humanTokensInline(inputTok)} in · ${outputTok} out · ${tokPerSec.toFixed(0)} tok/s\n  timing   ${durSec}s · ttfb ${formatMsDynamic(ttfbMs)} (${ttfbPct}%) · ttft ${formatMsDynamic(ttftMs)} (${ttftPct}%) · stream ${(streamingMs / 1000).toFixed(1)}s (${streamPct}%)`
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
        currentRunStats: { ...state.currentRunStats, contextTokens: estTokens, contextWindow: window, systemPromptTokens: sysTok },
        verboseEvents: [...state.verboseEvents, { kind: 'compact_call', text }],
      }
    }

    case 'context_compaction_completed': {
      const result = p.result as Record<string, any> | undefined
      const type = (result?.type as string) ?? 'done'
      const stats = { ...state.currentRunStats }
      let action = type
      if (type === 'no_op') {
        action = 'no-op'
      } else if (type === 'run_once_cleared') {
        const saved = (result?.saved_tokens as number) ?? 0
        const before = (result?.before_estimated_tokens as number) ?? stats.contextTokens
        const after = before - saved
        action = `run-once · ${humanTokensInline(before)} → ${humanTokensInline(after)} · saved ${humanTokensInline(saved)} tokens`
        stats.compactionHistory = [...stats.compactionHistory, {
          kind: 'run_once',
          beforeTokens: before,
          afterTokens: after,
          savedTokens: saved,
        }]
      } else if (type === 'level_compacted') {
        const level = (result?.level as number) ?? 0
        const before = (result?.before_estimated_tokens as number) ?? 0
        const after = (result?.after_estimated_tokens as number) ?? 0
        const saved = before - after
        const savedPct = before > 0 ? ((saved / before) * 100).toFixed(1) : '0'
        action = `L${level} · ${humanTokensInline(before)} → ${humanTokensInline(after)} · saved ${humanTokensInline(saved)} (${savedPct}%)`
        stats.compactionHistory = [...stats.compactionHistory, {
          kind: 'level',
          level,
          beforeTokens: before,
          afterTokens: after,
          savedTokens: saved,
        }]
      }
      return {
        ...state,
        currentRunStats: stats,
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
