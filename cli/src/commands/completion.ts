/**
 * Tab completion for the REPL input.
 * Supports slash commands and file paths.
 */

import { readdirSync, statSync } from 'fs'
import { join, dirname, basename } from 'path'
import { ALL_COMMANDS, COMMANDS, type SlashCommand } from './index.js'

/**
 * Returns true when text looks like a hand-typed slash command prefix:
 * `/` followed by zero or more ASCII lowercase letters.
 * Pasted paths like `/some/path.rs` are rejected.
 */
function isSlashPrefix(text: string): boolean {
  if (!text.startsWith('/')) return false
  const rest = text.slice(1)
  const cmdPart = rest.split(/\s/)[0] ?? ''
  return /^[a-z]*$/.test(cmdPart)
}

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

  // Slash command completion: input looks like a hand-typed command
  if (isSlashPrefix(beforeCursor)) {
    return completeSlashCommand(beforeCursor)
  }

  // File path completion: walk back from the cursor collecting only
  // path-friendly characters so that paths glued to surrounding text
  // (CJK, punctuation, etc.) are still detected. Trigger only when the
  // captured segment looks like a real path: contains `/` or starts with
  // `~/` (a bare `~` is not enough because we'd otherwise replace it).
  const word = currentPathLike(beforeCursor)
  if (word && (word.includes('/') || word.startsWith('~/'))) {
    return completeFilePath(word, cursorCol)
  }

  return null
}

/**
 * Compute inline ghost hint for the current input.
 * Returns gray text to show after the cursor (Rust REPL style).
 *
 * - Bare `/` → compact list of all visible commands
 * - `/he` → `lp — Show help` (single match: completion + description)
 * - `/skill ` → `[install  list  remove]` (sub-command hints)
 */
export function getGhostHint(line: string, cursorCol: number): string {
  const beforeCursor = line.slice(0, cursorCol)
  const afterCursor = line.slice(cursorCol)

  // Only show hints when cursor is at end of line
  if (afterCursor.trim().length > 0) return ''
  if (!isSlashPrefix(beforeCursor)) return ''

  const parts = beforeCursor.split(/\s+/)
  const cmd = parts[0]!.toLowerCase()

  // Sub-command hints (after space)
  if (parts.length >= 2) {
    const partial = parts.slice(1).join(' ').toLowerCase()
    return getSubCommandHint(cmd, partial)
  }

  // Bare `/` → show all visible command names
  if (cmd === '/') {
    return `  [${COMMANDS.map(c => c.name.slice(1)).join('  ')}]`
  }

  // Ghost hints match all commands (hidden ones stay completable);
  // only the bare-`/` list above is limited to visible commands.
  const matches = commandCandidates().filter(c => c.label.startsWith(cmd))

  if (matches.length === 0) return ''

  // Single match — show completion suffix + sub-commands or description
  if (matches.length === 1) {
    const m = matches[0]!
    const suffix = m.label.slice(cmd.length)
    const subcmds = SUB_COMMANDS[m.command.name]
    if (subcmds && subcmds.length > 0 && (suffix === '' || m.label === cmd + suffix)) {
      return `${suffix}  [${subcmds.join('  ')}]`
    }
    return `${suffix}  ${m.command.description}`
  }

  // Multiple matches — show common suffix + candidate names
  const common = commonPrefix(matches.map(m => m.label))
  const suffix = common.slice(cmd.length)
  const names = matches.map(m => m.label).join('  ')
  return suffix ? `${suffix}  [${names}]` : `  [${names}]`
}

/**
 * Every typeable command spelling paired with its command. Aliases are first
 * class here: a user who types an alias gets the same hints as the canonical
 * name, and sub-command hints resolve through to the canonical entry.
 */
function commandCandidates(): { label: string; command: SlashCommand }[] {
  const candidates: { label: string; command: SlashCommand }[] = []
  for (const command of ALL_COMMANDS) {
    candidates.push({ label: command.name, command })
    for (const alias of command.aliases ?? []) candidates.push({ label: alias, command })
  }
  return candidates
}

function getSubCommandHint(cmd: string, partial: string): string {
  // Resolve through aliases so `/sessions <tab>` hints like `/resume`.
  const candidates = commandCandidates()
  const resolved = (candidates.find(c => c.label === cmd)
    ?? candidates.find(c => c.label.startsWith(cmd)))?.command
  if (!resolved) return ''

  const subcmds = SUB_COMMANDS[resolved.name] ?? []
  if (subcmds.length === 0) return ''

  if (!partial) {
    return `  [${subcmds.join('  ')}]`
  }

  const matches = subcmds.filter(s => s.startsWith(partial))
  if (matches.length === 0) return ''
  if (matches.length === 1) {
    return matches[0]!.slice(partial.length)
  }
  const common = commonPrefix(matches)
  const suffix = common.slice(partial.length)
  return suffix ? `${suffix}  [${matches.join('  ')}]` : `  [${matches.join('  ')}]`
}

const SUB_COMMANDS: Record<string, string[]> = {
  '/clip': ['all'],
  '/share': ['<session-id>', '<url#password>'],
  '/help': COMMANDS.map(c => c.name.slice(1)),
  '/skill': ['install', 'list', 'remove'],
  '/env': ['set', 'del', 'load'],
  '/log': ['shot', 'query'],
  '/resume': ['<id>', '<query>'],
  '/sessions': ['<id>', '<query>'],
  '/model': ['<name>'],
  '/harden': ['plan', 'changes', 'arch', '<subject>'],
}

// ---------------------------------------------------------------------------
// Slash command completion
// ---------------------------------------------------------------------------

function completeSlashCommand(input: string): CompletionResult | null {
  const parts = input.split(/\s+/)
  const cmd = parts[0]!.toLowerCase()

  // Only complete the command name itself (first word)
  if (parts.length > 1) return null

  const allCmds = ALL_COMMANDS
  const allNames: string[] = []
  for (const c of allCmds) {
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

  // Always treat the path as parent + basename so tab works whether or not
  // the typed path happens to resolve to an existing directory. This avoids
  // the previous bug where typing `~/github/pi` (where `pi` is a real
  // directory) produced candidates like `~/github/piLICENSE` because the
  // trailing `/` separator was missing in the candidate prefix.
  let dir: string
  let prefix: string
  if (expanded.endsWith('/')) {
    dir = expanded
    prefix = ''
  } else {
    dir = dirname(expanded) || '.'
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

  // Build full paths relative to the original word, preserving the user's
  // tilde / relative form rather than the expanded absolute path.
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

// Walk back from the end collecting characters that look like part of a
// Unix path. Stops at any non-path character — whitespace, CJK, quotes,
// brackets, etc. — so a path glued to surrounding text (e.g. `看下~/x`)
// is still extracted correctly.
function currentPathLike(beforeCursor: string): string | null {
  const isPathChar = (ch: string): boolean => /[A-Za-z0-9_./~+-]/.test(ch)
  let i = beforeCursor.length
  while (i > 0 && isPathChar(beforeCursor[i - 1]!)) i--
  if (i === beforeCursor.length) return null
  return beforeCursor.slice(i)
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
