/**
 * ScreenLog — writes OutputLines to ~/.evotai/logs/{session_id}.screen.log.
 *
 * Session-level logger that records the expanded (full) version of all
 * screen output for post-hoc debugging.  Callers use:
 *
 *   screenLog.bind(sessionId)   — attach to a session (lazy, idempotent)
 *   screenLog.logLines(rendered) — append rendered text lines (ANSI-stripped)
 *
 * All I/O errors are silently swallowed so callers never need try/catch.
 */

import { appendFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'

const LOGS_DIR = join(homedir(), '.evotai', 'logs')

export class ScreenLog {
  private path: string | null = null
  private boundSessionId: string | null = null
  private buffer: string[] = []

  /** Bind (or re-bind) to a session. Flushes any buffered lines. */
  bind(sessionId: string): void {
    if (this.boundSessionId === sessionId) return
    try {
      mkdirSync(LOGS_DIR, { recursive: true })
      this.path = join(LOGS_DIR, `${sessionId}.screen.log`)
      this.boundSessionId = sessionId
      // Flush lines that were logged before bind
      if (this.buffer.length > 0) {
        for (const line of this.buffer) this.appendLine(line)
        this.buffer = []
      }
    } catch { /* silently ignore */ }
  }

  get filePath(): string | null {
    return this.path
  }

  /** Append rendered lines (with ANSI-stripped) to the log. Buffers if not yet bound. */
  logLines(rendered: string[]): void {
    if (rendered.length === 0) return
    for (const raw of rendered) {
      const line = stripAnsi(raw)
      if (this.path) {
        this.appendLine(line)
      } else {
        this.buffer.push(line)
      }
    }
  }

  private appendLine(line: string): void {
    if (!this.path) return
    try {
      const ts = formatTimestamp()
      appendFileSync(this.path, `[${ts}] ${line}\n`, { mode: 0o600 })
    } catch { /* silently ignore */ }
  }
}

/** Strip ANSI escape codes from a string. */
function stripAnsi(s: string): string {
  return s.replace(/\x1b\[[0-9;]*m/g, '')
}

/** Format current time as YYYY-MM-DD HH:MM:SS.mmm */
function formatTimestamp(): string {
  const d = new Date()
  const y = d.getFullYear()
  const mo = (d.getMonth() + 1).toString().padStart(2, '0')
  const day = d.getDate().toString().padStart(2, '0')
  const h = d.getHours().toString().padStart(2, '0')
  const mi = d.getMinutes().toString().padStart(2, '0')
  const s = d.getSeconds().toString().padStart(2, '0')
  const ms = d.getMilliseconds().toString().padStart(3, '0')
  return `${y}-${mo}-${day} ${h}:${mi}:${s}.${ms}`
}
