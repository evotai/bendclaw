/**
 * Reducer-style state updates from RunEvents.
 */

import type { RunEvent } from '../../native/index.js'
import { humanTokens as humanTokensInline, renderBar } from '../../render/format.js'
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
      const attempt = (p.attempt as number) ?? 0
      const msgCount = (p.message_count as number) ?? 0
      const tools = p.tools as any[] | undefined
      const toolCount = (p.tool_count as number) ?? tools?.length ?? 0
      const sysTok = (p.system_prompt_tokens as number) ?? 0
      const retryStr = attempt > 0 ? ` · retry ${attempt}` : ''
      const injectedCount = (p.injected_count as number) ?? 0
      const injectedStr = injectedCount > 0 ? ` · ${injectedCount} injected` : ''

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

      // Build detail lines
      const detailLines: string[] = []

      // Messages line
      const parts: string[] = []
      if (msgStats) {
        if (msgStats.userCount > 0) parts.push(`user ${msgStats.userCount}`)
        if (msgStats.assistantCount > 0) parts.push(`asst ${msgStats.assistantCount}`)
        if (msgStats.toolResultCount > 0) parts.push(`tool ${msgStats.toolResultCount}`)
      }
      const msgPart = parts.length > 0 ? `${msgCount} msgs (${parts.join(' · ')})` : `${msgCount} msgs`
      detailLines.push(`  ${msgPart}`)

      // Budget bar
      const budgetTokens = (p.budget_tokens as number) ?? 0
      if (budgetTokens > 0 && msgStats) {
        const total = sysTok + msgStats.userTokens + msgStats.assistantTokens + msgStats.toolResultTokens + msgStats.imageTokens
        const pct = ((total / budgetTokens) * 100).toFixed(0)
        const bar = renderBar(total, budgetTokens, 20)
        detailLines.push(`  ${bar} ~${humanTokensInline(total)}/${humanTokensInline(budgetTokens)} (${pct}%)`)
      }

      // Token distribution by role
      if (msgStats) {
        const dist: string[] = []
        if (sysTok > 0) dist.push(`sys ${humanTokensInline(sysTok)}`)
        if (msgStats.userTokens > 0) dist.push(`user ${humanTokensInline(msgStats.userTokens)}`)
        if (msgStats.assistantTokens > 0) dist.push(`asst ${humanTokensInline(msgStats.assistantTokens)}`)
        if (msgStats.toolResultTokens > 0) dist.push(`tool ${humanTokensInline(msgStats.toolResultTokens)}`)
        if (msgStats.imageTokens > 0) dist.push(`img ${humanTokensInline(msgStats.imageTokens)}`)
        if (dist.length > 0) detailLines.push(`  ${dist.join(' · ')}`)
      }

      // Per-tool token breakdown
      if (msgStats && msgStats.toolDetails.length >= 2) {
        const agg = new Map<string, number>()
        for (const [name, tokens] of msgStats.toolDetails) {
          agg.set(name, (agg.get(name) ?? 0) + tokens)
        }
        const sorted = [...agg.entries()].sort((a, b) => b[1] - a[1])
        const total = msgStats.toolResultTokens || 1
        const maxNameLen = Math.max(...sorted.map(([n]) => n.length), 4)
        for (const [name, tokens] of sorted) {
          const pct = ((tokens / total) * 100).toFixed(0)
          const bar = renderBar(tokens, total, 20)
          detailLines.push(`      ${name.padEnd(maxNameLen)}  ~${humanTokensInline(tokens).padEnd(6)} (${pct.padStart(3)}%)  ${bar}`)
        }
      }

      const text = `[LLM] call  ${model}  turn ${turn}${retryStr}${injectedStr}\n${detailLines.join('\n')}`

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
          lastMessageStats: msgStats,
          cumulativeStats: cumulative,
          systemPromptTokens: sysTok,
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

      let text: string
      if (error) {
        const durSec = (durationMs / 1000).toFixed(1)
        text = `[LLM] failed  ${durSec}s  ${error}`
      } else {
        const durSec = (durationMs / 1000).toFixed(1)
        text = `[LLM] completed  ${durSec}s  ${tokPerSec.toFixed(0)} tok/s  ${humanTokensInline(inputTok)} in · ${outputTok} out\n  ttfb ${(ttfbMs / 1000).toFixed(1)}s · stream ${(streamingMs / 1000).toFixed(1)}s`
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
      const bar = renderBar(estTokens, budget, 20)

      // Token distribution (compact)
      const cms = p.message_stats as Record<string, any> | undefined
      const compactDetails: string[] = []
      compactDetails.push(`  ${bar}  ~${humanTokensInline(estTokens)}/${humanTokensInline(budget)} (${pct}%)`)
      if (cms) {
        const parts: string[] = []
        const uTok = (cms.user_tokens as number) ?? 0
        const aTok = (cms.assistant_tokens as number) ?? 0
        const trTok = (cms.tool_result_tokens as number) ?? 0
        const imgTok = (cms.image_tokens as number) ?? 0
        if (sysTok > 0) parts.push(`sys ${humanTokensInline(sysTok)}`)
        if (uTok > 0) parts.push(`user ${humanTokensInline(uTok)}`)
        if (aTok > 0) parts.push(`asst ${humanTokensInline(aTok)}`)
        if (trTok > 0) parts.push(`tool ${humanTokensInline(trTok)}`)
        if (imgTok > 0) parts.push(`img ${humanTokensInline(imgTok)}`)
        if (parts.length > 0) compactDetails.push(`  ${parts.join(' · ')}`)
      }
      const text = `[COMPACT] call  ${msgCount} msgs\n${compactDetails.join('\n')}`
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
        action = `cleared (−${humanTokensInline(saved)})`
      } else if (type === 'level_compacted') {
        const level = (result?.level as number) ?? 0
        const beforeMsgs = (result?.before_message_count as number) ?? 0
        const afterMsgs = (result?.after_message_count as number) ?? 0
        const before = (result?.before_estimated_tokens as number) ?? 0
        const after = (result?.after_estimated_tokens as number) ?? 0
        const msgsDropped = (result?.messages_dropped as number) ?? 0
        const saved = before - after
        const savedPct = before > 0 ? ((saved / before) * 100).toFixed(0) : '0'

        const allActions = result?.actions as any[] | undefined
        const sorted = allActions
          ? [...allActions]
              .filter((a: any) => a.method !== 'Skipped')
              .sort((a: any, b: any) => {
                const sa = (a.before_tokens ?? 0) - (a.after_tokens ?? 0)
                const sb = (b.before_tokens ?? 0) - (b.after_tokens ?? 0)
                return sb - sa
              })
          : []

        let summary: string
        if (level === 1) {
          const outlineCount = sorted.filter((a: any) => a.method === 'Outline').length
          const headtailCount = sorted.filter((a: any) => a.method === 'HeadTail').length
          const parts: string[] = []
          if (outlineCount > 0) parts.push(`outlined ${outlineCount}`)
          if (headtailCount > 0) parts.push(`head-tail ${headtailCount}`)
          summary = parts.length > 0 ? `↓ ${parts.join(', ')}` : '↓ no changes'
        } else if (level === 2) {
          const turnCount = sorted.length
          const totalMsgs = sorted.reduce((s: number, a: any) => s + 1 + ((a.related_count as number) ?? 0), 0)
          summary = `↓ summarized ${turnCount} turns (${totalMsgs} msgs → ${turnCount} summaries)`
        } else if (level === 3) {
          const kept = Math.max(afterMsgs - 1, 0)
          summary = `↓ dropped ${msgsDropped} msgs, kept ${kept} + 1 marker`
        } else {
          summary = '↓ no changes'
        }

        // Compact single-line summary
        const compactBar = renderBar(saved, before || 1, 12)
        action = `L${level}  ${beforeMsgs} msgs ~${humanTokensInline(before)} → ${afterMsgs} msgs ~${humanTokensInline(after)}  (−${humanTokensInline(saved)}, ${savedPct}%)  ${compactBar}  ${summary}`
      }

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
