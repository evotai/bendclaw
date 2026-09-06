import type { SkillEntry } from '../../commands/skill.js'
import type { SelectorState } from '../selector.js'
import { createAppSelectorState } from './selector-identity.js'

export const SKILL_SELECTOR_TITLE = 'Skills'

/** Build the read-only live inventory shown while `/skill` is in the composer. */
export function createSkillSelectorState(entries: SkillEntry[]): SelectorState {
  const items = entries.map(entry => ({
    id: entry.name,
    label: entry.name,
    ...(entry.group ? { detail: `in ${entry.group}/`, group: entry.group } : {}),
    searchText: `${entry.name} ${entry.group ?? ''} ${entry.dir}`,
  }))
  return {
    ...createAppSelectorState('skill', SKILL_SELECTOR_TITLE, items, items),
    subtitle: `${entries.length} installed · /skill list for sources and management`,
    ...(items.length === 0 ? { emptyMessage: 'No skills installed' } : {}),
    hints: [
      { keys: ['up', 'down'], action: 'move' },
      { keys: 'type', action: 'filter' },
      { keys: 'escape', action: 'close' },
    ],
  }
}
