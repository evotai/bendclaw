/**
 * Composer appearance per input mode.
 *
 * Plan and log mode change what a submission does — plan withholds edits, log
 * drives a forked agent. Both used to show up only as a dim `[plan]` tag at the
 * far left of the footer, which is the easiest thing on screen to miss. A mode
 * with consequences should be visible where the user is already looking: at the
 * composer.
 *
 * Each mode contributes a border label, a border hue, and its own placeholder.
 * All three cost zero extra rows.
 */

import { getTheme } from '../../render/theme/index.js'

export type PromptMode = 'default' | 'plan' | 'log'

export interface PromptModeStyle {
  /** Frame hue. The default mode keeps the brand colour. */
  hex: string
  /** Full-width placeholder. */
  hint: string
  /** Narrow-terminal placeholder, roughly half the full width. */
  shortHint: string
}

/** The mode that decides hue and placeholder when several are active. */
export function promptMode(input: { planning: boolean; logMode: boolean }): PromptMode {
  // Log mode is the narrower context: it forks a separate agent, so when both
  // are somehow set it is the one that describes what submitting will do.
  if (input.logMode) return 'log'
  if (input.planning) return 'plan'
  return 'default'
}

/**
 * Every active mode, in the order the footer lists them. Plan and log are
 * independent flags, so both can hold at once; the border names both rather
 * than dropping one, even though hue and placeholder follow the primary.
 */
export function promptModeLabels(input: { planning: boolean; logMode: boolean }): string[] {
  const labels: string[] = []
  if (input.logMode) labels.push('log')
  if (input.planning) labels.push('plan')
  return labels
}

export function promptModeStyle(mode: PromptMode): PromptModeStyle {
  const theme = getTheme()
  switch (mode) {
    case 'plan':
      return {
        hex: theme.accentHex,
        hint: 'Describe what to plan — no edits until you approve',
        shortHint: 'Describe what to plan',
      }
    case 'log':
      return {
        hex: theme.accentHex,
        hint: 'Ask about the captured log',
        shortHint: 'Ask about the log',
      }
    default:
      return {
        hex: theme.brandHex,
        hint: 'Enter a coding task or / for commands',
        shortHint: 'Enter a coding task',
      }
  }
}
