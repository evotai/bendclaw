import type { RunInteractionState } from '../app/run-interaction.js'
import { backgroundChord } from '../design/key-hints.js'

/** Copy and display policy only. Never reads native state or handles keys. */
export function runStatusPresentation(state: RunInteractionState): {
  label?: string
  hint: string
  showUsage: boolean
  allowSlowWarning: boolean
} {
  // Show the next useful action, not every supported key. Confirmation wins
  // over backgrounding; hiding Esc here never disables its actual binding.
  let hint = ''
  const target = state.interruptTarget === 'compaction' ? ' compaction' : ''
  if (state.interruptTarget && state.interruptPending) {
    hint = `esc again to interrupt${target}`
  } else if (state.backgroundAction) {
    hint = `${backgroundChord()} to ${state.backgroundAction === 'background-shell' ? 'background' : 'release wait'}`
  } else if (state.interruptTarget) {
    hint = `esc twice to interrupt${target}`
  }
  return {
    label: state.kind === 'waiting-task' ? 'Waiting for task result…'
      : state.kind === 'waiting-background' ? 'Background task running · resumes when finished' : undefined,
    hint: hint ? ` · ${hint}` : '',
    showUsage: state.showUsage,
    allowSlowWarning: state.allowSlowWarning,
  }
}
