/**
 * Reducer-style state updates from RunEvents.
 */

import type { RunEvent } from '../../native/index.js'
import { formatLlmCallStarted, formatLlmCallRetry, formatLlmCallCompleted, formatCompactionStarted, formatCompactionCompleted } from '../../render/verbose.js'
import { emptyRunStats, type AppState } from './state.js'
import type { CompactRecord, MessageStats, UIAssistantBlock, UIMessage, UIToolCall, VerboseEvent } from './types.js'
import { appendAssistantDelta, assistantToolCalls, completedAssistantContent, findAssistantToolCall, updateAssistantToolCall, updateToolCallInMessages, upsertAssistantToolCall } from './assistant-content.js'
import { parseStreamingToolArgs } from './tool-args.js'


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
        currentAssistantContent: [],
        currentRunStats: emptyRunStats(),
        runStartTime: Date.now(),
        verboseEvents: [],
      }

    case 'turn_started':
      return {
        ...state,
        currentRunStats: {
          ...state.currentRunStats,
          turnCount: state.currentRunStats.turnCount + 1,
        },
      }

    case 'assistant_delta': {
      const delta = p.delta as string | undefined
      if (!Number.isInteger(p.content_index) || !delta || (p.content_type !== 'text' && p.content_type !== 'thinking')) {
        return state
      }
      return {
        ...state,
        currentAssistantContent: appendAssistantDelta(state.currentAssistantContent, p),
        lastTokenAt: Date.now(),
      }
    }

    case 'assistant_tool_call': {
      const id = p.tool_call_id as string
      const contentIndex = p.content_index as number
      if (!id || !Number.isInteger(contentIndex)) return state
      const current = findAssistantToolCall(state.currentAssistantContent, id)
      const phase = p.phase as string | undefined
      const delta = p.delta as string | undefined
      const partialArgs = phase === 'start'
        ? ''
        : `${current?.partialArgs ?? ''}${delta ?? ''}`
      const finalArgs = p.args as Record<string, unknown> | undefined
      const toolCall: UIToolCall = {
        ...current,
        id,
        name: (p.tool_name as string) || current?.name || '',
        args: finalArgs ?? (delta !== undefined ? parseStreamingToolArgs(partialArgs) : current?.args ?? {}),
        status: current?.status ?? 'queued',
        partialArgs: phase === 'end' ? undefined : partialArgs,
        argsComplete: phase === 'end' || current?.argsComplete,
      }
      return {
        ...state,
        currentAssistantContent: upsertAssistantToolCall(
          state.currentAssistantContent,
          contentIndex,
          toolCall,
        ),
      }
    }

    case 'assistant_completed': {
      const completed = p.content as unknown[] | undefined
      const streamedContent = state.currentAssistantContent
      let content = completedAssistantContent(completed, streamedContent)
      for (const toolCall of assistantToolCalls(content)) {
        content = updateAssistantToolCall(content, toolCall.id, current => ({
          ...current,
          argsComplete: true,
          partialArgs: undefined,
        }))
      }
      const text = content
        .filter((block): block is Extract<UIAssistantBlock, { type: 'text' }> => block.type === 'text')
        .map(block => block.text)
        .join('')
      const msg: UIMessage = {
        id: event.event_id,
        role: 'assistant',
        text,
        timestamp: Date.now(),
        content,
        verboseEvents: state.verboseEvents.length > 0 ? [...state.verboseEvents] : undefined,
      }

      return {
        ...state,
        messages: [...state.messages, msg],
        currentAssistantContent: content,
        verboseEvents: [],
      }
    }

    case 'tool_started': {
      const id = p.tool_call_id as string
      if (!id) return state
      return {
        ...state,
        currentAssistantContent: updateAssistantToolCall(state.currentAssistantContent, id, current => ({
          ...current,
          name: (p.tool_name as string) ?? current.name,
          args: (p.args as Record<string, unknown>) ?? current.args,
          status: 'running',
          argsComplete: true,
          partialArgs: undefined,
          startedAt: current.startedAt ?? Date.now(),
          previewCommand: p.preview_command ?? current.previewCommand,
          isFanout: (p.is_fanout as boolean) ?? current.isFanout,
          invocationCount: (p.invocation_count as number) ?? current.invocationCount,
          parallel: (p.parallel as boolean) ?? current.parallel,
        })),
      }
    }

    case 'tool_progress': {
      const id = p.tool_call_id as string
      if (!id || !findAssistantToolCall(state.currentAssistantContent, id)) return state
      const text = p.text as string | undefined
      return {
        ...state,
        currentAssistantContent: updateAssistantToolCall(state.currentAssistantContent, id, current => ({
          ...current,
          // Heartbeats carry no information the UI lacks — the spinner already
          // renders an elapsed clock — so they must not displace streamed
          // partial output. `bash` says "Running", `task_output` says "Waiting".
          progress: text && !/^(Running|Waiting)\.\.\. \d+s$/.test(text.trim()) ? text : current.progress,
          details: mergeToolDetails(current.details, p.details),
        })),
      }
    }

    case 'tool_finished': {
      const id = p.tool_call_id as string
      const isError = !!p.is_error
      const current = findAssistantToolCall(state.currentAssistantContent, id)
      const toolName = p.tool_name ?? current?.name ?? 'unknown'
      const durationMs = (p.duration_ms as number) ?? 0

      const finalDetails = mergeToolDetails(current?.details, p.details)
      const finished: UIToolCall = {
        id,
        name: toolName,
        args: current?.args ?? (p.args as Record<string, unknown>) ?? {},
        status: isError ? 'error' : 'done',
        result: p.content,
        details: finalDetails,
        previewCommand: current?.previewCommand,
        durationMs,
        isFanout: current?.isFanout,
        invocationCount: current?.invocationCount,
        parallel: current?.parallel,
      }

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
        currentAssistantContent: updateAssistantToolCall(
          state.currentAssistantContent,
          id,
          () => finished,
        ),
        messages: updateToolCallInMessages(state.messages, id, finished),
        currentRunStats: stats,
      }
    }

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
        sessionTokens: {
          ...state.sessionTokens,
          contextTokens: (p.estimated_context_tokens as number) ?? state.sessionTokens.contextTokens,
          contextWindow: (p.context_window as number) ?? state.sessionTokens.contextWindow,
        },
        verboseEvents: [...state.verboseEvents, { kind: 'llm_call', text }],
      }
    }

    case 'llm_call_retry':
    case 'api_retry': {
      const text = formatLlmCallRetry(p)
      return {
        ...state,
        verboseEvents: [...state.verboseEvents, { kind: 'llm_retry', text }],
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
      // Real generation speed: output tokens over the pure streaming window
      // (first delta → done), not total wall-clock. duration_ms would dilute the
      // rate with the ttfb wait (queueing + prompt processing).
      const tokPerSec = streamingMs > 0 ? outputTok / (streamingMs / 1000) : 0

      const cacheReadTok = (usage?.cache_read as number) ?? 0
      const cacheWriteTok = (usage?.cache_write as number) ?? 0
      const model = (p.model as string) ?? state.model

      if (usage) {
        stats.inputTokens += inputTok
        stats.outputTokens += outputTok
        stats.cacheReadTokens += cacheReadTok
        stats.cacheWriteTokens += cacheWriteTok
        stats.lastLlmUsage = {
          inputTokens: inputTok,
          outputTokens: outputTok,
          cacheReadTokens: cacheReadTok,
          cacheWriteTokens: cacheWriteTok,
        }

        // Provider usage buckets are disjoint.
        const realContextTokens =
          inputTok + cacheReadTok + cacheWriteTok + outputTok
        if (realContextTokens > 0) {
          stats.contextTokens = realContextTokens
        }
      }

      stats.llmCallDetails = [...stats.llmCallDetails, {
        model,
        durationMs,
        inputTokens: inputTok,
        outputTokens: outputTok,
        cacheReadTokens: cacheReadTok,
        cacheWriteTokens: cacheWriteTok,
        ttfbMs,
        ttftMs,
        tokPerSec,
      }]

      const data: Record<string, unknown> = {
        ...p,
        model,
        turn: event.turn,
        estimated_context_tokens: state.currentRunStats.contextTokens,
        context_window: state.currentRunStats.contextWindow,
      }
      const result = formatLlmCallCompleted(data)

      return {
        ...state,
        currentRunStats: stats,
        sessionTokens: {
          inputTokens: state.sessionTokens.inputTokens + inputTok,
          outputTokens: state.sessionTokens.outputTokens + outputTok,
          cacheReadTokens: state.sessionTokens.cacheReadTokens + cacheReadTok,
          cacheWriteTokens: state.sessionTokens.cacheWriteTokens + cacheWriteTok,
          contextTokens: stats.contextTokens,
          contextWindow: stats.contextWindow,
        },
        verboseEvents: [...state.verboseEvents, {
          kind: 'llm_completed',
          text: result.text,
          expandedText: result.expandedText,
        }],
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
        sessionTokens: { ...state.sessionTokens, contextTokens: (p.estimated_tokens as number) ?? state.sessionTokens.contextTokens, contextWindow: (p.context_window as number) ?? state.sessionTokens.contextWindow },
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
        type === 'level_compacted' || type === 'level_done' || type === 'compacted'
          ? {
              level: (result?.level as number) ?? (result?.messages_evicted ? 3 : 1),
              beforeTokens: ((result?.before_estimated_tokens as number) ?? (result?.before_tokens as number) ?? (result?.tokens_before as number)) ?? 0,
              afterTokens: ((result?.after_estimated_tokens as number) ?? (result?.after_tokens as number) ?? (result?.tokens_after as number)) ?? 0,
            }
          : type === 'run_once_cleared'
            ? {
                level: 0,
                beforeTokens: ((result?.before_estimated_tokens as number) ?? state.currentRunStats.contextTokens) ?? 0,
                afterTokens: ((result?.after_estimated_tokens as number) ?? (state.currentRunStats.contextTokens - ((result?.saved_tokens as number) ?? 0))) ?? 0,
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

      return {
        ...state,
        isLoading: false,
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

function mergeToolDetails(current: unknown, next: unknown): unknown {
  const currentRecord = asToolDetails(current)
  const nextRecord = asToolDetails(next)
  if (currentRecord && nextRecord) return { ...currentRecord, ...nextRecord }
  return next ?? current
}

function asToolDetails(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined
}
