/**
 * Parse raw transcript items (from NAPI loadTranscript) into UIMessages
 * with verbose events, tool calls, thinking, and run stats.
 */

import type { UIMessage, UIToolCall, VerboseEvent, RunStats } from '../term/app/types.js'
import { formatLlmCallStarted, formatLlmCallCompleted, formatCompactionStarted, formatCompactionCompleted } from '../render/verbose.js'

// ---------------------------------------------------------------------------
// Raw transcript item shapes (from Rust TranscriptItem serialization)
// ---------------------------------------------------------------------------

interface RawItem {
  type: string
  // User
  text?: string
  // Assistant
  thinking?: string
  tool_calls?: { id: string; name: string; input: Record<string, unknown> }[]
  stop_reason?: string
  // ToolResult
  tool_call_id?: string
  tool_name?: string
  content?: string
  is_error?: boolean
  // Stats
  kind?: string
  data?: Record<string, unknown>
}

// ---------------------------------------------------------------------------
// Main conversion
// ---------------------------------------------------------------------------

export function transcriptToMessages(items: RawItem[]): UIMessage[] {
  const messages: UIMessage[] = []
  const toolResults = collectToolResults(items)
  let acc = newRunAccumulator()
  let idx = 0

  for (const item of items) {
    const t = item.type
    if (t === 'user') {
      messages.push({
        id: `transcript-user-${idx++}`,
        role: 'user',
        text: item.text ?? '',
        timestamp: 0,
      })
    } else if (t === 'assistant') {
      // Flush accumulated verbose events onto this message
      const verboseEvents = acc.verboseEvents.length > 0 ? [...acc.verboseEvents] : undefined
      acc.verboseEvents = []

      const toolCalls = buildToolCalls(item.tool_calls ?? [], toolResults)

      let text = item.text ?? ''

      messages.push({
        id: `transcript-assistant-${idx++}`,
        role: 'assistant',
        text,
        timestamp: 0,
        toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
        verboseEvents,
      })
    } else if (t === 'stats') {
      handleStats(item, acc)
    }
    // tool_result, system, extension, compact, marker — silently skipped
  }

  // Attach run stats to last assistant message
  if (acc.runStats.llmCalls > 0) {
    const last = lastAssistantMessage(messages)
    if (last) last.runStats = buildRunStats(acc)
  }

  return messages
}

// ---------------------------------------------------------------------------
// Tool results
// ---------------------------------------------------------------------------

function collectToolResults(items: RawItem[]): Map<string, { content: string; isError: boolean }> {
  const map = new Map<string, { content: string; isError: boolean }>()
  for (const item of items) {
    if (item.type === 'tool_result' && item.tool_call_id) {
      map.set(item.tool_call_id, {
        content: item.content ?? '',
        isError: item.is_error ?? false,
      })
    }
  }
  return map
}

function buildToolCalls(
  calls: { id: string; name: string; input: Record<string, unknown> }[],
  results: Map<string, { content: string; isError: boolean }>,
): UIToolCall[] {
  return calls.map(tc => {
    const r = results.get(tc.id)
    return {
      id: tc.id,
      name: tc.name,
      args: tc.input,
      status: r ? (r.isError ? 'error' : 'done') : 'running' as const,
      result: r?.content,
    }
  })
}

// ---------------------------------------------------------------------------
// Stats → verbose events + run stats accumulation
// ---------------------------------------------------------------------------

interface RunAcc {
  verboseEvents: VerboseEvent[]
  runStats: {
    durationMs: number
    inputTokens: number
    outputTokens: number
    cacheReadTokens: number
    cacheWriteTokens: number
    llmCalls: number
    toolCallCount: number
    toolErrorCount: number
  }
}

function newRunAccumulator(): RunAcc {
  return {
    verboseEvents: [],
    runStats: {
      durationMs: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      llmCalls: 0,
      toolCallCount: 0,
      toolErrorCount: 0,
    },
  }
}

function handleStats(item: RawItem, acc: RunAcc): void {
  const kind = item.kind ?? ''
  const data = item.data ?? {}

  switch (kind) {
    case 'llm_call_started':
      acc.verboseEvents.push({ kind: 'llm_call', text: formatLlmCallStarted(data) })
      break
    case 'llm_call_completed': {
      const result = formatLlmCallCompleted(data)
      acc.verboseEvents.push({ kind: 'llm_completed', text: result.text, expandedText: result.expandedText })
      accumulateLlmStats(data, acc)
      break
    }
    case 'context_compaction_started':
      acc.verboseEvents.push({ kind: 'compact_call', text: formatCompactionStarted(data) })
      break
    case 'context_compaction_completed':
      acc.verboseEvents.push({ kind: 'compact_done', text: formatCompactionCompleted(data) })
      break
    case 'tool_finished':
      if (data.is_error) acc.runStats.toolErrorCount++
      acc.runStats.toolCallCount++
      break
    // run_finished, etc. — handled by buildRunStats
  }
}

// ---------------------------------------------------------------------------
// Stats accumulation
// ---------------------------------------------------------------------------

function accumulateLlmStats(data: Record<string, unknown>, acc: RunAcc): void {
  const usage = data.usage as Record<string, number> | undefined
  const metrics = data.metrics as Record<string, number> | undefined
  acc.runStats.llmCalls++
  acc.runStats.inputTokens += usage?.input ?? 0
  acc.runStats.outputTokens += usage?.output ?? 0
  acc.runStats.cacheReadTokens += usage?.cache_read ?? 0
  acc.runStats.cacheWriteTokens += usage?.cache_write ?? 0
  acc.runStats.durationMs += metrics?.duration_ms ?? 0
}

// ---------------------------------------------------------------------------
// Run stats builder
// ---------------------------------------------------------------------------

function buildRunStats(acc: RunAcc): RunStats {
  return {
    durationMs: acc.runStats.durationMs,
    turnCount: 0,
    toolCallCount: acc.runStats.toolCallCount,
    toolErrorCount: acc.runStats.toolErrorCount,
    inputTokens: acc.runStats.inputTokens,
    outputTokens: acc.runStats.outputTokens,
    cacheReadTokens: acc.runStats.cacheReadTokens,
    cacheWriteTokens: acc.runStats.cacheWriteTokens,
    llmCalls: acc.runStats.llmCalls,
    contextTokens: 0,
    contextWindow: 0,
    toolBreakdown: [],
    llmCallDetails: [],
    compactHistory: [],
    lastMessageStats: null,
    cumulativeStats: {
      userCount: 0,
      assistantCount: 0,
      toolResultCount: 0,
      imageCount: 0,
      userTokens: 0,
      assistantTokens: 0,
      toolResultTokens: 0,
      imageTokens: 0,
      toolDetails: [],
    },
    systemPromptTokens: 0,
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function lastAssistantMessage(messages: UIMessage[]): UIMessage | undefined {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === 'assistant') return messages[i]
  }
  return undefined
}
