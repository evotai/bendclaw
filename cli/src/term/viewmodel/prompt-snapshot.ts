import type { ConfigInfo } from '../../native/contracts/config-info.js'
import type { AppState } from '../app/state.js'
import { providerDisplayName } from '../app/provider.js'
import { shouldDownOpenPanel } from '../app/background-panel.js'
import { isEditorEmpty, type EditorState } from '../input/editor.js'
import type { PromptVMInput } from './prompt.js'

export interface PromptSnapshot {
  editor: EditorState
  session: Pick<AppState, 'model' | 'cwd' | 'sessionTokens'>
  config?: ConfigInfo
  active: boolean
  caretVisible: boolean
  planning: boolean
  logMode: boolean
  dashboardUrl: string | null
  exitHint: boolean
  columns: number
  rows: number
  gitBranch: string | null
  backgroundProcessCount: number
}

/** Pure projection of a single host snapshot. No agent, terminal or clocks. */
export function promptFromSnapshot(input: PromptSnapshot): PromptVMInput {
  const { editor, session, config } = input
  const empty = isEditorEmpty(editor)
  return {
    lines: editor.lines,
    cursorLine: editor.cursorLine,
    cursorCol: editor.cursorCol,
    active: input.active,
    caretVisible: input.caretVisible,
    model: session.model,
    provider: providerDisplayName(
      config?.availableModels.find(model => model.model === session.model && model.provider === (config?.provider ?? ''))
        ?? config?.availableModels.find(model => model.model === session.model),
      config?.provider ?? '',
    ),
    planning: input.planning,
    logMode: input.logMode,
    dashboardUrl: input.dashboardUrl,
    exitHint: input.exitHint,
    completion: editor.completion,
    ghostHint: editor.ghostHint,
    columns: input.columns,
    rows: input.rows,
    placeholder: empty,
    cwd: session.cwd,
    gitBranch: input.gitBranch,
    contextTokens: session.sessionTokens.contextTokens,
    contextWindow: session.sessionTokens.contextWindow,
    backgroundProcessCount: input.backgroundProcessCount,
    backgroundPanelDownAvailable: shouldDownOpenPanel({ editorEmpty: empty, running: input.backgroundProcessCount }),
    thinkingLevel: config?.thinkingLevel ?? '',
  }
}
