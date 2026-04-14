/**
 * Transcript log writer — writes session events to ~/.evotai/logs/{session_id}.log.
 * Ported from Rust transcript_log.rs.
 */

import { appendFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'
import type { RunEvent } from '../native/index.js'
import { formatDuration } from './format.js'

const LOGS_DIR = join(homedir(), '.evotai', 'logs')

export class TranscriptLog {
  private path: string

  constructor(sessionId: string) {
    mkdirSync(LOGS_DIR, { recursive: true })
    this.path = join(LOGS_DIR, `${sessionId}.log`)
  }

  get filePath(): string {
    return this.path
  }

  writeUserPrompt(text: string): void {
    const ts = formatTimestamp()
    this.appendLine(`[${ts}] > ${text}`)
    this.appendLine('')
  }

  writeEvent(event: RunEvent): void {
    const lines = formatEvent(event)
    if (lines.length === 0) return
    const ts = formatTimestamp(event.created_at)
    // First line gets timestamp prefix
    this.appendLine(`[${ts}] ${lines[0]}`)
    for (let i = 1; i < lines.length; i++) {
      this.appendLine(lines[i]!)
    }
  }

  private appendLine(line: string): void {
    try {
      appendFileSync(this.path, line + '\n', { mode: 0o600 })
    } catch { /* silently ignore */ }
  }
}

function formatTimestamp(iso?: string): string {
  const d = iso ? new Date(iso) : new Date()
  const h = d.getHours().toString().padStart(2, '0')
  const m = d.getMinutes().toString().padStart(2, '0')
  const s = d.getSeconds().toString().padStart(2, '0')
  return `${h}:${m}:${s}`
}

function formatEvent(event: RunEvent): string[] {
  const p = event.payload
  switch (event.kind) {
    case 'run_started':
    case 'turn_started':
      return []
    case 'assistant_delta':
    case 'tool_progress':
      return [] // too noisy for log
    case 'assistant_completed': {
      const lines: string[] = []
      const content = p.content as any[] | undefined
      if (content) {
        for (const block of content) {
          if (block.type === 'text' && block.text) {
            lines.push(block.text)
          } else if (block.type === 'tool_call' || block.type === 'tool_use') {
            lines.push(`[${block.name} call]`)
            const input = block.input ?? block.args
            if (input) {
              const str = typeof input === 'string' ? input : JSON.stringify(input)
              lines.push(`  ${str.slice(0, 200)}`)
            }
          }
        }
      }
      return lines
    }
    case 'tool_started': {
      const name = p.tool_name ?? 'unknown'
      const args = p.args ? JSON.stringify(p.args).slice(0, 200) : ''
      return [`[${name} call] ${args}`]
    }
    case 'tool_finished': {
      const name = p.tool_name ?? 'unknown'
      const ok = p.is_error ? 'failed' : 'completed'
      const content = typeof p.content === 'string' ? p.content.slice(0, 200) : ''
      return [`[${name} ${ok}] ${content}`]
    }
    case 'llm_call_started': {
      const model = p.model ?? ''
      const turn = p.turn ?? event.turn
      const messageCount = Number(p.message_count ?? 0)
      const toolCount = Array.isArray(p.tools) ? p.tools.length : 0
      const messageBytes = Number(p.message_bytes ?? 0)
      const systemPromptTokens = Number(p.system_prompt_tokens ?? 0)
      const estimatedTokens = systemPromptTokens + Math.floor(messageBytes / 4)
      return [`[llm call] ${model} · turn ${turn} · ${messageCount} messages · ${toolCount} tools · ~${estimatedTokens} est tokens`]
    }
    case 'llm_call_completed': {
      const usage = p.usage as Record<string, any> | undefined
      const metrics = p.metrics as Record<string, any> | undefined
      const input = usage?.input ?? 0
      const output = usage?.output ?? 0
      const durationMs = Number(metrics?.duration_ms ?? 0)
      const ttftMs = Number(metrics?.ttft_ms ?? 0)
      const streamingMs = Number(metrics?.streaming_ms ?? 0)
      const tokPerSec = streamingMs > 0 ? Math.floor(output / (streamingMs / 1000)) : 0
      const parts = [
        `[llm completed] ${input} input · ${output} output tokens`,
        durationMs > 0 ? `${durationMs}ms` : undefined,
        ttftMs > 0 ? `ttft ${ttftMs}ms` : undefined,
        streamingMs > 0 ? `${tokPerSec} tok/s` : undefined,
      ].filter(Boolean)
      return [parts.join(' · ')]
    }
    case 'context_compaction_started': {
      const estimatedTokens = Number(p.estimated_tokens ?? 0)
      const budgetTokens = Number(p.budget_tokens ?? 0)
      const messageCount = Number(p.message_count ?? 0)
      const pct = budgetTokens > 0 ? Math.floor((estimatedTokens / budgetTokens) * 100) : 0
      return [`[compact] ${messageCount} messages · ~${estimatedTokens} tokens · ${pct}% of budget`]
    }
    case 'context_compaction_completed':
      return [`[compact completed] ${formatCompactionResult(p.result as Record<string, any> | undefined)}`]
    case 'run_finished': {
      const durationMs = Number(p.duration_ms ?? 0)
      const turnCount = Number(p.turn_count ?? 0)
      const usage = p.usage as Record<string, any> | undefined
      const input = Number(usage?.input ?? 0)
      const output = Number(usage?.output ?? 0)
      return [
        '---',
        `run ${formatDuration(durationMs)}  ·  turns ${turnCount}  ·  tokens ${input + output} (in ${input} · out ${output})`,
        '',
      ]
    }
    case 'error':
      return [`[error] ${p.message ?? p.error ?? 'unknown'}`]
    default:
      return []
  }
}

function formatCompactionResult(result: Record<string, any> | undefined): string {
  const type = result?.type
  if (type === 'no_op') return 'no compaction needed'
  if (type === 'run_once_cleared') {
    const saved = Number(result?.saved_tokens ?? 0)
    return `cleared run-once context · saved ${saved} tokens`
  }
  if (type === 'level_compacted') {
    const before = Number(result?.before_estimated_tokens ?? 0)
    const after = Number(result?.after_estimated_tokens ?? 0)
    return `compacted context ${before} → ${after} tokens`
  }
  return 'completed'
}

export function formatEventForTest(event: RunEvent): string[] {
  return formatEvent(event)
}
