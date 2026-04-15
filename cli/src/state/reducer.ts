/**
 * Reducer-style state updates from RunEvents.
 */

import type { RunEvent } from '../native/index.js'
import { humanTokens as humanTokensInline, renderBar, renderPositionBar } from '../render/format.js'
import { emptyRunStats, type AppState } from './app.js'
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
      const toolCount = (p.tool_count as number) ?? 0
      const sysTok = (p.system_prompt_tokens as number) ?? 0
      const retryStr = attempt > 0 ? ` · retry ${attempt}` : ''

      // Use pre-computed message stats from Rust side
      const ms = p.message_stats as Record<string, any> | undefined
      const msgStats: MessageStats | null = ms
        ? {
            userCount: (ms.user_count as number) ?? 0,
            assistantCount: (ms.assistant_count as number) ?? 0,
            toolResultCount: (ms.tool_result_count as number) ?? 0,
            userTokens: (ms.user_tokens as number) ?? 0,
            assistantTokens: (ms.assistant_tokens as number) ?? 0,
            toolResultTokens: (ms.tool_result_tokens as number) ?? 0,
            toolDetails: (ms.tool_details as [string, number][]) ?? [],
          }
        : null

      let msgLine = `  ${msgCount} messages`
      if (msgStats) {
        const parts: string[] = []
        if (msgStats.userCount > 0) parts.push(`user ${msgStats.userCount}`)
        if (msgStats.assistantCount > 0) parts.push(`assistant ${msgStats.assistantCount}`)
        if (msgStats.toolResultCount > 0) parts.push(`tool_result ${msgStats.toolResultCount}`)
        msgLine += ` · ${toolCount} tools`
        if (parts.length > 0) msgLine += ` (${parts.join(' · ')})`
      } else {
        msgLine += ` · ${toolCount} tools`
      }

      let tokLine = '  tokens unknown'
      if (msgStats) {
        const total = sysTok + msgStats.userTokens + msgStats.assistantTokens + msgStats.toolResultTokens
        const tokParts: string[] = []
        if (sysTok > 0) tokParts.push(`system ~${humanTokensInline(sysTok)}`)
        if (msgStats.userTokens > 0) tokParts.push(`user ~${humanTokensInline(msgStats.userTokens)}`)
        if (msgStats.assistantTokens > 0) tokParts.push(`assistant ~${humanTokensInline(msgStats.assistantTokens)}`)
        if (msgStats.toolResultTokens > 0) tokParts.push(`tool_result ~${humanTokensInline(msgStats.toolResultTokens)}`)
        tokLine = `  ~${humanTokensInline(total)} est tokens (${tokParts.join(' · ')})`
      }

      let toolBreakdownLines = ''
      if (msgStats && msgStats.toolDetails.length >= 2) {
        const agg = new Map<string, number>()
        for (const [name, tokens] of msgStats.toolDetails) {
          agg.set(name, (agg.get(name) ?? 0) + tokens)
        }
        const sorted = [...agg.entries()].sort((a, b) => b[1] - a[1])
        const total = msgStats.toolResultTokens || 1
        const maxNameLen = Math.max(...sorted.map(([n]) => n.length), 4)
        for (const [name, tokens] of sorted) {
          const pct = ((tokens / total) * 100).toFixed(1)
          const bar = renderBar(tokens, total, 20)
          toolBreakdownLines += `\n    ${name.padEnd(maxNameLen)}  ~${humanTokensInline(tokens).padEnd(8)} (${pct.padStart(5)}%)  ${bar}`
        }
        if (toolBreakdownLines) {
          toolBreakdownLines = '\n  tool results:' + toolBreakdownLines
        }
      }

      const text = `[LLM] call · ${model} · turn ${turn}${retryStr}\n${msgLine}\n${tokLine}${toolBreakdownLines}`
      return {
        ...state,
        currentRunStats: {
          ...state.currentRunStats,
          lastMessageStats: msgStats,
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
        text = `[LLM] failed · ${durSec}s\n  ${error}`
      } else {
        const durSec = (durationMs / 1000).toFixed(1)
        const dur = durationMs || 1
        const ttfbPct = ((ttfbMs / dur) * 100).toFixed(0)
        const ttftPct = ((ttftMs / dur) * 100).toFixed(0)
        const streamPct = ((streamingMs / dur) * 100).toFixed(0)
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
        const msgsDropped = (result?.messages_dropped as number) ?? 0
        const saved = before - after
        const savedPct = before > 0 ? ((saved / before) * 100).toFixed(1) : '0.0'

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

        const posBar = renderPositionBar(beforeMsgs, sorted, level)

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

        const lines: string[] = []
        lines.push(`L${level}`)
        lines.push(`  ${beforeMsgs} messages ~${humanTokensInline(before)} tok`)
        lines.push(`  ${posBar}`)
        lines.push(`  ${summary}`)
        lines.push(`  ${afterMsgs} messages ~${humanTokensInline(after)} tok  (saved ~${humanTokensInline(saved)}, ${savedPct}%)`)

        if (sorted.length > 0) {
          const totalActions = allActions?.length ?? 0
          const changed = sorted.length
          let header: string
          if (level === 1) {
            header = `  actions: (${changed} of ${totalActions} changed, sorted by savings)`
          } else if (level === 2) {
            const totalMsgs = sorted.reduce((s: number, a: any) => s + 1 + ((a.related_count as number) ?? 0), 0)
            header = `  actions: (${changed} turns, ${totalMsgs} msgs → ${changed} summaries)`
          } else if (level === 3) {
            const kept = Math.max(afterMsgs - 1, 0)
            header = `  actions: (${msgsDropped} dropped, ${kept} kept, 1 marker)`
          } else {
            header = `  actions: (${changed} changed)`
          }
          lines.push(header)

          const TOP = 3
          const TAIL = 2
          const fmtAction = (a: any) => {
            const idx = (a.index as number) ?? 0
            const toolName = (a.tool_name as string) ?? ''
            const method = (a.method as string) ?? 'unknown'
            const bTok = (a.before_tokens as number) ?? 0
            const aTok = (a.after_tokens as number) ?? 0
            const aSaved = bTok - aTok
            if (method === 'Summarized') {
              const rc = (a.related_count as number) ?? 0
              return `    #${String(idx).padEnd(3)} turn(${1 + rc} msgs)  ${method.padEnd(12)} ~${humanTokensInline(bTok)} → ~${humanTokensInline(aTok)}  (saved ~${humanTokensInline(aSaved)})`
            }
            if (method === 'Dropped') {
              const endIdx = a.end_index as number | undefined
              const idxStr = endIdx != null ? `#${idx}..#${String(endIdx).padEnd(3)}` : `#${String(idx).padEnd(3)}`
              return `    ${idxStr} ${method.padEnd(12)} ~${humanTokensInline(bTok)} → ~${humanTokensInline(aTok)}  (saved ~${humanTokensInline(aSaved)})`
            }
            return `    #${String(idx).padEnd(3)} ${toolName.padEnd(12)} ${method.padEnd(12)} ~${humanTokensInline(bTok)} → ~${humanTokensInline(aTok)}  (saved ~${humanTokensInline(aSaved)})`
          }

          if (sorted.length <= TOP + TAIL) {
            for (const a of sorted) lines.push(fmtAction(a))
          } else {
            for (const a of sorted.slice(0, TOP)) lines.push(fmtAction(a))
            const omitted = sorted.length - TOP - TAIL
            lines.push(`    ... ${omitted} more ...`)
            for (const a of sorted.slice(sorted.length - TAIL)) lines.push(fmtAction(a))
          }
        }

        action = lines.join('\n')
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
