/**
 * Persistent command history backed by ~/.evotai/evot_history.
 * One entry per line. Multi-line entries are escaped (\\n for newlines).
 */

import { readFileSync, appendFileSync, mkdirSync, existsSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'

const STATE_DIR = join(homedir(), '.evotai')
const HISTORY_FILE = join(STATE_DIR, 'evot_history')
const DEFAULT_LIMIT = 200

function escape(s: string): string {
  return s.replace(/\\/g, '\\\\').replace(/\n/g, '\\n')
}

function unescape(s: string): string {
  let result = ''
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '\\' && i + 1 < s.length) {
      const next = s[i + 1]
      if (next === 'n') { result += '\n'; i++; continue }
      if (next === '\\') { result += '\\'; i++; continue }
    }
    result += s[i]
  }
  return result
}

export class HistoryManager {
  private filePath: string
  private lastEntry: string | null = null

  constructor(filePath?: string) {
    this.filePath = filePath ?? HISTORY_FILE
  }

  /**
   * Load the last N history entries from disk, oldest first.
   */
  load(limit = DEFAULT_LIMIT): string[] {
    try {
      const content = readFileSync(this.filePath, 'utf-8')
      const lines = content.split('\n').filter(l => l.length > 0)
      const entries = lines.slice(-limit).map(unescape)
      if (entries.length > 0) {
        this.lastEntry = entries[entries.length - 1]!
      }
      return entries
    } catch {
      return []
    }
  }

  /**
   * Append an entry to the history file. Skips consecutive duplicates.
   */
  append(entry: string): void {
    const trimmed = entry.trim()
    if (trimmed.length === 0) return
    if (trimmed === this.lastEntry) return

    try {
      if (!existsSync(this.filePath)) {
        mkdirSync(join(this.filePath, '..'), { recursive: true })
      }
      appendFileSync(this.filePath, escape(trimmed) + '\n', { mode: 0o600 })
      this.lastEntry = trimmed
    } catch {
      // silently ignore write failures
    }
  }
}
