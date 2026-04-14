/**
 * ScreenLog — writes OutputLines to ~/.evotai/logs/{session_id}.screen.log.
 *
 * Records exactly what appears on screen (1:1 with <Static> output).
 * Each OutputLine is written as a single timestamped line.
 */

import { appendFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'
import type { OutputLine } from './outputLines.js'

const LOGS_DIR = join(homedir(), '.evotai', 'logs')

export class ScreenLog {
  private path: string

  constructor(sessionId: string) {
    mkdirSync(LOGS_DIR, { recursive: true })
    this.path = join(LOGS_DIR, `${sessionId}.screen.log`)
  }

  get filePath(): string {
    return this.path
  }

  /** Write one or more OutputLines to the log. */
  writeLines(lines: OutputLine[]): void {
    const ts = formatTimestamp()
    for (const line of lines) {
      const prefix = formatPrefix(line.kind)
      this.appendLine(`[${ts}] ${prefix}${line.text}`)
    }
  }

  private appendLine(line: string): void {
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
