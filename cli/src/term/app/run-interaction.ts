import type { SpinnerPhase } from '../spinner.js'
import { InterruptConfirmation } from './interrupt-confirmation.js'

/** Runtime facts, not a second copy of engine or process state. */
export interface RunInteractionInput {
  active: boolean
  owner: unknown
  phase?: SpinnerPhase
  localOperation?: boolean
  compacting?: boolean
  foregroundTasks?: number
  blockingWaits?: number
  backgroundTasks?: number
}

export interface RunInteractionState {
  kind: 'idle' | 'active' | 'waiting-task' | 'waiting-background' | 'retrying' | 'local-operation' | 'compacting'
  interruptTarget: 'agent' | 'compaction' | null
  backgroundAction: 'background-shell' | 'release-wait' | null
  interruptPending: boolean
  showUsage: boolean
  allowSlowWarning: boolean
}

/** One capability policy shared by key routing and status presentation. */
export function resolveRunInteraction(input: RunInteractionInput): RunInteractionState {
  const kind: RunInteractionState['kind'] = !input.active
    ? (input.backgroundTasks ?? 0) > 0 ? 'waiting-background' : 'idle'
    : input.compacting ? 'compacting'
    : input.localOperation ? 'local-operation'
    : (input.foregroundTasks ?? 0) === 0 && (input.blockingWaits ?? 0) > 0 ? 'waiting-task'
    : ['retrying', 'quota_waiting', 'outage_waiting'].includes(input.phase ?? '') ? 'retrying'
    : 'active'
  const owned = input.owner !== null && input.owner !== undefined
  const interruptTarget = !owned || kind === 'idle' || kind === 'waiting-background' || kind === 'local-operation'
    ? null : kind === 'compacting' ? 'compaction' : 'agent'
  return {
    kind,
    interruptTarget,
    backgroundAction: interruptTarget !== 'agent' ? null
      : (input.foregroundTasks ?? 0) > 0 ? 'background-shell'
      : (input.blockingWaits ?? 0) > 0 ? 'release-wait' : null,
    interruptPending: false,
    showUsage: kind === 'active',
    allowSlowWarning: kind === 'active' || kind === 'local-operation' || kind === 'compacting',
  }
}

/** Owns only the operation-scoped confirmation; side effects remain with the host. */
export class RunInteraction {
  private readonly confirmation: InterruptConfirmation
  constructor(now = Date.now) { this.confirmation = new InterruptConfirmation(now) }

  snapshot(input: RunInteractionInput): RunInteractionState {
    const state = resolveRunInteraction(input)
    return { ...state, interruptPending: state.interruptTarget !== null && this.confirmation.pending(input.owner) }
  }

  requestInterrupt(input: RunInteractionInput): 'unavailable' | 'confirm' | 'interrupt' {
    if (this.snapshot(input).interruptTarget === null) return 'unavailable'
    return this.confirmation.press(input.owner) ? 'interrupt' : 'confirm'
  }

  clear(): void { this.confirmation.clear() }
}
