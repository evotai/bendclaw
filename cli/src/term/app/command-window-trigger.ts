import { resolveCommand } from '../../commands/index.js'

export type CommandWindowTrigger = 'resume' | 'model' | 'skill' | 'help'

const COMMAND_WINDOW_NAMES = ['/resume', '/sessions', '/model', '/skill', '/help'] as const

/**
 * An incomplete command-window spelling that is not unique yet. When a command
 * window is already mounted, this is a transition state rather than a reason
 * to remove it: `/re` → `/` → `/mo` can then swap content in place without an
 * empty frame between the session and model windows.
 */
export function isCommandWindowBridge(text: string): boolean {
  if (!/^\/[a-z_]*$/.test(text)) return false
  return COMMAND_WINDOW_NAMES.some(name => name.startsWith(text))
}

/**
 * Resolve an argument-free slash command or unique prefix that owns a formal
 * window. Resolution drives the live preview; the REPL still requires an
 * explicit up/down keypress before moving focus out of the composer.
 */
export function resolveCommandWindowTrigger(text: string): CommandWindowTrigger | null {
  // Whitespace means argument entry. Arrow keys must remain editor/history
  // controls rather than unexpectedly moving focus into a command window.
  if (!/^\/[a-z_]+$/.test(text)) return null

  const resolved = resolveCommand(text)
  if (resolved.kind !== 'resolved' || resolved.args) return null

  if (resolved.name === '/resume') return 'resume'
  if (resolved.name === '/model') return 'model'
  if (resolved.name === '/skill') return 'skill'
  if (resolved.name === '/help') return 'help'
  return null
}
