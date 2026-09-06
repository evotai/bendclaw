import type { RunInteractionState } from '../app/run-interaction.js'
import { backgroundChord } from '../design/key-hints.js'

/** Copy and display policy only. Never reads native state or handles keys. */
export function runStatusPresentation(state: RunInteractionState): {
  label?: string
  hint: string
  showUsage: boolean
  allowSlowWarning: boolean
} {
  const hints: string[] = []
  if (state.interruptTarget) {
    const target = state.interruptTarget === 'compaction' ? ' compaction' : ''
    hints.push(`${state.interruptPending ? 'esc again' : 'esc twice'} to interrupt${target}`)
    if (state.kind === 'waiting-task' && state.interruptPending) hints.push('background task keeps running')
  }
  if (state.backgroundAction) {
    hints.push(`${backgroundChord()} to ${state.backgroundAction === 'background-shell' ? 'background' : 'release wait'}`)
  }
  return {
    label: state.kind === 'waiting-task' ? 'Waiting for task result…'
      : state.kind === 'waiting-background' ? 'Background task running · resumes when finished' : undefined,
    hint: hints.length ? ` · ${hints.join(' · ')}` : '',
    showUsage: state.showUsage,
    allowSlowWarning: state.allowSlowWarning,
  }
}
