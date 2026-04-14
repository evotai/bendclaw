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

/** Count messages by role and estimate tokens from JSON byte size (mirrors Rust count_messages_by_role). */
function countMessagesByRole(messages: any[]): MessageStats {
  const stats: MessageStats = {
    userCount: 0, assistantCount: 0, toolResultCount: 0,
    userTokens: 0, assistantTokens: 0, toolResultTokens: 0,
    toolDetails: [],
  }
  for (const msg of messages) {
    const role = (msg.role as string) ?? 'unknown'
    const est = Math.floor(JSON.stringify(msg).length / 4)
    switch (role) {
      case 'user':
        stats.userCount++
        stats.userTokens += est
        break
      case 'assistant':
        stats.assistantCount++
        stats.assistantTokens += est
        break
      case 'toolResult':
      case 'tool_result':
      case 'tool':
        stats.toolResultCount++
        stats.toolResultTokens += est
        stats.toolDetails.push({
          name: (msg.toolName ?? msg.tool_name ?? msg.name ?? 'unknown') as string,
          tokens: est,
        })
        break
      default:
        stats.userCount++
        stats.userTokens += est
        break
    }
  }
  stats.toolDetails.sort((a, b) => b.tokens - a.tokens)
  return stats
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

export interface CompactionActionDetail {
  index: number
  toolName: string
  method: string
  beforeTokens: number
  afterTokens: number
}

export interface CompactionEntry {
  kind: 'run_once' | 'level'
  level?: number
  beforeTokens: number
  afterTokens: number
  savedTokens: number
  actions?: CompactionActionDetail[]
}

export interface MessageStats {
  userCount: number
  assistantCount: number
  toolResultCount: number
  userTokens: number
  assistantTokens: number
  toolResultTokens: number
  /** Per-tool token breakdown (name, tokens), sorted by tokens desc. */
  toolDetails: Array<{ name: string; tokens: number }>
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
  /** Per-role message stats from the last LLM call (for run summary breakdown). */
  lastMessageStats: MessageStats | null
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
    lastMessageStats: null,
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
      const messages = p.messages as any[] | undefined

      // Compute per-role stats from messages array (mirrors Rust count_messages_by_role)
      const msgStats = messages && messages.length > 0
        ? countMessagesByRole(messages)
        : null

      const estTotal = msgStats
        ? msgStats.userTokens + msgStats.assistantTokens + msgStats.toolResultTokens + sysTok
        : sysTok + Math.floor(((p.message_bytes as number) ?? 0) / 4)

      // Build role count string
      let roleStr = ''
      if (msgStats) {
        const parts: string[] = []
        if (msgStats.userCount > 0) parts.push(`user ${msgStats.userCount}`)
        if (msgStats.assistantCount > 0) parts.push(`assistant ${msgStats.assistantCount}`)
        if (msgStats.toolResultCount > 0) parts.push(`tool_result ${msgStats.toolResultCount}`)
        if (parts.length > 0) roleStr = ` (${parts.join(' · ')})`
      } else {
        // Fallback to message_role_counts from Rust
        const roleCounts = p.message_role_counts as Record<string, number> | undefined
        if (roleCounts) {
          const parts: string[] = []
          for (const [role, count] of Object.entries(roleCounts)) {
            parts.push(`${role} ${count}`)
          }
          roleStr = ` (${parts.join(' · ')})`
        }
      }

      const lines = [
        `[LLM] call · ${model} · turn ${turn}`,
        `  ${msgCount} messages${roleStr} · ${toolCount} tools`,
      ]

      // Token estimates by role
      if (msgStats) {
        const tokenParts = [`sys ~${humanTokensInline(sysTok)}`]
        if (msgStats.userTokens > 0) tokenParts.push(`user ~${humanTokensInline(msgStats.userTokens)}`)
        if (msgStats.assistantTokens > 0) tokenParts.push(`assistant ~${humanTokensInline(msgStats.assistantTokens)}`)
        if (msgStats.toolResultTokens > 0) tokenParts.push(`tool_result ~${humanTokensInline(msgStats.toolResultTokens)}`)
        lines.push(`  ~${humanTokensInline(estTotal)} est tokens (${tokenParts.join(' · ')})`)
      } else {
        lines.push(`  ~${humanTokensInline(estTotal)} est tokens (sys ~${humanTokensInline(sysTok)})`)
      }

      // Per-tool token breakdown from message stats (like Rust, only if >= 2 tool results)
      if (msgStats && msgStats.toolDetails.length >= 2) {
        // Aggregate same-name tools
        const aggMap = new Map<string, number>()
        for (const d of msgStats.toolDetails) {
          aggMap.set(d.name, (aggMap.get(d.name) ?? 0) + d.tokens)
        }
        const agg = [...aggMap.entries()].sort((a, b) => b[1] - a[1])
        const totalToolTok = msgStats.toolResultTokens

        lines.push('  tool results:')
        for (const [name, tokens] of agg) {
          const pctVal = totalToolTok > 0 ? ((tokens / totalToolTok) * 100) : 0
          const bar = renderBar(tokens, totalToolTok, 20)
          lines.push(`    ${padToolName(name)}~${humanTokensInline(tokens).padStart(5)}     (${pctVal.toFixed(1).padStart(5)}%)  ${bar}`)
        }
      }

      return {
        ...state,
        currentRunStats: { ...state.currentRunStats, systemPromptTokens: sysTok, lastMessageStats: msgStats },
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
      const lines: string[] = []

      if (type === 'no_op') {
        lines.push('[COMPACT] · no-op')
      } else if (type === 'run_once_cleared') {
        const saved = (result?.saved_tokens as number) ?? 0
        const before = (result?.before_estimated_tokens as number) ?? stats.contextTokens
        const after = before - saved
        lines.push(`[COMPACT] · run-once · ${humanTokensInline(before)} → ${humanTokensInline(after)} · saved ${humanTokensInline(saved)} tokens`)
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
        const beforeMsgCount = (result?.before_message_count as number) ?? 0
        const afterMsgCount = (result?.after_message_count as number) ?? 0
        const saved = before - after
        const savedPct = before > 0 ? ((saved / before) * 100).toFixed(1) : '0'

        // Parse per-message actions
        const rawActions = (result?.actions as any[]) ?? []
        const actions: CompactionActionDetail[] = rawActions.map((a: any) => ({
          index: (a.index as number) ?? 0,
          toolName: (a.tool_name as string) ?? '',
          method: (a.method as string) ?? '',
          beforeTokens: (a.before_tokens as number) ?? 0,
          afterTokens: (a.after_tokens as number) ?? 0,
        }))

        // Build position bar: show which messages were affected
        // · = untouched, O = Outline, H = HeadTail, S = Summarized, D = Dropped, X = LifecycleCleared
        const actionMap = new Map<number, string>()
        for (const a of actions) {
          const ch = a.method === 'Outline' ? 'O'
            : a.method === 'HeadTail' ? 'H'
            : a.method === 'Summarized' ? 'S'
            : a.method === 'Dropped' ? 'D'
            : a.method === 'LifecycleCleared' ? 'X'
            : a.method === 'Skipped' ? '·'
            : '?'
          actionMap.set(a.index, ch)
        }
        const barChars: string[] = []
        for (let i = 0; i < beforeMsgCount; i++) {
          barChars.push(actionMap.get(i) ?? '·')
        }
        const positionBar = barChars.join('')

        const changedCount = actions.filter((a) => a.method !== 'Skipped').length

        lines.push(`[COMPACT] · L${level}`)
        lines.push(`  ${beforeMsgCount} messages ~${humanTokensInline(before)} tok`)
        if (positionBar.length > 0) {
          lines.push(`  [${positionBar}]`)
        }
        lines.push(`  ↓ ${afterMsgCount} messages ~${humanTokensInline(after)} tok`)
        lines.push(`  (saved ~${humanTokensInline(saved)}, ${savedPct}%)`)

        // Per-action breakdown: sorted by savings desc, show top actions
        const significantActions = actions
          .filter((a) => a.method !== 'Skipped' && a.beforeTokens > a.afterTokens)
          .sort((a, b) => (b.beforeTokens - b.afterTokens) - (a.beforeTokens - a.afterTokens))

        if (significantActions.length > 0) {
          lines.push(`  actions: (${changedCount} of ${beforeMsgCount} changed, sorted by savings)`)
          const showCount = Math.min(significantActions.length, 5)
          for (let i = 0; i < showCount; i++) {
            const a = significantActions[i]!
            const aSaved = a.beforeTokens - a.afterTokens
            lines.push(`    #${String(a.index).padStart(3)}  ${padToolName(a.toolName, 14)}${a.method.padEnd(10)} ~${humanTokensInline(a.beforeTokens)} → ~${humanTokensInline(a.afterTokens)}  (saved ~${humanTokensInline(aSaved)})`)
          }
          if (significantActions.length > showCount) {
            const restCount = significantActions.length - showCount
            const restSaved = significantActions.slice(showCount).reduce((s, a) => s + (a.beforeTokens - a.afterTokens), 0)
            lines.push(`    ... ${restCount} more · saved ~${humanTokensInline(restSaved)}`)
          }
        }

        stats.compactionHistory = [...stats.compactionHistory, {
          kind: 'level',
          level,
          beforeTokens: before,
          afterTokens: after,
          savedTokens: saved,
          actions,
        }]
      }

      return {
        ...state,
        currentRunStats: stats,
        verboseEvents: [...state.verboseEvents, { kind: 'compact_done', text: lines.join('\n') }],
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
