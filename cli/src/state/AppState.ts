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
  isStreaming?: boolean
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
// App state
// ---------------------------------------------------------------------------

export interface AppState {
  messages: UIMessage[]
  isLoading: boolean
  sessionId: string | null
  model: string
  cwd: string
  error: string | null
  currentStreamText: string
  currentThinkingText: string
  activeToolCalls: Map<string, UIToolCall>
}

export function createInitialState(model: string, cwd: string): AppState {
  return {
    messages: [],
    isLoading: false,
    sessionId: null,
    model,
    cwd,
    error: null,
    currentStreamText: '',
    currentThinkingText: '',
    activeToolCalls: new Map(),
  }
}

// ---------------------------------------------------------------------------
// Reducer-style state updates from RunEvents
// ---------------------------------------------------------------------------

export function applyEvent(state: AppState, event: RunEvent): AppState {
  const kind = event.kind
  const p = event.payload

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
      }

    case 'assistant_delta': {
      const delta = (p as any).delta as string | undefined
      const thinkingDelta = (p as any).thinking_delta as string | undefined
      return {
        ...state,
        currentStreamText: state.currentStreamText + (delta ?? ''),
        currentThinkingText: state.currentThinkingText + (thinkingDelta ?? ''),
      }
    }

    case 'assistant_completed': {
      const content = (p as any).content as any[] | undefined
      const textParts = (content ?? [])
        .filter((b: any) => b.type === 'text')
        .map((b: any) => b.text)
      const text = textParts.join('') || state.currentStreamText

      const toolCalls = (content ?? [])
        .filter((b: any) => b.type === 'tool_call')
        .map((b: any) => ({
          id: b.id,
          name: b.name,
          args: b.input ?? {},
          status: 'done' as const,
        }))

      const msg: UIMessage = {
        id: event.event_id,
        role: 'assistant',
        text,
        timestamp: Date.now(),
        toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
      }

      return {
        ...state,
        messages: [...state.messages, msg],
        currentStreamText: '',
        currentThinkingText: '',
      }
    }

    case 'tool_started': {
      const tc: UIToolCall = {
        id: (p as any).tool_call_id,
        name: (p as any).tool_name,
        args: (p as any).args ?? {},
        status: 'running',
        previewCommand: (p as any).preview_command,
      }
      const newMap = new Map(state.activeToolCalls)
      newMap.set(tc.id, tc)
      return { ...state, activeToolCalls: newMap }
    }

    case 'tool_finished': {
      const id = (p as any).tool_call_id as string
      const newMap = new Map(state.activeToolCalls)
      const existing = newMap.get(id)
      if (existing) {
        newMap.set(id, {
          ...existing,
          status: (p as any).is_error ? 'error' : 'done',
          result: (p as any).content,
          durationMs: (p as any).duration_ms,
        })
      }
      return { ...state, activeToolCalls: newMap }
    }

    case 'run_finished':
      return {
        ...state,
        isLoading: false,
        activeToolCalls: new Map(),
      }

    case 'error':
      return {
        ...state,
        isLoading: false,
        error: (p as any).message ?? 'Unknown error',
      }

    default:
      return state
  }
}
