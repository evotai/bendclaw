/**
 * App state management for the CLI.
 */

import { type RunEvent } from '../native/index.js'

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
}

export interface ToolBreakdownEntry {
  name: string
  count: number
  totalDurationMs: number
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
    toolBreakdown: [],
  }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

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
      }

      return {
        ...state,
        messages: [...state.messages, msg],
        currentStreamText: '',
        currentThinkingText: '',
        activeToolCalls: new Map(),
        turnToolCalls: [],
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

      const newMap = new Map(state.activeToolCalls)
      newMap.delete(id)

      // Update run stats
      const stats = { ...state.currentRunStats }
      stats.toolCallCount++
      if (isError) stats.toolErrorCount++

      // Update tool breakdown
      const breakdown = [...stats.toolBreakdown]
      const existing = breakdown.find((e) => e.name === toolName)
      if (existing) {
        existing.count++
        existing.totalDurationMs += durationMs
        if (isError) existing.errors++
      } else {
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

    case 'llm_call_completed': {
      const usage = p.usage as Record<string, any> | undefined
      const stats = { ...state.currentRunStats }
      stats.llmCalls++
      if (usage) {
        stats.inputTokens += (usage.input as number) ?? 0
        stats.outputTokens += (usage.output as number) ?? 0
        stats.cacheReadTokens += (usage.cache_read as number) ?? 0
        stats.cacheWriteTokens += (usage.cache_write as number) ?? 0
      }
      return { ...state, currentRunStats: stats }
    }

    case 'run_finished': {
      const stats = {
        ...state.currentRunStats,
        durationMs: Date.now() - state.runStartTime,
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
