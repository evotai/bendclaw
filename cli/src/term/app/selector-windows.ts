import type { ConfigInfo } from '../../native/contracts/config-info.js'
import { selectorFocusOn, type SelectorItem, type SelectorState } from '../selector.js'
import { createAppSelectorState } from './selector-identity.js'
import { currentModelSpec, modelOptions, modelSelectorItems } from './provider.js'
import { RESUME_SELECTOR_TITLE } from './resume.js'

/** One factory for preview and explicitly opened model windows. */
export function createModelWindow(config: ConfigInfo | undefined, model: string, listFocused = false): SelectorState {
  const models = modelOptions(config, model)
  const activeSpec = currentModelSpec(config, model)
  return selectorFocusOn({
    ...createAppSelectorState('model', 'Models', modelSelectorItems(models, activeSpec)),
    presentation: 'model',
    circularNavigation: true,
    listFocused,
  }, item => item.id === activeSpec)
}

export function createResumeWindow(items: SelectorItem[], initialQuery?: string): SelectorState {
  const state = createAppSelectorState('resume', RESUME_SELECTOR_TITLE, items, items, initialQuery)
  return {
    ...state,
    listFocused: false,
    lowercaseHints: true,
    ...(state.query.length === 0 && state.items.length === 0 && state.allItems.some(item => !item.header)
      ? { emptyMessage: 'No sessions in current cwd · type to search all sessions' }
      : {}),
  }
}
