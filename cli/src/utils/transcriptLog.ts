/**
 * Transcript log writer — writes session events to ~/.evotai/logs/{session_id}.log.
 * Ported from Rust transcript_log.rs.
 */

import { appendFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'
import type { RunEvent } from '../native/index.js'

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
      return [`--- run started (turn ${event.turn}) ---`]
    case 'turn_started':
      return [`--- turn ${event.turn} ---`]
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
          } else if (block.type === 'tool_use') {
            lines.push(`[${block.name} call]`)
            if (block.input) {
              const input = typeof block.input === 'string' ? block.input : JSON.stringify(block.input)
              lines.push(`  ${input.slice(0, 200)}`)
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
      return [`[llm] ${model} turn=${event.turn}`]
    }
    case 'llm_call_completed': {
      const usage = p.usage as Record<string, any> | undefined
      const metrics = p.metrics as Record<string, any> | undefined
      const input = usage?.input ?? 0
      const output = usage?.output ?? 0
      const dur = metrics?.duration_ms ? `${metrics.duration_ms}ms` : ''
      return [`[llm done] in=${input} out=${output} ${dur}`]
    }
    case 'context_compaction_started':
      return ['[compaction started]']
    case 'context_compaction_completed':
      return ['[compaction completed]']
    case 'run_finished': {
      const dur = p.duration_ms ? `${p.duration_ms}ms` : ''
      return [`--- run finished ${dur} ---`, '']
    }
    case 'error':
      return [`[error] ${p.message ?? p.error ?? 'unknown'}`]
    default:
      return []
  }
}
