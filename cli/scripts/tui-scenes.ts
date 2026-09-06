import type { ConfigInfo } from '../src/native/contracts/config-info.js'
import { createAskState } from '../src/term/ask.js'
import { createModelWindow, createResumeWindow } from '../src/term/app/selector-windows.js'
import { createInitialState } from '../src/term/app/state.js'
import { createEditorState, insertText } from '../src/term/input/editor.js'
import { promptFromSnapshot } from '../src/term/viewmodel/prompt-snapshot.js'
import type { ShellSnapshot } from '../src/term/viewmodel/shell.js'

export const SCENES = ['idle', 'streaming', 'model-preview', 'model-focused', 'resume', 'ask', 'help', 'planning'] as const
export type Scene = typeof SCENES[number]

/** Offline product fixtures. Deliberately no Agent, user config, network or
 * process-global theme changes. Shared by preview tooling and layout tests. */
export function previewScene(scene: Scene, columns: number, rows: number): ShellSnapshot {
  const config: ConfigInfo = {
    provider: 'fixture', protocol: 'openai', envPath: '', hasApiKey: false, baseUrl: null, thinkingLevel: 'high',
    availableModels: [
      { provider: 'fixture', protocol: 'openai', model: 'fast', spec: 'fixture:fast' },
      { provider: 'fixture', protocol: 'openai', model: 'reasoning', spec: 'fixture:reasoning' },
    ],
  }
  const input: ShellSnapshot = {
    contentLines: ['evot · offline component preview', '', 'You: Review the implementation.', 'Assistant: Ready to help.'],
    preEditorBlocks: [],
    prompt: promptFromSnapshot({
      editor: createEditorState(), session: createInitialState('fast', '/workspace/project'), config,
      active: true, caretVisible: true, planning: false, logMode: false, dashboardUrl: null,
      exitHint: false, columns, rows, gitBranch: 'main', backgroundProcessCount: 0,
    }),
    overlay: { kind: 'none' }, preview: null, commandFocused: false,
  }
  switch (scene) {
    case 'idle': break
    case 'streaming':
      input.preEditorBlocks = [{ lines: [{ spans: [{ text: 'Thinking…  3s · Esc to interrupt', dim: true }] }], marginTop: 1 }]
      break
    case 'model-preview':
    case 'model-focused': {
      const editor = insertText(createEditorState(), '/model')
      input.prompt = { ...input.prompt, lines: editor.lines, cursorCol: editor.cursorCol, placeholder: false }
      const state = createModelWindow(config, 'fast', scene === 'model-focused')
      if (scene === 'model-preview') input.preview = { kind: 'selector', state }
      else {
        input.overlay = { kind: 'selector', state }
        input.commandFocused = true
        input.prompt.active = false
      }
      break
    }
    case 'resume':
      input.overlay = { kind: 'selector', state: createResumeWindow([
        { id: 'session-a', label: 'Fix streaming layout', preview: ['Fix streaming layout', 'fast · 3 turns', '', '› Support 中文 and emoji 🙂'] },
        { id: 'session-b', label: 'Review configuration transactions' },
      ]) }
      input.prompt.active = false
      break
    case 'ask':
      input.overlay = { kind: 'ask-user', state: createAskState([
        { header: 'Scope', question: 'Which scope should be reviewed?', options: [
          { label: 'Current change', description: 'Review only the working diff.' },
          { label: 'Whole module', description: 'Include module boundaries.' },
        ] },
      ]) }
      input.prompt.active = false
      break
    case 'help':
      input.overlay = { kind: 'help' }
      input.prompt.active = false
      break
    case 'planning': input.prompt.planning = true; break
  }
  return input
}
