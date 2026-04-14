/**
 * Tab completion for the REPL input.
 * Supports slash commands and file paths.
 */

import { readdirSync, statSync } from 'fs'
import { join, dirname, basename } from 'path'
import { COMMANDS } from './index.js'

export interface CompletionResult {
  /** The completed text to replace the current word */
  replacement: string
  /** All candidates (shown when multiple matches) */
  candidates: string[]
  /** Start index of the word being completed in the line */
  wordStart: number
}

/**
 * Compute tab completion for the current input line and cursor position.
 */
export function complete(line: string, cursorCol: number): CompletionResult | null {
  const beforeCursor = line.slice(0, cursorCol)

  // Slash command completion: input starts with /
  if (beforeCursor.startsWith('/')) {
    return completeSlashCommand(beforeCursor)
  }

  // File path completion: word containing /
  const word = currentWord(beforeCursor)
  if (word && (word.includes('/') || word.startsWith('~') || word.startsWith('.'))) {
    return completeFilePath(word, cursorCol)
  }

  return null
}

export interface CommandHint {
  name: string
  description: string
}

/**
 * Get matching command hints for dropdown display below input.
 */
export function getCommandHints(line: string, cursorCol: number): CommandHint[] {
  const beforeCursor = line.slice(0, cursorCol)
  if (!beforeCursor.startsWith('/')) return []

  const parts = beforeCursor.split(/\s+/)
  if (parts.length > 1) return [] // already past command name

  const cmd = parts[0]!.toLowerCase()

  return COMMANDS
    .filter(c => c.name.startsWith(cmd) || (c.aliases?.some(a => a.startsWith(cmd)) ?? false))
    .map(c => ({ name: c.name, description: c.description }))
}

/**
 * Compute inline ghost hint for the current input.
 * Returns gray text to show after the cursor.
 */
export function getGhostHint(line: string, cursorCol: number): string {
  const beforeCursor = line.slice(0, cursorCol)
  const afterCursor = line.slice(cursorCol)

  // Only show hints when cursor is at end of line
  if (afterCursor.trim().length > 0) return ''
  if (!beforeCursor.startsWith('/')) return ''

  const parts = beforeCursor.split(/\s+/)
  const cmd = parts[0]!.toLowerCase()

  if (parts.length > 1) return ''

  // Single match — show completion suffix only
  const allCmds = COMMANDS.flatMap(c => [c, ...(c.aliases ?? []).map(a => ({ ...c, name: a }))])
  const matches = allCmds.filter(c => c.name.startsWith(cmd))

  if (matches.length === 1 && cmd !== matches[0]!.name) {
    return matches[0]!.name.slice(cmd.length)
  }

  return ''
}

// ---------------------------------------------------------------------------
// Slash command completion
// ---------------------------------------------------------------------------

function completeSlashCommand(input: string): CompletionResult | null {
  const parts = input.split(/\s+/)
  const cmd = parts[0]!.toLowerCase()

  // Only complete the command name itself (first word)
  if (parts.length > 1) return null

  const allNames: string[] = []
  for (const c of COMMANDS) {
    allNames.push(c.name)
    if (c.aliases) allNames.push(...c.aliases)
  }

  const matches = allNames.filter((n) => n.startsWith(cmd))

  if (matches.length === 0) return null
  if (matches.length === 1) {
    return {
      replacement: matches[0]! + ' ',
      candidates: matches,
      wordStart: 0,
    }
  }

  // Multiple matches — complete common prefix
  const common = commonPrefix(matches)
  return {
    replacement: common,
    candidates: matches,
    wordStart: 0,
  }
}

// ---------------------------------------------------------------------------
// File path completion
// ---------------------------------------------------------------------------

function completeFilePath(word: string, cursorCol: number): CompletionResult | null {
  const wordStart = cursorCol - word.length

  // Expand ~
  const expanded = word.startsWith('~')
    ? (process.env.HOME ?? '') + word.slice(1)
    : word

  let dir: string
  let prefix: string

  try {
    const stat = statSync(expanded)
    if (stat.isDirectory()) {
      dir = expanded.endsWith('/') ? expanded : expanded + '/'
      prefix = ''
    } else {
      return null // exact file, nothing to complete
    }
  } catch {
    // Not a valid path yet — complete the last segment
    dir = dirname(expanded)
    prefix = basename(expanded)
  }

  let entries: string[]
  try {
    entries = readdirSync(dir)
  } catch {
    return null
  }

  const matches = prefix
    ? entries.filter((e) => e.startsWith(prefix))
    : entries.filter((e) => !e.startsWith('.')) // hide dotfiles by default

  if (matches.length === 0) return null

  // Build full paths relative to original word
  const dirPrefix = word.endsWith('/')
    ? word
    : word.slice(0, word.length - prefix.length)

  const fullMatches = matches.map((m) => {
    const fullPath = join(dir, m)
    try {
      if (statSync(fullPath).isDirectory()) return dirPrefix + m + '/'
    } catch { /* ignore */ }
    return dirPrefix + m
  })

  if (fullMatches.length === 1) {
    return {
      replacement: fullMatches[0]!,
      candidates: fullMatches,
      wordStart,
    }
  }

  const common = commonPrefix(fullMatches)
  return {
    replacement: common,
    candidates: fullMatches,
    wordStart,
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function currentWord(beforeCursor: string): string | null {
  // Extract the last whitespace-delimited word
  const match = beforeCursor.match(/(\S+)$/)
  return match ? match[1]! : null
}

function commonPrefix(strings: string[]): string {
  if (strings.length === 0) return ''
  let prefix = strings[0]!
  for (let i = 1; i < strings.length; i++) {
    const s = strings[i]!
    let j = 0
    while (j < prefix.length && j < s.length && prefix[j] === s[j]) j++
    prefix = prefix.slice(0, j)
  }
  return prefix
}
