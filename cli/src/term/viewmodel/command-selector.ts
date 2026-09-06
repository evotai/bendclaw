import { createAppSelectorState } from '../app/selector-identity.js'
import { RESUME_SELECTOR_TITLE } from '../app/resume.js'
import { SELECTOR_VIEWPORT, type SelectorState } from '../selector.js'
import { buildSelectorRegionLines } from './selector.js'

function fullResumeSlot(): SelectorState {
  const items = Array.from({ length: SELECTOR_VIEWPORT + 2 }, (_, index) => ({
    id: `command-window-slot-${index}`,
    label: `session-${index}`,
    detail: 'source title [1 turn] now',
    preview: ['Session title', 'model · 1 turn · now'],
  }))
  return {
    ...createAppSelectorState('resume', RESUME_SELECTOR_TITLE, items),
    subtitle: 'Recent sessions', focusIndex: 1, scrollOffset: 1,
  }
}

function fullModelSlot(): SelectorState {
  const items = Array.from({ length: 7 }, (_, groupIndex) => {
    const group = `Provider ${groupIndex}`
    return [
      { label: group, header: true, focusable: false, group },
      { id: `provider-${groupIndex}:model`, label: `Model ${groupIndex}`, group },
    ]
  }).flat()
  return {
    ...createAppSelectorState('model', 'Models', items),
    presentation: 'model', circularNavigation: true, focusIndex: 7,
  }
}

/** Reserve populated-window geometry before async rows arrive, preserving the
 * existing resize/preview/focus contract without knowing the terminal host. */
export function buildCommandSelectorRegion(state: SelectorState, columns: number, rows: number, active: boolean): string[] {
  const lines = buildSelectorRegionLines(state, columns, rows, active)
  const slotHeight = Math.max(
    buildSelectorRegionLines(fullResumeSlot(), columns, rows, false).length,
    buildSelectorRegionLines(fullModelSlot(), columns, rows, false).length,
  )
  return [...Array(Math.max(0, slotHeight - lines.length)).fill(''), ...lines]
}
