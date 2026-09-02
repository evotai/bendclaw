/**
 * Slash commands for the REPL.
 */

export interface SlashCommand {
  name: string
  aliases?: string[]
  description: string
  usage?: string
  handler: 'builtin'
  /** Minimum typed prefix length, including `/`, before prefix matching applies. */
  minPrefixLength?: number
}

export const COMMANDS: SlashCommand[] = [
  { name: '/help', description: 'Show help information', usage: '/help [command]', handler: 'builtin' },
  { name: '/resume', aliases: ['/sessions'], description: 'Resume a session', usage: '/resume [id | query]', handler: 'builtin' },
  { name: '/new', description: 'Start a new session', handler: 'builtin' },
  { name: '/model', description: 'Show or change model', usage: '/model [name]', handler: 'builtin' },
  { name: '/plan', description: 'Enter planning mode', handler: 'builtin' },
  { name: '/login', description: 'Log in to evot cloud', handler: 'builtin' },
  { name: '/logout', description: 'Log out of evot cloud', handler: 'builtin' },
]

/** Hidden commands — recognised but not shown in /help or ghost hints */
export const HIDDEN_COMMANDS: SlashCommand[] = [
  { name: '/restart', description: 'Restart evot in place', handler: 'builtin', minPrefixLength: 5 },
  { name: '/update', description: 'Update evot to latest version', handler: 'builtin' },
  { name: '/version', description: 'Show current version', handler: 'builtin' },
  { name: '/exit', aliases: ['/quit', '/q'], description: 'Exit the REPL', handler: 'builtin' },
  { name: '/act', description: 'Return to normal action mode', handler: 'builtin' },
  { name: '/done', description: 'Exit log/plan mode', handler: 'builtin' },
  { name: '/harden', description: 'Stress-test the previous plan or current changes', usage: '/harden [plan | changes | arch | subject]', handler: 'builtin' },
  { name: '/skill', description: 'Manage skills', usage: '/skill [list | install <source> | remove <name>]', handler: 'builtin' },
  { name: '/copy', description: 'Copy last agent message (Markdown source) to clipboard', handler: 'builtin' },
  { name: '/clip', description: 'Clip last reply to the memory vault; all = distill session', usage: '/clip [all]', handler: 'builtin' },
  { name: '/share', description: 'Share a session or import a shared session', usage: '/share [session-id | url#password]', handler: 'builtin' },
  { name: '/compact', description: 'Compact session context', usage: '/compact [instructions]', handler: 'builtin' },
  { name: '/clear', description: 'Clear session context', handler: 'builtin' },
  { name: '/env', description: 'Manage variables', usage: '/env [set K=V | del K | load FILE]', handler: 'builtin' },
  { name: '/log', description: 'Show or analyze session logs; shot exports markdown', usage: '/log [shot | query]', handler: 'builtin' },
  { name: '/_dump', description: 'Dump system prompt + tools + skills as JSON', usage: '/_dump [path]', handler: 'builtin' },
]

/** All commands (visible + hidden) for resolution */
export const ALL_COMMANDS: SlashCommand[] = [...COMMANDS, ...HIDDEN_COMMANDS]

export type ResolvedCommand =
  | { kind: 'resolved'; name: string; args: string }
  | { kind: 'ambiguous'; candidates: string[] }
  | { kind: 'unknown' }

export function buildHardenPrompt(args: string): string {
  const subject = args.trim()
  if (!subject || subject === 'plan') {
    return [
      'harden the plan, strategy, or conclusion from the immediately preceding conversation context.',
      'If local git changes exist, inspect them only as supporting context and combine any relevant findings with the hardening pass; do not default to hardening the diff as the primary subject.',
    ].join(' ')
  }
  if (subject === 'changes') {
    return 'harden current git changes'
  }
  if (subject === 'arch') {
    return [
      'harden the architecture of the current git changes or the immediately preceding plan.',
      'Evaluate: simplicity, decoupling, clarity of responsibility, and cohesion.',
      'In the final output, include an annotated file tree showing the proposed directory structure with short comments explaining each module\'s role.',
    ].join(' ')
  }
  return `harden this strategy: ${subject}`
}

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

  // Prefix match. Low-frequency commands may require a longer explicit prefix
  // so they do not steal common prefixes from high-frequency commands.
  const matches = ALL_COMMANDS.filter(
    (c) => cmd.length >= (c.minPrefixLength ?? 1)
      && (c.name.startsWith(cmd) || (c.aliases?.some((a) => a.startsWith(cmd)) ?? false))
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
 * Returns true when text looks like a hand-typed slash command prefix:
 * `/` followed by zero or more ASCII lowercase letters.
 * Pasted paths like `/some/path.rs` are rejected.
 */
function isSlashPrefix(text: string): boolean {
  if (!text.startsWith('/')) return false
  const rest = text.slice(1)
  const cmdPart = rest.split(/\s/)[0] ?? ''
  // Allow lowercase letters plus `_` so hidden commands like `/_dump` work.
  return /^[a-z_]*$/.test(cmdPart)
}

/**
 * Check if input has a valid slash-command shape.
 * Unknown names still route through the command handler so they produce an
 * explicit error instead of being submitted to the model.
 */
export function isSlashCommand(input: string): boolean {
  const trimmed = input.trim()
  if (!isSlashPrefix(trimmed)) return false
  return trimmed.split(/\s+/)[0] !== '/'
}
