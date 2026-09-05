/**
 * Persistent command history bound to the project (cwd).
 * History file: ~/.evotai/projects/<slug>/evot_history
 */

import { readFileSync, appendFileSync, mkdirSync, existsSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'
import { spawnSync } from 'child_process'

function stateDir(): string {
  return process.env.EVOT_HOME || join(homedir(), '.evotai')
}
const DEFAULT_LIMIT = 200
const MAX_SLUG_LENGTH = 200

function findGitRoot(cwd: string): string {
  try {
    const result = spawnSync('git', ['--no-optional-locks', 'rev-parse', '--show-toplevel'], {
      cwd,
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    if (result.status === 0 && result.stdout) {
      const root = result.stdout.trim()
      if (root.length > 0) return root
    }
  } catch {}
  return cwd
}

function sanitizeForPath(name: string): string {
  const sanitized = name.replace(/[^a-zA-Z0-9]/g, '-')
  if (sanitized.length <= MAX_SLUG_LENGTH) return sanitized
  // FNV-1a hash for long paths
  let hash = BigInt('0xcbf29ce484222325')
  for (let i = 0; i < name.length; i++) {
    hash ^= BigInt(name.charCodeAt(i))
    hash = BigInt.asUintN(64, hash * BigInt('0x100000001b3'))
  }
  return sanitized.slice(0, MAX_SLUG_LENGTH) + '-' + hash.toString()
}

function resolveHistoryPath(cwd: string): string {
  const root = findGitRoot(cwd)
  const slug = sanitizeForPath(root)
  return join(stateDir(), 'projects', slug, 'evot_history')
}

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

  constructor(cwdOrPath?: string, opts?: { explicitPath?: boolean }) {
    if (opts?.explicitPath) {
      this.filePath = cwdOrPath!
    } else {
      this.filePath = cwdOrPath ? resolveHistoryPath(cwdOrPath) : join(stateDir(), 'evot_history')
    }
  }

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
