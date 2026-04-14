/**
 * Parse transcript items (from NAPI loadTranscript) into UIMessages.
 */

import type { UIMessage, UIToolCall } from '../state/AppState.js'

interface TranscriptUser {
  type: 'user'
  text: string
}

interface TranscriptAssistant {
  type: 'assistant'
  text: string
  thinking?: string
  tool_calls: { id: string; name: string; input: Record<string, unknown> }[]
  stop_reason: string
}

interface TranscriptToolResult {
  type: 'tool_result'
  tool_call_id: string
  tool_name: string
  content: string
  is_error: boolean
}

export type TranscriptItem = TranscriptUser | TranscriptAssistant | TranscriptToolResult | { type: string }

export function transcriptToMessages(items: TranscriptItem[]): UIMessage[] {
  const messages: UIMessage[] = []
  // Map tool_call_id -> tool result for merging into assistant messages
  const toolResults = new Map<string, TranscriptToolResult>()

  // First pass: collect all tool results
  for (const item of items) {
    if (item.type === 'tool_result') {
      const tr = item as TranscriptToolResult
      toolResults.set(tr.tool_call_id, tr)
    }
  }

  // Second pass: build messages
  let idx = 0
  for (const item of items) {
    if (item.type === 'user') {
      const u = item as TranscriptUser
      messages.push({
        id: `transcript-user-${idx++}`,
        role: 'user',
        text: u.text,
        timestamp: 0,
      })
    } else if (item.type === 'assistant') {
      const a = item as TranscriptAssistant
      const toolCalls: UIToolCall[] = a.tool_calls.map(tc => {
        const result = toolResults.get(tc.id)
        return {
          id: tc.id,
          name: tc.name,
          args: tc.input,
          status: result?.is_error ? 'error' as const : 'done' as const,
          result: result?.content,
        }
      })
      messages.push({
        id: `transcript-assistant-${idx++}`,
        role: 'assistant',
        text: a.text,
        timestamp: 0,
        toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
      })
    }
    // Skip tool_result, system, extension, compact, stats — handled above or not displayed
  }

  return messages
}
