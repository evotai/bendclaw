/**
 * Reducer-style state updates from RunEvents.
 */

import type { RunEvent } from '../../native/index.js'
import { formatLlmCallStarted, formatLlmCallCompleted, formatCompactionStarted, formatCompactionCompleted } from '../../render/verbose.js'
import { emptyRunStats, type AppState } from './state.js'
import type { CompactRecord, MessageStats, UIMessage, UIToolCall } from './types.js'


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

      const contentToolCalls = (content ?? [])
        .filter((b: any) => b.type === 'tool_call')
        .map((b: any) => {
          const finished = state.turnToolCalls.find((tc) => tc.id === b.id)
          if (finished) return finished
          return {
            id: b.id as string,
            name: b.name as string,
            args: (b.input ?? {}) as Record<string, unknown>,
            status: 'running' as const,
          }
        })

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

      const details = p.details as Record<string, any> | undefined
      if (details?.diff && typeof details.diff === 'string') {
        finished.args = { ...finished.args, diff: details.diff }
      }

      const newMap = new Map(state.activeToolCalls)
      newMap.delete(id)

      const stats = { ...state.currentRunStats }
      stats.toolCallCount++
      if (isError) stats.toolErrorCount++

      const breakdown = stats.toolBreakdown.map((e) =>
        e.name === toolName
          ? { ...e, count: e.count + 1, totalDurationMs: e.totalDurationMs + durationMs, errors: e.errors + (isError ? 1 : 0) }
          : e,
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
        messages: updateToolCallInMessages(state.messages, id, finished),
        currentRunStats: stats,
      }
    }

    case 'tool_progress':
      return state

    case 'llm_call_started': {
      const model = (p.model as string) ?? state.model
      const turn = event.turn
      const sysTok = (p.system_prompt_tokens as number) ?? 0
      const toolDefTok = (p.tool_definition_tokens as number) ?? 0

      // Pre-computed message stats from Rust side (always present)
      const ms = p.message_stats as Record<string, any> | undefined
      const msgStats: MessageStats | null = ms
        ? {
            userCount: (ms.user_count as number) ?? 0,
            assistantCount: (ms.assistant_count as number) ?? 0,
            toolResultCount: (ms.tool_result_count as number) ?? 0,
            imageCount: (ms.image_count as number) ?? 0,
            userTokens: (ms.user_tokens as number) ?? 0,
            assistantTokens: (ms.assistant_tokens as number) ?? 0,
            toolResultTokens: (ms.tool_result_tokens as number) ?? 0,
            imageTokens: (ms.image_tokens as number) ?? 0,
            toolDetails: (ms.tool_details as [string, number][]) ?? [],
          }
        : null

      const data: Record<string, unknown> = {
        ...p,
        model,
        turn,
        context_window: state.currentRunStats.contextWindow,
      }
      const text = formatLlmCallStarted(data)

      // Accumulate cumulative stats across all LLM calls
      const prev = state.currentRunStats.cumulativeStats
      const cumulative: MessageStats = msgStats
        ? {
            userCount: prev.userCount + msgStats.userCount,
            assistantCount: prev.assistantCount + msgStats.assistantCount,
            toolResultCount: prev.toolResultCount + msgStats.toolResultCount,
            imageCount: prev.imageCount + msgStats.imageCount,
            userTokens: prev.userTokens + msgStats.userTokens,
            assistantTokens: prev.assistantTokens + msgStats.assistantTokens,
            toolResultTokens: prev.toolResultTokens + msgStats.toolResultTokens,
            imageTokens: prev.imageTokens + msgStats.imageTokens,
            toolDetails: [...prev.toolDetails, ...msgStats.toolDetails],
          }
        : prev

      return {
        ...state,
        currentRunStats: {
          ...state.currentRunStats,
          contextTokens: (p.estimated_context_tokens as number) ?? state.currentRunStats.contextTokens,
          contextWindow: (p.context_window as number) ?? state.currentRunStats.contextWindow,
          lastMessageStats: msgStats,
          cumulativeStats: cumulative,
          systemPromptTokens: sysTok + toolDefTok,
        },
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
      const tokPerSec = durationMs > 0 ? outputTok / (durationMs / 1000) : 0

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

      const data: Record<string, unknown> = {
        ...p,
        model: (p.model as string) ?? state.model,
        turn: event.turn,
        estimated_context_tokens: state.currentRunStats.contextTokens,
        context_window: state.currentRunStats.contextWindow,
      }
      const result = formatLlmCallCompleted(data)

      return {
        ...state,
        currentRunStats: stats,
        verboseEvents: [...state.verboseEvents, { kind: 'llm_completed', text: result.text, expandedText: result.expandedText }],
      }
    }

    case 'context_compaction_started': {
      const data: Record<string, unknown> = {
        ...p,
        context_window: state.currentRunStats.contextWindow,
      }
      const text = formatCompactionStarted(data)

      return {
        ...state,
        currentRunStats: { ...state.currentRunStats, contextTokens: (p.estimated_tokens as number) ?? 0, contextWindow: (p.context_window as number) ?? 0 },
        verboseEvents: [...state.verboseEvents, { kind: 'compact_call', text }],
      }
    }

    case 'context_compaction_completed': {
      const result = p.result as Record<string, any> | undefined
      const type = (result?.type as string) ?? 'done'

      const data: Record<string, unknown> = {
        ...p,
        context_window: state.currentRunStats.contextWindow,
      }
      const text = formatCompactionCompleted(data)

      const compactRecord: CompactRecord | null =
        type === 'level_compacted'
          ? {
              level: (result?.level as number) ?? 0,
              beforeTokens: (result?.before_estimated_tokens as number) ?? 0,
              afterTokens: (result?.after_estimated_tokens as number) ?? 0,
            }
          : type === 'run_once_cleared'
            ? {
                level: 0,
                beforeTokens: state.currentRunStats.contextTokens,
                afterTokens: state.currentRunStats.contextTokens - ((result?.saved_tokens as number) ?? 0),
              }
            : null

      const updatedStats = compactRecord
        ? { ...state.currentRunStats, compactHistory: [...state.currentRunStats.compactHistory, compactRecord] }
        : state.currentRunStats

      return {
        ...state,
        currentRunStats: updatedStats,
        verboseEvents: [...state.verboseEvents, { kind: 'compact_done', text }],
      }
    }

    case 'run_finished': {
      const serverDuration = (p.duration_ms as number) ?? 0
      const stats = {
        ...state.currentRunStats,
        durationMs: serverDuration || (Date.now() - state.runStartTime),
        turnCount: (p.turn_count as number) ?? state.currentRunStats.turnCount,
      }

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

/** Rough token estimate: ~4 chars per token. */
function estimateTokens(text: string): number {
  return Math.ceil(text.length / 4)
}

/**
 * Count messages by role and estimate token usage.
 * Unknown roles are counted as user.
 */
export function countMessagesByRole(messages: { role: string; content?: string; toolName?: string }[]): MessageStats {
  let userCount = 0
  let assistantCount = 0
  let toolResultCount = 0
  let userTokens = 0
  let assistantTokens = 0
  let toolResultTokens = 0
  const toolDetails: [string, number][] = []

  for (const msg of messages) {
    const tokens = estimateTokens(msg.content ?? '')
    if (msg.role === 'assistant') {
      assistantCount++
      assistantTokens += tokens
    } else if (msg.role === 'tool_result') {
      toolResultCount++
      toolResultTokens += tokens
      toolDetails.push([msg.toolName ?? 'unknown', tokens])
    } else {
      userCount++
      userTokens += tokens
    }
  }

  toolDetails.sort((a, b) => b[1] - a[1])

  return { userCount, assistantCount, toolResultCount, imageCount: 0, userTokens, assistantTokens, toolResultTokens, imageTokens: 0, toolDetails }
}

function updateToolCallInMessages(messages: UIMessage[], toolCallId: string, finished: UIToolCall): UIMessage[] {
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
