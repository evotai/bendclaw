/**
 * Parse raw transcript items (from NAPI loadTranscript) into UIMessages
 * with verbose events, tool calls, thinking, and run stats.
 */

import type { UIMessage, UIToolCall, VerboseEvent, RunStats } from '../term/app/types.js'
import { humanTokens, renderBar } from '../render/format.js'

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
    case 'llm_call_completed':
      acc.verboseEvents.push({ kind: 'llm_completed', text: formatLlmCallCompleted(data) })
      accumulateLlmStats(data, acc)
      break
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
// Verbose event text formatters
// ---------------------------------------------------------------------------

function formatLlmCallStarted(data: Record<string, unknown>): string {
  const model = (data.model as string) ?? '?'
  const turn = (data.turn as number) ?? 0
  const attempt = (data.attempt as number) ?? 1
  const injected = (data.injected_count as number) ?? 0
  const msgCount = (data.message_count as number) ?? 0
  const bytes = (data.message_bytes as number) ?? 0
  const sysTok = (data.system_prompt_tokens as number) ?? 0

  const retryStr = attempt > 1 ? `  retry ${attempt}` : ''
  const injectedStr = injected > 0 ? `  +${injected} injected` : ''
  const kb = bytes >= 1024 ? `${(bytes / 1024).toFixed(0)} KB` : `${bytes} B`

  return `[LLM] call  ${model}  turn ${turn}${retryStr}${injectedStr}\n  ${msgCount} msgs · ${kb} · system ${humanTokens(sysTok)}`
}

function formatLlmCallCompleted(data: Record<string, unknown>): string {
  const error = data.error as string | undefined
  const usage = data.usage as Record<string, number> | undefined
  const metrics = data.metrics as Record<string, number> | undefined
  const durationMs = metrics?.duration_ms ?? 0

  if (error) {
    return `[LLM] failed · ${(durationMs / 1000).toFixed(1)}s\n  ${error}`
  }

  const inputTok = usage?.input ?? 0
  const outputTok = usage?.output ?? 0
  const durSec = (durationMs / 1000).toFixed(1)
  const tokPerSec = durationMs > 0 ? (outputTok / (durationMs / 1000)).toFixed(0) : '0'
  const ttfbMs = metrics?.ttfb_ms ?? 0
  const streamingMs = metrics?.streaming_ms ?? 0
  const dur = durationMs || 1
  const ttfbPct = ((ttfbMs / dur) * 100).toFixed(0)
  const streamPct = ((streamingMs / dur) * 100).toFixed(0)

  return `[LLM] completed · ${durSec}s · ${tokPerSec} tok/s\n  tokens  ${humanTokens(inputTok)} in · ${humanTokens(outputTok)} out\n  timing  ttfb ${(ttfbMs / 1000).toFixed(1)}s (${ttfbPct}%) · stream ${(streamingMs / 1000).toFixed(1)}s (${streamPct}%)`
}

function formatCompactionStarted(data: Record<string, unknown>): string {
  const msgCount = (data.message_count as number) ?? 0
  const estTokens = (data.estimated_tokens as number) ?? 0
  const budget = (data.budget_tokens as number) ?? 0
  const pct = budget > 0 ? ((estTokens / budget) * 100).toFixed(0) : '0'
  const bar = renderBar(estTokens, budget, 20)

  return `[COMPACT] call  ${msgCount} msgs\n  ${bar} ~${humanTokens(estTokens)}/${humanTokens(budget)} (${pct}%)`
}

function formatCompactionCompleted(data: Record<string, unknown>): string {
  const result = data.result as Record<string, unknown> | undefined
  if (!result) return '[COMPACT] done'

  const type = (result.type as string) ?? 'done'
  switch (type) {
    case 'no_op':
      return '[COMPACT] no-op'
    case 'run_once_cleared': {
      const saved = (result.saved_tokens as number) ?? 0
      const before = (result.before_estimated_tokens as number) ?? 0
      const after = (result.after_estimated_tokens as number) ?? 0
      const savedPct = before > 0 ? ((saved / before) * 100).toFixed(0) : '0'
      return `[COMPACT] cleared (−${humanTokens(saved)})\n  ~${humanTokens(before)} → ~${humanTokens(after)} (${savedPct}%)`
    }
    case 'level_compacted': {
      const level = (result.level as number) ?? 0
      const beforeMsgs = (result.before_message_count as number) ?? 0
      const afterMsgs = (result.after_message_count as number) ?? 0
      const before = (result.before_estimated_tokens as number) ?? 0
      const after = (result.after_estimated_tokens as number) ?? 0
      const saved = before - after
      const savedPct = before > 0 ? ((saved / before) * 100).toFixed(0) : '0'
      const truncated = (result.tool_outputs_truncated as number) ?? 0
      const summarized = (result.turns_summarized as number) ?? 0
      const dropped = (result.messages_dropped as number) ?? 0
      const parts: string[] = []
      if (summarized > 0) parts.push(`${summarized} summarized`)
      if (truncated > 0) parts.push(`${truncated} truncated`)
      if (dropped > 0) parts.push(`${dropped} dropped`)
      return `[COMPACT] L${level}  ${beforeMsgs}→${afterMsgs} msgs  saved ${humanTokens(saved)} (${savedPct}%)\n  ${parts.join(' · ')}`
    }
    default:
      return `[COMPACT] ${type}`
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
