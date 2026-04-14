/**
 * Slash commands for the REPL.
 */

export interface SlashCommand {
  name: string
  aliases?: string[]
  description: string
  usage?: string
  handler: 'builtin'
}

export const COMMANDS: SlashCommand[] = [
  { name: '/help', description: 'Show help information', usage: '/help [command]', handler: 'builtin' },
  { name: '/resume', description: 'Resume a session', usage: '/resume [id]', handler: 'builtin' },
  { name: '/new', description: 'Start a new session', handler: 'builtin' },
  { name: '/model', description: 'Show or change model', usage: '/model [name]', handler: 'builtin' },
  { name: '/plan', description: 'Enter planning mode', handler: 'builtin' },
  { name: '/act', description: 'Return to normal action mode', handler: 'builtin' },
  { name: '/env', description: 'Manage variables', usage: '/env [set K=V | del K | load FILE]', handler: 'builtin' },
  { name: '/log', description: 'Analyze session log in a side conversation', usage: '/log [query]', handler: 'builtin' },
  { name: '/skill', description: 'Manage skills', usage: '/skill [list | install <source> | remove <name>]', handler: 'builtin' },
]

/** Hidden commands — recognised but not shown in /help or ghost hints */
export const HIDDEN_COMMANDS: SlashCommand[] = [
  { name: '/clear', description: 'Clear message display', handler: 'builtin' },
  { name: '/verbose', aliases: ['/v'], description: 'Toggle verbose mode', handler: 'builtin' },
  { name: '/exit', aliases: ['/quit', '/q'], description: 'Exit the REPL', handler: 'builtin' },
]

/** All commands (visible + hidden) for resolution */
const ALL_COMMANDS: SlashCommand[] = [...COMMANDS, ...HIDDEN_COMMANDS]

export type ResolvedCommand =
  | { kind: 'resolved'; name: string; args: string }
  | { kind: 'ambiguous'; candidates: string[] }
  | { kind: 'unknown' }

/**
 * Resolve a slash command input to a known command.
 * Supports prefix matching (e.g. "/h" → "/help").
 */
export function resolveCommand(input: string): ResolvedCommand {
  const parts = input.trim().split(/\s+/)
  const cmd = parts[0]!.toLowerCase()
  const args = parts.slice(1).join(' ')

  // Exact match first (visible + hidden)
  for (const c of ALL_COMMANDS) {
    if (c.name === cmd) return { kind: 'resolved', name: c.name, args }
    if (c.aliases?.includes(cmd)) return { kind: 'resolved', name: c.name, args }
  }

  // Prefix match (visible + hidden)
  const matches = ALL_COMMANDS.filter(
    (c) => c.name.startsWith(cmd) || (c.aliases?.some((a) => a.startsWith(cmd)) ?? false)
  )

  if (matches.length === 1) {
    return { kind: 'resolved', name: matches[0]!.name, args }
  }
  if (matches.length > 1) {
    return { kind: 'ambiguous', candidates: matches.map((c) => c.name) }
  }

  return { kind: 'unknown' }
}

/**
 * Check if input looks like a slash command.
 * Only the first word is checked against known commands (visible + hidden),
 * so pasted paths like `/some/path.rs` are not treated as commands.
 */
export function isSlashCommand(input: string): boolean {
  const firstWord = input.trim().split(/\s+/)[0]?.toLowerCase() ?? ''
  if (!firstWord.startsWith('/') || firstWord.length < 2) return false
  const allCmds = [...COMMANDS, ...HIDDEN_COMMANDS]
  return allCmds.some(c => c.name === firstWord || c.aliases?.includes(firstWord))
    || allCmds.some(c => c.name.startsWith(firstWord) || (c.aliases?.some(a => a.startsWith(firstWord)) ?? false))
}
