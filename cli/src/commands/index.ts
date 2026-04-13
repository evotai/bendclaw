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
  { name: '/help', description: 'Show available commands', handler: 'builtin' },
  { name: '/model', description: 'Switch model', usage: '/model [name]', handler: 'builtin' },
  { name: '/resume', description: 'Resume a previous session', usage: '/resume [id]', handler: 'builtin' },
  { name: '/new', description: 'Start a new session', handler: 'builtin' },
  { name: '/plan', description: 'Enter planning mode (read-only tools)', handler: 'builtin' },
  { name: '/act', description: 'Exit planning mode (full tools)', handler: 'builtin' },
  { name: '/clear', description: 'Clear message display', handler: 'builtin' },
  { name: '/verbose', aliases: ['/v'], description: 'Toggle verbose mode (run stats)', handler: 'builtin' },
  { name: '/exit', aliases: ['/quit', '/q'], description: 'Exit the REPL', handler: 'builtin' },
]

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

  // Exact match first
  for (const c of COMMANDS) {
    if (c.name === cmd) return { kind: 'resolved', name: c.name, args }
    if (c.aliases?.includes(cmd)) return { kind: 'resolved', name: c.name, args }
  }

  // Prefix match
  const matches = COMMANDS.filter(
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
 * Format the help text for all commands.
 */
export function formatHelp(): string {
  const lines: string[] = ['Available commands:', '']
  for (const cmd of COMMANDS) {
    const usage = cmd.usage ?? cmd.name
    lines.push(`  ${padRight(usage, 24)} ${cmd.description}`)
  }
  lines.push('')
  lines.push('  Tip: commands can be abbreviated (e.g. /h for /help)')
  return lines.join('\n')
}

function padRight(s: string, n: number): string {
  return s + ' '.repeat(Math.max(0, n - s.length))
}

/**
 * Check if input looks like a slash command.
 */
export function isSlashCommand(input: string): boolean {
  return input.startsWith('/') && input.length > 1 && !input.startsWith('//')
}
