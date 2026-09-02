/**
 * Pure helpers behind the background-terminal UI.
 *
 * These live outside `startRepl` so the footer count and the session-switch
 * decision can be tested without a terminal. Panel rendering lives in
 * `background-panel.ts`.
 */

import type { BackgroundProcess } from '../../native/index.js'
import { BACKGROUND_PANEL_SHORTCUT_HINT, formatCommandLabel } from './background-panel.js'

/** Terminal states, i.e. the task will never produce output again. */
function isSettled(status: BackgroundProcess['status']): boolean {
  return status === 'completed' || status === 'failed' || status === 'killed'
}

/**
 * Transcript line for a task that finished on its own.
 *
 * Without this a task that completes while the agent is idle is announced
 * nowhere: the spinner is gone, the terminal title has settled, and the footer
 * chip only counts down. The model already learns via the engine's queued
 * notification, so the user was the one party left uninformed.
 *
 * Deliberately not routed through the spinner. A spinner means "the agent owes
 * you a reply" and carries `esc to interrupt`, which does not apply to detached
 * work — reviving it for background tasks would make both states read alike and
 * advertise a key that would not do what it says.
 */
export function settledNoticeMessage(process: BackgroundProcess): string {
  const id = process.task_id.slice(0, 8)
  const command = formatCommandLabel(process.command)
  const exit = process.exit_code === null ? '' : ` · exit ${process.exit_code}`
  if (process.status === 'killed') {
    const how = process.stopped_by_user ? 'was cancelled by the user' : 'was stopped'
    return `  ■ ${id} ${how}  ${command}`
  }
  const outcome = process.status === 'failed' ? '✗ failed' : '✓ completed'
  return `  ${outcome} in background${exit} · ${id}  ${command}`
}

/**
 * Tasks that reached a terminal state between two polls.
 *
 * Compares by task id rather than list position: the engine reclaims idle
 * entries, so a task can leave the list entirely between polls. One that
 * disappears is not reported — its last observed state is all we ever saw, and
 * inventing an outcome for it would be a guess.
 */
export function newlySettled(
  previous: BackgroundProcess[],
  next: BackgroundProcess[],
): BackgroundProcess[] {
  if (previous.length === 0) return []
  const before = new Map(previous.map(process => [process.task_id, process.status]))
  return next.filter(process => {
    const was = before.get(process.task_id)
    // A task first seen already settled is not a transition: it may predate the
    // session, and announcing it would replay history on resume.
    if (was === undefined || isSettled(was)) return false
    return isSettled(process.status)
  })
}

/**
 * Whether a finished background task should open a turn to deliver its result.
 *
 * Without this, a long task that outlives the turn that started it is a dead
 * end: the engine queues the completion notice, but nothing consumes the queue
 * until the user types again, so an agent told to "run the build then fix what
 * breaks" simply stops after the build.
 *
 * A turn is the only thing that can carry a queued notification, so the
 * conditions are about turn ownership rather than about the task:
 *
 * - `pending` is the engine's own queue. Polling it (rather than the settled
 *   transition) means a wake happens exactly when there is something to
 *   deliver, and never re-fires once `build_turn` has drained it.
 * - A run in flight already drains the queue between turns via
 *   `get_follow_up_messages`, so waking would duplicate the delivery.
 * - Queued user messages will carry the notifications themselves when they
 *   submit, and they outrank a synthetic turn.
 * - A modal overlay means the agent is waiting on the user, so seizing the turn
 *   would answer a question the user has not answered yet.
 */
export function shouldWakeForNotifications(input: {
  pending: number
  hasSession: boolean
  runInFlight: boolean
  queuedMessages: number
  overlayBlocking: boolean
}): boolean {
  if (input.pending === 0) return false
  if (!input.hasSession) return false
  if (input.runInFlight) return false
  if (input.queuedMessages > 0) return false
  if (input.overlayBlocking) return false
  return true
}

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
