import { createBackgroundPanelState } from '../src/term/app/background-panel.js'
import { TermRenderer } from '../src/term/renderer.js'
import { ScreenHarness } from './helpers/screen.js'
import { describe, expect, test } from 'bun:test'
import { buildShellFrame, type ShellSnapshot } from '../src/term/viewmodel/shell.js'
import { promptFromSnapshot } from '../src/term/viewmodel/prompt-snapshot.js'
import { createEditorState } from '../src/term/input/editor.js'
import { createInitialState } from '../src/term/app/state.js'
import { createModelWindow } from '../src/term/app/selector-windows.js'
import { createQueueSelectorState } from '../src/term/app/queue-manage.js'
import { createAskState } from '../src/term/ask.js'
import { buildAskRegionLines } from '../src/term/viewmodel/ask.js'
import { buildSelectorRegionLines } from '../src/term/viewmodel/selector.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import { buildPromptFooterBlocks } from '../src/term/viewmodel/prompt-footer.js'

function snapshot(columns = 80, rows = 24): ShellSnapshot {
  return {
    contentLines: ['history'], preEditorBlocks: [{ lines: [{ spans: [{ text: 'status' }] }] }],
    overlay: { kind: 'none' }, preview: null, commandFocused: false,
    prompt: promptFromSnapshot({
      editor: createEditorState(), session: createInitialState('model', '/work'),
      active: true, planning: false, logMode: false,
      dashboardUrl: null, exitHint: false, columns, rows, gitBranch: null, backgroundProcessCount: 0,
    }),
  }
}

describe('shell composition', () => {
  test('preview to focused command preserves geometry at all supported sizes', () => {
    const selector = createModelWindow(undefined, 'model')
    for (const [columns, rows] of [[20, 8], [80, 24], [160, 40]]) {
      const input = snapshot(columns, rows)
      const preview = buildShellFrame({ ...input, preview: { kind: 'selector', state: selector } })
      const focused = buildShellFrame({ ...input, overlay: { kind: 'selector', state: selector }, commandFocused: true, prompt: { ...input.prompt, active: false } })
      expect(preview.lines.length).toBe(focused.lines.length)
      expect(preview.transientRows).toBe(focused.transientRows)
      expect(preview.stableViewport).toBe(true)
      expect(focused.stableViewport).toBe(true)
      expect(focused.bottomAnchorStart).toBe(1)
    }
  })

  test('queue and ask replace only editor, keeping status and footer', () => {
    const input = snapshot()
    const queue = createQueueSelectorState([])
    const ask = createAskState([{ header: 'Q', question: 'Choose?', options: [{ label: 'yes', description: '' }] }])
    for (const [overlay, lines] of [
      [{ kind: 'selector', state: queue }, buildSelectorRegionLines(queue, 80, 24)],
      [{ kind: 'ask-user', state: ask }, buildAskRegionLines(ask, 80)],
    ] as const) {
      const frame = buildShellFrame({ ...input, overlay })
      expect(frame.lines).toEqual(['history', 'status', ...lines, ...blocksToLines(buildPromptFooterBlocks(input.prompt))])
      expect(frame.transientRows).toBe(lines.length)
      expect(frame.stableViewport).toBeUndefined()
    }
  })

  test('background panel suppresses its duplicate entry hint, closing restores the down arrow', () => {
    const input = snapshot()
    input.prompt.backgroundProcessCount = 1
    input.prompt.backgroundPanelDownAvailable = true
    const open = buildShellFrame({ ...input, overlay: { kind: 'selector', state: createBackgroundPanelState([]) } })
    expect(open.lines.join('\n')).not.toContain('to manage')
    const closed = buildShellFrame(input)
    expect(closed.lines.join('\n')).toContain('↓ to manage')
    expect(closed.lines.join('\n')).not.toContain('ctrl+t')
  })

  test('help is a modal and closing it leaves the durable layout intact', () => {
    const input = snapshot()
    const normal = buildShellFrame(input)
    const help = buildShellFrame({ ...input, overlay: { kind: 'help' } })
    expect(help.lines).toEqual(normal.lines)
    expect(help.overlay?.lines.length).toBeGreaterThan(0)
    expect(normal.overlay).toBeUndefined()
    expect(normal.transientRows).toBe(0)
  })

  test('real compositor restores the composer after closing a transient selector', async () => {
    const screen = new ScreenHarness(80, 24)
    const renderer = new TermRenderer({ stdout: screen.stdout })
    const input = snapshot()
    const selector = createModelWindow(undefined, 'model')
    let frame = buildShellFrame(input)
    renderer.init()
    renderer.setRenderCallback(() => frame)
    const render = async () => {
      renderer.requestRender()
      await Bun.sleep(25)
      await screen.settle()
    }
    try {
      await render()
      const initial = screen.viewport()
      frame = buildShellFrame({ ...input, preview: { kind: 'selector', state: selector } })
      await render()
      frame = buildShellFrame({ ...input, overlay: { kind: 'selector', state: selector }, commandFocused: true, prompt: { ...input.prompt, active: false } })
      await render()
      frame = buildShellFrame(input)
      await render()
      expect(screen.viewport()).toEqual(initial)
    } finally {
      renderer.destroy()
      await screen.settle()
      screen.terminal.dispose()
    }
  })

  test('layout is deterministic and leaves caller snapshots unchanged', () => {
    const input = snapshot()
    const before = structuredClone(input)
    expect(buildShellFrame(input)).toEqual(buildShellFrame(input))
    expect(input).toEqual(before)
  })
})
