/**
 * ScreenLog — writes OutputLines to ~/.evotai/logs/{session_id}.screen.log.
 *
 * Session-level logger that records the expanded (full) version of all
 * screen output for post-hoc debugging.  Callers use:
 *
 *   screenLog.bind(sessionId)   — attach to a session (lazy, idempotent)
 *   screenLog.log(lines)        — append OutputLines
 *
 * All I/O errors are silently swallowed so callers never need try/catch.
 */

import { appendFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'
import type { OutputLine } from '../render/output.js'

const LOGS_DIR = join(homedir(), '.evotai', 'logs')

export class ScreenLog {
  private path: string | null = null
  private boundSessionId: string | null = null

  /** Bind (or re-bind) to a session. No-op if already bound to the same id. */
  bind(sessionId: string): void {
    if (this.boundSessionId === sessionId) return
    try {
      mkdirSync(LOGS_DIR, { recursive: true })
      this.path = join(LOGS_DIR, `${sessionId}.screen.log`)
      this.boundSessionId = sessionId
    } catch { /* silently ignore */ }
  }

  get filePath(): string | null {
    return this.path
  }

  /** Append OutputLines to the log. Ignored if not yet bound. */
  log(lines: OutputLine[]): void {
    if (!this.path || lines.length === 0) return
    const ts = formatTimestamp()
    for (const line of lines) {
      const prefix = formatPrefix(line.kind)
      this.appendLine(`[${ts}] ${prefix}${line.text}`)
    }
  }

  private appendLine(line: string): void {
    if (!this.path) return
    try {
      appendFileSync(this.path, line + '\n', { mode: 0o600 })
    } catch { /* silently ignore */ }
  }
}

function formatTimestamp(): string {
  const d = new Date()
  const h = d.getHours().toString().padStart(2, '0')
  const m = d.getMinutes().toString().padStart(2, '0')
  const s = d.getSeconds().toString().padStart(2, '0')
  const ms = d.getMilliseconds().toString().padStart(3, '0')
  return `${h}:${m}:${s}.${ms}`
}

function formatPrefix(kind: OutputLine['kind']): string {
  switch (kind) {
    case 'user': return '❯ '
    case 'assistant': return '  '
    case 'tool': return ''
    case 'tool_result': return '  '
    case 'verbose': return '  '
    case 'error': return 'ERROR: '
    case 'system': return '  '
    case 'run_summary': return '  '
    default: return ''
  }
}
