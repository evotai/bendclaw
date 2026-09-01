/**
 * Pure helpers behind the background-terminal UI.
 *
 * These live outside `startRepl` so the footer count and the session-switch
 * decision can be tested without a terminal. Panel rendering lives in
 * `background-panel.ts`.
 */

import type { BackgroundProcess } from '../../native/index.js'
import { BACKGROUND_PANEL_SHORTCUT_HINT } from './background-panel.js'

/**
 * Tasks the footer counts. Only backgrounded work is included: a foreground
 * Bash call is already visible as a running tool card, so counting it would
 * double-report the same command.
 */
export function runningBackgroundCount(processes: BackgroundProcess[]): number {
  return processes.filter(process => process.status === 'running').length
}

/** Change key for the poll: re-render only when a visible field moved. */
export function backgroundProcessFingerprint(processes: BackgroundProcess[]): string {
  return processes
    .map(process => [
      process.task_id,
      process.status,
      process.exit_code ?? '',
      process.output_file_truncated,
    ].join(':'))
    .join('|')
}

/** Commands that leave the current session behind. */
const SESSION_SWITCH_COMMANDS = new Set(['/new', '/clear', '/resume'])

export type SessionSwitchDecision =
  | { kind: 'proceed' }
  | { kind: 'warn'; running: number; message: string }

/**
 * Whether a session-switching command may run while background work is live.
 *
 * The first attempt warns; an immediate repeat of the same command proceeds.
 * A hard block would trap the user in the session whenever a task ignores
 * SIGKILL or a stop times out, so the escape hatch is deliberate and
 * mirrors the confirm-by-repeat pattern of the Ctrl+C exit hint.
 */
export function decideSessionSwitch(input: {
  command: string
  running: number
  warnedFor: string | null
}): SessionSwitchDecision {
  if (!SESSION_SWITCH_COMMANDS.has(input.command)) return { kind: 'proceed' }
  if (input.running === 0) return { kind: 'proceed' }
  if (input.warnedFor === input.command) return { kind: 'proceed' }
  const plural = input.running === 1 ? '' : 's'
  return {
    kind: 'warn',
    running: input.running,
    message: `  ${input.running} background terminal${plural} still running. Press ${BACKGROUND_PANEL_SHORTCUT_HINT} to manage them, or repeat ${input.command} to switch anyway.`,
  }
}

/** Summary line for stopping every task (`X` in the panel). */
export function stopAllMessage(stoppedCount: number): string {
  if (stoppedCount === 0) return '  No background terminals running.'
  const plural = stoppedCount === 1 ? '' : 's'
  return `  ■ Stopped ${stoppedCount} background terminal${plural}.`
}

/**
 * Result line for stopping one task (`x` in the panel). A task that outlived
 * the stop timeout is reported as still running rather than silently claimed as
 * stopped.
 */
export function stopOneMessage(stopped: BackgroundProcess): string {
  const command = stopped.command.split('\n', 1)[0] ?? stopped.command
  const id = stopped.task_id.slice(0, 8)
  if (stopped.status === 'running' || stopped.status === 'running_foreground') {
    return `  ● ${id} did not stop within the timeout and is still running  ${command}`
  }
  return `  ■ Stopped ${id}  ${command}`
}
